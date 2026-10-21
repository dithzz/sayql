//! English -> SQL generation over the Claude Messages API (raw HTTP + SSE streaming;
//! there is no official Anthropic SDK for Rust).

use futures_util::StreamExt;
use serde::Serialize;
use serde_json::{Value, json};
use std::time::Instant;
use tauri::{AppHandle, Emitter};

const API_URL: &str = "https://api.anthropic.com/v1/messages";

pub const MODELS: &[(&str, &str)] = &[
    ("claude-opus-5-5", "Claude Opus 5.5 — most accurate"),
    ("claude-sonnet-5-5", "Claude Sonnet 5.5 — balanced"),
    ("claude-haiku-5-5", "Claude Haiku 5.5 — fastest"),
];
pub const DEFAULT_MODEL: &str = "claude-opus-5-5";

const INSTRUCTIONS: &str = "You turn a person's plain-English request into one {DIALECT} query for the database described below. \
The person is usually not a developer, so infer what they mean.

Rules:
- Reply with the SQL only: no Markdown fences, no explanation before or after.
- Write a single statement in {DIALECT} syntax, using only tables and columns from the schema. Quote identifiers only when needed.
- Prefer readable results: pick useful columns instead of SELECT * when joining, use clear aliases, and join to show names rather than bare IDs.
- When the request lists rows without a limit, add LIMIT 100. Never add a limit to aggregates the person asked for in full.
- Interpret relative dates (\"last week\", \"this year\") relative to the current date in SQL, not a hard-coded date.
- Write INSERT, UPDATE, DELETE or DDL only when the person explicitly asks to change data or structure.
- If something is ambiguous, make the most reasonable choice and state the assumption in one leading `-- ` comment line.
- If the request cannot be answered from this schema, reply with only a `-- ` comment saying what is missing.";

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct Delta {
    req_id: u64,
    text: String,
    /// Set on the first chunk: milliseconds from request start to first SQL text.
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

pub struct GenRequest {
    pub req_id: u64,
    pub api_key: String,
    pub workspace_id: Option<String>,
    pub model: String,
    pub dialect: String,
    pub schema_text: String,
    pub question: String,
}

fn build_body(r: &GenRequest) -> (Value, Option<&'static str>) {
    let system = json!([
        { "type": "text", "text": INSTRUCTIONS.replace("{DIALECT}", &r.dialect) },
        // The schema is identical across keystrokes for a connection, so it is cached
        // and only the short question is processed fresh on each request.
        { "type": "text", "text": format!("<schema dialect=\"{}\">\n{}\n</schema>", r.dialect, r.schema_text),
          "cache_control": { "type": "ephemeral", "ttl": "1h" } }
    ]);
    let mut body = json!({
        "model": r.model,
        "max_tokens": 16000,
        "stream": true,
        "system": system,
        "messages": [{ "role": "user", "content": r.question }],
        "output_config": { "effort": "low" },
    });
    let mut beta = None;
    match r.model.as_str() {
        "claude-haiku-5-5" => {
            // Fastest path: no thinking. Haiku 5.5 has no server-side fallback.
            body["thinking"] = json!({ "type": "disabled" });
        }
        _ => {
            // Opus 5.5 / Sonnet 5.5: thinking stays adaptive (it cannot be disabled on
            // Opus 5.5); low effort keeps it short. If a safety classifier declines, the
            // API retries on Anthropic's recommended fallback model.
            body["fallbacks"] = json!("default");
            beta = Some("server-side-fallback-2026-07-01");
        }
    }
    (body, beta)
}

pub async fn stream_sql(app: AppHandle, http: reqwest::Client, r: GenRequest) {
    let start = Instant::now();
    let result = run(&app, &http, &r, start).await;
    let (error, cache_read_tokens) = match result {
        Ok(cache) => (None, cache),
        Err(e) => (Some(e), None),
    };
    let _ = app.emit(
        "ai://done",
        Done { req_id: r.req_id, error, total_ms: start.elapsed().as_millis(), cache_read_tokens },
    );
}

async fn run(app: &AppHandle, http: &reqwest::Client, r: &GenRequest, start: Instant) -> Result<Option<u64>, String> {
    let (body, beta) = build_body(r);
    let mut req = http
        .post(API_URL)
        .header("x-api-key", &r.api_key)
        .header("anthropic-version", "2023-06-01")
        .header("content-type", "application/json")
        .json(&body);
    if let Some(b) = beta {
        req = req.header("anthropic-beta", b);
    }
    if let Some(ws) = &r.workspace_id {
        req = req.header("anthropic-workspace-id", ws);
    }
    let resp = req.send().await.map_err(|e| format!("Could not reach the Claude API: {e}"))?;
    let status = resp.status();
    if !status.is_success() {
        let text = resp.text().await.unwrap_or_default();
        let msg = serde_json::from_str::<Value>(&text)
            .ok()
            .and_then(|v| v["error"]["message"].as_str().map(str::to_string))
            .unwrap_or(text);
        return Err(match status.as_u16() {
            401 => "Your Claude API key was rejected. Check it in Settings.".into(),
            400 if msg.contains("anthropic-workspace-id") => {
                "This API key isn't tied to a workspace. Add your Workspace ID in Settings.".into()
            }
            429 => format!("Rate limited by the Claude API: {msg}"),
            _ => format!("Claude API error {status}: {msg}"),
        });
    }

    let mut stream = resp.bytes_stream();
    let mut buf: Vec<u8> = Vec::new();
    let mut first = true;
    let mut stop_reason: Option<String> = None;
    let mut cache_read: Option<u64> = None;

    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| format!("Stream interrupted: {e}"))?;
        buf.extend_from_slice(&chunk);
        // SSE events are separated by a blank line.
        while let Some(pos) = find_event_end(&buf) {
            let event: Vec<u8> = buf.drain(..pos).collect();
            let text = String::from_utf8_lossy(&event);
            for line in text.lines() {
                let Some(data) = line.strip_prefix("data:") else { continue };
                let Ok(v) = serde_json::from_str::<Value>(data.trim()) else { continue };
                match v["type"].as_str().unwrap_or("") {
                    "message_start" => {
                        cache_read = v["message"]["usage"]["cache_read_input_tokens"].as_u64();
                    }
                    "content_block_start" if v["content_block"]["type"] == "fallback" => {
                        // The first model declined and a fallback model is answering from scratch.
                        let _ = app.emit("ai://reset", Reset { req_id: r.req_id });
                        first = true;
                    }
                    "content_block_delta" if v["delta"]["type"] == "text_delta" => {
                        let t = v["delta"]["text"].as_str().unwrap_or("").to_string();
                        let first_token_ms = if first { Some(start.elapsed().as_millis()) } else { None };
                        first = false;
                        let _ = app.emit("ai://delta", Delta { req_id: r.req_id, text: t, first_token_ms });
                    }
                    "message_delta" => {
                        if let Some(s) = v["delta"]["stop_reason"].as_str() {
                            stop_reason = Some(s.to_string());
                        }
                    }
                    "error" => {
                        let msg = v["error"]["message"].as_str().unwrap_or("unknown error");
                        return Err(format!("Claude API error: {msg}"));
                    }
                    _ => {}
                }
            }
        }
    }

    match stop_reason.as_deref() {
        Some("refusal") => Err("The model declined this request. Try rephrasing it.".into()),
        Some("max_tokens") => Err("The generated query was cut off. Try a simpler request.".into()),
        _ => Ok(cache_read),
    }
}

fn find_event_end(buf: &[u8]) -> Option<usize> {
    buf.windows(2).position(|w| w == b"\n\n").map(|p| p + 2)
}
