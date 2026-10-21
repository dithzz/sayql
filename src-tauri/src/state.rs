//! Process-wide state shared by the Tauri commands.

use crate::db::DbPool;
use crate::error::AppResult;
use crate::schema::Schema;
use crate::store::Store;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::async_runtime::JoinHandle;

/// One database on a saved connection. `database: None` is the connection's default
/// database (the one in its settings, or the server's default for the login).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Target {
    pub connection_id: String,
    pub database: Option<String>,
}

impl Target {
    pub fn new(connection_id: String, database: Option<String>) -> Self {
        Target { connection_id, database: database.filter(|d| !d.trim().is_empty()) }
    }

    pub fn default_of(connection_id: &str) -> Self {
        Target { connection_id: connection_id.to_string(), database: None }
    }
}

/// A schema read from a target.
pub struct LoadedSchema {
    pub schema: Schema,
    /// Rendered once per load so the prompt prefix is byte-identical (cacheable).
    pub schema_text: String,
}

pub struct AppState {
    pub store: Store,
    pub demo_path: PathBuf,
    pub http: reqwest::Client,
    /// Serializes `init` so a double call (React StrictMode in dev) can't add the demo twice.
    pub init_lock: tokio::sync::Mutex<()>,
    pools: Mutex<HashMap<Target, DbPool>>,
    schemas: Mutex<HashMap<Target, Arc<LoadedSchema>>>,
    generation: Mutex<Option<JoinHandle<()>>>,
}

impl AppState {
    pub fn new(config_dir: PathBuf, data_dir: PathBuf) -> AppResult<Self> {
        let http = reqwest::Client::builder()
            // Keep connections to the API warm between keystrokes.
            .pool_idle_timeout(Duration::from_secs(300))
            .tcp_keepalive(Duration::from_secs(30))
            .build()
            .map_err(|e| format!("Could not set up networking: {e}"))?;
        Ok(AppState {
            store: Store::new(config_dir),
            demo_path: data_dir.join("demo.sqlite"),
            http,
            init_lock: tokio::sync::Mutex::new(()),
            pools: Mutex::new(HashMap::new()),
            schemas: Mutex::new(HashMap::new()),
            generation: Mutex::new(None),
        })
    }

    pub fn pool(&self, target: &Target) -> Option<DbPool> {
        self.pools.lock().unwrap().get(target).cloned()
    }

    pub fn set_pool(&self, target: Target, pool: DbPool) {
        self.pools.lock().unwrap().insert(target, pool);
    }

    /// The pool of an open target, for running queries.
    pub fn open_pool(&self, target: &Target) -> AppResult<DbPool> {
        self.pool(target).ok_or_else(|| "Not connected. Open the database first.".into())
    }

    pub fn schema(&self, target: &Target) -> AppResult<Arc<LoadedSchema>> {
        self.schemas
            .lock()
            .unwrap()
            .get(target)
            .cloned()
            .ok_or_else(|| "Not connected. Open the database first.".into())
    }

    pub fn set_schema(&self, target: Target, loaded: LoadedSchema) {
        self.schemas.lock().unwrap().insert(target, Arc::new(loaded));
    }

    /// Forgets every open database of a connection; the next open reconnects.
    pub fn close(&self, connection_id: &str) {
        self.pools.lock().unwrap().retain(|t, _| t.connection_id != connection_id);
        self.schemas.lock().unwrap().retain(|t, _| t.connection_id != connection_id);
    }

    /// Makes `handle` the current AI generation, aborting the previous one.
    pub fn replace_generation(&self, handle: JoinHandle<()>) {
        if let Some(prev) = self.generation.lock().unwrap().replace(handle) {
            prev.abort();
        }
    }

    pub fn cancel_generation(&self) {
        if let Some(prev) = self.generation.lock().unwrap().take() {
            prev.abort();
        }
    }
}
