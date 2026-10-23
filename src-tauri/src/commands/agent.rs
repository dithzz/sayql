use crate::ai::agent::model::{Llm, ToolMode};
use crate::ai::agent::run::{self, Event, Io, Reply, RunInput};
use crate::ai::endpoint::endpoint;
use crate::ai::{Api, Endpoint};
use crate::error::{AppError, AppResult};
use crate::state::{AgentRun, AppState, Target};
use crate::store::agent::{AgentAccess, Note};
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
    let mut access = state.store.agent_access(&id)?;
    if !access.enabled {
        return Err("Agent mode is off for this connection. Turn it on in its agent settings first.".into());
    }
    let target = Target::new(id.clone(), database.clone());
    let live = state.schema(&target)?;
    let pool = state.target_pool(&target).await?;
    let mut settings = state.store.settings()?;
    let endpoint = endpoint(&settings)?;
    // Auto mode: no stop for the plan or before each send. What the AI may see (the results
    // setting, masking, never-send columns) stays as set; questions with two meanings still ask.
    if auto == Some(true) {
        settings.agent.skip_plan = true;
        access.ask_before_send = false;
    }
    let model = settings.provider_settings(&settings.provider).agent().to_string();
    let mode_key = format!("{}/{model}", settings.provider);
    let mode = match &endpoint {
        Endpoint::Anthropic { .. } => ToolMode::Native,
        Endpoint::OpenAi { .. } => settings.tool_modes.get(&mode_key).copied().unwrap_or(ToolMode::Native),
    };
    let skill = match skill {
        None => None,
        Some(slug) => Some(
            state.store.skills()?.into_iter().find(|s| s.slug == slug).map(|s| s.instruction).ok_or_else(|| {
                AppError::User(format!("There's no skill called /{slug}. Create it in Settings."))
            })?,
        ),
    };
    let notes = state.store.notes(&id, database.as_deref())?.into_iter().map(|n| n.text).collect();
    let focus = focus_tables.unwrap_or_default();
    let relevance = format!("{question} {}", focus.join(" "));
    let history = if follow_up == Some(true) { state.agent_history(&slot) } else { None };
    let question = match &history {
        Some(_) => format!("Follow-up from the person: {question}"),
        None => question,
    };
    let input = RunInput {
        run_id: run_id.clone(),
        question,
        kind: pool.kind(),
        schema_text: crate::ai::agent::prompt::agent_schema(&live.schema, &live.schema_text, &relevance),
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
    state.start_agent(&slot, AgentRun { run_id: run_id.clone(), replies: tx, stop: stop.clone() });
    let emitter = app.clone();
    let mut io = Io { emit: Arc::new(move |e: Event| { let _ = emitter.emit(EVENT, e); }), replies: rx, stop };
    let http = state.http.clone();
    let provider = settings.provider.clone();
    tauri::async_runtime::spawn(async move {
        let mut llm = Llm::Remote { http, endpoint, model, mode };
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
    state.reply_agent(&run_id, reply).then_some(()).ok_or_else(|| "This run has already ended.".into())
}

#[tauri::command]
pub fn agent_access(state: State<'_, AppState>, id: String) -> AppResult<AgentAccess> {
    state.store.agent_access(&id)
}

#[tauri::command]
pub fn save_agent_access(state: State<'_, AppState>, id: String, access: AgentAccess) -> AppResult<AgentAccess> {
    state.store.connection(&id)?;
    state.store.save_agent_access(&id, access)?;
    state.store.agent_access(&id)
}

#[tauri::command]
pub fn list_notes(state: State<'_, AppState>, id: String, database: Option<String>) -> AppResult<Vec<Note>> {
    state.store.notes(&id, database.as_deref().filter(|d| !d.trim().is_empty()))
}

/// Saves a database note: new (empty `id`) or edited.
#[tauri::command]
pub fn save_note(state: State<'_, AppState>, note: Note) -> AppResult<Note> {
    state.store.connection(&note.connection_id)?;
    let id = if note.id.is_empty() { uuid::Uuid::new_v4().to_string() } else { note.id.clone() };
    let database = note.database.clone().filter(|d| !d.trim().is_empty());
    state.store.save_note(Note { id, database, ..note })
}

#[tauri::command]
pub fn delete_note(state: State<'_, AppState>, id: String) -> AppResult<()> {
    state.store.delete_note(&id)
}
