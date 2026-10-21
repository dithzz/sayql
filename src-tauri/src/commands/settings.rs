use crate::ai;
use crate::error::{AppError, AppResult};
use crate::state::AppState;
use crate::store::{Settings, secrets};
use serde::Serialize;
use tauri::State;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsView {
    typing_model: String,
    expert_model: String,
    has_api_key: bool,
    workspace_id: String,
    models: Vec<ModelOption>,
}

#[derive(Serialize)]
struct ModelOption {
    id: &'static str,
    label: &'static str,
}

/// What the UI may know about the settings: never the API key itself.
pub fn settings_view(s: &Settings) -> SettingsView {
    SettingsView {
        typing_model: s.typing_model.clone(),
        expert_model: s.expert_model.clone(),
        has_api_key: secrets::api_key().is_some(),
        workspace_id: s.workspace_id.clone(),
        models: ai::MODELS.iter().map(|(id, label)| ModelOption { id, label }).collect(),
    }
}

/// `api_key: None` keeps the saved key.
#[tauri::command]
pub fn save_settings(
    state: State<'_, AppState>,
    typing_model: String,
    expert_model: String,
    api_key: Option<String>,
    workspace_id: String,
) -> AppResult<SettingsView> {
    for model in [&typing_model, &expert_model] {
        if !ai::is_known(model) {
            return Err(format!("Unknown model {model}").into());
        }
    }
    let workspace_id = valid_workspace_id(&workspace_id)?;
    if let Some(key) = api_key {
        secrets::set_api_key(&key)?;
    }
    let mut s = state.store.settings();
    s.typing_model = typing_model;
    s.expert_model = expert_model;
    s.workspace_id = workspace_id;
    state.store.save_settings(&s)?;
    Ok(settings_view(&s))
}

/// Where the `sayql-mcp` server binary is, for connecting Claude to Sayql.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct McpSetup {
    path: String,
    /// False when the binary hasn't been built or bundled next to the app.
    exists: bool,
}

/// The MCP server ships next to the app's own executable.
#[tauri::command]
pub fn mcp_setup() -> AppResult<McpSetup> {
    let exe = std::env::current_exe()?;
    let name = if cfg!(windows) { "sayql-mcp.exe" } else { "sayql-mcp" };
    let path = exe.with_file_name(name);
    Ok(McpSetup { exists: path.exists(), path: path.to_string_lossy().into_owned() })
}

/// Workspace IDs are short codes (no spaces or punctuation beyond `_`/`-`), not display names.
pub fn valid_workspace_id(raw: &str) -> AppResult<String> {
    let id = raw.trim();
    if id.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') {
        Ok(id.to_string())
    } else {
        Err(AppError::User(
            "The Workspace ID in Settings looks like a name. Paste the workspace's ID instead \
             (a code without spaces), or leave the field empty."
                .into(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workspace_ids_are_codes_not_names() {
        assert_eq!(valid_workspace_id("  wrkspc_01AbC-9 ").unwrap(), "wrkspc_01AbC-9");
        assert_eq!(valid_workspace_id("").unwrap(), "");
        assert!(valid_workspace_id("Adith‘s Individual Org").is_err());
    }
}
