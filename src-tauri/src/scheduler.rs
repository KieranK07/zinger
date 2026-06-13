//! Background polling. Ticks once a minute; a search is due when its
//! interval (per-search override or the global setting) has elapsed since
//! its last run, give or take ±20% jitter so traffic never looks like a
//! metronome. Due times live in memory: a restart just re-polls early, and
//! ingest dedups whatever comes back twice.

use crate::commands::AppState;
use crate::poll;
use rand::RngExt;
use std::collections::HashMap;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager};

const TICK: Duration = Duration::from_secs(60);
const DEFAULT_INTERVAL_MINUTES: i64 = 30;

pub fn start(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        // First tick after a short warm-up rather than at launch, so app
        // startup isn't competing with a poll burst.
        tokio::time::sleep(Duration::from_secs(15)).await;
        let mut next_due: HashMap<i64, Instant> = HashMap::new();
        loop {
            run_due_searches(&app, &mut next_due).await;
            tokio::time::sleep(TICK).await;
        }
    });
}

/// One scheduler pass: poll every enabled search whose due time has arrived.
async fn run_due_searches(app: &AppHandle, next_due: &mut HashMap<i64, Instant>) {
    let state = app.state::<AppState>();

    // (search_id, interval_minutes) under one short DB lock.
    let searches: Vec<(i64, i64)> = {
        let Ok(conn) = state.db.lock() else { return };
        let global: i64 = crate::settings::get_all(&conn)
            .ok()
            .and_then(|s| s.get("poll_interval_minutes")?.parse().ok())
            .unwrap_or(DEFAULT_INTERVAL_MINUTES);
        let Ok(mut stmt) =
            conn.prepare("SELECT id, poll_interval_minutes FROM searches WHERE enabled = 1")
        else {
            return;
        };
        let Ok(rows) = stmt.query_map([], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, Option<i64>>(1)?))
        }) else {
            return;
        };
        rows.filter_map(|r| r.ok())
            .map(|(id, per_search)| (id, per_search.unwrap_or(global).max(1)))
            .collect()
    };

    // Drop schedule entries for deleted/disabled searches.
    next_due.retain(|id, _| searches.iter().any(|(sid, _)| sid == id));

    let now = Instant::now();
    for (search_id, interval_minutes) in searches {
        let due = next_due.get(&search_id).copied().unwrap_or(now);
        if due > now {
            continue;
        }

        let report = poll::run_poll_cycle(&state.db, &state.registry, search_id).await;

        // Reschedule regardless of outcome; adapter-level backoff already
        // throttles failing sources, no need to double-penalize the search.
        let jitter = rand::rng().random_range(0.8..=1.2);
        let next = Duration::from_secs_f64(interval_minutes as f64 * 60.0 * jitter);
        next_due.insert(search_id, Instant::now() + next);

        match report {
            Ok(report) => {
                let _ = app.emit("poll-completed", &report);
            }
            Err(e) => {
                eprintln!("scheduler: poll cycle for search {search_id} failed: {e}");
            }
        }
    }
}
