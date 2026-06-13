pub mod adapters;
pub mod commands;
pub mod db;
pub mod models;
pub mod pipeline;
pub mod poll;
pub mod scheduler;
pub mod settings;

use adapters::{craigslist::CraigslistAdapter, ebay::EbayAdapter, AdapterRegistry};
use commands::AppState;
use std::sync::Mutex;
use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let db_path = app
                .path()
                .app_data_dir()
                .expect("no app data dir")
                .join("nexus.db");
            let conn = db::open(&db_path)?;

            let craigslist_site = settings::get_all(&conn)
                .ok()
                .and_then(|s| s.get("craigslist_site").cloned())
                .filter(|s| !s.trim().is_empty());

            let mut registry = AdapterRegistry::new();
            registry.register(Box::new(CraigslistAdapter::new(craigslist_site)));
            registry.register(Box::new(EbayAdapter::new()));
            // Mock data would pollute real searches; keep it to dev builds.
            #[cfg(debug_assertions)]
            registry.register(Box::new(adapters::mock::MockAdapter::new()));

            app.manage(AppState {
                db: Mutex::new(conn),
                registry,
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
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
