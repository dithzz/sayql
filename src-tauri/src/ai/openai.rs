//! Any service with an OpenAI-compatible Chat Completions API: OpenAI, Gemini, OpenRouter,
//! Mistral, Groq, DeepSeek, Ollama, LM Studio and self-hosted gateways.
//!
//! Only fields every one of them accepts are sent (no `max_tokens`, `temperature` or
//! reasoning options), since some reject parameters their models don't support.

use super::agent::model::{Block, Msg, Role, ToolSpec, Turn, Usage};
use super::sse::SseParser;
use super::{GenRequest, Output, prompt};
use crate::error::{AppError, AppResult};
use futures_util::StreamExt;
use serde_json::{Value, json};

/// Where to send requests, and how to name the service in messages.
pub(crate) struct Service<'a> {
    pub label: &'a str,
    pub base_url: &'a str,
    pub api_key: Option<&'a str>,
}

impl Service<'_> {
    fn request(&self, http: &reqwest::Client, method: reqwest::Method, path: &str) -> reqwest::RequestBuilder {
        let req = http.request(method, format!("{}/{path}", self.base_url.trim_end_matches('/')));
        match self.api_key {
            Some(key) => req.bearer_auth(key),
            None => req,
        }
    }

    fn unreachable(&self, e: reqwest::Error) -> AppError {
        format!("Could not reach {} at {}. Check that it's running and the base URL in Settings. ({e})", self.label, self.base_url)
            .into()
    }
}

fn request_body(r: &GenRequest) -> Value {
    // The schema goes in the system message so the prefix is identical across keystrokes,
    // which is what providers with automatic prompt caching reuse.
    let system = format!("{}\n\n{}", prompt::instructions(&r.dialect), prompt::schema_block(&r.dialect, &r.schema_text));
    json!({
        "model": r.model,
        "stream": true,
        "messages": [
            { "role": "system", "content": system },
            { "role": "user", "content": prompt::user_message(&r.question, &r.task, r.skill.as_deref()) },
        ],
    })
}

/// Streams the reply into `out`. Returns the number of prompt tokens served from cache, when reported.
pub(super) async fn run(out: &mut Output<'_>, http: &reqwest::Client, svc: &Service<'_>, r: &GenRequest) -> AppResult<Option<u64>> {
    let resp = svc
        .request(http, reqwest::Method::POST, "chat/completions")
        .json(&request_body(r))
        .send()
        .await
        .map_err(|e| svc.unreachable(e))?;
    let status = resp.status().as_u16();
    if !resp.status().is_success() {
        return Err(api_error(svc.label, status, &resp.text().await.unwrap_or_default()));
    }

    let mut parser = SseParser::default();
    let mut think = ThinkFilter::default();
    let mut stream = resp.bytes_stream();
    let mut finish: Option<String> = None;
    let mut cached: Option<u64> = None;

    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| format!("The response was interrupted: {e}"))?;
        for v in parser.push(&chunk) {
            if let Some(msg) = error_message(&v) {
                return Err(format!("{} error: {msg}", svc.label).into());
            }
            let choice = &v["choices"][0];
            if let Some(text) = choice["delta"]["content"].as_str() {
                out.text(&think.push(text));
            }
            if let Some(f) = choice["finish_reason"].as_str() {
                finish = Some(f.to_string());
            }
            if let Some(c) = v["usage"]["prompt_tokens_details"]["cached_tokens"].as_u64() {
                cached = Some(c);
            }
        }
    }
    out.text(&think.finish());

    match finish.as_deref() {
        Some("content_filter") => Err("The model declined this request. Try rephrasing it.".into()),
        Some("length") => Err("The generated query was cut off. Try a simpler request.".into()),
        _ => Ok(cached),
    }
}

/// One agent turn with tools over Chat Completions. Models without tool support reject
/// `tools` or ignore it; the agent then switches to the text protocol.
pub(crate) async fn turn(
    http: &reqwest::Client,
    svc: &Service<'_>,
    model: &str,
    system: &str,
    schema: &str,
    tools: &[ToolSpec],
    messages: &[Msg],
) -> AppResult<Turn> {
    let resp = svc
        .request(http, reqwest::Method::POST, "chat/completions")
        .json(&turn_body(model, system, schema, tools, messages))
        .send()
        .await
        .map_err(|e| svc.unreachable(e))?;
    let status = resp.status().as_u16();
    let text = resp.text().await.map_err(|e| format!("The response was interrupted: {e}"))?;
    if !(200..300).contains(&status) {
        return Err(api_error(svc.label, status, &text));
    }
    let v: Value = serde_json::from_str(&text).map_err(|_| format!("{} returned an unexpected reply.", svc.label))?;
    if let Some(msg) = error_message(&v) {
        return Err(format!("{} error: {msg}", svc.label).into());
    }
    parse_turn(&v)
}

fn turn_body(model: &str, system: &str, schema: &str, tools: &[ToolSpec], messages: &[Msg]) -> Value {
    let mut out = vec![json!({ "role": "system", "content": format!("{system}\n\n{schema}") })];
    for m in messages {
        match m.role {
            Role::Assistant => {
                let text: Vec<&str> = m.blocks.iter().filter_map(|b| if let Block::Text(t) = b { Some(t.as_str()) } else { None }).collect();
                let calls: Vec<Value> = m
                    .blocks
                    .iter()
                    .filter_map(|b| match b {
                        Block::ToolUse { id, name, input } => {
                            Some(json!({ "id": id, "type": "function", "function": { "name": name, "arguments": input.to_string() } }))
                        }
                        _ => None,
                    })
                    .collect();
                let mut msg = json!({ "role": "assistant", "content": text.join("\n") });
                if !calls.is_empty() {
                    msg["tool_calls"] = Value::Array(calls);
                }
                out.push(msg);
            }
            Role::User => {
                let mut text = Vec::new();
                for b in &m.blocks {
                    match b {
                        Block::ToolResult { id, content, is_error } => {
                            let content = if *is_error { format!("Error: {content}") } else { content.clone() };
                            out.push(json!({ "role": "tool", "tool_call_id": id, "content": content }));
                        }
                        Block::Text(t) => text.push(t.as_str()),
                        _ => {}
                    }
                }
                if !text.is_empty() {
                    out.push(json!({ "role": "user", "content": text.join("\n") }));
                }
            }
        }
    }
    let mut body = json!({ "model": model, "messages": out });
    if !tools.is_empty() {
        body["tools"] = tools
            .iter()
            .map(|t| json!({ "type": "function", "function": { "name": t.name, "description": t.description, "parameters": t.schema } }))
            .collect();
    }
    body
}

fn parse_turn(v: &Value) -> AppResult<Turn> {
    let choice = &v["choices"][0];
    let msg = &choice["message"];
    let mut blocks = Vec::new();
    if let Some(text) = msg["content"].as_str() {
        let mut think = ThinkFilter::default();
        let text = think.push(text) + &think.finish();
        if !text.trim().is_empty() {
            blocks.push(Block::Text(text));
        }
    }
    for (i, call) in msg["tool_calls"].as_array().into_iter().flatten().enumerate() {
        let f = &call["function"];
        // Arguments arrive as a JSON string; some local servers send an object.
        let input = match &f["arguments"] {
            Value::String(s) => serde_json::from_str(s).unwrap_or(Value::Null),
            other => other.clone(),
        };
        let id = call["id"].as_str().map(str::to_string).unwrap_or_else(|| format!("call_{i}"));
        blocks.push(Block::ToolUse { id, name: f["name"].as_str().unwrap_or("").to_string(), input });
    }
    let u = &v["usage"];
    let cached = u["prompt_tokens_details"]["cached_tokens"].as_u64().unwrap_or(0);
    let usage = Usage {
        input: u["prompt_tokens"].as_u64().unwrap_or(0).saturating_sub(cached),
        cached,
        output: u["completion_tokens"].as_u64().unwrap_or(0),
    };
    match choice["finish_reason"].as_str() {
        Some("content_filter") => Err("The model declined this request. Try rephrasing it.".into()),
        finish => Ok(Turn { blocks, usage, truncated: finish == Some("length") }),
    }
}

/// The model ids the service offers (`GET /models`). Also tells whether the key and URL work.
pub(super) async fn list_models(http: &reqwest::Client, svc: &Service<'_>) -> AppResult<Vec<String>> {
    let resp = svc
        .request(http, reqwest::Method::GET, "models")
        .timeout(std::time::Duration::from_secs(20))
        .send()
        .await
        .map_err(|e| svc.unreachable(e))?;
    let status = resp.status().as_u16();
    let body = resp.text().await.unwrap_or_default();
    if !(200..300).contains(&status) {
        return Err(api_error(svc.label, status, &body));
    }
    let v: Value = serde_json::from_str(&body).map_err(|_| format!("{} returned an unexpected model list.", svc.label))?;
    let mut ids: Vec<String> =
        v["data"].as_array().into_iter().flatten().filter_map(|m| m["id"].as_str().map(str::to_string)).collect();
    ids.sort();
    ids.dedup();
    Ok(ids)
}

/// The error text in a response body. Shapes differ: `{"error":{"message":…}}` (OpenAI),
/// `[{"error":{…}}]` (Gemini), `{"error":"…"}` (Ollama).
fn error_message(v: &Value) -> Option<String> {
    let v = if v.is_array() { &v[0] } else { v };
    let e = &v["error"];
    e["message"].as_str().or_else(|| e.as_str()).map(str::to_string)
}

/// A message for a non-2xx response. Mentions Settings when that's where the fix is.
fn api_error(label: &str, status: u16, body: &str) -> AppError {
    let msg = serde_json::from_str::<Value>(body).ok().as_ref().and_then(error_message).unwrap_or_else(|| {
        let b = body.trim();
        if b.is_empty() { "no details".into() } else { b.chars().take(300).collect() }
    });
    match status {
        401 => format!("Your {label} API key was rejected ({msg}). Check it in Settings."),
        403 => format!("{label} refused this request ({msg}). Check your key in Settings."),
        404 => format!("{label} doesn't know this model or address ({msg}). Check the model and base URL in Settings."),
        429 => format!("Rate limited by {label}: {msg}"),
        _ => format!("{label} error {status}: {msg}"),
    }
    .into()
}

/// Drops a leading `<think>…</think>` block, which some reasoning models (DeepSeek R1, Qwen
/// and others, often run locally) write into the reply before the answer.
#[derive(Default)]
struct ThinkFilter {
    state: Think,
    buf: String,
}

#[derive(Default, PartialEq)]
enum Think {
    /// Not enough text yet to tell whether the reply starts with `<think>`.
    #[default]
    Undecided,
    Inside,
    Pass,
}

const OPEN: &str = "<think>";
const CLOSE: &str = "</think>";

impl ThinkFilter {
    /// Returns the text to show for this chunk; empty while it may still be thinking.
    fn push(&mut self, text: &str) -> String {
        match self.state {
            Think::Pass => text.to_string(),
            Think::Inside => {
                self.buf.push_str(text);
                self.after_close()
            }
            Think::Undecided => {
                self.buf.push_str(text);
                let start = self.buf.trim_start();
                if let Some(rest) = start.strip_prefix(OPEN) {
                    self.buf = rest.to_string();
                    self.state = Think::Inside;
                    self.after_close()
                } else if OPEN.starts_with(start) {
                    String::new()
                } else {
                    self.state = Think::Pass;
                    std::mem::take(&mut self.buf)
                }
            }
        }
    }

    fn after_close(&mut self) -> String {
        match self.buf.find(CLOSE) {
            Some(i) => {
                let rest = self.buf[i + CLOSE.len()..].trim_start().to_string();
                self.buf.clear();
                self.state = Think::Pass;
                rest
            }
            None => String::new(),
        }
    }

    /// Text held back when the stream ends (a reply too short to decide on).
    fn finish(&mut self) -> String {
        if self.state == Think::Undecided { std::mem::take(&mut self.buf) } else { String::new() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_turns_round_trip() {
        let tools = [ToolSpec { name: "run_query", description: "Run SQL", schema: json!({ "type": "object" }) }];
        let msgs = vec![
            Msg::user("How many?"),
            Msg { role: Role::Assistant, blocks: vec![Block::ToolUse { id: "c1".into(), name: "run_query".into(), input: json!({ "sql": "SELECT 1" }) }] },
            Msg { role: Role::User, blocks: vec![Block::ToolResult { id: "c1".into(), content: "bad".into(), is_error: true }] },
        ];
        let body = turn_body("qwen3", "sys", "schema", &tools, &msgs);
        assert_eq!(body["messages"][0]["content"], "sys\n\nschema");
        assert_eq!(body["messages"][2]["tool_calls"][0]["function"]["arguments"], "{\"sql\":\"SELECT 1\"}");
        assert_eq!(body["messages"][3], json!({ "role": "tool", "tool_call_id": "c1", "content": "Error: bad" }));
        assert_eq!(body["tools"][0]["function"]["name"], "run_query");

        let reply = json!({
            "choices": [{ "message": { "content": "<think>x</think>Looking.", "tool_calls": [
                { "id": "c2", "type": "function", "function": { "name": "finish", "arguments": "{\"answer\":\"3\"}" } },
                { "type": "function", "function": { "name": "ask_user", "arguments": { "question": "?" } } }
            ] }, "finish_reason": "tool_calls" }],
            "usage": { "prompt_tokens": 500, "completion_tokens": 20, "prompt_tokens_details": { "cached_tokens": 400 } }
        });
        let t = parse_turn(&reply).unwrap();
        assert_eq!(t.blocks[0], Block::Text("Looking.".into()));
        let calls: Vec<_> = t.tool_uses().map(|(id, n, i)| (id.to_string(), n.to_string(), i.clone())).collect();
        assert_eq!(calls[0], ("c2".into(), "finish".into(), json!({ "answer": "3" })));
        assert_eq!(calls[1].0, "call_1");
        assert_eq!(t.usage, Usage { input: 100, cached: 400, output: 20 });
    }

    fn filter(chunks: &[&str]) -> String {
        let mut f = ThinkFilter::default();
        let mut out: String = chunks.iter().map(|c| f.push(c)).collect();
        out.push_str(&f.finish());
        out
    }

    #[test]
    fn passes_plain_replies_through() {
        assert_eq!(filter(&["SELECT ", "1;"]), "SELECT 1;");
        assert_eq!(filter(&["<"]), "<");
    }

    #[test]
    fn drops_a_leading_think_block_split_across_chunks() {
        assert_eq!(filter(&["\n<th", "ink>The user wants…</th", "ink>\n\nSELECT", " 1;"]), "SELECT 1;");
        assert_eq!(filter(&["<think>never closed"]), "");
    }

    #[test]
    fn reads_every_error_shape() {
        let openai: Value = serde_json::from_str(r#"{"error":{"message":"Incorrect API key"}}"#).unwrap();
        let gemini: Value = serde_json::from_str(r#"[{"error":{"code":400,"message":"API key not valid"}}]"#).unwrap();
        let ollama: Value = serde_json::from_str(r#"{"error":"model \"llama9\" not found"}"#).unwrap();
        assert_eq!(error_message(&openai).unwrap(), "Incorrect API key");
        assert_eq!(error_message(&gemini).unwrap(), "API key not valid");
        assert!(error_message(&ollama).unwrap().contains("not found"));
        assert!(error_message(&json!({"choices": []})).is_none());
    }

    /// A one-request HTTP server: answers with `status` and `body`, and hands back the raw request.
    async fn fake_server(status: &'static str, body: &'static str) -> (String, tokio::sync::oneshot::Receiver<String>) {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let (tx, rx) = tokio::sync::oneshot::channel();
        tokio::spawn(async move {
            let (mut sock, _) = listener.accept().await.unwrap();
            let mut got = Vec::new();
            let mut buf = [0u8; 8192];
            // Read the headers, then as much body as Content-Length says.
            loop {
                let n = sock.read(&mut buf).await.unwrap();
                got.extend_from_slice(&buf[..n]);
                let text = String::from_utf8_lossy(&got).to_string();
                if let Some(end) = text.find("\r\n\r\n") {
                    let len = text
                        .lines()
                        .find_map(|l| l.to_ascii_lowercase().strip_prefix("content-length:").map(|v| v.trim().parse::<usize>().unwrap()))
                        .unwrap_or(0);
                    if got.len() >= end + 4 + len {
                        break;
                    }
                }
                if n == 0 {
                    break;
                }
            }
            let reply = format!("HTTP/1.1 {status}\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
            sock.write_all(reply.as_bytes()).await.unwrap();
            let _ = tx.send(String::from_utf8_lossy(&got).to_string());
        });
        (format!("http://{addr}/v1"), rx)
    }

    fn request() -> GenRequest {
        GenRequest {
            req_id: 1,
            endpoint: super::super::Endpoint::OpenAi { label: "Ollama", base_url: String::new(), api_key: None },
            model: "qwen3".into(),
            dialect: "SQLite".into(),
            schema_text: "TABLE customers (id INTEGER, name TEXT)".into(),
            question: "all customers".into(),
            task: super::super::Task::Draft { previous: None, force_follow_up: false },
            skill: None,
        }
    }

    #[tokio::test]
    async fn streams_a_reply_from_an_openai_compatible_server() {
        let stream = "data: {\"choices\":[{\"delta\":{\"role\":\"assistant\",\"content\":\"<think>They want\"}}]}\r\n\r\n\
                      data: {\"choices\":[{\"delta\":{\"content\":\" everyone.</think>\\n\"}}]}\r\n\r\n\
                      data: {\"choices\":[{\"delta\":{\"content\":\"SELECT id, name\"}}]}\r\n\r\n\
                      data: {\"choices\":[{\"delta\":{\"content\":\" FROM customers;\"},\"finish_reason\":\"stop\"}]}\r\n\r\n\
                      data: [DONE]\r\n\r\n";
        let (url, req) = fake_server("200 OK", stream).await;
        let svc = Service { label: "Ollama", base_url: &url, api_key: Some("k") };
        let mut out = Output::new(super::super::Sink::Collect(String::new()), 1);
        run(&mut out, &reqwest::Client::new(), &svc, &request()).await.unwrap();

        let super::super::Sink::Collect(text) = out.sink else { unreachable!() };
        assert_eq!(text, "SELECT id, name FROM customers;");
        let req = req.await.unwrap();
        assert!(req.starts_with("POST /v1/chat/completions "), "{req}");
        assert!(req.to_ascii_lowercase().contains("authorization: bearer k"), "{req}");
        assert!(req.contains("\"stream\":true") && req.contains("<schema dialect=\\\"SQLite\\\">"), "{req}");
    }

    #[tokio::test]
    async fn reports_a_rejected_key_and_a_cut_off_reply() {
        let (url, _) = fake_server("401 Unauthorized", r#"{"error":{"message":"Incorrect API key"}}"#).await;
        let svc = Service { label: "OpenAI", base_url: &url, api_key: Some("bad") };
        let mut out = Output::new(super::super::Sink::Collect(String::new()), 1);
        let e = run(&mut out, &reqwest::Client::new(), &svc, &request()).await.unwrap_err().to_string();
        assert!(e.contains("OpenAI API key was rejected (Incorrect API key)"), "{e}");

        let cut = "data: {\"choices\":[{\"delta\":{\"content\":\"SELECT\"},\"finish_reason\":\"length\"}]}\n\n";
        let (url, _) = fake_server("200 OK", cut).await;
        let svc = Service { label: "Groq", base_url: &url, api_key: None };
        let e = run(&mut out, &reqwest::Client::new(), &svc, &request()).await.unwrap_err().to_string();
        assert!(e.contains("cut off"), "{e}");
    }

    #[tokio::test]
    async fn lists_models() {
        let (url, req) = fake_server("200 OK", r#"{"object":"list","data":[{"id":"qwen3:32b"},{"id":"llama3.3:70b"}]}"#).await;
        let svc = Service { label: "Ollama", base_url: &url, api_key: None };
        assert_eq!(list_models(&reqwest::Client::new(), &svc).await.unwrap(), ["llama3.3:70b", "qwen3:32b"]);
        let req = req.await.unwrap();
        assert!(req.starts_with("GET /v1/models ") && !req.to_ascii_lowercase().contains("authorization"), "{req}");
    }

    #[test]
    fn api_errors_name_the_service_and_the_fix() {
        let e = api_error("OpenAI", 401, r#"{"error":{"message":"Incorrect API key provided"}}"#).to_string();
        assert_eq!(e, "Your OpenAI API key was rejected (Incorrect API key provided). Check it in Settings.");
        assert!(api_error("Ollama", 404, r#"{"error":"model not found"}"#).to_string().contains("base URL in Settings"));
        assert_eq!(api_error("Groq", 500, "").to_string(), "Groq error 500: no details");
    }
}
