use crate::ai::endpoint::endpoint;
use crate::ai::{self, GenRequest, Task, context::schema_text};
use crate::error::{AppError, AppResult};
use crate::state::{AppState, Target};
use tauri::{AppHandle, State};

/// Starts streaming SQL for `question`; results arrive as `ai://` events tagged with `req_id`.
/// `task` says whether to draft (fast model), fix a rejected query or improve one (expert
/// model). `focus_tables` (qualified names picked in the navigator) narrow what a draft sees.
/// `skill` (a saved skill's slug) adds that skill's instruction to the request. Supersedes the generation still running in the same `slot` (one per editor tab).
#[tauri::command]
pub fn generate_sql(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
    database: Option<String>,
    question: String,
    req_id: u64,
    slot: String,
    task: Task,
    focus_tables: Option<Vec<String>>,
    skill: Option<String>,
) -> AppResult<()> {
    let live = state.schema(&Target::new(id, database))?;
    let skill = match skill {
        None => None,
        Some(slug) => Some(
            state.store.skills()?.into_iter().find(|s| s.slug == slug).map(|s| s.instruction).ok_or_else(|| {
                AppError::User(format!("There's no skill called /{slug}. Create it in Settings."))
            })?,
        ),
    };
    let settings = state.store.settings()?;
    let request = GenRequest {
        req_id,
        endpoint: endpoint(&settings)?,
        model: settings.provider_settings(&settings.provider).model(task.needs_expert()).to_string(),
        dialect: live.schema.dialect.clone(),
        schema_text: schema_text(&live.schema, &live.schema_text, &question, &task, focus_tables.as_deref().unwrap_or(&[])),
        question,
        task,
        skill,
    };
    let handle = tauri::async_runtime::spawn(ai::stream_sql(app, state.http.clone(), request));
    state.replace_generation(&slot, handle);
    Ok(())
}

#[tauri::command]
pub fn cancel_generation(state: State<'_, AppState>, slot: String) {
    state.cancel_generation(&slot);
}
