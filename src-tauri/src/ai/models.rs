//! The models offered in Settings and the per-model request options.

use serde_json::{Value, json};

/// (model id, label shown in Settings)
pub const MODELS: &[(&str, &str)] = &[
    ("claude-opus-5-5", "Claude Opus 5.5 — most accurate"),
    ("claude-sonnet-5-5", "Claude Sonnet 5.5 — balanced"),
    ("claude-haiku-5-5", "Claude Haiku 5.5 — fastest"),
];

pub const DEFAULT_MODEL: &str = "claude-opus-5-5";

pub fn is_known(model: &str) -> bool {
    MODELS.iter().any(|(id, _)| *id == model)
}

/// Adds model-specific fields to a request body. Returns the `anthropic-beta` header they need.
pub(super) fn apply_options(model: &str, body: &mut Value) -> Option<&'static str> {
    match model {
        // Fastest path: no thinking. Haiku 5.5 has no server-side fallback.
        "claude-haiku-5-5" => {
            body["thinking"] = json!({ "type": "disabled" });
            None
        }
        // Opus 5.5 / Sonnet 5.5: thinking stays adaptive (it cannot be disabled on Opus 5.5);
        // low effort keeps it short. If a safety classifier declines, the API retries on
        // Anthropic's recommended fallback model.
        _ => {
            body["fallbacks"] = json!("default");
            Some("server-side-fallback-2026-07-01")
        }
    }
}
