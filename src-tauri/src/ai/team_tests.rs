//! Team AI: requests on team connections go through a fake team server's passthrough, with the
//! team token, for both provider shapes, streaming included; personal connections never do.

use super::agent::model::{Llm, Msg, Role, ToolMode};
use super::{Api, Endpoint, GenRequest, Output, Sink, Task, anthropic, openai};
use crate::error::AppError;
use crate::team::ai::{AiStatus, TeamAi, route};
use crate::team::{SIGNED_OUT, TeamClient};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// A one-request HTTP server: answers with `status` and `body`, and hands back the raw request.
async fn fake_server(status: &'static str, content_type: &'static str, body: &'static str) -> (String, tokio::sync::oneshot::Receiver<String>) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let (tx, rx) = tokio::sync::oneshot::channel();
    tokio::spawn(async move {
        let (mut sock, _) = listener.accept().await.unwrap();
        let mut got = Vec::new();
        let mut buf = [0u8; 8192];
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
        let reply = format!("HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
        sock.write_all(reply.as_bytes()).await.unwrap();
        let _ = tx.send(String::from_utf8_lossy(&got).to_string());
    });
    (format!("http://{addr}"), rx)
}

fn status(provider: &str) -> AiStatus {
    serde_json::from_value(serde_json::json!({
        "enabled": true, "provider": provider, "models": ["claude-sonnet-5-5", "claude-haiku-5-5"],
        "default_model": "claude-sonnet-5-5", "fast_model": "claude-haiku-5-5", "month": "2026-11",
        "you": { "tokens": 120000, "limit": 2000000 }, "team": { "tokens": 900000, "limit": null }
    }))
    .unwrap()
}

fn team_ai(server: &str, api: Api, own_ready: bool) -> (TeamAi, Arc<AtomicBool>) {
    let off = Arc::new(AtomicBool::new(false));
    (TeamAi::new(server, "tok-1".into(), api, "ask", "c_1", off.clone(), own_ready), off)
}

fn request(endpoint: Endpoint) -> GenRequest {
    GenRequest {
        req_id: 1,
        endpoint,
        model: "claude-haiku-5-5".into(),
        dialect: "PostgreSQL".into(),
        schema_text: "users(id, email)".into(),
        question: "how many users".into(),
        task: Task::Draft { previous: None, force_follow_up: false },
        skill: None,
    }
}

/// The request line and headers, lower-cased, of what the fake server got.
fn head(raw: &str) -> String {
    raw.split("\r\n\r\n").next().unwrap_or("").to_ascii_lowercase()
}

#[tokio::test]
async fn claude_requests_stream_through_the_team_server_with_the_team_token() {
    let stream = "event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"usage\":{}}}\n\n\
                  event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"delta\":{\"type\":\"text_delta\",\"text\":\"SELECT count(*) \"}}\n\n\
                  event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"delta\":{\"type\":\"text_delta\",\"text\":\"FROM users\"}}\n\n\
                  event: message_delta\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"}}\n\n";
    let (url, got) = fake_server("200 OK", "text/event-stream", stream).await;
    let (ai, _) = team_ai(&url, Api::Anthropic, false);
    let r = request(Endpoint::Team(ai));
    let mut out = Output::new(Sink::Collect(String::new()), 1);
    anthropic::run(&mut out, &reqwest::Client::new(), &r.endpoint.claude().unwrap(), &r).await.unwrap();
    let Sink::Collect(text) = out.sink else { unreachable!() };
    assert_eq!(text, "SELECT count(*) FROM users");

    let raw = got.await.unwrap();
    let h = head(&raw);
    assert!(h.starts_with("post /api/v1/ai/anthropic/v1/messages "), "{h}");
    assert!(h.contains("authorization: bearer tok-1") && !h.contains("x-api-key"), "the team token, never a personal key: {h}");
    assert!(h.contains("x-sayql-source: ask") && h.contains("x-sayql-connection: c_1"), "{h}");
    assert!(raw.contains("\"model\":\"claude-haiku-5-5\"") && raw.contains("\"stream\":true"));
}

#[tokio::test]
async fn openai_compatible_requests_stream_through_the_team_server() {
    let stream = "data: {\"choices\":[{\"delta\":{\"content\":\"SELECT 1\"}}]}\n\n\
                  data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":10}}\n\ndata: [DONE]\n\n";
    let (url, got) = fake_server("200 OK", "text/event-stream", stream).await;
    let (ai, _) = team_ai(&url, Api::OpenAi, false);
    let r = request(Endpoint::Team(ai));
    let mut out = Output::new(Sink::Collect(String::new()), 1);
    openai::run(&mut out, &reqwest::Client::new(), &r.endpoint.openai().unwrap(), &r).await.unwrap();
    let Sink::Collect(text) = out.sink else { unreachable!() };
    assert_eq!(text, "SELECT 1");
    let h = head(&got.await.unwrap());
    assert!(h.starts_with("post /api/v1/ai/openai/v1/chat/completions "), "{h}");
    assert!(h.contains("authorization: bearer tok-1") && h.contains("x-sayql-source: ask"), "{h}");
}

#[tokio::test]
async fn agent_turns_go_through_the_team_server_too() {
    let reply = r#"{"content":[{"type":"text","text":"Done."}],"stop_reason":"end_turn","usage":{"input_tokens":5,"output_tokens":2}}"#;
    let (url, got) = fake_server("200 OK", "application/json", reply).await;
    let off = Arc::new(AtomicBool::new(false));
    let ai = TeamAi::new(&url, "tok-1".into(), Api::Anthropic, "agent", "c_1", off, false);
    let llm = Llm::Remote { http: reqwest::Client::new(), endpoint: Endpoint::Team(ai), model: "claude-sonnet-5-5".into(), mode: ToolMode::Native };
    let msgs = vec![Msg { role: Role::User, blocks: vec![super::agent::model::Block::Text("hi".into())] }];
    llm.turn("system", "schema", &[], &msgs).await.unwrap();
    let h = head(&got.await.unwrap());
    assert!(h.starts_with("post /api/v1/ai/anthropic/v1/messages ") && h.contains("x-sayql-source: agent"), "{h}");
}

#[tokio::test]
async fn the_monthly_limit_is_said_plainly_and_is_not_a_database_error() {
    let body = r#"{"error":{"code":"ai_limit","message":"You've used your team AI allowance for November. It resets on 1 December."}}"#;
    let (url, _) = fake_server("429 Too Many Requests", "application/json", body).await;
    let (ai, _) = team_ai(&url, Api::Anthropic, true);
    let r = request(Endpoint::Team(ai));
    let mut out = Output::new(Sink::Collect(String::new()), 1);
    let e = anthropic::run(&mut out, &reqwest::Client::new(), &r.endpoint.claude().unwrap(), &r).await.unwrap_err();
    assert!(matches!(&e, AppError::TeamRefused(_)), "a refusal, so nothing retries or offers an AI fix");
    assert_eq!(e.to_string(), "You've used your team AI allowance for November. It resets on 1 December.");
}

#[test]
fn other_team_ai_errors() {
    let (ai, off) = team_ai("http://x", Api::Anthropic, true);
    let e = ai.error(400, r#"{"error":{"code":"bad_request","message":"Your team admin hasn't allowed that model"}}"#);
    assert_eq!(e.to_string(), "Your team admin hasn't allowed that model");
    assert_eq!(ai.error(401, r#"{"error":{"code":"unauthorized","message":"x"}}"#).to_string(), SIGNED_OUT);
    // The provider's own errors pass through, named as the team's AI (not "check your key in Settings").
    let e = ai.error(529, r#"{"type":"error","error":{"type":"overloaded_error","message":"Overloaded"}}"#).to_string();
    assert_eq!(e, "Your team's AI provider returned an error (529): Overloaded");

    // Team AI turned off: the next request uses the person's own AI, if they have one.
    assert!(!off.load(Ordering::Relaxed));
    let e = ai.error(403, r#"{"error":{"code":"ai_not_configured","message":"x"}}"#).to_string();
    assert!(e.contains("use your own AI provider") && off.load(Ordering::Relaxed), "{e}");
    let (no_own, _) = team_ai("http://x", Api::Anthropic, false);
    let e = no_own.error(403, r#"{"error":{"code":"ai_not_configured"}}"#).to_string();
    assert!(e.contains("Add one in Settings"), "{e}");
}

#[test]
fn only_team_connections_use_team_ai_with_the_teams_models() {
    let client = TeamClient::new(reqwest::Client::new(), "https://sayql.acme.test".into(), "tok-1".into(), None);
    let s = status("anthropic");
    let off = Arc::new(AtomicBool::new(false));
    assert!(route("demo", &s, &client, "ask", off.clone(), true).is_none(), "personal connections keep the person's own AI");
    let (endpoint, ai) = route("team:c_1", &s, &client, "ask", off.clone(), true).unwrap();
    assert!(matches!(&endpoint, Endpoint::Team(t) if t.base == "https://sayql.acme.test/api/v1/ai/anthropic" && t.api == Api::Anthropic));
    assert_eq!((ai.fast_model(), ai.default_model()), ("claude-haiku-5-5".to_string(), "claude-sonnet-5-5".to_string()));
    assert_eq!(ai.models, ["claude-sonnet-5-5", "claude-haiku-5-5"]);
    assert_eq!(ai.you.as_ref().map(|u| (u.tokens, u.limit)), Some((120000, Some(2000000))));

    // OpenAI-compatible, a missing fast model, team AI off, or turned off since: as without team AI.
    let (e, _) = route("team:c_1", &status("openai"), &client, "ask", off.clone(), true).unwrap();
    assert!(matches!(&e, Endpoint::Team(t) if t.base.ends_with("/api/v1/ai/openai/v1")) && e.is_openai());
    let no_fast = AiStatus { fast_model: None, ..s.clone() };
    assert_eq!(no_fast.fast_model(), "claude-sonnet-5-5");
    let disabled: AiStatus = serde_json::from_str(r#"{"enabled": false}"#).unwrap();
    assert!(route("team:c_1", &disabled, &client, "ask", off.clone(), true).is_none());
    off.store(true, Ordering::Relaxed);
    assert!(route("team:c_1", &s, &client, "ask", off, true).is_none());
}

/// Against a real SayQL server with team AI on (Anthropic, models claude-sonnet-5-5 and
/// claude-haiku-5-5, a personal limit under 200 tokens) and an unused member invite:
/// `SAYQL_TEST_TEAM_URL=… SAYQL_TEST_TEAM_CODE=… cargo test --lib ai::team_tests::real_server -- --ignored`.
#[tokio::test]
#[ignore]
async fn real_server() {
    let (Ok(url), Ok(code)) = (std::env::var("SAYQL_TEST_TEAM_URL"), std::env::var("SAYQL_TEST_TEAM_CODE")) else { return };
    let http = reqwest::Client::new();
    let (token, _) = crate::team::redeem(&http, &url, &code, "e2e").await.unwrap();
    let client = TeamClient::new(http.clone(), url, token, None);
    let ai = client.ai_status().await.unwrap();
    assert!(ai.usable(), "{ai:?}");
    let conn = client.connections().await.unwrap().remove(0);
    let off = Arc::new(AtomicBool::new(false));
    let (endpoint, ai) = route(&conn.local_id(), &ai, &client, "ask", off, false).unwrap();
    let mut r = request(endpoint);
    r.model = ai.fast_model();
    // Streams through the server until the personal limit is reached, then says so plainly.
    let mut answered = 0;
    let limit = loop {
        let mut out = Output::new(Sink::Collect(String::new()), 1);
        match anthropic::run(&mut out, &http, &r.endpoint.claude().unwrap(), &r).await {
            Ok(_) => {
                let Sink::Collect(text) = out.sink else { unreachable!() };
                assert_eq!(text, "SELECT count(*) FROM patients");
                answered += 1;
                assert!(answered < 20, "the limit never applied");
            }
            Err(e) => break e,
        }
    };
    assert!(answered >= 1 && matches!(limit, AppError::TeamRefused(_)), "{limit}");
    println!("limit message: {limit}");
    assert!(client.ai_status().await.unwrap().you.unwrap().tokens >= 100);
    // A model the admin didn't allow.
    r.model = "claude-opus-5-5".into();
    let mut out = Output::new(Sink::Collect(String::new()), 1);
    let e = anthropic::run(&mut out, &http, &r.endpoint.claude().unwrap(), &r).await.unwrap_err();
    println!("model message: {e}");
}
