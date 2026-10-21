//! Sayql backend: wires the Tauri app together. Logic lives in the modules below.

mod ai;
mod commands;
mod db;
mod error;
pub mod mcp;
mod schema;
mod state;
mod store;

use commands::{connections, generation, query, scripts, settings, updates};
use state::AppState;
use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(|app| {
            let state = AppState::new(app.path().app_config_dir()?, app.path().app_data_dir()?)?;
            app.manage(state);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            connections::init,
            connections::save_connection,
            connections::delete_connection,
            connections::test_connection,
            connections::open_database,
            connections::refresh_schema,
            connections::list_databases,
            query::run_query,
            scripts::list_scripts,
            scripts::save_script,
            scripts::delete_script,
            generation::generate_sql,
            generation::cancel_generation,
            settings::save_settings,
            settings::list_models,
            settings::mcp_setup,
            updates::check_update,
            updates::install_update,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
