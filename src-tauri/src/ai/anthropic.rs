//! Claude over the Messages API. Raw HTTP + SSE, since there is no official Anthropic SDK for Rust.

use super::sse::SseParser;
use super::{GenRequest, Output, models, prompt};
use crate::error::{AppError, AppResult};
use futures_util::StreamExt;
use serde_json::{Value, json};

const API_URL: &str = "https://api.anthropic.com/v1/messages";
const API_VERSION: &str = "2023-06-01";

/// The Messages API body (without model-specific options) for one request.
fn request_body(r: &GenRequest) -> Value {
    json!({
        "model": r.model,
        "max_tokens": 16000,
        "stream": true,
        "system": [
            { "type": "text", "text": prompt::instructions(&r.dialect) },
            {
                "type": "text",
                "text": prompt::schema_block(&r.dialect, &r.schema_text),
                "cache_control": { "type": "ephemeral", "ttl": "1h" }
            }
        ],
        "messages": [{ "role": "user", "content": prompt::user_message(&r.question, &r.task, r.skill.as_deref()) }],
        "output_config": { "effort": r.task.effort() },
    })
}

/// Streams the reply into `out`. Returns the number of prompt tokens served from cache.
pub(super) async fn run(
    out: &mut Output<'_>,
    http: &reqwest::Client,
    api_key: &str,
    workspace_id: Option<&str>,
    r: &GenRequest,
) -> AppResult<Option<u64>> {
    let mut body = request_body(r);
    let beta = models::apply_options(&r.model, &mut body);

    let mut req = http.post(API_URL).header("x-api-key", api_key).header("anthropic-version", API_VERSION).json(&body);
    if let Some(b) = beta {
        req = req.header("anthropic-beta", b);
    }
    if let Some(ws) = workspace_id {
        req = req.header("anthropic-workspace-id", ws);
    }

    let resp = req.send().await.map_err(|e| format!("Could not reach the Claude API: {e}"))?;
    let status = resp.status().as_u16();
    if !resp.status().is_success() {
        return Err(api_error(status, &resp.text().await.unwrap_or_default()));
    }

    let mut parser = SseParser::default();
    let mut stream = resp.bytes_stream();
    let mut stop_reason: Option<String> = None;
    let mut cache_read: Option<u64> = None;

    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| format!("The response was interrupted: {e}"))?;
        for v in parser.push(&chunk) {
            match v["type"].as_str().unwrap_or("") {
                "message_start" => cache_read = v["message"]["usage"]["cache_read_input_tokens"].as_u64(),
                // The first model declined and a fallback model answers from scratch.
                "content_block_start" if v["content_block"]["type"] == "fallback" => out.reset(),
                "content_block_delta" if v["delta"]["type"] == "text_delta" => {
                    out.text(v["delta"]["text"].as_str().unwrap_or(""));
                }
                "message_delta" => {
                    if let Some(s) = v["delta"]["stop_reason"].as_str() {
                        stop_reason = Some(s.to_string());
                    }
                }
                "error" => {
                    let msg = v["error"]["message"].as_str().unwrap_or("unknown error");
                    return Err(format!("Claude API error: {msg}").into());
                }
                _ => {}
            }
        }
    }

    match stop_reason.as_deref() {
        Some("refusal") => Err("The model declined this request. Try rephrasing it.".into()),
        Some("max_tokens") => Err("The generated query was cut off. Try a simpler request.".into()),
        _ => Ok(cache_read),
    }
}

/// Checks that an API key works, with the cheapest call there is (listing one model).
pub(super) async fn check_key(http: &reqwest::Client, api_key: &str, workspace_id: Option<&str>) -> AppResult<()> {
    let mut req = http
        .get("https://api.anthropic.com/v1/models?limit=1")
        .header("x-api-key", api_key)
        .header("anthropic-version", API_VERSION)
        .timeout(std::time::Duration::from_secs(20));
    if let Some(ws) = workspace_id {
        req = req.header("anthropic-workspace-id", ws);
    }
    let resp = req.send().await.map_err(|e| format!("Could not reach the Claude API: {e}"))?;
    let status = resp.status().as_u16();
    if !resp.status().is_success() {
        return Err(api_error(status, &resp.text().await.unwrap_or_default()));
    }
    Ok(())
}

/// A message for a non-2xx API response. Mentions Settings when that's where the fix is.
fn api_error(status: u16, body: &str) -> AppError {
    let msg = serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|v| v["error"]["message"].as_str().map(str::to_string))
        .unwrap_or_else(|| body.to_string());
    match status {
        401 => format!("Your Claude API key was rejected ({msg}). Check it in Settings."),
        403 => format!("This API key isn't allowed to do that ({msg}). Check it in Settings."),
        400 if msg.contains("anthropic-workspace-id") => {
            "This API key isn't tied to a workspace. Add your Workspace ID in Settings.".into()
        }
        429 => format!("Rate limited by the Claude API: {msg}"),
        _ => format!("Claude API error {status}: {msg}"),
    }
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn api_errors_point_to_settings_when_the_fix_is_there() {
        let body = r#"{"type":"error","error":{"type":"authentication_error","message":"invalid x-api-key"}}"#;
        let e = api_error(401, body).to_string();
        assert!(e.contains("invalid x-api-key") && e.contains("Settings"), "{e}");

        let ws = r#"{"error":{"message":"...must include the anthropic-workspace-id header..."}}"#;
        assert!(api_error(400, ws).to_string().contains("Workspace ID in Settings"));

        assert_eq!(api_error(500, "oops").to_string(), "Claude API error 500: oops");
    }
}
