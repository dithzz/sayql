mod ai;
mod db;
mod schema;
mod store;

use db::{ConnConfig, DbPool, Kind, QueryResult};
use schema::Schema;
use serde::Serialize;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use store::{Settings, Store};
use tauri::async_runtime::JoinHandle;
use tauri::{AppHandle, Manager, State};

const DEMO_ID: &str = "demo";

struct Live {
    pool: DbPool,
    schema: Schema,
    /// Rendered once per schema load so the prompt prefix is byte-identical (cacheable).
    schema_text: String,
}

struct AppState {
    store: Store,
    demo_path: std::path::PathBuf,
    live: Mutex<HashMap<String, Arc<Live>>>,
    generation: Mutex<Option<JoinHandle<()>>>,
    http: reqwest::Client,
    /// Serializes `init` so a double call (React StrictMode in dev) can't add the demo twice.
    init_lock: tokio::sync::Mutex<()>,
}

impl AppState {
    fn live(&self, id: &str) -> Result<Arc<Live>, String> {
        self.live
            .lock()
            .unwrap()
            .get(id)
            .cloned()
            .ok_or_else(|| "Not connected. Open the connection first.".to_string())
    }

    fn config(&self, id: &str) -> Result<ConnConfig, String> {
        self.store
            .connections()
            .into_iter()
            .find(|c| c.id == id)
            .ok_or_else(|| "Connection not found.".to_string())
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SettingsView {
    model: String,
    has_api_key: bool,
    workspace_id: String,
    models: Vec<ModelOption>,
}

#[derive(Serialize)]
struct ModelOption {
    id: &'static str,
    label: &'static str,
}

fn settings_view(s: &Settings) -> SettingsView {
    SettingsView {
        model: s.model.clone(),
        has_api_key: store::api_key().is_some(),
        workspace_id: s.workspace_id.clone(),
        models: ai::MODELS.iter().map(|(id, label)| ModelOption { id, label }).collect(),
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct InitView {
    connections: Vec<ConnConfig>,
    settings: SettingsView,
}

#[tauri::command]
async fn init(state: State<'_, AppState>) -> Result<InitView, String> {
    let _guard = state.init_lock.lock().await;
    let mut settings = state.store.settings();
    if !settings.demo_added {
        db::ensure_demo(&state.demo_path).await?;
        let mut list = state.store.connections();
        list.insert(
            0,
            ConnConfig {
                id: DEMO_ID.into(),
                name: "Demo store (SQLite)".into(),
                kind: Kind::Sqlite,
                host: String::new(),
                port: None,
                database: String::new(),
                user: String::new(),
                file: state.demo_path.to_string_lossy().into_owned(),
                ssl: false,
            },
        );
        state.store.save_connections(&list)?;
        settings.demo_added = true;
        state.store.save_settings(&settings)?;
    }
    Ok(InitView { connections: state.store.connections(), settings: settings_view(&settings) })
}

#[tauri::command]
fn save_connection(
    state: State<'_, AppState>,
    mut config: ConnConfig,
    password: Option<String>,
) -> Result<ConnConfig, String> {
    if config.id.is_empty() {
        config.id = uuid::Uuid::new_v4().to_string();
    }
    if let Some(pw) = password {
        store::set_password(&config.id, &pw)?;
    }
    let mut list = state.store.connections();
    match list.iter_mut().find(|c| c.id == config.id) {
        Some(existing) => *existing = config.clone(),
        None => list.push(config.clone()),
    }
    state.store.save_connections(&list)?;
    // Settings may have changed; reconnect on next open.
    state.live.lock().unwrap().remove(&config.id);
    Ok(config)
}

#[tauri::command]
fn delete_connection(state: State<'_, AppState>, id: String) -> Result<(), String> {
    let list: Vec<ConnConfig> = state.store.connections().into_iter().filter(|c| c.id != id).collect();
    state.store.save_connections(&list)?;
    let _ = store::set_password(&id, "");
    state.live.lock().unwrap().remove(&id);
    Ok(())
}

#[tauri::command]
async fn test_connection(config: ConnConfig, password: Option<String>) -> Result<String, String> {
    let pw = password.or_else(|| store::password(&config.id));
    let pool = db::connect(&config, pw).await?;
    let schema = db::introspect(&pool, config.kind).await?;
    Ok(format!("Connected. Found {} tables and views.", schema.tables.len()))
}

async fn load(state: &AppState, id: &str) -> Result<Schema, String> {
    let cfg = state.config(id)?;
    let existing = state.live.lock().unwrap().get(id).map(|l| l.pool.clone());
    let pool = match existing {
        Some(p) => p,
        None => db::connect(&cfg, store::password(id)).await?,
    };
    let schema = db::introspect(&pool, cfg.kind).await?;
    let schema_text = schema::render_full(&schema);
    let live = Arc::new(Live { pool, schema: schema.clone(), schema_text });
    state.live.lock().unwrap().insert(id.to_string(), live);
    Ok(schema)
}

#[tauri::command]
async fn open_connection(state: State<'_, AppState>, id: String) -> Result<Schema, String> {
    if let Ok(live) = state.live(&id) {
        return Ok(live.schema.clone());
    }
    load(&state, &id).await
}

#[tauri::command]
async fn refresh_schema(state: State<'_, AppState>, id: String) -> Result<Schema, String> {
    load(&state, &id).await
}

#[tauri::command]
async fn run_query(
    state: State<'_, AppState>,
    id: String,
    sql: String,
    allow_write: bool,
) -> Result<QueryResult, String> {
    let live = state.live(&id)?;
    let read_only = db::is_read_only(&sql);
    if !read_only && !allow_write {
        // The UI shows a confirmation and calls again with allow_write = true.
        return Err("WRITE_CONFIRM".into());
    }
    db::run_query(&live.pool, &sql, read_only).await
}

#[tauri::command]
fn generate_sql(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
    question: String,
    req_id: u64,
) -> Result<(), String> {
    let live = state.live(&id)?;
    let api_key = store::api_key().ok_or("Add your Claude API key in Settings to generate SQL.")?;
    let settings = state.store.settings();
    let request = ai::GenRequest {
        req_id,
        api_key,
        workspace_id: Some(settings.workspace_id).filter(|w| !w.is_empty()),
        model: settings.model,
        dialect: live.schema.dialect.clone(),
        schema_text: schema::schema_for_prompt(&live.schema, &live.schema_text, &question),
        question,
    };
    let http = state.http.clone();
    let handle = tauri::async_runtime::spawn(ai::stream_sql(app, http, request));
    // Only the newest request matters while the person is typing.
    if let Some(prev) = state.generation.lock().unwrap().replace(handle) {
        prev.abort();
    }
    Ok(())
}

#[tauri::command]
fn cancel_generation(state: State<'_, AppState>) {
    if let Some(prev) = state.generation.lock().unwrap().take() {
        prev.abort();
    }
}

#[tauri::command]
fn save_settings(
    state: State<'_, AppState>,
    model: String,
    api_key: Option<String>,
    workspace_id: String,
) -> Result<SettingsView, String> {
    if !ai::MODELS.iter().any(|(id, _)| *id == model) {
        return Err(format!("Unknown model {model}"));
    }
    if let Some(k) = api_key {
        store::set_api_key(&k)?;
    }
    let mut s = state.store.settings();
    s.model = model;
    s.workspace_id = workspace_id.trim().to_string();
    state.store.save_settings(&s)?;
    Ok(settings_view(&s))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let config_dir = app.path().app_config_dir()?;
            let data_dir = app.path().app_data_dir()?;
            let http = reqwest::Client::builder()
                // Keep connections to the API warm between keystrokes.
                .pool_idle_timeout(std::time::Duration::from_secs(300))
                .tcp_keepalive(std::time::Duration::from_secs(30))
                .build()?;
            app.manage(AppState {
                store: Store::new(config_dir),
                demo_path: data_dir.join("demo.sqlite"),
                live: Mutex::new(HashMap::new()),
                generation: Mutex::new(None),
                http,
                init_lock: tokio::sync::Mutex::new(()),
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            init,
            save_connection,
            delete_connection,
            test_connection,
            open_connection,
            refresh_schema,
            run_query,
            generate_sql,
            cancel_generation,
            save_settings,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
