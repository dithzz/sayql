use super::settings::{SettingsView, settings_view};
use crate::db::{self, ConnConfig, DatabaseList, DbPool, Kind};
use crate::error::AppResult;
use crate::schema::{self, Schema};
use crate::state::{AppState, LoadedSchema, Target};
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
        trust_cert: false,
    }
}

/// Startup: saved connections and settings. Adds the demo database on first launch.
#[tauri::command]
pub async fn init(state: State<'_, AppState>) -> AppResult<InitView> {
    let _guard = state.init_lock.lock().await;
    // Read every saved file now, so an unreadable one is reported on the startup screen
    // (with where it was moved) instead of quietly showing an empty list later.
    state.store.scripts()?;
    state.store.skills()?;
    let mut settings = state.store.settings()?;
    if !settings.demo_added {
        db::ensure_demo(&state.demo_path).await?;
        let mut list = state.store.connections()?;
        list.insert(0, demo_connection(&state.demo_path));
        state.store.save_connections(&list)?;
        settings.demo_added = true;
        state.store.save_settings(&settings)?;
    }
    Ok(InitView { connections: state.store.connections()?, settings: settings_view(&settings) })
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
    let mut list = state.store.connections()?;
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
    let list: Vec<ConnConfig> = state.store.connections()?.into_iter().filter(|c| c.id != id).collect();
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

/// Opens one database of a connection (or reuses it) and returns its schema.
/// `database: None` opens the connection's default database.
#[tauri::command]
pub async fn open_database(state: State<'_, AppState>, id: String, database: Option<String>) -> AppResult<Schema> {
    let target = Target::new(id, database);
    match state.schema(&target) {
        Ok(loaded) => Ok(loaded.schema.clone()),
        Err(_) => load_schema(&state, &target).await,
    }
}

/// Re-reads the schema, e.g. after tables were added.
#[tauri::command]
pub async fn refresh_schema(state: State<'_, AppState>, id: String, database: Option<String>) -> AppResult<Schema> {
    load_schema(&state, &Target::new(id, database)).await
}

/// The databases on the connection's server; `None` for SQLite.
#[tauri::command]
pub async fn list_databases(state: State<'_, AppState>, id: String) -> AppResult<Option<DatabaseList>> {
    let pool = target_pool(&state, &Target::default_of(&id)).await?;
    db::list_databases(&pool).await
}

/// The pool for `target`, connecting if it isn't open yet.
pub(crate) async fn target_pool(state: &AppState, target: &Target) -> AppResult<DbPool> {
    if let Some(pool) = state.pool(target) {
        return Ok(pool);
    }
    let id = &target.connection_id;
    let cfg = state.store.connection(id)?;
    let pool = match &target.database {
        None => db::connect(&cfg, secrets::password(id)).await?,
        Some(database) => {
            let base = Box::pin(target_pool(state, &Target::default_of(id))).await?;
            db::open_database(&base, &cfg, secrets::password(id), database).await?
        }
    };
    state.set_pool(target.clone(), pool.clone());
    Ok(pool)
}

/// Reads `target`'s schema and caches it, with its prompt text, in `state`.
pub(crate) async fn load_schema(state: &AppState, target: &Target) -> AppResult<Schema> {
    let kind = state.store.connection(&target.connection_id)?.kind;
    let pool = target_pool(state, target).await?;
    let schema = db::introspect(&pool, kind).await?;
    let schema_text = schema::render_full(&schema);
    state.set_schema(target.clone(), LoadedSchema { schema: schema.clone(), schema_text });
    Ok(schema)
}
