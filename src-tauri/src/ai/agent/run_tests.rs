//! The agent loop with a scripted model against the tenant fixture: no API key, no network.

use super::model::{Llm, Mock, MockTurn};
use super::run::{self, Event, Io, Outcome, Reply, RunInput, StepStatus};
use crate::db::{self, DbPool, Kind, tenant_fixture};
use crate::store::agent::{AgentAccess, AgentSettings, Results};
use serde_json::json;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio_util::sync::CancellationToken;

struct Fixture {
    pool: DbPool,
    dir: std::path::PathBuf,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

async fn fixture() -> Fixture {
    let dir = std::env::temp_dir().join(format!("sayql-agent-run-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    Fixture { pool: tenant_fixture::sqlite(&dir.join("tenant.sqlite")).await, dir }
}

fn access(results: Results, ask: bool) -> AgentAccess {
    AgentAccess { enabled: true, results, ask_before_send: ask, never_send: vec![] }
}

struct Ran {
    events: Vec<Event>,
    outcome: Outcome,
    /// Everything the model was sent, turn by turn.
    seen: Vec<String>,
}

impl Ran {
    fn done(&self) -> (&Option<run::Answer>, &Option<String>, bool) {
        match self.events.last() {
            Some(Event::Done { answer, error, stopped, .. }) => (answer, error, *stopped),
            other => panic!("no Done event: {other:?}"),
        }
    }

    fn steps(&self) -> Vec<run::Step> {
        steps_of(&self.events)
    }
}

/// The last event for each step id.
fn steps_of(events: &[Event]) -> Vec<run::Step> {
    let mut out: Vec<run::Step> = Vec::new();
    for e in events {
        if let Event::Step { step, .. } = e {
            match out.iter_mut().find(|s| s.id == step.id) {
                Some(s) => *s = step.clone(),
                None => out.push(step.clone()),
            }
        }
    }
    out
}

async fn go(pool: &DbPool, turns: Vec<MockTurn>, access: AgentAccess, limits: AgentSettings, replies: Vec<Reply>) -> Ran {
    go_with(pool, turns, access, limits, replies, CancellationToken::new()).await
}

async fn go_with(pool: &DbPool, turns: Vec<MockTurn>, access: AgentAccess, limits: AgentSettings, replies: Vec<Reply>, stop: CancellationToken) -> Ran {
    let schema = db::introspect(pool, Kind::Sqlite).await.unwrap();
    let schema_text = crate::schema::render_full(&schema);
    let input = RunInput {
        run_id: "r1".into(),
        question: "Who owns Northfield Family Practice?".into(),
        kind: Kind::Sqlite,
        schema,
        schema_text,
        notes: vec!["owner = memberships.role = 'owner'".into()],
        focus: vec![],
        skill: None,
        access,
        limits,
        history: None,
    };
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    for r in replies {
        tx.send(r).unwrap();
    }
    let events = Arc::new(Mutex::new(Vec::new()));
    let sink = events.clone();
    let mut io = Io { emit: Arc::new(move |e| sink.lock().unwrap().push(e)), replies: rx, stop };
    let mut llm = Llm::Mock(Mock::new(turns));
    let outcome = run::run(&mut llm, pool, input, &mut io).await;
    let Llm::Mock(mock) = llm else { unreachable!() };
    let seen = mock.seen.lock().unwrap().clone();
    let events = events.lock().unwrap().clone();
    Ran { events, outcome, seen }
}

fn plan() -> MockTurn {
    MockTurn::Tools(vec![("propose_plan", json!({ "steps": ["Find the owner role", "Look up the owner"] }))])
}

fn go_on() -> Reply {
    Reply::Go { plan: None }
}

const OWNER_EMAIL: &str = "SELECT u.email FROM memberships m JOIN users u ON u.id = m.user_id JOIN tenants t ON t.id = m.tenant_id \
                           WHERE t.name = 'Northfield Family Practice' AND m.role = 'owner'";

#[tokio::test]
async fn a_full_run_ends_with_a_cited_answer_and_sends_no_personal_data() {
    let f = fixture().await;
    let turns = vec![
        plan(),
        MockTurn::Tools(vec![("profile_column", json!({ "table": "memberships", "column": "role" }))]),
        MockTurn::Tools(vec![("run_query", json!({ "sql": OWNER_EMAIL, "purpose": "Finding the owner" }))]),
        // Follows the lead with the token it got back, without seeing the email.
        MockTurn::Dynamic(Box::new(|last| {
            let token = &last[last.find('‹').unwrap()..last.find('›').unwrap() + '›'.len_utf8()];
            assert!(token.starts_with("‹email-"), "{last}");
            vec![("run_query", json!({ "sql": format!("SELECT full_name FROM users WHERE email = '{token}'"), "purpose": "Getting the owner's name" }))]
        })),
        MockTurn::Dynamic(Box::new(|last| {
            let token = &last[last.find('‹').unwrap()..last.find('›').unwrap() + '›'.len_utf8()];
            vec![(
                "finish",
                json!({
                    "answer": format!("The owner is {token}."),
                    "citations": [{ "sentence": format!("The owner is {token}."), "steps": [2, 3] }],
                    "confidence": "high",
                    "notes": ["owner = memberships.role = 'owner'", format!("the owner is {token}")]
                }),
            )]
        })),
    ];
    let ran = go(&f.pool, turns, access(Results::Rows, false), AgentSettings::default(), vec![go_on()]).await;
    let (answer, error, stopped) = ran.done();
    assert!(error.is_none() && !stopped, "{error:?}");
    let answer = answer.as_ref().unwrap();
    assert_eq!(answer.text, "The owner is Priya Natarajan.");
    assert_eq!(answer.sentences[0].steps, [2, 3]);
    assert!(answer.unverified.is_empty(), "{:?}", answer.unverified);
    assert_eq!(answer.notes, ["owner = memberships.role = 'owner'"], "a note with a value is dropped");
    let steps = ran.steps();
    assert_eq!(steps.len(), 3);
    assert!(steps.iter().all(|s| s.status == StepStatus::Done), "{steps:?}");
    assert_eq!(steps[2].rows, [[json!("Priya Natarajan")]], "the person sees the real rows");
    assert!(steps[2].sql.as_deref().unwrap().contains("@example.com"), "the SQL that ran has the real value");
    assert!(steps[1].sent_summary.as_deref().unwrap().contains("1 column masked"), "{:?}", steps[1].sent_summary);
    assert!(ran.events.iter().any(|e| matches!(e, Event::Plan { waiting: true, .. })));
    // Nothing planted reached the model, on any turn.
    for sent in &ran.seen {
        for p in tenant_fixture::planted_pii() {
            assert!(!sent.contains(p.as_str()), "{p:?} was sent: {sent}");
        }
    }
    assert_eq!(ran.outcome.history.next_step, 4);
}

#[tokio::test]
async fn writes_never_run_and_come_back_as_proposals() {
    let f = fixture().await;
    let before = db::run_query(&f.pool, "SELECT COUNT(*) FROM appointments", true).await.unwrap().rows;
    let turns = vec![
        plan(),
        MockTurn::Tools(vec![("run_query", json!({ "sql": "DELETE FROM appointments WHERE status_id = 3", "purpose": "Deleting" }))]),
        MockTurn::Tools(vec![("run_query", json!({ "sql": "SELECT 1; DELETE FROM appointments", "purpose": "Sneaky" }))]),
        MockTurn::Tools(vec![(
            "finish",
            json!({ "answer": "Run the statement below to delete them.", "proposed_sql": "DELETE FROM appointments WHERE status_id = 3" }),
        )]),
    ];
    let ran = go(&f.pool, turns, access(Results::Counts, false), AgentSettings::default(), vec![go_on()]).await;
    let steps = ran.steps();
    assert!(steps[0].error.as_deref().unwrap().contains("read-only"), "{steps:?}");
    assert!(steps[1].error.is_some(), "{steps:?}");
    let after = db::run_query(&f.pool, "SELECT COUNT(*) FROM appointments", true).await.unwrap().rows;
    assert_eq!(before, after);
    assert_eq!(ran.done().0.as_ref().unwrap().proposed_sql.as_deref(), Some("DELETE FROM appointments WHERE status_id = 3"));
}

#[tokio::test]
async fn limits_hold() {
    let f = fixture().await;
    let q = |sql: &str| MockTurn::Tools(vec![("run_query", json!({ "sql": sql, "purpose": "x" }))]);
    let limits = AgentSettings { max_queries: 1, max_steps: 4, skip_plan: true, ..Default::default() };
    let turns = vec![plan(), q("SELECT 1"), q("SELECT 2"), MockTurn::Text("I'm not sure.")];
    let ran = go(&f.pool, turns, access(Results::Counts, false), limits, vec![]).await;
    let steps = ran.steps();
    assert!(steps[1].error.as_deref().unwrap().contains("used its 1 queries"), "{steps:?}");
    // The last turn was offered only finish, and told so.
    assert!(ran.seen[3].contains("reached the step limit"), "{}", ran.seen[3]);
    assert_eq!(ran.done().0.as_ref().unwrap().confidence, "low");
    assert!(ran.events.iter().any(|e| matches!(e, Event::Plan { waiting: false, .. })));
}

#[tokio::test]
async fn stop_ends_a_waiting_model_quickly() {
    let f = fixture().await;
    let stop = CancellationToken::new();
    let later = stop.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(200)).await;
        later.cancel();
    });
    let start = Instant::now();
    let ran = go_with(&f.pool, vec![MockTurn::Hang], access(Results::Counts, false), AgentSettings::default(), vec![], stop).await;
    assert!(ran.done().2, "stopped");
    assert!(start.elapsed() < Duration::from_secs(2), "{:?}", start.elapsed());
}

#[tokio::test]
async fn stop_ends_a_running_query_quickly() {
    let f = fixture().await;
    let slow = "WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM n WHERE i < 500000000) SELECT COUNT(*) FROM n";
    let turns = vec![MockTurn::Tools(vec![("run_query", json!({ "sql": slow, "purpose": "slow" }))])];
    let stop = CancellationToken::new();
    let later = stop.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(300)).await;
        later.cancel();
    });
    let start = Instant::now();
    let ran = go_with(&f.pool, turns, access(Results::Counts, false), AgentSettings::default(), vec![], stop).await;
    assert!(ran.done().2, "stopped: {:?}", ran.done());
    assert!(start.elapsed() < Duration::from_secs(2), "{:?}", start.elapsed());
}

#[tokio::test]
async fn the_run_time_limit_stops_a_slow_model() {
    let f = fixture().await;
    let limits = AgentSettings { run_seconds: 1, ..Default::default() };
    let ran = go(&f.pool, vec![MockTurn::Hang], access(Results::Counts, false), limits, vec![]).await;
    assert!(ran.done().1.as_deref().unwrap().contains("time limit"), "{:?}", ran.done());
}

#[tokio::test]
async fn ask_before_send_pauses_and_skip_sends_only_the_count() {
    let f = fixture().await;
    let turns = vec![
        plan(),
        MockTurn::Tools(vec![("run_query", json!({ "sql": "SELECT full_name FROM users", "purpose": "names" }))]),
        MockTurn::Tools(vec![("run_query", json!({ "sql": "SELECT COUNT(*) AS n FROM users", "purpose": "count" }))]),
        MockTurn::Tools(vec![("finish", json!({ "answer": "There are 24 users." }))]),
    ];
    let ran = go(&f.pool, turns, access(Results::Rows, true), AgentSettings::default(), vec![go_on(), Reply::Skip, Reply::Send]).await;
    let sends: Vec<&String> = ran.events.iter().filter_map(|e| if let Event::Send { payload, .. } = e { Some(payload) } else { None }).collect();
    assert_eq!(sends.len(), 2);
    assert!(sends[0].contains("‹name-1›") && !sends[0].contains("Dana Reyes"), "the pause shows the masked payload: {}", sends[0]);
    let steps = ran.steps();
    assert!(steps[0].sent.as_deref().unwrap().contains("chose not to send"), "{:?}", steps[0].sent);
    // The prompt's own example uses ‹name-1›; the skipped rows add none.
    assert_eq!(ran.seen[2].matches("‹name-1›").count(), ran.seen[0].matches("‹name-1›").count(), "skipped rows never reached the model");
    assert!(ran.seen[2].contains("chose not to send"));
    assert!(steps[1].sent.as_deref().unwrap().contains("24"));
    assert!(ran.done().0.as_ref().unwrap().unverified.is_empty());
}

#[tokio::test]
async fn counts_and_off_send_no_values() {
    let f = fixture().await;
    for results in [Results::Off, Results::Counts] {
        let turns = vec![
            MockTurn::Tools(vec![("run_query", json!({ "sql": "SELECT t.name, COUNT(*) AS n FROM tenants t JOIN patients p ON p.tenant_id = t.id GROUP BY t.name", "purpose": "x" }))]),
            MockTurn::Tools(vec![("profile_column", json!({ "table": "patients", "column": "last_name" }))]),
            MockTurn::Tools(vec![("finish", json!({ "answer": "Done." }))]),
        ];
        let ran = go(&f.pool, turns, access(results, false), AgentSettings::default(), vec![]).await;
        let sent = ran.seen.join("\n");
        for hidden in ["Harbor Lane Clinic", "Pinecrest", "Abernathy", "Kowalczyk"] {
            assert!(!sent.contains(hidden), "{results:?}: {hidden} was sent");
        }
        assert!(ran.steps()[0].rows.len() == 4, "the person still sees the rows");
    }
}

#[tokio::test]
async fn never_send_columns_are_refused_in_sql() {
    let f = fixture().await;
    let turns = vec![
        MockTurn::Tools(vec![("run_query", json!({ "sql": "SELECT COUNT(*) FROM patients WHERE notes LIKE '%penicillin%'", "purpose": "x" }))]),
        MockTurn::Tools(vec![("finish", json!({ "answer": "I can't check notes." , "cannot_answer": true }))]),
    ];
    let mut a = access(Results::Rows, false);
    a.never_send = vec!["patients.notes".into()];
    let ran = go(&f.pool, turns, a, AgentSettings::default(), vec![]).await;
    assert!(ran.steps()[0].error.as_deref().unwrap().contains("never send"), "{:?}", ran.steps());
    assert!(ran.done().0.as_ref().unwrap().cannot_answer);
}

#[tokio::test]
async fn ask_user_waits_for_the_answer() {
    let f = fixture().await;
    let turns = vec![
        plan(),
        MockTurn::Tools(vec![("ask_user", json!({ "question": "Owner as creator, or the owner role?", "choices": ["Creator", "Owner role"] }))]),
        MockTurn::Tools(vec![("finish", json!({ "answer": "OK." }))]),
    ];
    let ran = go(&f.pool, turns, access(Results::Counts, false), AgentSettings::default(), vec![go_on(), Reply::Answer { text: "Owner role".into() }]).await;
    assert!(ran.events.iter().any(|e| matches!(e, Event::Question { choices, .. } if choices.len() == 2)));
    assert!(ran.seen[2].contains("The person answered: Owner role"), "{}", ran.seen[2]);
}

#[tokio::test]
async fn an_edited_plan_is_followed() {
    let f = fixture().await;
    let turns = vec![plan(), MockTurn::Tools(vec![("finish", json!({ "answer": "OK." }))])];
    let edited = Reply::Go { plan: Some(vec!["Only check memberships".into()]) };
    let ran = go(&f.pool, turns, access(Results::Counts, false), AgentSettings::default(), vec![edited]).await;
    assert!(ran.seen[1].contains("1. Only check memberships"), "{}", ran.seen[1]);
}

#[tokio::test]
async fn made_up_numbers_and_double_counting_joins_are_flagged() {
    let f = fixture().await;
    let sum = "SELECT SUM(t.id) FROM tenants t JOIN patients p ON p.tenant_id = t.id";
    let turns = vec![
        MockTurn::Tools(vec![("run_query", json!({ "sql": "SELECT COUNT(*) AS n FROM appointments", "purpose": "count" }))]),
        MockTurn::Tools(vec![("run_query", json!({ "sql": sum, "purpose": "sum" }))]),
        MockTurn::Tools(vec![("finish", json!({ "answer": "There were 255 appointments, up 12% on last year." }))]),
    ];
    let ran = go(&f.pool, turns, access(Results::Counts, false), AgentSettings::default(), vec![]).await;
    let answer = ran.done().0.clone().unwrap();
    assert_eq!(answer.unverified, ["12"], "255 is in a result; 12% is not");
    let steps = ran.steps();
    assert!(steps[1].checks[0].contains("tenants rows repeat"), "{:?}", steps[1].checks);
    assert!(ran.seen[2].contains("Check: tenants rows repeat"), "the model is told");
}

#[tokio::test]
async fn a_follow_up_continues_the_conversation() {
    let f = fixture().await;
    let first = go(&f.pool, vec![MockTurn::Tools(vec![("finish", json!({ "answer": "Hi." }))])], access(Results::Counts, false), AgentSettings::default(), vec![]).await;
    let schema = db::introspect(&f.pool, Kind::Sqlite).await.unwrap();
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<Reply>();
    drop(tx);
    let mut io = Io { emit: Arc::new(|_| {}), replies: rx, stop: CancellationToken::new() };
    let input = RunInput {
        run_id: "r2".into(),
        question: "Follow-up from the person: and Harbor?".into(),
        kind: Kind::Sqlite,
        schema_text: crate::schema::render_full(&schema),
        schema,
        notes: vec![],
        focus: vec![],
        skill: None,
        access: access(Results::Counts, false),
        limits: AgentSettings::default(),
        history: Some(first.outcome.history),
    };
    let mut llm = Llm::Mock(Mock::new(vec![MockTurn::Tools(vec![("finish", json!({ "answer": "Also." }))])]));
    let out = run::run(&mut llm, &f.pool, input, &mut io).await;
    let Llm::Mock(mock) = llm else { unreachable!() };
    let seen = mock.seen.lock().unwrap().clone();
    assert!(seen[0].contains("Who owns Northfield") && seen[0].contains("and Harbor?"), "{}", seen[0]);
    assert_eq!(out.answer.unwrap().text, "Also.");
}

#[tokio::test]
async fn big_tables_need_a_filter_or_a_checked_plan() {
    let f = fixture().await;
    let mut schema = db::introspect(&f.pool, Kind::Sqlite).await.unwrap();
    schema.tables.iter_mut().find(|t| t.name == "audit_log").unwrap().row_estimate = Some(9_000_000);
    let all = "SELECT COUNT(*) AS n FROM audit_log";
    let turns = vec![
        MockTurn::Tools(vec![("run_query", json!({ "sql": all, "purpose": "x" }))]),
        MockTurn::Tools(vec![("explain_query", json!({ "sql": all }))]),
        MockTurn::Tools(vec![("run_query", json!({ "sql": all, "purpose": "x" }))]),
        MockTurn::Tools(vec![("finish", json!({ "answer": "Done." }))]),
    ];
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<Reply>();
    drop(tx);
    let events = Arc::new(Mutex::new(Vec::new()));
    let sink = events.clone();
    let mut io = Io { emit: Arc::new(move |e| sink.lock().unwrap().push(e)), replies: rx, stop: CancellationToken::new() };
    let input = RunInput {
        run_id: "r".into(),
        question: "How many audit rows?".into(),
        kind: Kind::Sqlite,
        schema_text: crate::schema::render_full(&schema),
        schema,
        notes: vec![],
        focus: vec![],
        skill: None,
        access: access(Results::Counts, false),
        limits: AgentSettings::default(),
        history: None,
    };
    let mut llm = Llm::Mock(Mock::new(turns));
    run::run(&mut llm, &f.pool, input, &mut io).await;
    let steps = steps_of(&events.lock().unwrap());
    assert!(steps[0].error.as_deref().unwrap().contains("reads all of audit_log"), "{steps:?}");
    assert_eq!(steps[1].status, StepStatus::Done, "{steps:?}");
    assert_eq!(steps[2].status, StepStatus::Done, "after the plan check it runs: {steps:?}");
}

#[tokio::test]
async fn follow_ups_mask_values_the_person_quotes() {
    let f = fixture().await;
    let first = go(
        &f.pool,
        vec![
            MockTurn::Tools(vec![("run_query", json!({ "sql": "SELECT email FROM users WHERE id = 9", "purpose": "x" }))]),
            MockTurn::Tools(vec![("finish", json!({ "answer": "Found it." }))]),
        ],
        access(Results::Rows, false),
        AgentSettings::default(),
        vec![],
    )
    .await;
    let schema = db::introspect(&f.pool, Kind::Sqlite).await.unwrap();
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<Reply>();
    drop(tx);
    let mut io = Io { emit: Arc::new(|_| {}), replies: rx, stop: CancellationToken::new() };
    let input = RunInput {
        run_id: "r2".into(),
        question: "I changed step 1's query. Run this instead: SELECT full_name FROM users WHERE email = 'priya.natarajan@example.com'".into(),
        kind: Kind::Sqlite,
        schema_text: crate::schema::render_full(&schema),
        schema,
        notes: vec![],
        focus: vec![],
        skill: None,
        access: access(Results::Rows, false),
        limits: AgentSettings::default(),
        history: Some(first.outcome.history),
    };
    let mut llm = Llm::Mock(Mock::new(vec![MockTurn::Tools(vec![("finish", json!({ "answer": "OK." }))])]));
    run::run(&mut llm, &f.pool, input, &mut io).await;
    let Llm::Mock(mock) = llm else { unreachable!() };
    let seen = mock.seen.lock().unwrap().clone();
    assert!(!seen[0].contains("priya.natarajan@example.com") && seen[0].contains("'‹email-1›'"), "{}", seen[0]);
}

/// profile_column, explain_query and a token follow-up on every database: SQLite always, the
/// servers when SAYQL_TEST_*_URL is set (see db/integration_tests.rs).
#[tokio::test]
async fn tools_work_on_every_database() {
    let f = fixture().await;
    let mut pools = vec![f.pool.clone()];
    pools.extend(tenant_fixture::servers().await);
    for pool in &pools {
        let kind = pool.kind();
        let turns = vec![
            MockTurn::Tools(vec![("profile_column", json!({ "table": "appointments", "column": "status_id" }))]),
            MockTurn::Tools(vec![("explain_query", json!({ "sql": "SELECT COUNT(*) FROM appointments WHERE tenant_id = 3" }))]),
            MockTurn::Tools(vec![("run_query", json!({ "sql": "SELECT email FROM users WHERE id = 9", "purpose": "x" }))]),
            MockTurn::Dynamic(Box::new(|last| {
                let token = &last[last.find('‹').unwrap()..last.find('›').unwrap() + '›'.len_utf8()];
                vec![("run_query", json!({ "sql": format!("SELECT full_name FROM users WHERE email = '{token}'"), "purpose": "y" }))]
            })),
            MockTurn::Tools(vec![("finish", json!({ "answer": "Done." }))]),
        ];
        let schema = db::introspect(pool, kind).await.unwrap();
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<Reply>();
        drop(tx);
        let events = Arc::new(Mutex::new(Vec::new()));
        let sink = events.clone();
        let mut io = Io { emit: Arc::new(move |e| sink.lock().unwrap().push(e)), replies: rx, stop: CancellationToken::new() };
        let input = RunInput {
            run_id: "r".into(),
            question: "q".into(),
            kind,
            schema_text: crate::schema::render_full(&schema),
            schema,
            notes: vec![],
            focus: vec![],
            skill: None,
            access: access(Results::Rows, false),
            limits: AgentSettings::default(),
            history: None,
        };
        let mut llm = Llm::Mock(Mock::new(turns));
        run::run(&mut llm, pool, input, &mut io).await;
        let steps = steps_of(&events.lock().unwrap());
        let profile = &steps[0];
        assert_eq!(profile.status, StepStatus::Done, "{kind:?}: {profile:?}");
        assert!(profile.sent.as_deref().unwrap().contains("\"rows\":255"), "{kind:?}: {:?}", profile.sent);
        if kind == Kind::Mssql {
            assert!(steps[1].error.as_deref().unwrap().contains("SQL Server plans"), "{kind:?}");
        } else {
            assert_eq!(steps[1].status, StepStatus::Done, "{kind:?}: {:?}", steps[1]);
        }
        assert_eq!(steps[3].rows, [[json!("Priya Natarajan")]], "{kind:?}: {:?}", steps[3]);
    }
}
