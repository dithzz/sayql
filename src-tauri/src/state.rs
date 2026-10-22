//! Process-wide state shared by the Tauri commands.

use crate::db::{self, DbPool};
use crate::error::AppResult;
use crate::schema::{self, Schema};
use crate::store::{Store, secrets};
use crate::voice::Voice;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;
use tauri::async_runtime::JoinHandle;
use tokio_util::sync::CancellationToken;

/// Locks `m`, recovering the data if another thread panicked while holding it. Every map
/// guarded here stays valid between operations, so a panic elsewhere shouldn't take the
/// whole app down with it.
pub(crate) fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

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
    /// The running AI generation per slot (one slot per editor tab).
    generations: Mutex<HashMap<String, JoinHandle<()>>>,
    /// Running queries the UI can stop, by the id it gave each run.
    queries: Mutex<HashMap<String, CancellationToken>>,
    pub voice: Voice,
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
            voice: Voice::new(&data_dir),
            http,
            init_lock: tokio::sync::Mutex::new(()),
            pools: Mutex::new(HashMap::new()),
            schemas: Mutex::new(HashMap::new()),
            generations: Mutex::new(HashMap::new()),
            queries: Mutex::new(HashMap::new()),
        })
    }

    pub fn pool(&self, target: &Target) -> Option<DbPool> {
        lock(&self.pools).get(target).cloned()
    }

    pub fn set_pool(&self, target: Target, pool: DbPool) {
        lock(&self.pools).insert(target, pool);
    }

    /// The pool of an open target, for running queries.
    pub fn open_pool(&self, target: &Target) -> AppResult<DbPool> {
        self.pool(target).ok_or_else(|| "Not connected. Open the database first.".into())
    }

    pub fn schema(&self, target: &Target) -> AppResult<Arc<LoadedSchema>> {
        lock(&self.schemas).get(target).cloned().ok_or_else(|| "Not connected. Open the database first.".into())
    }

    pub fn set_schema(&self, target: Target, loaded: LoadedSchema) {
        lock(&self.schemas).insert(target, Arc::new(loaded));
    }

    /// Forgets every open database of a connection; the next open reconnects.
    pub fn close(&self, connection_id: &str) {
        lock(&self.pools).retain(|t, _| t.connection_id != connection_id);
        lock(&self.schemas).retain(|t, _| t.connection_id != connection_id);
    }

    /// Makes `handle` the slot's AI generation, aborting the one it replaces.
    pub fn replace_generation(&self, slot: &str, handle: JoinHandle<()>) {
        if let Some(prev) = lock(&self.generations).insert(slot.to_string(), handle) {
            prev.abort();
        }
    }

    /// A token that `cancel_query(run_id)` fires, for the query about to run.
    pub fn start_query(&self, run_id: &str) -> CancellationToken {
        let token = CancellationToken::new();
        lock(&self.queries).insert(run_id.to_string(), token.clone());
        token
    }

    pub fn finish_query(&self, run_id: &str) {
        lock(&self.queries).remove(run_id);
    }

    pub fn cancel_query(&self, run_id: &str) {
        if let Some(token) = lock(&self.queries).remove(run_id) {
            token.cancel();
        }
    }

    pub fn cancel_generation(&self, slot: &str) {
        if let Some(prev) = lock(&self.generations).remove(slot) {
            prev.abort();
        }
    }

    /// The pool for `target`, connecting if it isn't open yet.
    pub async fn target_pool(&self, target: &Target) -> AppResult<DbPool> {
        if let Some(pool) = self.pool(target) {
            return Ok(pool);
        }
        let id = &target.connection_id;
        let cfg = self.store.connection(id)?;
        let pool = match &target.database {
            None => db::connect(&cfg, secrets::password(id)).await?,
            Some(database) => {
                let base = Box::pin(self.target_pool(&Target::default_of(id))).await?;
                db::open_database(&base, &cfg, secrets::password(id), database).await?
            }
        };
        self.set_pool(target.clone(), pool.clone());
        Ok(pool)
    }

    /// Reads `target`'s schema and caches it, with its prompt text.
    pub async fn load_schema(&self, target: &Target) -> AppResult<Schema> {
        let kind = self.store.connection(&target.connection_id)?.kind;
        let pool = self.target_pool(target).await?;
        let schema = db::introspect(&pool, kind).await?;
        let schema_text = schema::render_full(&schema);
        self.set_schema(target.clone(), LoadedSchema { schema: schema.clone(), schema_text });
        Ok(schema)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lock_recovers_from_a_poisoned_mutex() {
        let m = Arc::new(Mutex::new(vec![1]));
        let m2 = Arc::clone(&m);
        let _ = std::thread::spawn(move || {
            let mut g = m2.lock().unwrap();
            g.push(2);
            panic!("poison it");
        })
        .join();
        assert!(m.is_poisoned());
        let mut g = lock(&m);
        g.push(3);
        assert_eq!(*g, vec![1, 2, 3]);
    }
}
