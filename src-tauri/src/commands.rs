use crate::adapters::{ebay, AdapterRegistry};
use crate::models::{AdapterStatus, Condition, Listing, SavedSearch};
use crate::poll;
use crate::settings;
use rusqlite::{params, Connection, Row};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Mutex;
use tauri::State;

pub struct AppState {
    pub db: Mutex<Connection>,
    pub registry: AdapterRegistry,
}

type CmdResult<T> = Result<T, String>;

fn lock_db<'a>(state: &'a State<'_, AppState>) -> CmdResult<std::sync::MutexGuard<'a, Connection>> {
    state.db.lock().map_err(|e| format!("db lock poisoned: {e}"))
}

// ---------- searches ----------

fn search_from_row(row: &Row) -> rusqlite::Result<SavedSearch> {
    Ok(SavedSearch {
        id: row.get("id")?,
        name: row.get("name")?,
        keywords: row.get("keywords")?,
        category: row.get("category")?,
        location_text: row.get("location_text")?,
        lat: row.get("lat")?,
        lng: row.get("lng")?,
        radius_km: row.get("radius_km")?,
        price_ceiling: row.get("price_ceiling")?,
        enabled: row.get("enabled")?,
        notify_score_threshold: row.get("notify_score_threshold")?,
        poll_interval_minutes: row.get("poll_interval_minutes")?,
    })
}

/// Shared with the poll layer and scheduler.
pub fn get_search(conn: &Connection, id: i64) -> rusqlite::Result<SavedSearch> {
    conn.query_row("SELECT * FROM searches WHERE id=?1", params![id], search_from_row)
}

#[tauri::command]
pub fn list_searches(state: State<'_, AppState>) -> CmdResult<Vec<SavedSearch>> {
    let conn = lock_db(&state)?;
    let mut stmt = conn
        .prepare("SELECT * FROM searches ORDER BY created_at DESC")
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], search_from_row)
        .map_err(|e| e.to_string())?;
    rows.collect::<Result<_, _>>().map_err(|e| e.to_string())
}

#[derive(Debug, Deserialize)]
pub struct SearchInput {
    pub name: String,
    pub keywords: String,
    pub category: Option<String>,
    pub location_text: String,
    pub lat: Option<f64>,
    pub lng: Option<f64>,
    pub radius_km: f64,
    pub price_ceiling: Option<f64>,
    #[serde(default)]
    pub poll_interval_minutes: Option<i64>,
}

#[tauri::command]
pub fn create_search(state: State<'_, AppState>, input: SearchInput) -> CmdResult<i64> {
    let conn = lock_db(&state)?;
    conn.execute(
        "INSERT INTO searches (name, keywords, category, location_text, lat, lng, radius_km,
            price_ceiling, poll_interval_minutes)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            input.name,
            input.keywords,
            input.category,
            input.location_text,
            input.lat,
            input.lng,
            input.radius_km,
            input.price_ceiling,
            input.poll_interval_minutes
        ],
    )
    .map_err(|e| e.to_string())?;
    Ok(conn.last_insert_rowid())
}

#[tauri::command]
pub fn update_search(state: State<'_, AppState>, search: SavedSearch) -> CmdResult<()> {
    let conn = lock_db(&state)?;
    conn.execute(
        "UPDATE searches SET name=?2, keywords=?3, category=?4, location_text=?5, lat=?6,
            lng=?7, radius_km=?8, price_ceiling=?9, enabled=?10, notify_score_threshold=?11,
            poll_interval_minutes=?12, updated_at=datetime('now')
         WHERE id=?1",
        params![
            search.id,
            search.name,
            search.keywords,
            search.category,
            search.location_text,
            search.lat,
            search.lng,
            search.radius_km,
            search.price_ceiling,
            search.enabled,
            search.notify_score_threshold,
            search.poll_interval_minutes
        ],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn delete_search(state: State<'_, AppState>, id: i64) -> CmdResult<()> {
    let conn = lock_db(&state)?;
    conn.execute("DELETE FROM searches WHERE id=?1", params![id])
        .map_err(|e| e.to_string())?;
    Ok(())
}

// ---------- polling ----------

/// Run all adapters for one saved search and ingest results. The heavy
/// lifting (isolation, backoff, phash enrichment) lives in poll.rs and is
/// shared with the background scheduler.
#[tauri::command]
pub async fn run_search(state: State<'_, AppState>, search_id: i64) -> CmdResult<poll::RunReport> {
    poll::run_poll_cycle(&state.db, &state.registry, search_id).await
}

// ---------- listings ----------

fn listing_from_row(row: &Row) -> rusqlite::Result<Listing> {
    let images_json: String = row.get("images")?;
    let condition: Option<String> = row.get("condition")?;
    Ok(Listing {
        id: row.get("id")?,
        search_id: row.get("search_id")?,
        source: row.get("source")?,
        source_id: row.get("source_id")?,
        source_url: row.get("source_url")?,
        title: row.get("title")?,
        description: row.get("description")?,
        price: row.get("price")?,
        currency: row.get("currency")?,
        location_text: row.get("location_text")?,
        lat: row.get("lat")?,
        lng: row.get("lng")?,
        images: serde_json::from_str(&images_json).unwrap_or_default(),
        posted_at: row.get("posted_at")?,
        fetched_at: row.get("fetched_at")?,
        condition: condition.as_deref().and_then(Condition::from_db_str),
        category_guess: row.get("category_guess")?,
        dedup_hash: row.get("dedup_hash")?,
        seen: row.get::<_, Option<bool>>("seen")?.unwrap_or(false),
        hidden: row.get::<_, Option<bool>>("hidden")?.unwrap_or(false),
        saved: row.get::<_, Option<bool>>("saved")?.unwrap_or(false),
    })
}

#[tauri::command]
pub fn list_listings(
    state: State<'_, AppState>,
    search_id: i64,
    include_hidden: bool,
) -> CmdResult<Vec<Listing>> {
    let conn = lock_db(&state)?;
    let sql = format!(
        "SELECT l.*, ua.seen, ua.hidden, ua.saved
         FROM listings l
         LEFT JOIN user_actions ua ON ua.listing_id = l.id
         WHERE l.search_id = ?1 {}
         ORDER BY l.fetched_at DESC, l.id DESC",
        if include_hidden { "" } else { "AND COALESCE(ua.hidden, 0) = 0" }
    );
    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(params![search_id], listing_from_row)
        .map_err(|e| e.to_string())?;
    rows.collect::<Result<_, _>>().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn set_listing_action(
    state: State<'_, AppState>,
    listing_id: i64,
    field: String,
    value: bool,
) -> CmdResult<()> {
    if !["seen", "hidden", "saved"].contains(&field.as_str()) {
        return Err(format!("invalid action field: {field}"));
    }
    let conn = lock_db(&state)?;
    let sql = format!(
        "INSERT INTO user_actions (listing_id, {field}, updated_at) VALUES (?1, ?2, datetime('now'))
         ON CONFLICT(listing_id) DO UPDATE SET {field} = ?2, updated_at = datetime('now')"
    );
    conn.execute(&sql, params![listing_id, value])
        .map_err(|e| e.to_string())?;
    Ok(())
}

// ---------- settings & adapters ----------

#[tauri::command]
pub fn get_settings(state: State<'_, AppState>) -> CmdResult<HashMap<String, String>> {
    let conn = lock_db(&state)?;
    settings::get_all(&conn).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn set_setting(state: State<'_, AppState>, key: String, value: String) -> CmdResult<()> {
    let conn = lock_db(&state)?;
    settings::set(&conn, &key, &value).map_err(|e| e.to_string())
}

#[derive(Debug, Serialize)]
pub struct AdapterInfo {
    pub id: String,
    pub display_name: String,
    pub status: AdapterStatus,
}

/// Trait-level health (e.g. eBay NotConfigured) merged with runtime state
/// from adapter_state (backoff/disable bookkeeping owned by the poll layer).
/// Runtime trouble wins over a trait-level "Ok".
#[tauri::command]
pub fn adapter_health(state: State<'_, AppState>) -> CmdResult<Vec<AdapterInfo>> {
    let conn = lock_db(&state)?;
    Ok(state
        .registry
        .adapters()
        .iter()
        .map(|a| {
            let mut status = a.health();
            if matches!(status, AdapterStatus::Ok) {
                let db_state: Option<(String, Option<String>)> = conn
                    .query_row(
                        "SELECT status, status_detail FROM adapter_state
                         WHERE adapter_id = ?1 AND backoff_until > datetime('now')",
                        params![a.id()],
                        |row| Ok((row.get(0)?, row.get(1)?)),
                    )
                    .ok();
                if let Some((db_status, detail)) = db_state {
                    let reason = detail.unwrap_or_else(|| "recent failures".into());
                    status = if db_status == "disabled" {
                        AdapterStatus::Disabled { reason }
                    } else {
                        AdapterStatus::Degraded { reason }
                    };
                }
            }
            AdapterInfo {
                id: a.id().to_string(),
                display_name: a.display_name().to_string(),
                status,
            }
        })
        .collect())
}

// ---------- eBay credentials (OS keychain, never the DB) ----------

#[tauri::command]
pub fn set_ebay_credentials(client_id: String, client_secret: String) -> CmdResult<()> {
    ebay::store_credentials(client_id.trim(), client_secret.trim())
}

#[tauri::command]
pub fn ebay_credentials_status() -> bool {
    ebay::stored_credentials().is_some()
}

/// Fetches an OAuth token with the stored keys. Success message only —
/// secrets and tokens are never returned to the UI.
#[tauri::command]
pub async fn test_ebay_connection() -> CmdResult<String> {
    let adapter = ebay::EbayAdapter::new();
    adapter
        .fetch_token()
        .await
        .map(|_| "Connected: eBay accepted your API keys.".to_string())
        .map_err(|e| e.to_string())
}
