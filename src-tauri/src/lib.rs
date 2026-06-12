pub mod adapters;
pub mod commands;
pub mod db;
pub mod models;
pub mod pipeline;
pub mod settings;

use adapters::{mock::MockAdapter, AdapterRegistry};
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

            let mut registry = AdapterRegistry::new();
            registry.register(Box::new(MockAdapter::new()));
            // M2: registry.register(Box::new(CraigslistAdapter::new()));
            // M2: registry.register(Box::new(EbayAdapter::new()));

            app.manage(AppState {
                db: Mutex::new(conn),
                registry,
            });
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
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
