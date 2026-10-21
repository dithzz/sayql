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

/// An open connection and the schema read from it.
pub struct LiveConnection {
    pub pool: DbPool,
    pub schema: Schema,
    /// Rendered once per schema load so the prompt prefix is byte-identical (cacheable).
    pub schema_text: String,
}

pub struct AppState {
    pub store: Store,
    pub demo_path: PathBuf,
    pub http: reqwest::Client,
    /// Serializes `init` so a double call (React StrictMode in dev) can't add the demo twice.
    pub init_lock: tokio::sync::Mutex<()>,
    live: Mutex<HashMap<String, Arc<LiveConnection>>>,
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
            live: Mutex::new(HashMap::new()),
            generation: Mutex::new(None),
        })
    }

    pub fn live(&self, id: &str) -> AppResult<Arc<LiveConnection>> {
        self.live
            .lock()
            .unwrap()
            .get(id)
            .cloned()
            .ok_or_else(|| "Not connected. Open the connection first.".into())
    }

    /// The pool of an already-open connection, to reuse when reloading its schema.
    pub fn open_pool(&self, id: &str) -> Option<DbPool> {
        self.live.lock().unwrap().get(id).map(|l| l.pool.clone())
    }

    pub fn set_live(&self, id: &str, live: LiveConnection) {
        self.live.lock().unwrap().insert(id.to_string(), Arc::new(live));
    }

    /// Forgets an open connection; the next open reconnects.
    pub fn close(&self, id: &str) {
        self.live.lock().unwrap().remove(id);
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
