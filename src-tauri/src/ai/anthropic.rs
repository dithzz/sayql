//! Claude over the Messages API. Raw HTTP + SSE, since there is no official Anthropic SDK for Rust.

use super::agent::model::{Block, Msg, Role, ToolSpec, Turn, Usage};
use super::sse::SseParser;
use super::{GenRequest, Output, models, prompt};
use crate::error::{AppError, AppResult};
use futures_util::StreamExt;
use serde_json::{Value, json};

const API_URL: &str = "https://api.anthropic.com/v1/messages";
const API_VERSION: &str = "2023-06-01";

/// How a request reaches Claude: with the person's key, or through their team's server.
pub(crate) enum Access<'a> {
    Key { api_key: &'a str, workspace_id: Option<&'a str> },
    Team(&'a crate::team::ai::TeamAi),
}

impl Access<'_> {
    /// A Messages API request carrying `body`, authorized for this access.
    fn post(&self, http: &reqwest::Client, body: &Value, beta: Option<&str>) -> reqwest::RequestBuilder {
        let mut req = match self {
            Access::Key { api_key, workspace_id } => {
                let mut req = http.post(API_URL).header("x-api-key", *api_key);
                if let Some(ws) = workspace_id {
                    req = req.header("anthropic-workspace-id", *ws);
                }
                req
            }
            // The server sets the company key and the API version itself.
            Access::Team(t) => t.authorize(http.post(format!("{}/v1/messages", t.base))),
        };
        req = req.header("anthropic-version", API_VERSION).json(body);
        if let Some(b) = beta {
            req = req.header("anthropic-beta", b);
        }
        req
    }

    fn unreachable(&self, e: reqwest::Error) -> AppError {
        match self {
            Access::Key { .. } => format!("Could not reach the Claude API: {e}").into(),
            Access::Team(t) => t.unreachable(e),
        }
    }

    fn error(&self, status: u16, body: &str) -> AppError {
        match self {
            Access::Key { .. } => api_error(status, body),
            Access::Team(t) => t.error(status, body),
        }
    }
}

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
    access: &Access<'_>,
    r: &GenRequest,
) -> AppResult<Option<u64>> {
    let mut body = request_body(r);
    let beta = models::apply_options(&r.model, &mut body);
    let resp = access.post(http, &body, beta).send().await.map_err(|e| access.unreachable(e))?;
    let status = resp.status().as_u16();
    if !resp.status().is_success() {
        return Err(access.error(status, &resp.text().await.unwrap_or_default()));
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

/// One agent turn with tools: the reply's text and tool calls. Thinking blocks come back as
/// `Block::Opaque`, to be passed back unchanged on the next turn as the API requires. The
/// instructions, schema and tools are cached, so later turns pay the cache-read rate for them.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn turn(
    http: &reqwest::Client,
    access: &Access<'_>,
    model: &str,
    system: &str,
    schema: &str,
    tools: &[ToolSpec],
    messages: &[Msg],
) -> AppResult<Turn> {
    let mut body = turn_body(model, system, schema, tools, messages);
    let beta = models::apply_options(model, &mut body);
    let resp = access.post(http, &body, beta).send().await.map_err(|e| access.unreachable(e))?;
    let status = resp.status().as_u16();
    let text = resp.text().await.map_err(|e| format!("The response was interrupted: {e}"))?;
    if !(200..300).contains(&status) {
        return Err(access.error(status, &text));
    }
    parse_turn(&serde_json::from_str(&text)?)
}

fn turn_body(model: &str, system: &str, schema: &str, tools: &[ToolSpec], messages: &[Msg]) -> Value {
    let mut tools: Vec<Value> =
        tools.iter().map(|t| json!({ "name": t.name, "description": t.description, "input_schema": t.schema })).collect();
    if let Some(last) = tools.last_mut() {
        last["cache_control"] = json!({ "type": "ephemeral", "ttl": "1h" });
    }
    let messages: Vec<Value> = messages
        .iter()
        .map(|m| {
            let content: Vec<Value> = m
                .blocks
                .iter()
                .map(|b| match b {
                    Block::Text(t) => json!({ "type": "text", "text": t }),
                    Block::ToolUse { id, name, input } => json!({ "type": "tool_use", "id": id, "name": name, "input": input }),
                    Block::ToolResult { id, content, is_error } => {
                        json!({ "type": "tool_result", "tool_use_id": id, "content": content, "is_error": is_error })
                    }
                    Block::Opaque(v) => v.clone(),
                })
                .collect();
            json!({ "role": if m.role == Role::User { "user" } else { "assistant" }, "content": content })
        })
        .collect();
    let mut body = json!({
        "model": model,
        "max_tokens": 16000,
        "system": [
            { "type": "text", "text": system },
            { "type": "text", "text": schema, "cache_control": { "type": "ephemeral", "ttl": "1h" } }
        ],
        "messages": messages,
        "output_config": { "effort": "medium" },
    });
    if !tools.is_empty() {
        body["tools"] = Value::Array(tools);
    }
    body
}

fn parse_turn(v: &Value) -> AppResult<Turn> {
    let mut blocks = Vec::new();
    for b in v["content"].as_array().into_iter().flatten() {
        match b["type"].as_str().unwrap_or("") {
            "text" => blocks.push(Block::Text(b["text"].as_str().unwrap_or("").to_string())),
            "tool_use" => blocks.push(Block::ToolUse {
                id: b["id"].as_str().unwrap_or("").to_string(),
                name: b["name"].as_str().unwrap_or("").to_string(),
                input: b["input"].clone(),
            }),
            "thinking" | "redacted_thinking" => blocks.push(Block::Opaque(b.clone())),
            _ => {}
        }
    }
    let u = &v["usage"];
    let usage = Usage {
        input: u["input_tokens"].as_u64().unwrap_or(0) + u["cache_creation_input_tokens"].as_u64().unwrap_or(0),
        cached: u["cache_read_input_tokens"].as_u64().unwrap_or(0),
        output: u["output_tokens"].as_u64().unwrap_or(0),
    };
    match v["stop_reason"].as_str() {
        Some("refusal") => Err("The model declined this request. Try rephrasing it.".into()),
        stop => Ok(Turn { blocks, usage, truncated: stop == Some("max_tokens") }),
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

    #[test]
    fn tool_turns_round_trip() {
        let tools = [ToolSpec { name: "run_query", description: "Run SQL", schema: json!({ "type": "object" }) }];
        let thinking = json!({ "type": "thinking", "thinking": "hmm", "signature": "sig" });
        let msgs = vec![
            Msg::user("Who owns it?"),
            Msg {
                role: Role::Assistant,
                blocks: vec![Block::Opaque(thinking.clone()), Block::ToolUse { id: "tu1".into(), name: "run_query".into(), input: json!({ "sql": "SELECT 1" }) }],
            },
            Msg { role: Role::User, blocks: vec![Block::ToolResult { id: "tu1".into(), content: "1 row".into(), is_error: false }] },
        ];
        let body = turn_body("claude-opus-5-5", "sys", "schema", &tools, &msgs);
        assert_eq!(body["tools"][0]["cache_control"]["type"], "ephemeral");
        assert_eq!(body["messages"][1]["content"][0], thinking);
        assert_eq!(body["messages"][2]["content"][0]["tool_use_id"], "tu1");
        assert!(turn_body("m", "s", "x", &[], &msgs).get("tools").is_none());

        let reply = json!({
            "content": [thinking, { "type": "text", "text": "Checking." }, { "type": "tool_use", "id": "tu2", "name": "finish", "input": { "answer": "x" } }],
            "stop_reason": "tool_use",
            "usage": { "input_tokens": 12, "cache_creation_input_tokens": 100, "cache_read_input_tokens": 900, "output_tokens": 30 }
        });
        let t = parse_turn(&reply).unwrap();
        assert_eq!(t.blocks.len(), 3);
        assert_eq!(t.usage, Usage { input: 112, cached: 900, output: 30 });
        assert_eq!(t.tool_uses().next().unwrap().1, "finish");
        assert!(parse_turn(&json!({ "content": [], "stop_reason": "refusal" })).is_err());
    }
}
