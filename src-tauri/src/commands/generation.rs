use super::settings::valid_workspace_id;
use crate::ai::{self, GenRequest};
use crate::error::AppResult;
use crate::schema;
use crate::state::AppState;
use crate::store::secrets;
use tauri::{AppHandle, State};

/// Starts streaming SQL for `question`; results arrive as `ai://` events tagged with `req_id`.
/// Supersedes any generation still running.
#[tauri::command]
pub fn generate_sql(app: AppHandle, state: State<'_, AppState>, id: String, question: String, req_id: u64) -> AppResult<()> {
    let live = state.live(&id)?;
    let api_key = secrets::api_key().ok_or("Add your Claude API key in Settings to generate SQL.")?;
    let settings = state.store.settings();
    let request = GenRequest {
        req_id,
        api_key,
        workspace_id: Some(valid_workspace_id(&settings.workspace_id)?).filter(|w| !w.is_empty()),
        model: settings.model,
        dialect: live.schema.dialect.clone(),
        schema_text: schema::schema_for_prompt(&live.schema, &live.schema_text, &question),
        question,
    };
    let handle = tauri::async_runtime::spawn(ai::stream_sql(app, state.http.clone(), request));
    state.replace_generation(handle);
    Ok(())
}

#[tauri::command]
pub fn cancel_generation(state: State<'_, AppState>) {
    state.cancel_generation();
}
