use super::settings::valid_workspace_id;
use crate::ai::{self, GenRequest, Task};
use crate::error::AppResult;
use crate::schema::{self, Schema};
use crate::state::{AppState, Target};
use crate::store::secrets;
use tauri::{AppHandle, State};

/// Starts streaming SQL for `question`; results arrive as `ai://` events tagged with `req_id`.
/// `task` says whether to draft (fast model), fix a rejected query or improve one (expert
/// model). `focus_tables` (qualified names picked in the navigator) narrow what a draft sees. Supersedes the generation still running in the same `slot` (one per editor tab).
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
) -> AppResult<()> {
    let live = state.schema(&Target::new(id, database))?;
    let api_key = secrets::api_key().ok_or("Add your Claude API key in Settings to generate SQL.")?;
    let settings = state.store.settings();
    let request = GenRequest {
        req_id,
        api_key,
        workspace_id: Some(valid_workspace_id(&settings.workspace_id)?).filter(|w| !w.is_empty()),
        model: if task.needs_expert() { settings.expert_model } else { settings.typing_model },
        dialect: live.schema.dialect.clone(),
        schema_text: schema_text(&live.schema, &live.schema_text, &question, &task, focus_tables.as_deref().unwrap_or(&[])),
        question,
        task,
    };
    let handle = tauri::async_runtime::spawn(ai::stream_sql(app, state.http.clone(), request));
    state.replace_generation(&slot, handle);
    Ok(())
}

#[tauri::command]
pub fn cancel_generation(state: State<'_, AppState>, slot: String) {
    state.cancel_generation(&slot);
}

/// What schema the model sees. Drafts with picked tables see just those and their neighbours
/// (smaller and faster); fixes and improvements always see the usual schema, so a narrow
/// pick can't make the right query impossible.
fn schema_text(schema: &Schema, full: &str, question: &str, task: &Task, focus: &[String]) -> String {
    if !task.needs_expert() {
        if let Some(text) = schema::schema_with_focus(schema, focus) {
            return text;
        }
    }
    // A follow-up like "remove the middle name" names no table; the SQL that comes with it does.
    let relevance = format!("{question} {} {}", task.context_sql(), focus.join(" "));
    schema::schema_for_prompt(schema, full, &relevance)
}
