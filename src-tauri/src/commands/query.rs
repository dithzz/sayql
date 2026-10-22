use crate::db::{self, Kind, QueryResult};
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
    let read_only = write_gate(&sql, pool.kind(), allow_write)?;
    db::run_query(&pool, &sql, read_only).await
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
