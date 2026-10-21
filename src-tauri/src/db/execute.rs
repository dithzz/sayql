use super::decode::{lite_cell, my_cell, pg_cell};
use super::{DbPool, MAX_ROWS, QueryResult};
use crate::error::AppResult;
use futures_util::TryStreamExt;
use serde_json::Value;
use sqlx::{Column as _, Either, Executor, Row, Statement};
use std::time::Instant;

pub async fn run_query(pool: &DbPool, sql: &str, read_only: bool) -> AppResult<QueryResult> {
    let sql = sql.trim().trim_end_matches(';').trim();
    if sql.is_empty() {
        return Err("Nothing to run.".into());
    }
    let start = Instant::now();
    let mut res = match pool {
        DbPool::Pg(p) => {
            let mut conn = p.acquire().await?;
            if read_only {
                conn.execute("BEGIN READ ONLY").await?;
            }
            let out = collect_pg(&mut conn, sql).await;
            if read_only && conn.execute("ROLLBACK").await.is_err() {
                // Never hand a connection with an open transaction back to the pool.
                conn.detach();
            }
            out?
        }
        DbPool::My(p) => {
            let mut conn = p.acquire().await?;
            if read_only {
                conn.execute("START TRANSACTION READ ONLY").await?;
            }
            let out = collect_my(&mut conn, sql).await;
            if read_only && conn.execute("ROLLBACK").await.is_err() {
                conn.detach();
            }
            out?
        }
        DbPool::Lite(p) => {
            let mut conn = if read_only { p.ro.acquire() } else { p.rw.acquire() }.await?;
            collect_lite(&mut conn, sql).await?
        }
    };
    res.elapsed_ms = start.elapsed().as_millis();
    Ok(res)
}

/// Runs one statement, collecting up to MAX_ROWS rows and the affected-row count.
/// A macro rather than a generic fn: generic sqlx executors make the future
/// non-`Send` across the Tauri command boundary.
macro_rules! collect_fn {
    ($name:ident, $conn:ty, $cell:ident) => {
        async fn $name(conn: &mut $conn, sql: &str) -> AppResult<QueryResult> {
            let mut columns: Vec<String> = Vec::new();
            let mut rows: Vec<Vec<Value>> = Vec::new();
            let mut affected: Option<u64> = None;
            let mut truncated = false;
            {
                #[allow(deprecated)]
                let mut stream = sqlx::query(sql).fetch_many(&mut *conn);
                while let Some(item) = stream.try_next().await? {
                    match item {
                        Either::Left(r) => {
                            affected = Some(affected.unwrap_or(0) + r.rows_affected());
                        }
                        Either::Right(row) => {
                            if columns.is_empty() {
                                columns = row.columns().iter().map(|c| c.name().to_string()).collect();
                            }
                            if rows.len() >= MAX_ROWS {
                                truncated = true;
                                break;
                            }
                            rows.push((0..row.columns().len()).map(|i| $cell(&row, i)).collect());
                        }
                    }
                }
            }
            if columns.is_empty() && rows.is_empty() {
                // No rows came back; still show the column headers if the statement has any.
                if let Ok(stmt) = (&mut *conn).prepare(sql).await {
                    columns = stmt.columns().iter().map(|c| c.name().to_string()).collect();
                }
            }
            let rows_affected = if columns.is_empty() { affected } else { None };
            Ok(QueryResult { columns, rows, rows_affected, truncated, elapsed_ms: 0 })
        }
    };
}

collect_fn!(collect_pg, sqlx::PgConnection, pg_cell);
collect_fn!(collect_my, sqlx::MySqlConnection, my_cell);
collect_fn!(collect_lite, sqlx::SqliteConnection, lite_cell);
