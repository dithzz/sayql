use crate::db::{self, DbPool, Kind, QueryResult};
use crate::error::{AppError, AppResult};
use crate::state::{AppState, Target};
use crate::team::{self, Source};
use tauri::State;

/// Runs one statement. Statements that change data fail with `WRITE_CONFIRM` unless
/// `allow_write` is set; the UI asks the person and calls again. `cancel_query(run_id)` stops it.
/// On a team connection the query goes to the team server, tagged with `source` (`ask` or
/// `editor`) and the `question` it answers, and only read-only statements are sent.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn run_query(
    state: State<'_, AppState>,
    id: String,
    database: Option<String>,
    sql: String,
    allow_write: bool,
    run_id: Option<String>,
    source: Option<Source>,
    question: Option<String>,
) -> AppResult<QueryResult> {
    let pool = state.open_pool(&Target::new(id, database))?;
    let pool = pool.with_context(source.unwrap_or_default(), question);
    let read_only = match pool {
        DbPool::Team(_) if !db::is_read_only(&sql, pool.kind()) => return Err(team::WRITE_REFUSED.into()),
        DbPool::Team(_) => true,
        _ => write_gate(&sql, pool.kind(), allow_write)?,
    };
    let Some(run_id) = run_id else { return db::run_query(&pool, &sql, read_only).await };
    let cancel = state.start_query(&run_id);
    let out = db::run_query_with(&pool, &sql, read_only, db::MAX_ROWS, &cancel).await;
    state.finish_query(&run_id);
    out
}

/// Stops the query started with `run_id`, if it's still running; it then fails with
/// `QUERY_CANCELLED`.
#[tauri::command]
pub fn cancel_query(state: State<'_, AppState>, run_id: String) {
    state.cancel_query(&run_id);
}

/// Whether `sql` may run, and if so whether it runs read-only. A statement that changes
/// data needs `allow_write`.
fn write_gate(sql: &str, kind: Kind, allow_write: bool) -> AppResult<bool> {
    let read_only = db::is_read_only(sql, kind);
    if !read_only && !allow_write {
        return Err(AppError::NeedsWriteConfirmation);
    }
    Ok(read_only)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_run_read_only_with_or_without_permission() {
        for allow_write in [false, true] {
            assert!(write_gate("SELECT * FROM orders", Kind::Postgres, allow_write).unwrap());
        }
    }

    #[test]
    fn writes_need_confirmation() {
        for sql in ["DELETE FROM orders", "UPDATE orders SET status = 'x'", "DROP TABLE orders"] {
            assert!(matches!(write_gate(sql, Kind::Postgres, false), Err(AppError::NeedsWriteConfirmation)), "{sql}");
        }
    }

    #[test]
    fn confirmed_writes_run_outside_a_read_only_transaction() {
        assert!(!write_gate("DELETE FROM orders", Kind::Postgres, true).unwrap());
    }
}
