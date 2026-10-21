//! Saved connections and settings, as JSON files in the app config directory.
//! Secrets never go in these files; see [`secrets`].

pub mod secrets;

use crate::db::ConnConfig;
use crate::error::AppResult;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

const CONNECTIONS_FILE: &str = "connections.json";
const SETTINGS_FILE: &str = "settings.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub model: String,
    /// The demo connection is added once; deleting it keeps it deleted.
    #[serde(default)]
    pub demo_added: bool,
    /// Needed for API keys that aren't scoped to a workspace (`anthropic-workspace-id` header).
    #[serde(default)]
    pub workspace_id: String,
}

impl Default for Settings {
    fn default() -> Self {
        Settings { model: crate::ai::DEFAULT_MODEL.to_string(), demo_added: false, workspace_id: String::new() }
    }
}

pub struct Store {
    dir: PathBuf,
}

impl Store {
    pub fn new(dir: PathBuf) -> Self {
        let _ = std::fs::create_dir_all(&dir);
        Store { dir }
    }

    /// A missing or unreadable file reads as the default, so a corrupt file never blocks startup.
    fn read<T: for<'de> Deserialize<'de> + Default>(&self, name: &str) -> T {
        std::fs::read_to_string(self.dir.join(name))
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    fn write<T: Serialize>(&self, name: &str, value: &T) -> AppResult<()> {
        std::fs::write(self.dir.join(name), serde_json::to_string_pretty(value)?)?;
        Ok(())
    }

    pub fn connections(&self) -> Vec<ConnConfig> {
        self.read(CONNECTIONS_FILE)
    }

    pub fn connection(&self, id: &str) -> AppResult<ConnConfig> {
        self.connections().into_iter().find(|c| c.id == id).ok_or_else(|| "Connection not found.".into())
    }

    pub fn save_connections(&self, list: &[ConnConfig]) -> AppResult<()> {
        self.write(CONNECTIONS_FILE, &list)
    }

    pub fn settings(&self) -> Settings {
        self.read(SETTINGS_FILE)
    }

    pub fn save_settings(&self, s: &Settings) -> AppResult<()> {
        self.write(SETTINGS_FILE, s)
    }
}
