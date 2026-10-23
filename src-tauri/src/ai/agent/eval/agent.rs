//! The evaluation with a real model. Opt-in: set SAYQL_EVAL_ANTHROPIC_KEY (and optionally
//! SAYQL_EVAL_MODEL), or SAYQL_EVAL_OPENAI_URL with SAYQL_EVAL_MODEL (and SAYQL_EVAL_OPENAI_KEY)
//! for an OpenAI-compatible server such as Ollama. It costs API credits, so it never runs
//! by default. Writes one record per question to `target/agent-eval.json`.
//!
//!   SAYQL_EVAL_ANTHROPIC_KEY=sk-ant-... cargo test --lib agent_answers_the_question_set -- --nocapture

use super::{Answer, Answerer, Fixture, Question, check, questions};
use crate::ai::Endpoint;
use crate::ai::agent::model::{Llm, ToolMode, render};
use crate::ai::agent::run::{self, Event, Io, Reply, RunInput};
use crate::db::{self, DbPool, tenant_fixture};
use crate::store::agent::{AgentAccess, AgentSettings, Results};
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};
use tokio_util::sync::CancellationToken;

/// Runs each question as agent mode would, with "Up to 20 rows, masked" and no pauses. A
/// question from the agent is answered with its first choice.
pub struct Agent {
    pub endpoint: fn() -> Endpoint,
    pub model: String,
    pub records: Mutex<Vec<Value>>,
}

impl Agent {
    pub fn from_env() -> Option<Agent> {
        let model = std::env::var("SAYQL_EVAL_MODEL").ok();
        if std::env::var("SAYQL_EVAL_ANTHROPIC_KEY").is_ok() {
            let endpoint: fn() -> Endpoint =
                || Endpoint::Anthropic { api_key: std::env::var("SAYQL_EVAL_ANTHROPIC_KEY").unwrap(), workspace_id: None };
            return Some(Agent { endpoint, model: model.unwrap_or_else(|| crate::ai::DEFAULT_EXPERT_MODEL.into()), records: Mutex::default() });
        }
        if std::env::var("SAYQL_EVAL_OPENAI_URL").is_ok() {
            let endpoint: fn() -> Endpoint = || Endpoint::OpenAi {
                label: "Evaluation server",
                base_url: std::env::var("SAYQL_EVAL_OPENAI_URL").unwrap(),
                api_key: std::env::var("SAYQL_EVAL_OPENAI_KEY").ok(),
            };
            return Some(Agent { endpoint, model: model?, records: Mutex::default() });
        }
        eprintln!("skipping: set SAYQL_EVAL_ANTHROPIC_KEY or SAYQL_EVAL_OPENAI_URL to run the agent evaluation");
        None
    }
}

impl Answerer for Agent {
    async fn answer(&self, q: &Question, pool: &DbPool) -> Result<Answer, String> {
        let schema = db::introspect(pool, pool.kind()).await.map_err(|e| e.to_string())?;
        let full = crate::schema::render_full(&schema);
        let input = RunInput {
            run_id: q.id.clone(),
            question: q.question.clone(),
            kind: pool.kind(),
            schema_text: crate::schema::schema_for_prompt(&schema, &full, &q.question),
            schema,
            notes: Vec::new(),
            focus: Vec::new(),
            skill: None,
            access: AgentAccess { enabled: true, results: Results::Rows, ask_before_send: false, never_send: Vec::new() },
            limits: AgentSettings { skip_plan: true, ..Default::default() },
            history: None,
        };
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let events = Arc::new(Mutex::new(Vec::new()));
        let sink = events.clone();
        let emit = move |e: Event| {
            if let Event::Question { choices, .. } = &e {
                let _ = tx.send(Reply::Answer { text: choices.first().cloned().unwrap_or_else(|| "Use your best judgement.".into()) });
            }
            sink.lock().unwrap().push(e);
        };
        let mut io = Io { emit: Arc::new(emit), replies: rx, stop: CancellationToken::new() };
        let mut llm = Llm::Remote { http: reqwest::Client::new(), endpoint: (self.endpoint)(), model: self.model.clone(), mode: ToolMode::Native };
        let start = std::time::Instant::now();
        let out = run::run(&mut llm, pool, input, &mut io).await;
        let events = events.lock().unwrap().clone();
        let mut steps = Vec::new();
        for e in &events {
            if let Event::Step { step, .. } = e {
                steps.retain(|s: &run::Step| s.id != step.id);
                steps.push(step.clone());
            }
        }
        let error = events.iter().find_map(|e| if let Event::Done { error, .. } = e { error.clone() } else { None });
        let sent: Vec<String> = out.history.messages.iter().map(render).collect();
        let a = out.answer;
        self.records.lock().unwrap().push(json!({
            "id": q.id,
            "dialect": pool.kind().dialect(),
            "model": self.model,
            "answer": a.as_ref().map(|a| &a.text),
            "unverified": a.as_ref().map(|a| &a.unverified),
            "error": error,
            "steps": steps.len(),
            "queries": steps.iter().filter_map(|s| s.sql.clone()).collect::<Vec<_>>(),
            "usage": a.as_ref().map(|a| a.usage),
            "ms": start.elapsed().as_millis(),
            "sent": sent,
        }));
        let a = a.ok_or_else(|| error.unwrap_or_else(|| "no answer".into()))?;
        Ok(Answer {
            result: None,
            queries: steps.iter().filter(|s| s.tool == "run_query" && s.error.is_none()).filter_map(|s| s.sql.clone()).collect(),
            proposed_sql: a.proposed_sql.clone(),
            sent,
            text: Some(a.text.clone()),
            asked: events.iter().any(|e| matches!(e, Event::Question { .. })),
            declined: a.cannot_answer,
        })
    }
}

#[tokio::test]
async fn agent_answers_the_question_set() {
    let Some(agent) = Agent::from_env() else { return };
    let dir = std::env::temp_dir().join(format!("sayql-agent-eval-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let demo_file = dir.join("demo.sqlite");
    db::ensure_demo(&demo_file).await.unwrap();
    let demo = db::connect(
        &db::ConnConfig {
            id: "demo".into(),
            name: "Demo".into(),
            kind: db::Kind::Sqlite,
            host: String::new(),
            port: None,
            database: String::new(),
            user: String::new(),
            file: demo_file.to_string_lossy().into_owned(),
            ssl: false,
            trust_cert: false,
        },
        None,
    )
    .await
    .unwrap();
    let tenant = tenant_fixture::sqlite(&dir.join("tenant.sqlite")).await;
    let (mut right, mut total, mut unsafe_runs) = (0, 0, Vec::new());
    for q in questions() {
        let pool = if q.db == Fixture::Demo { &demo } else { &tenant };
        total += 1;
        let outcome = match agent.answer(&q, pool).await {
            Ok(a) => check(&q, pool.kind(), &a),
            Err(e) => Err(e),
        };
        match outcome {
            Ok(()) => right += 1,
            Err(e) => {
                if e.contains("change data") || e.contains("personal data") {
                    unsafe_runs.push(format!("{}: {e}", q.id));
                }
                eprintln!("{}: {e}", q.id);
            }
        }
    }
    let report = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target").join("agent-eval.json");
    let _ = std::fs::write(&report, serde_json::to_string_pretty(&*agent.records.lock().unwrap()).unwrap());
    eprintln!("{right} of {total} right with {}; records in {}", agent.model, report.display());
    let _ = std::fs::remove_dir_all(dir);
    assert!(unsafe_runs.is_empty(), "safety or privacy failures:\n{}", unsafe_runs.join("\n"));
}
