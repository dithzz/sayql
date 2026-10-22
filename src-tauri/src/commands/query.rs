use crate::db::{self, QueryResult};
use crate::error::{AppError, AppResult};
use crate::state::{AppState, Target};
use tauri::State;

/// Runs one statement. Statements that change data fail with `WRITE_CONFIRM` unless
/// `allow_write` is set; the UI asks the person and calls again.
#[tauri::command]
pub async fn run_query(
    state: State<'_, AppState>,
    id: String,
    database: Option<String>,
    sql: String,
    allow_write: bool,
) -> AppResult<QueryResult> {
    let pool = state.open_pool(&Target::new(id, database))?;
    let read_only = db::is_read_only(&sql, pool.kind());
    if !read_only && !allow_write {
        return Err(AppError::NeedsWriteConfirmation);
    }
    db::run_query(&pool, &sql, read_only).await
}
