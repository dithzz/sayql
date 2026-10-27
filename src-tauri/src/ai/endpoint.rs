//! Where SQL requests go with the saved settings: the provider, its address and its key.

use super::{Api, Endpoint, KeyNeed, Provider};
use crate::error::{AppError, AppResult};
use crate::store::{ProviderSettings, Settings, secrets};

/// The catalog entry for `id`, or a message for the UI.
pub fn provider(id: &str) -> AppResult<&'static Provider> {
    super::find_provider(id).ok_or_else(|| AppError::User(format!("Unknown AI provider \"{id}\".")))
}

/// The address requests go to: the person's override, else the provider's own.
pub fn address(p: &Provider, chosen: &ProviderSettings) -> String {
    if chosen.base_url.is_empty() { p.base_url.to_string() } else { chosen.base_url.clone() }
}

/// Where SQL requests go with the current settings, or what is missing in Settings.
pub fn endpoint(s: &Settings) -> AppResult<Endpoint> {
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
            // A workspace ID belongs to Anthropic keys; SayQL credit keys have none.
            workspace_id: if p.id == super::ANTHROPIC {
                Some(valid_workspace_id(&s.workspace_id)?).filter(|w| !w.is_empty())
            } else {
                None
            },
            base: url,
        },
        Api::OpenAi => Endpoint::OpenAi { label: p.label, base_url: url, api_key: key },
    })
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
        assert!(valid_workspace_id("ws.1").is_err());
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
            _ => panic!("expected an OpenAI-compatible endpoint"),
        }
    }

    #[test]
    fn a_base_url_override_wins() {
        let mut s = Settings { provider: "ollama".into(), ..Default::default() };
        s.set_provider_settings(
            "ollama",
            ProviderSettings { typing_model: "qwen3".into(), base_url: "http://box:11434/v1".into(), ..Default::default() },
        );
        match endpoint(&s).unwrap() {
            Endpoint::OpenAi { base_url, .. } => assert_eq!(base_url, "http://box:11434/v1"),
            _ => panic!("expected an OpenAI-compatible endpoint"),
        }
    }

    #[test]
    fn other_needs_a_base_url() {
        let mut s = Settings { provider: "custom".into(), ..Default::default() };
        s.set_provider_settings("custom", ProviderSettings { typing_model: "gpt-x".into(), ..Default::default() });
        assert!(endpoint(&s).err().unwrap().to_string().contains("base URL"));
    }

    #[test]
    fn sayql_credits_start_on_claudes_models_and_ask_for_a_credit_key() {
        let s = Settings { provider: crate::ai::SAYQL_CREDITS.into(), workspace_id: "wrkspc_x".into(), ..Default::default() };
        let p = s.provider_settings(crate::ai::SAYQL_CREDITS);
        assert_eq!((p.typing_model.as_str(), p.expert_model.as_str()), ("claude-haiku-5-5", "claude-opus-5-5"));
        assert_eq!(address(provider("sayql").unwrap(), &p), "https://sayql.dev/api/ai");
        // No key saved in the test keychain: the message names the credits, not a model.
        let e = endpoint(&s).err().unwrap().to_string();
        assert!(e.contains("SayQL AI credits") && !e.contains("model"), "{e}");
    }

    #[test]
    fn an_unknown_provider_is_a_user_error() {
        let s = Settings { provider: "nope".into(), ..Default::default() };
        assert!(endpoint(&s).err().unwrap().to_string().contains("Unknown AI provider"));
    }
}
