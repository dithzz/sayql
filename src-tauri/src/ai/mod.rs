//! English → SQL generation over the Claude Messages API, streamed to the UI as Tauri
//! events. Raw HTTP + SSE, since there is no official Anthropic SDK for Rust.

mod models;
mod prompt;
mod sse;

pub use models::{DEFAULT_EXPERT_MODEL, DEFAULT_TYPING_MODEL, MODELS, is_known};
pub use prompt::Task;

use crate::error::{AppError, AppResult};
use futures_util::StreamExt;
use serde::Serialize;
use serde_json::Value;
use sse::SseParser;
use std::time::Instant;
use tauri::{AppHandle, Emitter};

const API_URL: &str = "https://api.anthropic.com/v1/messages";
const API_VERSION: &str = "2023-06-01";

// Event names; must match src/api/events.ts.
const EVENT_DELTA: &str = "ai://delta";
const EVENT_DONE: &str = "ai://done";
const EVENT_RESET: &str = "ai://reset";

pub struct GenRequest {
    pub req_id: u64,
    pub api_key: String,
    pub workspace_id: Option<String>,
    pub model: String,
    pub dialect: String,
    pub schema_text: String,
    pub question: String,
    pub task: Task,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct Delta {
    req_id: u64,
    text: String,
    /// Set on the first chunk: milliseconds from request start to the first SQL text.
    first_token_ms: Option<u128>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct Done {
    req_id: u64,
    error: Option<String>,
    total_ms: u128,
    cache_read_tokens: Option<u64>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct Reset {
    req_id: u64,
}

/// Streams SQL for one question. Emits `ai://delta` chunks, then exactly one `ai://done`.
pub async fn stream_sql(app: AppHandle, http: reqwest::Client, r: GenRequest) {
    let start = Instant::now();
    let (error, cache_read_tokens) = match run(&app, &http, &r, start).await {
        Ok(cached) => (None, cached),
        Err(e) => (Some(e.to_string()), None),
    };
    let _ = app.emit(EVENT_DONE, Done { req_id: r.req_id, error, total_ms: start.elapsed().as_millis(), cache_read_tokens });
}

/// Returns the number of prompt tokens served from cache.
async fn run(app: &AppHandle, http: &reqwest::Client, r: &GenRequest, start: Instant) -> AppResult<Option<u64>> {
    let mut body = prompt::request_body(&r.model, &r.dialect, &r.schema_text, &r.question, &r.task);
    let beta = models::apply_options(&r.model, &mut body);

    let mut req = http
        .post(API_URL)
        .header("x-api-key", &r.api_key)
        .header("anthropic-version", API_VERSION)
        .json(&body);
    if let Some(b) = beta {
        req = req.header("anthropic-beta", b);
    }
    if let Some(ws) = &r.workspace_id {
        req = req.header("anthropic-workspace-id", ws);
    }

    let resp = req.send().await.map_err(|e| format!("Could not reach the Claude API: {e}"))?;
    let status = resp.status().as_u16();
    if !resp.status().is_success() {
        return Err(api_error(status, &resp.text().await.unwrap_or_default()));
    }

    let mut parser = SseParser::default();
    let mut stream = resp.bytes_stream();
    let mut first = true;
    let mut stop_reason: Option<String> = None;
    let mut cache_read: Option<u64> = None;

    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| format!("The response was interrupted: {e}"))?;
        for v in parser.push(&chunk) {
            match v["type"].as_str().unwrap_or("") {
                "message_start" => cache_read = v["message"]["usage"]["cache_read_input_tokens"].as_u64(),
                "content_block_start" if v["content_block"]["type"] == "fallback" => {
                    // The first model declined and a fallback model answers from scratch.
                    let _ = app.emit(EVENT_RESET, Reset { req_id: r.req_id });
                    first = true;
                }
                "content_block_delta" if v["delta"]["type"] == "text_delta" => {
                    let text = v["delta"]["text"].as_str().unwrap_or("").to_string();
                    let first_token_ms = first.then(|| start.elapsed().as_millis());
                    first = false;
                    let _ = app.emit(EVENT_DELTA, Delta { req_id: r.req_id, text, first_token_ms });
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
