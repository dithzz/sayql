use crate::db::{self, QueryResult};
use crate::error::{AppError, AppResult};
use crate::state::AppState;
use tauri::State;

/// Runs one statement. Statements that change data fail with `WRITE_CONFIRM` unless
/// `allow_write` is set; the UI asks the person and calls again.
#[tauri::command]
pub async fn run_query(state: State<'_, AppState>, id: String, sql: String, allow_write: bool) -> AppResult<QueryResult> {
    let live = state.live(&id)?;
    let read_only = db::is_read_only(&sql);
    if !read_only && !allow_write {
        return Err(AppError::NeedsWriteConfirmation);
    }
    db::run_query(&live.pool, &sql, read_only).await
}
