//! SayQL backend: wires the Tauri app together. Logic lives in the modules below.

mod ai;
mod commands;
mod db;
mod error;
pub mod handoff;
pub mod mcp;
mod readonly;
mod schema;
mod state;
mod store;
mod team;
mod voice;

use commands::{agent, app, connections, generation, handoff as handoffs, query, scripts, settings, skills, team as teams, updates};
use state::AppState;
use tauri::Manager;
use tauri_plugin_deep_link::DeepLinkExt;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        // First: a second launch (e.g. from a sayql:// link) hands its link to this one.
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| handoffs::focus_main(app)))
        .plugin(tauri_plugin_deep_link::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(|app| {
            let state = AppState::new(app.path().app_config_dir()?, app.path().app_data_dir()?)?;
            app.manage(state);
            app.manage(handoffs::PendingHandoffs::default());
            // Installers register sayql:// on Windows and Linux; this covers AppImages and dev builds.
            #[cfg(any(windows, target_os = "linux"))]
            let _ = app.deep_link().register_all();
            let handle = app.handle().clone();
            app.deep_link().on_open_url(move |e| handoffs::receive(&handle, e.urls().into_iter().map(String::from)));
            if let Ok(Some(urls)) = app.deep_link().get_current() {
                handoffs::receive(app.handle(), urls.into_iter().map(String::from));
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            app::app_info,
            connections::init,
            connections::save_connection,
            connections::delete_connection,
            connections::test_connection,
            connections::open_database,
            connections::refresh_schema,
            connections::list_databases,
            query::run_query,
            query::cancel_query,
            scripts::list_scripts,
            scripts::save_script,
            scripts::delete_script,
            skills::list_skills,
            skills::save_skill,
            skills::delete_skill,
            generation::generate_sql,
            generation::cancel_generation,
            agent::start_agent,
            agent::stop_agent,
            agent::reply_agent,
            agent::agent_access,
            agent::save_agent_access,
            agent::list_notes,
            agent::save_note,
            agent::delete_note,
            settings::save_agent_settings,
            settings::choose_model,
            handoffs::take_handoffs,
            settings::save_settings,
            settings::list_models,
            settings::finish_onboarding,
            settings::mcp_setup,
            settings::mcp_activity,
            settings::mcp_apps,
            settings::set_mcp_app_blocked,
            settings::clear_mcp_activity,
            updates::check_update,
            updates::install_update,
            teams::team_sign_in,
            teams::team_status,
            teams::team_sign_out,
            teams::team_connections,
            teams::team_ai,
            commands::voice::voice_status,
            commands::voice::voice_download,
            commands::voice::voice_cancel_download,
            commands::voice::voice_delete_model,
            commands::voice::voice_start,
            commands::voice::voice_stop,
            commands::voice::voice_cancel,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
