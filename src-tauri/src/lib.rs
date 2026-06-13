pub mod adapters;
pub mod commands;
pub mod db;
pub mod models;
pub mod pipeline;
pub mod poll;
pub mod scheduler;
pub mod settings;

use adapters::facebook::{FacebookAdapter, FbShared};
use adapters::{craigslist::CraigslistAdapter, ebay::EbayAdapter, AdapterRegistry};
use commands::AppState;
use std::sync::{Arc, Mutex};
use tauri::Manager;

/// Where the FB sidecar (Node + Playwright) lives. Overridable for packaged
/// builds; in dev it sits beside src-tauri in the repo.
fn sidecar_dir() -> std::path::PathBuf {
    if let Ok(dir) = std::env::var("ZINGER_SIDECAR_DIR") {
        return dir.into();
    }
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("sidecar")
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let db_path = app
                .path()
                .app_data_dir()
                .expect("no app data dir")
                .join("zinger.db");
            let conn = db::open(&db_path)?;

            let all_settings = settings::get_all(&conn).unwrap_or_default();
            let craigslist_site = all_settings
                .get("craigslist_site")
                .cloned()
                .filter(|s| !s.trim().is_empty());

            // FB shared state: profile lives beside zinger.db; flags restored
            // from settings. The sidecar/Chromium are NOT touched here — they
            // load only when the adapter actually runs.
            let profile_dir = app
                .path()
                .app_data_dir()
                .expect("no app data dir")
                .join("fb-profile");
            let fb = Arc::new(FbShared::new(profile_dir, sidecar_dir()));
            {
                let mut s = fb.state.lock().unwrap();
                s.enabled = all_settings.get("fb_enabled").map(|v| v == "1").unwrap_or(false);
                s.tos_accepted =
                    all_settings.get("fb_tos_accepted").map(|v| v == "1").unwrap_or(false);
                s.logged_in = all_settings.get("fb_logged_in").map(|v| v == "1").unwrap_or(false);
                if let Some(n) = all_settings.get("fb_max_pages").and_then(|v| v.parse().ok()) {
                    *fb.max_pages.lock().unwrap() = n;
                }
            }

            let mut registry = AdapterRegistry::new();
            registry.register(Box::new(CraigslistAdapter::new(craigslist_site)));
            registry.register(Box::new(EbayAdapter::new()));
            registry.register(Box::new(FacebookAdapter::new(fb.clone())));
            // Mock data would pollute real searches; keep it to dev builds.
            #[cfg(debug_assertions)]
            registry.register(Box::new(adapters::mock::MockAdapter::new()));

            app.manage(AppState {
                db: Mutex::new(conn),
                registry,
                fb,
            });
            scheduler::start(app.handle().clone());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::list_searches,
            commands::create_search,
            commands::update_search,
            commands::delete_search,
            commands::run_search,
            commands::list_listings,
            commands::set_listing_action,
            commands::get_settings,
            commands::set_setting,
            commands::adapter_health,
            commands::set_ebay_credentials,
            commands::ebay_credentials_status,
            commands::test_ebay_connection,
            commands::fb_status,
            commands::fb_accept_tos,
            commands::fb_set_enabled,
            commands::fb_set_max_pages,
            commands::fb_install_chromium,
            commands::fb_login,
            commands::fb_refresh_chromium,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
