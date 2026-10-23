//! English → SQL generation, streamed to the UI as Tauri events. Claude goes over the
//! Messages API; every other provider over the OpenAI-compatible Chat Completions API.

pub(crate) mod anthropic;
pub mod agent;
pub mod context;
pub mod endpoint;
mod models;
pub(crate) mod openai;
mod prompt;
mod providers;
pub(crate) mod redact;

#[cfg(test)]
pub(crate) use redact::database_error as mask_database_error;
mod sse;

pub use models::{DEFAULT_EXPERT_MODEL, DEFAULT_TYPING_MODEL, MODELS, is_known};
pub use prompt::Task;
pub use providers::{ANTHROPIC, Api, KeyNeed, PROVIDERS, Provider, clean_base_url, find as find_provider};

use crate::error::AppResult;
use serde::Serialize;
use std::time::Instant;
use tauri::{AppHandle, Emitter};

// Event names; must match src/api/events.ts.
const EVENT_DELTA: &str = "ai://delta";
const EVENT_DONE: &str = "ai://done";
const EVENT_RESET: &str = "ai://reset";

/// Where a request goes, with what it needs to authenticate.
pub enum Endpoint {
    Anthropic { api_key: String, workspace_id: Option<String> },
    OpenAi { label: &'static str, base_url: String, api_key: Option<String> },
}

pub struct GenRequest {
    pub req_id: u64,
    pub endpoint: Endpoint,
    pub model: String,
    pub dialect: String,
    pub schema_text: String,
    pub question: String,
    pub task: Task,
    /// The instruction text of the skill the person picked, if any.
    pub skill: Option<String>,
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

/// Where streamed text goes: the UI, or a string in tests.
enum Sink<'a> {
    App(&'a AppHandle),
    #[cfg(test)]
    Collect(String),
}

/// Sends one request's streamed text to the UI.
struct Output<'a> {
    sink: Sink<'a>,
    req_id: u64,
    start: Instant,
    first: bool,
}

impl<'a> Output<'a> {
    fn new(sink: Sink<'a>, req_id: u64) -> Self {
        Output { sink, req_id, start: Instant::now(), first: true }
    }

    fn text(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        let first_token_ms = self.first.then(|| self.start.elapsed().as_millis());
        self.first = false;
        match &mut self.sink {
            Sink::App(app) => {
                let _ = app.emit(EVENT_DELTA, Delta { req_id: self.req_id, text: text.to_string(), first_token_ms });
            }
            #[cfg(test)]
            Sink::Collect(s) => s.push_str(text),
        }
    }

    /// Discards what was sent so far; the answer starts over.
    fn reset(&mut self) {
        self.first = true;
        match &mut self.sink {
            Sink::App(app) => {
                let _ = app.emit(EVENT_RESET, Reset { req_id: self.req_id });
            }
            #[cfg(test)]
            Sink::Collect(s) => s.clear(),
        }
    }
}

/// Streams SQL for one question. Emits `ai://delta` chunks, then exactly one `ai://done`.
pub async fn stream_sql(app: AppHandle, http: reqwest::Client, r: GenRequest) {
    let mut out = Output::new(Sink::App(&app), r.req_id);
    let result = match &r.endpoint {
        Endpoint::Anthropic { api_key, workspace_id } => {
            anthropic::run(&mut out, &http, api_key, workspace_id.as_deref(), &r).await
        }
        Endpoint::OpenAi { label, base_url, api_key } => {
            let svc = openai::Service { label, base_url, api_key: api_key.as_deref() };
            openai::run(&mut out, &http, &svc, &r).await
        }
    };
    let (error, cache_read_tokens) = match result {
        Ok(cached) => (None, cached),
        Err(e) => (Some(e.to_string()), None),
    };
    let done = Done { req_id: r.req_id, error, total_ms: out.start.elapsed().as_millis(), cache_read_tokens };
    let _ = app.emit(EVENT_DONE, done);
}

/// Checks a Claude API key.
pub async fn check_claude_key(http: &reqwest::Client, api_key: &str, workspace_id: Option<&str>) -> AppResult<()> {
    anthropic::check_key(http, api_key, workspace_id).await
}

/// The models an OpenAI-compatible service offers; also checks its key and URL.
pub async fn list_models(http: &reqwest::Client, label: &str, base_url: &str, api_key: Option<&str>) -> AppResult<Vec<String>> {
    openai::list_models(http, &openai::Service { label, base_url, api_key }).await
}
