//! Saved connections and settings, as JSON files in the app config directory.
//! Secrets never go in these files; see [`secrets`].

pub mod secrets;

use crate::db::ConnConfig;
use crate::error::AppResult;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

const CONNECTIONS_FILE: &str = "connections.json";
const SETTINGS_FILE: &str = "settings.json";
const SCRIPTS_FILE: &str = "scripts.json";
/// The config folder name used before the app was renamed from AskDB.
const LEGACY_DIR_NAME: &str = "com.askdb.app";

/// Copies saved data from the pre-rename folder next to `dir`, the first time only (when
/// `dir` has no connections yet). The old files are left in place.
fn copy_legacy_files(dir: &std::path::Path) {
    let legacy = dir.with_file_name(LEGACY_DIR_NAME);
    if legacy == dir || dir.join(CONNECTIONS_FILE).exists() || !legacy.join(CONNECTIONS_FILE).exists() {
        return;
    }
    for file in [CONNECTIONS_FILE, SETTINGS_FILE, SCRIPTS_FILE] {
        let from = legacy.join(file);
        if from.exists() {
            let _ = std::fs::copy(&from, dir.join(file));
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    /// The AI provider that writes SQL (an id from `ai::PROVIDERS`).
    #[serde(default = "default_provider")]
    pub provider: String,
    /// Claude model that writes SQL while the person types.
    #[serde(default = "default_typing_model")]
    pub typing_model: String,
    /// Claude model that fixes rejected queries and improves queries on request.
    #[serde(default = "default_expert_model")]
    pub expert_model: String,
    /// Choices for providers other than Claude, by provider id.
    #[serde(default)]
    pub providers: BTreeMap<String, ProviderSettings>,
    /// The demo connection is added once; deleting it keeps it deleted.
    #[serde(default)]
    pub demo_added: bool,
    /// Needed for API keys that aren't scoped to a workspace (`anthropic-workspace-id` header).
    #[serde(default)]
    pub workspace_id: String,
}

/// How SayQL uses one AI provider.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderSettings {
    /// Empty means the provider's usual address.
    #[serde(default)]
    pub base_url: String,
    #[serde(default)]
    pub typing_model: String,
    /// Empty means the typing model does fixes and improvements too.
    #[serde(default)]
    pub expert_model: String,
}

impl ProviderSettings {
    pub fn model(&self, expert: bool) -> &str {
        if expert && !self.expert_model.is_empty() { &self.expert_model } else { &self.typing_model }
    }
}

impl Settings {
    /// Claude's choices live in the top-level fields, which predate other providers.
    pub fn provider_settings(&self, id: &str) -> ProviderSettings {
        if id == crate::ai::ANTHROPIC {
            ProviderSettings { base_url: String::new(), typing_model: self.typing_model.clone(), expert_model: self.expert_model.clone() }
        } else {
            self.providers.get(id).cloned().unwrap_or_default()
        }
    }

    pub fn set_provider_settings(&mut self, id: &str, p: ProviderSettings) {
        if id == crate::ai::ANTHROPIC {
            self.typing_model = p.typing_model;
            self.expert_model = p.expert_model;
        } else {
            self.providers.insert(id.to_string(), p);
        }
    }
}

fn default_provider() -> String {
    crate::ai::ANTHROPIC.to_string()
}

fn default_typing_model() -> String {
    crate::ai::DEFAULT_TYPING_MODEL.to_string()
}

fn default_expert_model() -> String {
    crate::ai::DEFAULT_EXPERT_MODEL.to_string()
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            provider: default_provider(),
            typing_model: default_typing_model(),
            expert_model: default_expert_model(),
            providers: BTreeMap::new(),
            demo_added: false,
            workspace_id: String::new(),
        }
    }
}

/// A query the person saved from a tab, bound to the database it was written for.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Script {
    pub id: String,
    pub name: String,
    pub connection_id: String,
    #[serde(default)]
    pub database: Option<String>,
    #[serde(default)]
    pub question: String,
    pub sql: String,
    /// Milliseconds since the Unix epoch.
    #[serde(default)]
    pub updated_at: u64,
}

pub struct Store {
    dir: PathBuf,
}

impl Store {
    pub fn new(dir: PathBuf) -> Self {
        let _ = std::fs::create_dir_all(&dir);
        copy_legacy_files(&dir);
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

    pub fn scripts(&self) -> Vec<Script> {
        self.read(SCRIPTS_FILE)
    }

    pub fn save_scripts(&self, list: &[Script]) -> AppResult<()> {
        self.write(SCRIPTS_FILE, &list)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_saved_before_other_providers_still_read_as_claude() {
        let s: Settings = serde_json::from_str(r#"{"typingModel":"claude-sonnet-5-5","expertModel":"claude-opus-5-5"}"#).unwrap();
        assert_eq!(s.provider, "anthropic");
        assert_eq!(s.provider_settings("anthropic").model(false), "claude-sonnet-5-5");
        assert_eq!(s.provider_settings("ollama"), ProviderSettings::default());
    }

    #[test]
    fn an_empty_expert_model_falls_back_to_the_typing_model() {
        let mut s = Settings::default();
        s.set_provider_settings("ollama", ProviderSettings { typing_model: "qwen3".into(), ..Default::default() });
        let p = s.provider_settings("ollama");
        assert_eq!((p.model(false), p.model(true)), ("qwen3", "qwen3"));
        assert_eq!(s.typing_model, crate::ai::DEFAULT_TYPING_MODEL, "Claude's choice is untouched");
    }

    #[test]
    fn copies_pre_rename_data_once_without_overwriting() {
        let base = std::env::temp_dir().join(format!("sayql-store-{}", uuid::Uuid::new_v4()));
        let legacy = base.join(LEGACY_DIR_NAME);
        std::fs::create_dir_all(&legacy).unwrap();
        std::fs::write(legacy.join(CONNECTIONS_FILE), r#"[{"id":"x","name":"Sales","kind":"mssql"}]"#).unwrap();
        std::fs::write(legacy.join(SCRIPTS_FILE), "[]").unwrap();

        let store = Store::new(base.join("com.sayql.app"));
        assert_eq!(store.connections()[0].name, "Sales");
        assert!(legacy.join(CONNECTIONS_FILE).exists(), "the old file stays");

        // Later starts never overwrite the new folder.
        std::fs::write(legacy.join(CONNECTIONS_FILE), "[]").unwrap();
        let again = Store::new(base.join("com.sayql.app"));
        assert_eq!(again.connections().len(), 1);
        let _ = std::fs::remove_dir_all(base);
    }
}
