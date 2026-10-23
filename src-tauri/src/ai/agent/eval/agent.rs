//! The evaluation with a real model. Opt-in: set SAYQL_EVAL_ANTHROPIC_KEY (and optionally
//! SAYQL_EVAL_MODEL), or SAYQL_EVAL_OPENAI_URL with SAYQL_EVAL_MODEL (and SAYQL_EVAL_OPENAI_KEY)
//! for an OpenAI-compatible server such as Ollama, or SAYQL_EVAL_USE_APP_KEY=1 for the Claude key saved in SayQL. It costs API credits, so it never runs
//! by default. Writes one record per question to `target/agent-eval.json`.
//!
//!   SAYQL_EVAL_ANTHROPIC_KEY=sk-ant-... cargo test --lib agent_answers_the_question_set -- --nocapture

use super::{Answer, Answerer, Fixture, Question, check, questions};
use crate::ai::Endpoint;
use crate::ai::agent::model::{Block, Llm, Role, ToolMode, render};
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
        // The Claude key saved in SayQL itself, read from the keychain (macOS may ask to allow it).
        if std::env::var("SAYQL_EVAL_USE_APP_KEY").is_ok() {
            let endpoint: fn() -> Endpoint = || {
                let dir = dirs::config_dir().expect("config dir").join("com.sayql.app");
                let workspace = crate::store::Store::new(dir).settings().map(|s| s.workspace_id).unwrap_or_default();
                Endpoint::Anthropic {
                    api_key: crate::store::secrets::api_key(crate::ai::ANTHROPIC).expect("no Claude API key saved in SayQL"),
                    workspace_id: Some(workspace).filter(|w| !w.is_empty()),
                }
            };
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

/// What the model sees of results: SAYQL_EVAL_RESULTS=off, counts or rows (the default).
fn results_setting() -> Results {
    match std::env::var("SAYQL_EVAL_RESULTS").as_deref() {
        Ok("off") => Results::Off,
        Ok("counts") => Results::Counts,
        _ => Results::Rows,
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
            access: AgentAccess { enabled: true, results: results_setting(), ask_before_send: false, never_send: Vec::new() },
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
        // What went to the AI from the database: tool results, not the question or the model's own words.
        let sent: Vec<String> = out
            .history
            .messages
            .iter()
            .filter(|m| m.role == Role::User && m.blocks.iter().any(|b| matches!(b, Block::ToolResult { .. })))
            .map(render)
            .collect();
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
    // The tenant fixture on SQLite and on each server whose SAYQL_TEST_*_URL is set; the demo store is SQLite only.
    let mut tenant = vec![tenant_fixture::sqlite(&dir.join("tenant.sqlite")).await];
    tenant.extend(tenant_fixture::servers().await);
    // SAYQL_EVAL_DIALECTS=postgres,mysql,mssql runs on just those (sqlite, postgres, mysql, mssql).
    let dialects: Option<Vec<String>> = std::env::var("SAYQL_EVAL_DIALECTS").ok().map(|s| s.split(',').map(|x| x.trim().to_lowercase()).collect());
    let wanted = |kind: db::Kind| dialects.as_ref().is_none_or(|d| d.contains(&super::dialect_key(kind).to_string()));
    let (mut right, mut total, mut unsafe_runs) = (0, 0, Vec::new());
    // SAYQL_EVAL_ONLY=id1,id2 runs just those questions.
    let only: Option<Vec<String>> = std::env::var("SAYQL_EVAL_ONLY").ok().map(|s| s.split(',').map(|x| x.trim().to_string()).collect());
    for q in questions().into_iter().filter(|q| only.as_ref().is_none_or(|o| o.contains(&q.id))) {
        let pools = if q.db == Fixture::Demo { std::slice::from_ref(&demo) } else { &tenant[..] };
        for pool in pools.iter().filter(|p| wanted(p.kind())) {
            total += 1;
            let outcome = match agent.answer(&q, pool).await {
                Ok(a) => check(&q, pool.kind(), &a),
                Err(e) => Err(e),
            };
            match outcome {
                Ok(()) => right += 1,
                Err(e) => {
                    if e.contains("change data") || e.contains("personal data") {
                        unsafe_runs.push(format!("{} on {:?}: {e}", q.id, pool.kind()));
                    }
                    eprintln!("{} on {:?}: {e}", q.id, pool.kind());
                }
            }
        }
    }
    // SAYQL_EVAL_REPORT names the records file, for runs side by side.
    let name = std::env::var("SAYQL_EVAL_REPORT").unwrap_or_else(|_| "agent-eval".into());
    let report = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target").join(format!("{name}.json"));
    let _ = std::fs::write(&report, serde_json::to_string_pretty(&*agent.records.lock().unwrap()).unwrap());
    eprintln!("{right} of {total} right with {} ({:?}); records in {}", agent.model, results_setting(), report.display());
    let _ = std::fs::remove_dir_all(dir);
    assert!(unsafe_runs.is_empty(), "safety or privacy failures:\n{}", unsafe_runs.join("\n"));
}
