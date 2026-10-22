use super::decode::{lite_cell, ms_cell, my_cell, pg_cell};
use super::mssql_pool::MssqlClient;
use super::{DbPool, MAX_ROWS, MssqlPool, QueryResult};
use crate::error::AppResult;
use futures_util::TryStreamExt;
use serde_json::Value;
use sqlx::{AssertSqlSafe, Column as _, Either, Executor, Row, SqlSafeStr, Statement};
use std::future::Future;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use tiberius::Command;
use tokio_util::sync::CancellationToken;

/// The error of a query stopped with its `CancellationToken`. Must match `QUERY_CANCELLED`
/// in src/api/types.ts.
pub const CANCELLED: &str = "Query cancelled.";

/// How long a cancelled query gets to stop before its connection is closed anyway.
const STOP_GRACE: Duration = Duration::from_secs(5);

pub async fn run_query(pool: &DbPool, sql: &str, read_only: bool) -> AppResult<QueryResult> {
    run_query_capped(pool, sql, read_only, MAX_ROWS).await
}

/// `run_query`, collecting up to `cap` rows instead of MAX_ROWS (for exports).
pub async fn run_query_capped(pool: &DbPool, sql: &str, read_only: bool, cap: usize) -> AppResult<QueryResult> {
    run_query_with(pool, sql, read_only, cap, &CancellationToken::new()).await
}

/// `run_query_capped` that can be stopped: when `cancel` fires, the database is asked to stop
/// the statement (it would otherwise run on after SayQL stops waiting), the connection is
/// closed, and the result is the `CANCELLED` error.
pub async fn run_query_with(
    pool: &DbPool,
    sql: &str,
    read_only: bool,
    cap: usize,
    cancel: &CancellationToken,
) -> AppResult<QueryResult> {
    let sql = sql.trim().trim_end_matches(';').trim();
    if sql.is_empty() {
        return Err("Nothing to run.".into());
    }
    if read_only && !super::is_read_only(sql, pool.kind()) {
        return Err("This statement can change data, so it can't run in read-only mode.".into());
    }
    let start = Instant::now();
    // A confirmed statement can leave session state behind: a SET, USE, LOCK TABLES, temp
    // table or session lock. Its connection is closed afterwards instead of going back to the
    // pool, so none of that carries over to later queries (a held LOCK TABLES would make
    // them wait forever).
    let mut res = match pool {
        DbPool::Pg(p) => {
            let mut conn = p.acquire().await?;
            // The server process's id, to cancel from another connection; in the same round trip.
            let begin = if read_only { "BEGIN READ ONLY; SELECT pg_backend_pid()" } else { "SELECT pg_backend_pid()" };
            let pid: i32 = sqlx::raw_sql(begin).fetch_all(&mut *conn).await?.first().map(|r| r.try_get(0)).transpose()?.unwrap_or(0);
            let out = {
                let mut query = Box::pin(collect_pg(&mut conn, sql, cap));
                match race(&mut query, cancel).await {
                    Some(out) => Some(out),
                    None => {
                        let _ = sqlx::query("SELECT pg_cancel_backend($1)").bind(pid).execute(p).await;
                        let _ = tokio::time::timeout(STOP_GRACE, &mut query).await;
                        None
                    }
                }
            };
            let Some(out) = out else {
                let _ = conn.close().await;
                return Err(CANCELLED.into());
            };
            if !read_only || conn.execute("ROLLBACK").await.is_err() {
                // Never hand a connection with an open transaction back to the pool.
                let _ = conn.close().await;
            }
            out?
        }
        DbPool::My(p) => {
            let mut conn = p.acquire().await?;
            if read_only {
                conn.execute("START TRANSACTION READ ONLY").await?;
            }
            // The connection's id, to stop the query from another connection with KILL QUERY.
            let id: u64 = sqlx::query_scalar("SELECT CONNECTION_ID()").fetch_one(&mut *conn).await?;
            let out = {
                let mut query = Box::pin(collect_my(&mut conn, sql, cap));
                match race(&mut query, cancel).await {
                    Some(out) => Some(out),
                    None => {
                        // KILL can't be a prepared statement; `id` is the server's own number.
                        let kill = format!("KILL QUERY {id}");
                        let _ = sqlx::raw_sql(AssertSqlSafe(kill)).execute(p).await;
                        let _ = tokio::time::timeout(STOP_GRACE, &mut query).await;
                        None
                    }
                }
            };
            let Some(out) = out else {
                let _ = conn.close().await;
                return Err(CANCELLED.into());
            };
            if !read_only || conn.execute("ROLLBACK").await.is_err() {
                let _ = conn.close().await;
            }
            out?
        }
        DbPool::Lite(p) => {
            let mut conn = if read_only { p.ro.acquire() } else { p.rw.acquire() }.await?;
            // SQLite runs in-process: a progress handler checked every few thousand steps
            // interrupts the statement once `stop` is set.
            let stop = Arc::new(AtomicBool::new(false));
            let flag = stop.clone();
            conn.lock_handle().await?.set_progress_handler(4000, move || !flag.load(Ordering::Relaxed));
            let out = {
                let mut query = Box::pin(collect_lite(&mut conn, sql, cap));
                match race(&mut query, cancel).await {
                    Some(out) => Some(out),
                    None => {
                        stop.store(true, Ordering::Relaxed);
                        let _ = tokio::time::timeout(STOP_GRACE, &mut query).await;
                        None
                    }
                }
            };
            let Some(out) = out else {
                let _ = conn.close().await;
                return Err(CANCELLED.into());
            };
            if !read_only {
                let _ = conn.close().await;
            }
            out?
        }
        DbPool::Ms(p) => Box::pin(run_mssql(p, sql, read_only, cap, cancel)).await?,
    };
    res.elapsed_ms = start.elapsed().as_millis();
    Ok(res)
}

/// Runs one statement, collecting up to `cap` rows and the affected-row count.
/// A macro rather than a generic fn: generic sqlx executors make the future
/// non-`Send` across the Tauri command boundary.
macro_rules! collect_fn {
    ($name:ident, $conn:ty, $cell:ident, $first_result_only:expr) => {
        async fn $name(conn: &mut $conn, sql: &str, cap: usize) -> AppResult<QueryResult> {
            // Running the user's own SQL is the point; read-only mode is enforced around this call.
            let sql = AssertSqlSafe(sql).into_sql_str();
            let mut columns: Vec<String> = Vec::new();
            let mut rows: Vec<Vec<Value>> = Vec::new();
            let mut affected: Option<u64> = None;
            let mut truncated = false;
            {
                #[allow(deprecated)]
                let mut stream = sqlx::query(sql.clone()).fetch_many(&mut *conn);
                while let Some(item) = stream.try_next().await? {
                    match item {
                        Either::Left(r) => {
                            affected = Some(affected.unwrap_or(0) + r.rows_affected());
                            // SQLite runs every statement of a script in one call; show the first
                            // result set only, as on the other databases. (Postgres and MySQL
                            // prepare one statement, and stopping early here would leave MySQL's
                            // last packet unread.)
                            if $first_result_only && !columns.is_empty() {
                                break;
                            }
                        }
                        Either::Right(row) => {
                            if columns.is_empty() {
                                columns = row.columns().iter().map(|c| c.name().to_string()).collect();
                            }
                            if rows.len() >= cap {
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

collect_fn!(collect_pg, sqlx::PgConnection, pg_cell, false);
collect_fn!(collect_my, sqlx::MySqlConnection, my_cell, false);
collect_fn!(collect_lite, sqlx::SqliteConnection, lite_cell, true);

/// SQL Server has no read-only transaction mode. Reads run inside a transaction that is
/// always rolled back, on top of the keyword check in `classify` that sends anything
/// containing a write keyword to the confirmation path instead.
async fn run_mssql(pool: &MssqlPool, sql: &str, read_only: bool, cap: usize, cancel: &CancellationToken) -> AppResult<QueryResult> {
    let mut conn = pool.acquire().await?;
    if !read_only {
        // sp_executesql reports both the rows a statement returns (INSERT ... OUTPUT,
        // DECLARE ...; SELECT) and how many rows it changed. It reads every row, which is fine
        // for statements the person confirmed one by one.
        let mut cmd = Command::new("sp_executesql");
        cmd.bind_param("@stmt", sql.to_string());
        let out = {
            let mut query = Box::pin(async {
                match cmd.exec(conn.client()).await {
                    Ok(stream) => stream.into_command_result().await,
                    Err(e) => Err(e),
                }
            });
            race(&mut query, cancel).await
        };
        let Some(out) = out else {
            stop_mssql(conn).await;
            return Err(CANCELLED.into());
        };
        // Like the other databases: a fresh connection afterwards, so no session state carries over.
        conn.discard();
        let out = out?;
        let affected: u64 = out.rows_affected().iter().sum();
        let first = out.into_iter().next().unwrap_or_default();
        let columns: Vec<String> = first.first().map(|r| r.columns().iter().map(|c| c.name().to_string()).collect()).unwrap_or_default();
        let truncated = first.len() > cap;
        let rows = first.iter().take(cap).map(|row| (0..row.len()).map(|i| ms_cell(row, i)).collect()).collect();
        let rows_affected = if columns.is_empty() { Some(affected) } else { None };
        return Ok(QueryResult { columns, rows, rows_affected, truncated, elapsed_ms: 0 });
    }

    conn.client().simple_query("BEGIN TRANSACTION").await?.into_results().await?;
    let out = {
        let mut query = Box::pin(collect_mssql(conn.client(), sql, cap));
        race(&mut query, cancel).await
    };
    let Some(out) = out else {
        stop_mssql(conn).await;
        return Err(CANCELLED.into());
    };
    // A cut-off result leaves unread rows on the wire, and a failed rollback leaves the
    // transaction open: either way, close the connection (the server rolls back).
    let cut_off = out.as_ref().map(|r| r.truncated).unwrap_or(false);
    let rolled_back = !cut_off && rollback(conn.client()).await.is_ok();
    if !rolled_back {
        conn.discard();
    }
    out
}

/// Sends SQL Server an attention (the protocol's cancel) for the running batch, then closes
/// the connection; the server rolls back what the batch had done.
async fn stop_mssql(mut conn: super::mssql_pool::MssqlConn<'_>) {
    let _ = tokio::time::timeout(STOP_GRACE, conn.client().cancel_query()).await;
    conn.discard();
}

/// The query's output, or `None` if `cancel` fired first. The query stays usable, so the
/// caller can wait for it to stop. Queries are boxed: their futures are large, and on the
/// stack they overflow a 2 MB thread in debug builds.
async fn race<F: Future + Unpin>(query: &mut F, cancel: &CancellationToken) -> Option<F::Output> {
    tokio::select! {
        out = query => Some(out),
        _ = cancel.cancelled() => None,
    }
}

async fn rollback(client: &mut MssqlClient) -> AppResult<()> {
    client.simple_query("IF @@TRANCOUNT > 0 ROLLBACK").await?.into_results().await?;
    Ok(())
}

/// Collects the first result set, up to `cap` rows.
async fn collect_mssql(client: &mut MssqlClient, sql: &str, cap: usize) -> AppResult<QueryResult> {
    let mut stream = client.simple_query(sql).await?;
    let columns: Vec<String> = stream
        .columns()
        .await?
        // An expression without an alias has no name; SQL Server's own tools show this.
        .map(|cols| cols.iter().map(|c| if c.name().is_empty() { "(No column name)".into() } else { c.name().to_string() }).collect())
        .unwrap_or_default();
    let mut rows = Vec::new();
    let mut truncated = false;
    while let Some(item) = stream.try_next().await? {
        let Some(row) = item.into_row() else { continue };
        if row.result_index() > 0 {
            continue;
        }
        if rows.len() >= cap {
            truncated = true;
            break;
        }
        rows.push((0..row.len()).map(|i| ms_cell(&row, i)).collect());
    }
    Ok(QueryResult { columns, rows, rows_affected: None, truncated, elapsed_ms: 0 })
}
