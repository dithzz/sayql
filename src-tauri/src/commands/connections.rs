use super::settings::{SettingsView, settings_view};
use crate::db::{self, ConnConfig, Kind};
use crate::error::AppResult;
use crate::schema::{self, Schema};
use crate::state::{AppState, LiveConnection};
use crate::store::secrets;
use serde::Serialize;
use std::path::Path;
use tauri::State;

const DEMO_ID: &str = "demo";

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InitView {
    connections: Vec<ConnConfig>,
    settings: SettingsView,
}

fn demo_connection(path: &Path) -> ConnConfig {
    ConnConfig {
        id: DEMO_ID.into(),
        name: "Demo store (SQLite)".into(),
        kind: Kind::Sqlite,
        host: String::new(),
        port: None,
        database: String::new(),
        user: String::new(),
        file: path.to_string_lossy().into_owned(),
        ssl: false,
    }
}

/// Startup: saved connections and settings. Adds the demo database on first launch.
#[tauri::command]
pub async fn init(state: State<'_, AppState>) -> AppResult<InitView> {
    let _guard = state.init_lock.lock().await;
    let mut settings = state.store.settings();
    if !settings.demo_added {
        db::ensure_demo(&state.demo_path).await?;
        let mut list = state.store.connections();
        list.insert(0, demo_connection(&state.demo_path));
        state.store.save_connections(&list)?;
        settings.demo_added = true;
        state.store.save_settings(&settings)?;
    }
    Ok(InitView { connections: state.store.connections(), settings: settings_view(&settings) })
}

/// Creates (empty id) or updates a connection. `password: None` keeps the saved password.
#[tauri::command]
pub fn save_connection(state: State<'_, AppState>, mut config: ConnConfig, password: Option<String>) -> AppResult<ConnConfig> {
    if config.id.is_empty() {
        config.id = uuid::Uuid::new_v4().to_string();
    }
    if let Some(pw) = password {
        secrets::set_password(&config.id, &pw)?;
    }
    let mut list = state.store.connections();
    match list.iter_mut().find(|c| c.id == config.id) {
        Some(existing) => *existing = config.clone(),
        None => list.push(config.clone()),
    }
    state.store.save_connections(&list)?;
    // The details may have changed: reconnect on next open.
    state.close(&config.id);
    Ok(config)
}

/// Removes the saved connection and its password. The database itself is not touched.
#[tauri::command]
pub fn delete_connection(state: State<'_, AppState>, id: String) -> AppResult<()> {
    let list: Vec<ConnConfig> = state.store.connections().into_iter().filter(|c| c.id != id).collect();
    state.store.save_connections(&list)?;
    let _ = secrets::set_password(&id, "");
    state.close(&id);
    Ok(())
}

#[tauri::command]
pub async fn test_connection(config: ConnConfig, password: Option<String>) -> AppResult<String> {
    let password = password.or_else(|| secrets::password(&config.id));
    let pool = db::connect(&config, password).await?;
    let schema = db::introspect(&pool, config.kind).await?;
    Ok(format!("Connected. Found {} tables and views.", schema.tables.len()))
}

/// Opens the connection (or reuses it) and returns its schema.
#[tauri::command]
pub async fn open_connection(state: State<'_, AppState>, id: String) -> AppResult<Schema> {
    match state.live(&id) {
        Ok(live) => Ok(live.schema.clone()),
        Err(_) => load_schema(&state, &id).await,
    }
}

/// Re-reads the schema, e.g. after tables were added.
#[tauri::command]
pub async fn refresh_schema(state: State<'_, AppState>, id: String) -> AppResult<Schema> {
    load_schema(&state, &id).await
}

async fn load_schema(state: &AppState, id: &str) -> AppResult<Schema> {
    let cfg = state.store.connection(id)?;
    let pool = match state.open_pool(id) {
        Some(pool) => pool,
        None => db::connect(&cfg, secrets::password(id)).await?,
    };
    let schema = db::introspect(&pool, cfg.kind).await?;
    let schema_text = schema::render_full(&schema);
    state.set_live(id, LiveConnection { pool, schema: schema.clone(), schema_text });
    Ok(schema)
}
