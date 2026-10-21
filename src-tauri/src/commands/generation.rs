use super::settings::valid_workspace_id;
use crate::ai::{self, GenRequest, PreviousQuery};
use crate::error::AppResult;
use crate::schema;
use crate::state::{AppState, Target};
use crate::store::secrets;
use tauri::{AppHandle, State};

/// Starts streaming SQL for `question`; results arrive as `ai://` events tagged with `req_id`.
/// `previous` is the tab's last run query, which a follow-up request may modify.
/// Supersedes the generation still running in the same `slot` (one per editor tab).
#[tauri::command]
pub fn generate_sql(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
    database: Option<String>,
    question: String,
    req_id: u64,
    slot: String,
    previous: Option<PreviousQuery>,
) -> AppResult<()> {
    let live = state.schema(&Target::new(id, database))?;
    let api_key = secrets::api_key().ok_or("Add your Claude API key in Settings to generate SQL.")?;
    let settings = state.store.settings();
    let request = GenRequest {
        req_id,
        api_key,
        workspace_id: Some(valid_workspace_id(&settings.workspace_id)?).filter(|w| !w.is_empty()),
        model: settings.model,
        dialect: live.schema.dialect.clone(),
        // A follow-up like "remove the middle name" names no table; the previous SQL does.
        schema_text: schema::schema_for_prompt(&live.schema, &live.schema_text, &relevance_text(&question, previous.as_ref())),
        question,
        previous,
    };
    let handle = tauri::async_runtime::spawn(ai::stream_sql(app, state.http.clone(), request));
    state.replace_generation(&slot, handle);
    Ok(())
}

#[tauri::command]
pub fn cancel_generation(state: State<'_, AppState>, slot: String) {
    state.cancel_generation(&slot);
}

/// Text used to pick the relevant tables of a large schema.
fn relevance_text(question: &str, previous: Option<&PreviousQuery>) -> String {
    match previous {
        Some(p) => format!("{question} {} {}", p.question, p.sql),
        None => question.to_string(),
    }
}
