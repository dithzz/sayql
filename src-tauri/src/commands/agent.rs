use crate::ai::Api;
use crate::ai::agent::model::{Llm, ToolMode};
use crate::ai::agent::run::{self, Event, Io, Reply, RunInput, StepStatus};
use crate::ai::endpoint::endpoint;
use crate::error::{AppError, AppResult};
use crate::state::{AgentRun, AppState, Target};
use crate::store::agent::{AgentAccess, Note};
use crate::store::my_activity::MyActivity;
use crate::store::suggestions::NoteSuggestion;
use crate::team::{self, Source, TeamRules};
use serde::Serialize;
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager, State};
use tokio_util::sync::CancellationToken;

/// Must match AGENT_EVENT in src/api/events.ts.
const EVENT: &str = "agent://event";

/// Starts an agent run in `slot` (one per editor tab), stopping the run already there.
/// Progress arrives as `agent://event`s tagged with `run_id`. `follow_up` continues the
/// tab's last run (its queries and answer, not its rows) instead of starting fresh.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn start_agent(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
    database: Option<String>,
    question: String,
    run_id: String,
    slot: String,
    focus_tables: Option<Vec<String>>,
    skill: Option<String>,
    follow_up: Option<bool>,
    auto: Option<bool>,
) -> AppResult<()> {
    let question = question.trim().to_string();
    if question.is_empty() {
        return Err("Type a question first.".into());
    }
    // For your activity log: each query the run makes, with what went to the AI.
    let (log_id, log_database, log_question) = (id.clone(), database.clone(), question.clone());
    if state.team_connection(&id).is_some_and(|c| c.agent_off()) {
        return Err(team::AGENT_OFF.into());
    }
    let mut access = effective_access(&state, &id)?.access;
    if !access.enabled {
        return Err(
            "Investigate is off for this connection. Turn it on in its Investigate settings first."
                .into(),
        );
    }
    let target = Target::new(id.clone(), database.clone());
    let live = state.schema(&target)?;
    let pool = state
        .target_pool(&target)
        .await?
        .with_context(Source::Agent, Some(question.clone()));
    let mut settings = state.store.settings()?;
    let own = endpoint(&settings);
    let team_ai = state.team_ai_route(&id, "agent", own.is_ok());
    // Auto mode: no stop for the plan or before each send. What the AI may see (the results
    // setting, masking, never-send columns) stays as set; questions with two meanings still ask.
    if auto == Some(true) {
        settings.agent.skip_plan = true;
        access.ask_before_send = false;
    }
    // A team connection with team AI runs on the team's default model, through the team server.
    let (endpoint, model, provider) = match team_ai {
        Some((route, ai)) => (route, ai.default_model(), "team".to_string()),
        None => (
            own?,
            settings
                .provider_settings(&settings.provider)
                .agent()
                .to_string(),
            settings.provider.clone(),
        ),
    };
    let mode_key = format!("{provider}/{model}");
    // Claude Code runs without tools, so the agent's tools travel as text.
    let mode = if matches!(endpoint, crate::ai::Endpoint::Plan) {
        ToolMode::Text
    } else if endpoint.is_openai() {
        settings
            .tool_modes
            .get(&mode_key)
            .copied()
            .unwrap_or(ToolMode::Native)
    } else {
        ToolMode::Native
    };
    let skill = match skill {
        None => None,
        Some(slug) => Some(
            state
                .store
                .skills()?
                .into_iter()
                .find(|s| s.slug == slug)
                .map(|s| s.instruction)
                .ok_or_else(|| {
                    AppError::User(format!(
                        "There's no skill called /{slug}. Create it in Settings."
                    ))
                })?,
        ),
    };
    let notes = state
        .store
        .notes(&id, database.as_deref())?
        .into_iter()
        .map(|n| n.text)
        .collect();
    let focus = focus_tables.unwrap_or_default();
    let relevance = format!("{question} {}", focus.join(" "));
    let history = if follow_up == Some(true) {
        state.agent_history(&slot)
    } else {
        None
    };
    let question = match &history {
        Some(_) => format!("Follow-up from the person: {question}"),
        None => question,
    };
    let input = RunInput {
        run_id: run_id.clone(),
        question,
        kind: pool.kind(),
        schema_text: crate::ai::agent::prompt::agent_schema(
            &live.schema,
            &live.schema_text,
            &relevance,
        ),
        schema: live.schema.clone(),
        notes,
        focus,
        skill,
        access,
        limits: settings.agent.clone(),
        history,
    };
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    let stop = CancellationToken::new();
    state.start_agent(
        &slot,
        AgentRun {
            run_id: run_id.clone(),
            replies: tx,
            stop: stop.clone(),
        },
    );
    let emitter = app.clone();
    let mut io = Io {
        emit: Arc::new(move |e: Event| {
            if let Event::Step { step, .. } = &e
                && step.status != StepStatus::Running
                && let Some(sql) = &step.sql
            {
                let mut entry =
                    MyActivity::new(&log_id, log_database.as_deref(), "investigate", sql);
                entry.question = Some(log_question.clone());
                entry.rows = step.row_count;
                entry.ms = step.elapsed_ms.map(|ms| ms as u64);
                if step.status == StepStatus::Error {
                    entry.status = "error".into();
                    entry.detail = step.error.clone();
                } else {
                    entry.detail = step.sent_summary.clone();
                }
                let _ = emitter.state::<AppState>().store.log_my_activity(entry);
            }
            let _ = emitter.emit(EVENT, e);
        }),
        replies: rx,
        stop,
    };
    let http = state.http.clone();
    tauri::async_runtime::spawn(async move {
        let mut llm = Llm::Remote {
            http,
            endpoint,
            model,
            mode,
        };
        let outcome = run::run(&mut llm, &pool, input, &mut io).await;
        let state = app.state::<AppState>();
        state.finish_agent(&slot, &run_id, outcome.history);
        // Remember a model that needed the text protocol, so it isn't asked for tools again.
        if outcome.mode != mode
            && crate::ai::find_provider(&provider).is_some_and(|p| p.api == Api::OpenAi)
            && let Ok(mut s) = state.store.settings()
        {
            s.tool_modes.insert(mode_key, outcome.mode);
            let _ = state.store.save_settings(&s);
        }
    });
    Ok(())
}

/// Stops a run: the model request and the running query, on the server too.
#[tauri::command]
pub fn stop_agent(state: State<'_, AppState>, run_id: String) {
    state.stop_agent(&run_id);
}

/// The person's reply to a paused run: Go on the plan, Send or Skip, or an answer.
#[tauri::command]
pub fn reply_agent(state: State<'_, AppState>, run_id: String, reply: Reply) -> AppResult<()> {
    state
        .reply_agent(&run_id, reply)
        .then_some(())
        .ok_or_else(|| "This run has already ended.".into())
}

/// A connection's agent access as it applies, and on a team connection the admin's rules,
/// which the UI shows as "Set by your team admin".
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccessView {
    #[serde(flatten)]
    access: AgentAccess,
    team: Option<TeamRules>,
}

/// The person's choices with the team admin's rules on top, on a team connection.
fn effective_access(state: &AppState, id: &str) -> AppResult<AccessView> {
    let own = state.store.agent_access(id)?;
    Ok(match state.team_connection(id).map(|c| c.rules()) {
        Some(rules) => AccessView {
            access: team::apply_rules(own, &rules),
            team: Some(rules),
        },
        None if id.starts_with(team::ID_PREFIX) => return Err(team::SIGNED_OUT.into()),
        None => AccessView {
            access: own,
            team: None,
        },
    })
}

#[tauri::command]
pub fn agent_access(state: State<'_, AppState>, id: String) -> AppResult<AccessView> {
    effective_access(&state, &id)
}

/// Saves the person's choices. On a team connection the results setting can't go above the
/// admin's, and the admin's never-send columns aren't saved (they're always added back).
#[tauri::command]
pub fn save_agent_access(
    state: State<'_, AppState>,
    id: String,
    access: AgentAccess,
) -> AppResult<AccessView> {
    state.check_connection(&id)?;
    let access = match state.team_connection(&id) {
        Some(c) => team::own_access(access, &c.rules()),
        None => access,
    };
    state.store.save_agent_access(&id, access)?;
    effective_access(&state, &id)
}

#[tauri::command]
pub fn list_notes(
    state: State<'_, AppState>,
    id: String,
    database: Option<String>,
) -> AppResult<Vec<Note>> {
    state
        .store
        .notes(&id, database.as_deref().filter(|d| !d.trim().is_empty()))
}

/// Saves a database note: new (empty `id`) or edited.
#[tauri::command]
pub fn save_note(state: State<'_, AppState>, note: Note) -> AppResult<Note> {
    state.check_connection(&note.connection_id)?;
    let id = if note.id.is_empty() {
        uuid::Uuid::new_v4().to_string()
    } else {
        note.id.clone()
    };
    let database = note.database.clone().filter(|d| !d.trim().is_empty());
    state.store.save_note(Note {
        id,
        database,
        ..note
    })
}

#[tauri::command]
pub fn delete_note(state: State<'_, AppState>, id: String) -> AppResult<()> {
    state.store.delete_note(&id)
}

/// Notes AI apps suggested over MCP, waiting for the person (all connections, oldest first).
#[tauri::command]
pub fn list_note_suggestions(state: State<'_, AppState>) -> AppResult<Vec<NoteSuggestion>> {
    state.store.note_suggestions()
}

/// Applies a suggested note: saves it as an ordinary note, replaces the note it corrects, or
/// deletes the note it removes (then `None`).
#[tauri::command]
pub fn accept_note_suggestion(state: State<'_, AppState>, id: String) -> AppResult<Option<Note>> {
    state.store.accept_note_suggestion(&id)
}

#[tauri::command]
pub fn dismiss_note_suggestion(state: State<'_, AppState>, id: String) -> AppResult<()> {
    state.store.dismiss_note_suggestion(&id)
}
