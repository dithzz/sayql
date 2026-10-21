//! Saved connections and settings live in JSON files in the app config directory.
//! Secrets (database passwords, the Claude API key) live in the OS keychain only.

use crate::db::ConnConfig;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

const SERVICE: &str = "com.askdb.app";
const API_KEY_ACCOUNT: &str = "anthropic-api-key";

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

    fn read<T: for<'de> Deserialize<'de> + Default>(&self, name: &str) -> T {
        std::fs::read_to_string(self.dir.join(name))
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    fn write<T: Serialize>(&self, name: &str, value: &T) -> Result<(), String> {
        let s = serde_json::to_string_pretty(value).map_err(|e| e.to_string())?;
        std::fs::write(self.dir.join(name), s).map_err(|e| e.to_string())
    }

    pub fn connections(&self) -> Vec<ConnConfig> {
        self.read("connections.json")
    }

    pub fn save_connections(&self, list: &[ConnConfig]) -> Result<(), String> {
        self.write("connections.json", &list)
    }

    pub fn settings(&self) -> Settings {
        self.read("settings.json")
    }

    pub fn save_settings(&self, s: &Settings) -> Result<(), String> {
        self.write("settings.json", s)
    }
}

fn entry(account: &str) -> Result<keyring::Entry, String> {
    keyring::Entry::new(SERVICE, account).map_err(|e| e.to_string())
}

fn get_secret(account: &str) -> Option<String> {
    entry(account).ok()?.get_password().ok().filter(|s| !s.is_empty())
}

fn set_secret(account: &str, value: &str) -> Result<(), String> {
    let e = entry(account)?;
    if value.is_empty() {
        match e.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(err) => Err(err.to_string()),
        }
    } else {
        e.set_password(value).map_err(|e| e.to_string())
    }
}

pub fn password(conn_id: &str) -> Option<String> {
    get_secret(&format!("conn:{conn_id}"))
}

pub fn set_password(conn_id: &str, pw: &str) -> Result<(), String> {
    set_secret(&format!("conn:{conn_id}"), pw)
}

/// The keychain entry wins; `ANTHROPIC_API_KEY` is a fallback for development.
pub fn api_key() -> Option<String> {
    get_secret(API_KEY_ACCOUNT).or_else(|| std::env::var("ANTHROPIC_API_KEY").ok().filter(|s| !s.is_empty()))
}

pub fn set_api_key(key: &str) -> Result<(), String> {
    set_secret(API_KEY_ACCOUNT, key.trim())
}
