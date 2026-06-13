//! One poll cycle for one saved search: run each adapter (isolated), enrich
//! new listings with image phashes, ingest. Used by both the `run_search`
//! command and the background scheduler.
//!
//! This layer owns adapter failure bookkeeping: exponential backoff and
//! auto-disable live in the `adapter_state` table, not in adapters.

use crate::adapters::{AdapterRegistry, SearchContext};
use crate::models::{AdapterError, AdapterStatus, RawListing, SearchSpec};
use crate::pipeline;
use rusqlite::{params, Connection};
use serde::Serialize;
use std::collections::HashSet;
use std::sync::Mutex;
use std::time::Duration;

/// Backoff: 5 min base, doubling per consecutive failure, capped at 24h.
const BACKOFF_BASE_SECS: i64 = 300;
const BACKOFF_CAP_SECS: i64 = 86_400;
/// After this many consecutive failures the adapter is marked disabled
/// (still auto-recovers when the backoff window lapses).
const DISABLE_AFTER_FAILURES: i64 = 3;

const IMAGE_FETCH_TIMEOUT: Duration = Duration::from_secs(10);
const IMAGE_MAX_BYTES: usize = 1_000_000;

#[derive(Debug, Serialize, Clone)]
pub struct AdapterRunReport {
    pub adapter_id: String,
    pub fetched: usize,
    pub skipped: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Serialize, Clone)]
pub struct RunReport {
    pub search_id: i64,
    pub adapters: Vec<AdapterRunReport>,
    pub inserted: usize,
    pub duplicates_skipped: usize,
    pub already_known: usize,
}

#[derive(Debug, Clone, Copy)]
pub struct PollOptions {
    /// Download first images to compute perceptual hashes. Disabled in
    /// integration tests, which must never touch the network.
    pub fetch_images: bool,
}

impl Default for PollOptions {
    fn default() -> Self {
        PollOptions { fetch_images: true }
    }
}

pub async fn run_poll_cycle(
    db: &Mutex<Connection>,
    registry: &AdapterRegistry,
    search_id: i64,
) -> Result<RunReport, String> {
    run_poll_cycle_with(db, registry, search_id, PollOptions::default()).await
}

pub async fn run_poll_cycle_with(
    db: &Mutex<Connection>,
    registry: &AdapterRegistry,
    search_id: i64,
    opts: PollOptions,
) -> Result<RunReport, String> {
    let spec: SearchSpec = {
        let conn = db.lock().map_err(|e| format!("db lock poisoned: {e}"))?;
        let search = crate::commands::get_search(&conn, search_id)
            .map_err(|e| format!("search {search_id} not found: {e}"))?;
        SearchSpec::from(&search)
    };

    let mut report = RunReport {
        search_id,
        adapters: Vec::new(),
        inserted: 0,
        duplicates_skipped: 0,
        already_known: 0,
    };

    for adapter in registry.adapters() {
        let adapter_id = adapter.id();

        if let AdapterStatus::NotConfigured { reason } = adapter.health() {
            report.adapters.push(AdapterRunReport {
                adapter_id: adapter_id.into(),
                fetched: 0,
                skipped: Some(format!("not configured: {reason}")),
                error: None,
            });
            continue;
        }

        // Backoff / context reads under one short lock; no network yet.
        let (in_backoff, known_ids) = {
            let conn = db.lock().map_err(|e| format!("db lock poisoned: {e}"))?;
            (backoff_active(&conn, adapter_id), known_source_ids(&conn, adapter_id)?)
        };
        if let Some(until) = in_backoff {
            report.adapters.push(AdapterRunReport {
                adapter_id: adapter_id.into(),
                fetched: 0,
                skipped: Some(format!("backing off until {until}")),
                error: None,
            });
            continue;
        }

        let ctx = SearchContext { known_source_ids: known_ids };
        match adapter.search(&spec, &ctx).await {
            Ok(mut raw) => {
                let fetched = raw.len();
                if opts.fetch_images {
                    enrich_with_phashes(&mut raw).await;
                }
                let conn = db.lock().map_err(|e| format!("db lock poisoned: {e}"))?;
                record_success(&conn, adapter_id);
                let stats = pipeline::ingest(&conn, search_id, raw).map_err(|e| e.to_string())?;
                report.inserted += stats.inserted;
                report.duplicates_skipped += stats.duplicates_skipped;
                report.already_known += stats.already_known;
                report.adapters.push(AdapterRunReport {
                    adapter_id: adapter_id.into(),
                    fetched,
                    skipped: None,
                    error: None,
                });
            }
            Err(err) => {
                let conn = db.lock().map_err(|e| format!("db lock poisoned: {e}"))?;
                record_failure(&conn, adapter_id, &err);
                report.adapters.push(AdapterRunReport {
                    adapter_id: adapter_id.into(),
                    fetched: 0,
                    skipped: None,
                    error: Some(err.to_string()),
                });
            }
        }
    }
    Ok(report)
}

fn known_source_ids(conn: &Connection, source: &str) -> Result<HashSet<String>, String> {
    let mut stmt = conn
        .prepare("SELECT source_id FROM listings WHERE source = ?1")
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(params![source], |row| row.get::<_, String>(0))
        .map_err(|e| e.to_string())?;
    rows.collect::<Result<_, _>>().map_err(|e| e.to_string())
}

/// Some(backoff_until) if the adapter is inside its backoff window.
fn backoff_active(conn: &Connection, adapter_id: &str) -> Option<String> {
    conn.query_row(
        "SELECT backoff_until FROM adapter_state
         WHERE adapter_id = ?1 AND backoff_until > datetime('now')",
        params![adapter_id],
        |row| row.get::<_, String>(0),
    )
    .ok()
}

fn record_success(conn: &Connection, adapter_id: &str) {
    let _ = conn.execute(
        "INSERT INTO adapter_state (adapter_id, status, consecutive_failures, last_success_at, backoff_until)
         VALUES (?1, 'ok', 0, datetime('now'), NULL)
         ON CONFLICT(adapter_id) DO UPDATE SET
            status = 'ok', consecutive_failures = 0,
            last_success_at = datetime('now'), backoff_until = NULL,
            status_detail = NULL",
        params![adapter_id],
    );
}

fn record_failure(conn: &Connection, adapter_id: &str, err: &AdapterError) {
    let failures: i64 = conn
        .query_row(
            "SELECT consecutive_failures FROM adapter_state WHERE adapter_id = ?1",
            params![adapter_id],
            |row| row.get(0),
        )
        .unwrap_or(0)
        + 1;

    // Blocks back off aggressively; transient network errors more gently.
    let base = match err {
        AdapterError::Blocked(_) => BACKOFF_BASE_SECS * 4,
        _ => BACKOFF_BASE_SECS,
    };
    let backoff_secs =
        (base.saturating_mul(1 << (failures - 1).min(10))).min(BACKOFF_CAP_SECS);
    let status = if failures >= DISABLE_AFTER_FAILURES { "disabled" } else { "degraded" };

    let _ = conn.execute(
        "INSERT INTO adapter_state
            (adapter_id, status, status_detail, consecutive_failures, last_error_at, backoff_until)
         VALUES (?1, ?2, ?3, ?4, datetime('now'), datetime('now', ?5))
         ON CONFLICT(adapter_id) DO UPDATE SET
            status = ?2, status_detail = ?3, consecutive_failures = ?4,
            last_error_at = datetime('now'), backoff_until = datetime('now', ?5)",
        params![
            adapter_id,
            status,
            err.to_string(),
            failures,
            format!("+{backoff_secs} seconds"),
        ],
    );
}

/// Download each listing's first image and attach its perceptual hash.
/// Best-effort: any failure leaves phash = None and the title/price
/// fallback hash still applies at ingest.
async fn enrich_with_phashes(listings: &mut [RawListing]) {
    let client = match reqwest::Client::builder().timeout(IMAGE_FETCH_TIMEOUT).build() {
        Ok(c) => c,
        Err(_) => return,
    };
    let hasher = image_hasher::HasherConfig::new().hash_size(8, 8).to_hasher();

    for listing in listings.iter_mut() {
        if listing.phash.is_some() {
            continue;
        }
        let Some(url) = listing.images.first() else { continue };
        let Ok(resp) = client.get(url).send().await else { continue };
        if !resp.status().is_success() {
            continue;
        }
        let Ok(bytes) = resp.bytes().await else { continue };
        if bytes.len() > IMAGE_MAX_BYTES {
            continue;
        }
        let Ok(img) = image::load_from_memory(&bytes) else { continue };
        listing.phash = Some(hasher.hash_image(&img).to_base64());
    }
}
