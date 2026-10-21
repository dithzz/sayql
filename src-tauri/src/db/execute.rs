use super::decode::{lite_cell, ms_cell, my_cell, pg_cell};
use super::mssql_pool::MssqlClient;
use super::{DbPool, MAX_ROWS, MssqlPool, QueryResult};
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
    if read_only && !super::is_read_only(sql) {
        return Err("This statement can change data, so it can't run in read-only mode.".into());
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
        DbPool::Ms(p) => run_mssql(p, sql, read_only).await?,
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

/// SQL Server has no read-only transaction mode. Reads run inside a transaction that is
/// always rolled back, on top of the keyword check in `classify` that sends anything
/// containing a write keyword to the confirmation path instead.
async fn run_mssql(pool: &MssqlPool, sql: &str, read_only: bool) -> AppResult<QueryResult> {
    let mut conn = pool.acquire().await?;
    if !read_only {
        let done = conn.client().execute(sql, &[]).await?;
        let affected = done.rows_affected().iter().sum();
        return Ok(QueryResult { columns: vec![], rows: vec![], rows_affected: Some(affected), truncated: false, elapsed_ms: 0 });
    }

    conn.client().simple_query("BEGIN TRANSACTION").await?.into_results().await?;
    let out = collect_mssql(conn.client(), sql).await;
    // A cut-off result leaves unread rows on the wire, and a failed rollback leaves the
    // transaction open: either way, close the connection (the server rolls back).
    let cut_off = out.as_ref().map(|r| r.truncated).unwrap_or(false);
    let rolled_back = !cut_off && rollback(conn.client()).await.is_ok();
    if !rolled_back {
        conn.discard();
    }
    out
}

async fn rollback(client: &mut MssqlClient) -> AppResult<()> {
    client.simple_query("IF @@TRANCOUNT > 0 ROLLBACK").await?.into_results().await?;
    Ok(())
}

/// Collects the first result set, up to MAX_ROWS rows.
async fn collect_mssql(client: &mut MssqlClient, sql: &str) -> AppResult<QueryResult> {
    let mut stream = client.simple_query(sql).await?;
    let columns: Vec<String> = stream
        .columns()
        .await?
        .map(|cols| cols.iter().map(|c| c.name().to_string()).collect())
        .unwrap_or_default();
    let mut rows = Vec::new();
    let mut truncated = false;
    while let Some(item) = stream.try_next().await? {
        let Some(row) = item.into_row() else { continue };
        if row.result_index() > 0 {
            continue;
        }
        if rows.len() >= MAX_ROWS {
            truncated = true;
            break;
        }
        rows.push((0..row.len()).map(|i| ms_cell(&row, i)).collect());
    }
    Ok(QueryResult { columns, rows, rows_affected: None, truncated, elapsed_ms: 0 })
}
