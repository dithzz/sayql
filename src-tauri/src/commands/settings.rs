use crate::ai::{self, Api, Endpoint, KeyNeed, Provider};
use crate::error::{AppError, AppResult};
use crate::state::AppState;
use crate::store::{ProviderSettings, Settings, secrets};
use serde::Serialize;
use tauri::State;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsView {
    /// The provider that writes SQL, and the models it uses.
    provider: String,
    typing_model: String,
    /// Already resolved: the typing model when no separate one was chosen.
    expert_model: String,
    /// The provider has everything it needs (key, model) to write SQL.
    ai_ready: bool,
    /// The first-run setup was finished or dismissed.
    onboarded: bool,
    workspace_id: String,
    /// Claude's models; other providers list theirs on request (`list_models`).
    models: Vec<ModelOption>,
    providers: Vec<ProviderView>,
}

#[derive(Serialize)]
struct ModelOption {
    id: &'static str,
    label: &'static str,
}

/// One provider as Settings shows it: the catalog entry plus what the person chose for it.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ProviderView {
    id: &'static str,
    label: &'static str,
    note: &'static str,
    api: Api,
    key: KeyNeed,
    key_placeholder: &'static str,
    local: bool,
    default_base_url: &'static str,
    base_url: String,
    typing_model: String,
    expert_model: String,
    has_key: bool,
}

/// What the UI may know about the settings: never an API key itself.
pub fn settings_view(s: &Settings) -> SettingsView {
    let active = s.provider_settings(&s.provider);
    SettingsView {
        provider: s.provider.clone(),
        typing_model: active.typing_model.clone(),
        expert_model: active.model(true).to_string(),
        ai_ready: endpoint(s).is_ok(),
        onboarded: s.onboarded,
        workspace_id: s.workspace_id.clone(),
        models: ai::MODELS.iter().map(|(id, label)| ModelOption { id, label }).collect(),
        providers: ai::PROVIDERS
            .iter()
            .map(|p| {
                let chosen = s.provider_settings(p.id);
                ProviderView {
                    id: p.id,
                    label: p.label,
                    note: p.note,
                    api: p.api,
                    key: p.key,
                    key_placeholder: p.key_placeholder,
                    local: p.local,
                    default_base_url: p.base_url,
                    base_url: chosen.base_url,
                    typing_model: chosen.typing_model,
                    expert_model: chosen.expert_model,
                    has_key: p.key != KeyNeed::None && secrets::api_key(p.id).is_some(),
                }
            })
            .collect(),
    }
}

fn provider(id: &str) -> AppResult<&'static Provider> {
    ai::find_provider(id).ok_or_else(|| AppError::User(format!("Unknown AI provider \"{id}\".")))
}

/// The address requests go to: the person's override, else the provider's own.
fn address(p: &Provider, chosen: &ProviderSettings) -> String {
    if chosen.base_url.is_empty() { p.base_url.to_string() } else { chosen.base_url.clone() }
}

/// Where SQL requests go with the current settings, or what is missing in Settings.
pub(crate) fn endpoint(s: &Settings) -> AppResult<Endpoint> {
    let p = provider(&s.provider)?;
    let chosen = s.provider_settings(p.id);
    // Settings checks first; the keychain is only read once nothing else is missing.
    if chosen.typing_model.is_empty() {
        return Err(format!("Choose a model for {} in Settings to generate SQL.", p.label).into());
    }
    let url = address(p, &chosen);
    if p.api == Api::OpenAi && url.is_empty() {
        return Err(format!("Add the base URL for {} in Settings to generate SQL.", p.label).into());
    }
    let key = if p.key == KeyNeed::None { None } else { secrets::api_key(p.id) };
    if p.key == KeyNeed::Required && key.is_none() {
        return Err(format!("Add your {} API key in Settings to generate SQL.", p.label).into());
    }
    Ok(match p.api {
        Api::Anthropic => Endpoint::Anthropic {
            api_key: key.unwrap_or_default(),
            workspace_id: Some(valid_workspace_id(&s.workspace_id)?).filter(|w| !w.is_empty()),
        },
        Api::OpenAi => Endpoint::OpenAi { label: p.label, base_url: url, api_key: key },
    })
}

/// Saves the choices for `provider` and makes it the one that writes SQL.
/// `api_key: None` keeps the saved key; an empty `expert_model` reuses the typing model.
#[tauri::command]
pub fn save_settings(
    state: State<'_, AppState>,
    provider: String,
    typing_model: String,
    expert_model: String,
    base_url: String,
    api_key: Option<String>,
    workspace_id: String,
) -> AppResult<SettingsView> {
    let p = self::provider(&provider)?;
    let typing_model = typing_model.trim().to_string();
    let expert_model = expert_model.trim().to_string();
    let base_url = ai::clean_base_url(&base_url).map_err(AppError::User)?;
    match p.api {
        Api::Anthropic => {
            for model in [&typing_model, &expert_model] {
                if !ai::is_known(model) {
                    return Err(format!("Unknown model {model}").into());
                }
            }
        }
        Api::OpenAi => {
            if typing_model.is_empty() {
                return Err(AppError::User(format!("Choose the {} model that writes SQL while you type.", p.label)));
            }
            if base_url.is_empty() && p.base_url.is_empty() {
                return Err(AppError::User(format!("Add the base URL of your {} server.", p.label)));
            }
        }
    }
    let workspace_id = valid_workspace_id(&workspace_id)?;
    if let Some(key) = api_key.filter(|_| p.key != KeyNeed::None) {
        secrets::set_api_key(p.id, &key)?;
    }
    let mut s = state.store.settings();
    s.provider = p.id.to_string();
    s.workspace_id = workspace_id;
    // Claude's address is fixed; a base URL equal to the default is stored as "use the default".
    let base_url = if p.api == Api::Anthropic || base_url == p.base_url { String::new() } else { base_url };
    s.set_provider_settings(p.id, ProviderSettings { base_url, typing_model, expert_model });
    state.store.save_settings(&s)?;
    Ok(settings_view(&s))
}

/// The models a provider offers, using the key and base URL in the form (falling back to
/// the saved ones). Doubles as a check that the key and address work.
#[tauri::command]
pub async fn list_models(
    state: State<'_, AppState>,
    provider: String,
    base_url: String,
    api_key: Option<String>,
) -> AppResult<Vec<String>> {
    let p = self::provider(&provider)?;
    if p.api == Api::Anthropic {
        // Claude's models are fixed; this checks the key so a typo is caught before it's saved.
        let key = api_key.map(|k| k.trim().to_string()).filter(|k| !k.is_empty()).or_else(|| secrets::api_key(p.id));
        let key = key.ok_or_else(|| AppError::User("Paste your Claude API key first.".into()))?;
        let workspace = valid_workspace_id(&state.store.settings().workspace_id)?;
        let http = state.http.clone();
        ai::check_claude_key(&http, &key, Some(workspace.as_str()).filter(|w| !w.is_empty())).await?;
        return Ok(ai::MODELS.iter().map(|(id, _)| id.to_string()).collect());
    }
    let typed = ai::clean_base_url(&base_url).map_err(AppError::User)?;
    let saved = state.store.settings().provider_settings(p.id);
    let url = if typed.is_empty() { address(p, &saved) } else { typed };
    if url.is_empty() {
        return Err(AppError::User(format!("Add the base URL of your {} server first.", p.label)));
    }
    let key = match p.key {
        KeyNeed::None => None,
        _ => api_key.map(|k| k.trim().to_string()).filter(|k| !k.is_empty()).or_else(|| secrets::api_key(p.id)),
    };
    if p.key == KeyNeed::Required && key.is_none() {
        return Err(AppError::User(format!("Paste your {} API key first.", p.label)));
    }
    let http = state.http.clone();
    ai::list_models(&http, p.label, &url, key.as_deref()).await
}

/// Records that the first-run setup was finished or dismissed, so it isn't shown again.
#[tauri::command]
pub fn finish_onboarding(state: State<'_, AppState>) -> AppResult<SettingsView> {
    let mut s = state.store.settings();
    s.onboarded = true;
    state.store.save_settings(&s)?;
    Ok(settings_view(&s))
}

/// Where the `sayql-mcp` server binary is, for connecting Claude to SayQL.
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

    #[test]
    fn a_local_provider_needs_only_a_model() {
        let mut s = Settings { provider: "ollama".into(), ..Default::default() };
        let missing = endpoint(&s).err().unwrap().to_string();
        assert!(missing.contains("model for Ollama"), "{missing}");

        s.set_provider_settings("ollama", ProviderSettings { typing_model: "qwen3".into(), ..Default::default() });
        match endpoint(&s).unwrap() {
            Endpoint::OpenAi { base_url, api_key, .. } => {
                assert_eq!(base_url, "http://localhost:11434/v1");
                assert!(api_key.is_none());
            }
            Endpoint::Anthropic { .. } => panic!("expected an OpenAI-compatible endpoint"),
        }
    }

    #[test]
    fn other_needs_a_base_url() {
        let mut s = Settings { provider: "custom".into(), ..Default::default() };
        s.set_provider_settings("custom", ProviderSettings { typing_model: "gpt-x".into(), ..Default::default() });
        assert!(endpoint(&s).err().unwrap().to_string().contains("base URL"));
    }
}
