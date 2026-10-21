use crate::schema::{Column, ForeignKey, Schema, Table};
use futures_util::TryStreamExt;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::Either;
use sqlx::mysql::{MySqlConnectOptions, MySqlPool, MySqlPoolOptions, MySqlRow, MySqlSslMode};
use sqlx::postgres::{PgConnectOptions, PgPool, PgPoolOptions, PgRow, PgSslMode};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePool, SqlitePoolOptions, SqliteRow};
use sqlx::{Column as _, Executor, Row, Statement, TypeInfo, ValueRef};
use std::collections::HashMap;
use std::time::{Duration, Instant};

/// Rows beyond this are not sent to the UI.
const MAX_ROWS: usize = 5000;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Postgres,
    Mysql,
    Sqlite,
}

impl Kind {
    pub fn dialect(&self) -> &'static str {
        match self {
            Kind::Postgres => "PostgreSQL",
            Kind::Mysql => "MySQL",
            Kind::Sqlite => "SQLite",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnConfig {
    pub id: String,
    pub name: String,
    pub kind: Kind,
    #[serde(default)]
    pub host: String,
    #[serde(default)]
    pub port: Option<u16>,
    #[serde(default)]
    pub database: String,
    #[serde(default)]
    pub user: String,
    /// SQLite file path.
    #[serde(default)]
    pub file: String,
    #[serde(default)]
    pub ssl: bool,
}

#[derive(Clone)]
pub enum DbPool {
    Pg(PgPool),
    My(MySqlPool),
    Lite(SqlitePool),
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QueryResult {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<Value>>,
    pub rows_affected: Option<u64>,
    pub truncated: bool,
    pub elapsed_ms: u128,
}

pub async fn connect(cfg: &ConnConfig, password: Option<String>) -> Result<DbPool, String> {
    let timeout = Duration::from_secs(10);
    let pw = password.unwrap_or_default();
    match cfg.kind {
        Kind::Postgres => {
            let mut o = PgConnectOptions::new()
                .host(or(&cfg.host, "localhost"))
                .port(cfg.port.unwrap_or(5432))
                .username(&cfg.user)
                .database(or(&cfg.database, "postgres"))
                .ssl_mode(if cfg.ssl { PgSslMode::Require } else { PgSslMode::Prefer })
                .application_name("askdb");
            if !pw.is_empty() {
                o = o.password(&pw);
            }
            let pool = PgPoolOptions::new()
                .max_connections(4)
                .acquire_timeout(timeout)
                .connect_with(o)
                .await
                .map_err(err)?;
            Ok(DbPool::Pg(pool))
        }
        Kind::Mysql => {
            let mut o = MySqlConnectOptions::new()
                .host(or(&cfg.host, "localhost"))
                .port(cfg.port.unwrap_or(3306))
                .username(&cfg.user)
                .ssl_mode(if cfg.ssl { MySqlSslMode::Required } else { MySqlSslMode::Preferred });
            if !cfg.database.is_empty() {
                o = o.database(&cfg.database);
            }
            if !pw.is_empty() {
                o = o.password(&pw);
            }
            let pool = MySqlPoolOptions::new()
                .max_connections(4)
                .acquire_timeout(timeout)
                .connect_with(o)
                .await
                .map_err(err)?;
            Ok(DbPool::My(pool))
        }
        Kind::Sqlite => {
            if cfg.file.is_empty() {
                return Err("Choose a SQLite database file.".into());
            }
            let o = SqliteConnectOptions::new()
                .filename(&cfg.file)
                .create_if_missing(false)
                .foreign_keys(true);
            let pool = SqlitePoolOptions::new()
                .max_connections(4)
                .acquire_timeout(timeout)
                .connect_with(o)
                .await
                .map_err(err)?;
            Ok(DbPool::Lite(pool))
        }
    }
}

fn or<'a>(s: &'a str, default: &'a str) -> &'a str {
    if s.trim().is_empty() { default } else { s.trim() }
}

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

/// True when the statement only reads data, judged by its first keyword.
/// Read-only statements additionally run inside a read-only transaction, so the
/// database itself rejects anything that slips past this check.
pub fn is_read_only(sql: &str) -> bool {
    let s = strip_leading_comments(sql);
    let first = s
        .split(|c: char| c.is_whitespace() || c == '(' || c == ';')
        .find(|w| !w.is_empty())
        .unwrap_or("")
        .to_ascii_uppercase();
    match first.as_str() {
        "SELECT" | "WITH" | "EXPLAIN" | "SHOW" | "DESCRIBE" | "DESC" | "VALUES" | "TABLE" => true,
        // `PRAGMA x = y` changes settings; `PRAGMA x` / `PRAGMA x(y)` reads them.
        "PRAGMA" => !s.contains('='),
        _ => false,
    }
}

fn strip_leading_comments(sql: &str) -> &str {
    let mut s = sql.trim_start();
    loop {
        if let Some(rest) = s.strip_prefix("--") {
            s = rest.split_once('\n').map(|(_, r)| r).unwrap_or("").trim_start();
        } else if let Some(rest) = s.strip_prefix("/*") {
            s = rest.split_once("*/").map(|(_, r)| r).unwrap_or("").trim_start();
        } else {
            return s;
        }
    }
}

pub async fn run_query(pool: &DbPool, sql: &str, read_only: bool) -> Result<QueryResult, String> {
    let sql = sql.trim().trim_end_matches(';').trim();
    if sql.is_empty() {
        return Err("Nothing to run.".into());
    }
    let start = Instant::now();
    let mut res = match pool {
        DbPool::Pg(p) => {
            let mut conn = p.acquire().await.map_err(err)?;
            if read_only {
                conn.execute("BEGIN READ ONLY").await.map_err(err)?;
            }
            let out = collect_pg(&mut conn, sql).await;
            if read_only && conn.execute("ROLLBACK").await.is_err() {
                // Never hand a connection with an open transaction back to the pool.
                conn.detach();
            }
            out?
        }
        DbPool::My(p) => {
            let mut conn = p.acquire().await.map_err(err)?;
            if read_only {
                conn.execute("START TRANSACTION READ ONLY").await.map_err(err)?;
            }
            let out = collect_my(&mut conn, sql).await;
            if read_only && conn.execute("ROLLBACK").await.is_err() {
                conn.detach();
            }
            out?
        }
        DbPool::Lite(p) => {
            let mut conn = p.acquire().await.map_err(err)?;
            if read_only {
                conn.execute("PRAGMA query_only = ON").await.map_err(err)?;
            }
            let out = collect_lite(&mut conn, sql).await;
            if read_only && conn.execute("PRAGMA query_only = OFF").await.is_err() {
                conn.detach();
            }
            out?
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
        async fn $name(conn: &mut $conn, sql: &str) -> Result<QueryResult, String> {
            let mut columns: Vec<String> = Vec::new();
            let mut rows: Vec<Vec<Value>> = Vec::new();
            let mut affected: Option<u64> = None;
            let mut truncated = false;
            {
                #[allow(deprecated)]
                let mut stream = sqlx::query(sql).fetch_many(&mut *conn);
                while let Some(item) = stream.try_next().await.map_err(err)? {
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

// ---------- cell decoding ----------

macro_rules! try_as {
    ($row:expr, $i:expr, $t:ty) => {
        $row.try_get::<Option<$t>, _>($i).ok().flatten()
    };
}

fn num<T: Into<serde_json::Number>>(v: Option<T>) -> Option<Value> {
    v.map(|n| Value::Number(n.into()))
}

fn float(v: Option<f64>) -> Option<Value> {
    v.map(|f| serde_json::Number::from_f64(f).map(Value::Number).unwrap_or(Value::String(f.to_string())))
}

fn string<T: ToString>(v: Option<T>) -> Option<Value> {
    v.map(|s| Value::String(s.to_string()))
}

fn bytes(v: Option<Vec<u8>>) -> Option<Value> {
    v.map(|b| {
        let hex: String = b.iter().take(64).map(|x| format!("{x:02x}")).collect();
        let more = if b.len() > 64 { format!("… ({} bytes)", b.len()) } else { String::new() };
        Value::String(format!("0x{hex}{more}"))
    })
}

/// Last resort for types we don't decode explicitly (enums, domains, ...):
/// show the raw bytes as text if they are printable.
fn printable(v: Option<String>) -> Option<Value> {
    v.filter(|s| s.chars().all(|c| !c.is_control() || c == '\n' || c == '\t'))
        .map(Value::String)
}

fn pg_cell(row: &PgRow, i: usize) -> Value {
    let Ok(raw) = row.try_get_raw(i) else { return Value::Null };
    if raw.is_null() {
        return Value::Null;
    }
    let ty = raw.type_info().name().to_string();
    use chrono::{DateTime, NaiveDate, NaiveDateTime, NaiveTime, Utc};
    let v = match ty.as_str() {
        "BOOL" => try_as!(row, i, bool).map(Value::Bool),
        "INT2" => num(try_as!(row, i, i16)),
        "INT4" => num(try_as!(row, i, i32)),
        "INT8" => num(try_as!(row, i, i64)),
        "FLOAT4" => float(try_as!(row, i, f32).map(f64::from)),
        "FLOAT8" => float(try_as!(row, i, f64)),
        "NUMERIC" => string(try_as!(row, i, rust_decimal::Decimal)),
        "UUID" => string(try_as!(row, i, uuid::Uuid)),
        "JSON" | "JSONB" => try_as!(row, i, Value),
        "TIMESTAMP" => string(try_as!(row, i, NaiveDateTime)),
        "TIMESTAMPTZ" => string(try_as!(row, i, DateTime<Utc>)),
        "DATE" => string(try_as!(row, i, NaiveDate)),
        "TIME" => string(try_as!(row, i, NaiveTime)),
        "BYTEA" => bytes(try_as!(row, i, Vec<u8>)),
        "TEXT[]" | "VARCHAR[]" | "NAME[]" => try_as!(row, i, Vec<String>).map(|v| Value::from(v)),
        "INT4[]" => try_as!(row, i, Vec<i32>).map(|v| Value::from(v)),
        "INT8[]" => try_as!(row, i, Vec<i64>).map(|v| Value::from(v)),
        _ => try_as!(row, i, String).map(Value::String),
    };
    v.or_else(|| printable(row.try_get_unchecked::<Option<String>, _>(i).ok().flatten()))
        .unwrap_or_else(|| Value::String(format!("<{}>", ty.to_lowercase())))
}

fn my_cell(row: &MySqlRow, i: usize) -> Value {
    let Ok(raw) = row.try_get_raw(i) else { return Value::Null };
    if raw.is_null() {
        return Value::Null;
    }
    let ty = raw.type_info().name().to_string();
    use chrono::{DateTime, NaiveDate, NaiveDateTime, NaiveTime, Utc};
    let v = match ty.as_str() {
        "BOOLEAN" => try_as!(row, i, bool).map(Value::Bool),
        "TINYINT" => num(try_as!(row, i, i8)),
        "SMALLINT" => num(try_as!(row, i, i16)),
        "MEDIUMINT" | "INT" => num(try_as!(row, i, i32)),
        "BIGINT" => num(try_as!(row, i, i64)),
        "TINYINT UNSIGNED" => num(try_as!(row, i, u8)),
        "SMALLINT UNSIGNED" => num(try_as!(row, i, u16)),
        "MEDIUMINT UNSIGNED" | "INT UNSIGNED" => num(try_as!(row, i, u32)),
        "BIGINT UNSIGNED" | "BIT" => num(try_as!(row, i, u64)),
        "FLOAT" => float(try_as!(row, i, f32).map(f64::from)),
        "DOUBLE" => float(try_as!(row, i, f64)),
        "DECIMAL" => string(try_as!(row, i, rust_decimal::Decimal)),
        "DATE" => string(try_as!(row, i, NaiveDate)),
        "DATETIME" => string(try_as!(row, i, NaiveDateTime)),
        "TIMESTAMP" => string(try_as!(row, i, DateTime<Utc>)),
        "TIME" => string(try_as!(row, i, NaiveTime)),
        "YEAR" => num(try_as!(row, i, u16)),
        "JSON" => try_as!(row, i, Value),
        "BINARY" | "VARBINARY" | "TINYBLOB" | "BLOB" | "MEDIUMBLOB" | "LONGBLOB" => {
            printable(row.try_get_unchecked::<Option<String>, _>(i).ok().flatten())
                .or_else(|| bytes(try_as!(row, i, Vec<u8>)))
        }
        _ => try_as!(row, i, String).map(Value::String),
    };
    v.or_else(|| printable(row.try_get_unchecked::<Option<String>, _>(i).ok().flatten()))
        .unwrap_or_else(|| Value::String(format!("<{}>", ty.to_lowercase())))
}

fn lite_cell(row: &SqliteRow, i: usize) -> Value {
    let Ok(raw) = row.try_get_raw(i) else { return Value::Null };
    if raw.is_null() {
        return Value::Null;
    }
    // SQLite is dynamically typed: decode by the stored value's type, not the declared one.
    let ty = raw.type_info().name().to_string();
    let v = match ty.as_str() {
        "INTEGER" | "BOOLEAN" => num(row.try_get_unchecked::<Option<i64>, _>(i).ok().flatten()),
        "REAL" | "NUMERIC" => float(row.try_get_unchecked::<Option<f64>, _>(i).ok().flatten()),
        "BLOB" => bytes(row.try_get_unchecked::<Option<Vec<u8>>, _>(i).ok().flatten()),
        _ => row.try_get_unchecked::<Option<String>, _>(i).ok().flatten().map(Value::String),
    };
    v.unwrap_or_else(|| Value::String(format!("<{}>", ty.to_lowercase())))
}

// ---------- schema introspection ----------

pub async fn introspect(pool: &DbPool, kind: Kind) -> Result<Schema, String> {
    let tables = match pool {
        DbPool::Pg(p) => introspect_pg(p).await,
        DbPool::My(p) => introspect_my(p).await,
        DbPool::Lite(p) => introspect_lite(p).await,
    }
    .map_err(err)?;
    Ok(Schema { dialect: kind.dialect().to_string(), tables })
}

type Key = (String, String);

fn assemble(
    cols: Vec<(String, String, String, String, bool, String)>,
    pks: Vec<(String, String, String)>,
    fks: Vec<(String, String, String, String, String, String)>,
    enums: HashMap<String, Vec<String>>,
    rows: HashMap<Key, i64>,
    qualify_refs: bool,
) -> Vec<Table> {
    let pk: std::collections::HashSet<(String, String, String)> = pks.into_iter().collect();
    let mut fk_map: HashMap<Key, Vec<ForeignKey>> = HashMap::new();
    for (s, t, c, rs, rt, rc) in fks {
        let ref_table = if qualify_refs && rs != "public" { format!("{rs}.{rt}") } else { rt };
        fk_map.entry((s, t)).or_default().push(ForeignKey { column: c, ref_table, ref_column: rc });
    }
    let mut order: Vec<Key> = Vec::new();
    let mut map: HashMap<Key, Table> = HashMap::new();
    for (s, t, c, ty, nullable, kind) in cols {
        let key = (s.clone(), t.clone());
        let table = map.entry(key.clone()).or_insert_with(|| {
            order.push(key.clone());
            Table {
                schema: Some(s.clone()),
                name: t.clone(),
                kind: if kind.contains("VIEW") { "view".into() } else { "table".into() },
                columns: vec![],
                foreign_keys: fk_map.remove(&key).unwrap_or_default(),
                row_estimate: rows.get(&key).copied(),
            }
        });
        let enum_values = enums.get(&ty).cloned().or_else(|| parse_mysql_enum(&ty));
        table.columns.push(Column {
            primary_key: pk.contains(&(s, t, c.clone())),
            name: c,
            data_type: if enum_values.is_some() && ty.starts_with("enum(") { "enum".into() } else { ty },
            nullable,
            enum_values,
        });
    }
    order.into_iter().filter_map(|k| map.remove(&k)).collect()
}

fn parse_mysql_enum(ty: &str) -> Option<Vec<String>> {
    let inner = ty.strip_prefix("enum(")?.strip_suffix(')')?;
    Some(
        inner
            .split("','")
            .map(|v| v.trim_matches('\'').replace("''", "'"))
            .collect(),
    )
}

async fn introspect_pg(p: &PgPool) -> Result<Vec<Table>, sqlx::Error> {
    let cols: Vec<(String, String, String, String, bool, String)> = sqlx::query_as(
        r#"SELECT c.table_schema::text, c.table_name::text, c.column_name::text,
                  CASE WHEN c.data_type IN ('USER-DEFINED', 'ARRAY') THEN c.udt_name::text ELSE c.data_type::text END,
                  c.is_nullable = 'YES', t.table_type::text
           FROM information_schema.columns c
           JOIN information_schema.tables t ON t.table_schema = c.table_schema AND t.table_name = c.table_name
           WHERE c.table_schema NOT IN ('pg_catalog', 'information_schema') AND c.table_schema NOT LIKE 'pg_toast%'
           ORDER BY c.table_schema, c.table_name, c.ordinal_position"#,
    )
    .fetch_all(p)
    .await?;
    let pks: Vec<(String, String, String)> = sqlx::query_as(
        r#"SELECT kcu.table_schema::text, kcu.table_name::text, kcu.column_name::text
           FROM information_schema.table_constraints tc
           JOIN information_schema.key_column_usage kcu
             ON tc.constraint_name = kcu.constraint_name AND tc.table_schema = kcu.table_schema AND tc.table_name = kcu.table_name
           WHERE tc.constraint_type = 'PRIMARY KEY'"#,
    )
    .fetch_all(p)
    .await?;
    let fks: Vec<(String, String, String, String, String, String)> = sqlx::query_as(
        r#"SELECT ns.nspname::text, cl.relname::text, a.attname::text, fns.nspname::text, fcl.relname::text, fa.attname::text
           FROM pg_constraint con
           JOIN pg_class cl ON cl.oid = con.conrelid
           JOIN pg_namespace ns ON ns.oid = cl.relnamespace
           JOIN pg_class fcl ON fcl.oid = con.confrelid
           JOIN pg_namespace fns ON fns.oid = fcl.relnamespace
           CROSS JOIN LATERAL unnest(con.conkey, con.confkey) AS k(col, fcol)
           JOIN pg_attribute a ON a.attrelid = con.conrelid AND a.attnum = k.col
           JOIN pg_attribute fa ON fa.attrelid = con.confrelid AND fa.attnum = k.fcol
           WHERE con.contype = 'f'"#,
    )
    .fetch_all(p)
    .await?;
    let enum_rows: Vec<(String, String)> = sqlx::query_as(
        r#"SELECT t.typname::text, e.enumlabel::text FROM pg_type t
           JOIN pg_enum e ON e.enumtypid = t.oid ORDER BY t.typname, e.enumsortorder"#,
    )
    .fetch_all(p)
    .await?;
    let mut enums: HashMap<String, Vec<String>> = HashMap::new();
    for (t, v) in enum_rows {
        enums.entry(t).or_default().push(v);
    }
    let est: Vec<(String, String, i64)> = sqlx::query_as(
        r#"SELECT n.nspname::text, c.relname::text, c.reltuples::bigint FROM pg_class c
           JOIN pg_namespace n ON n.oid = c.relnamespace WHERE c.relkind IN ('r', 'p')"#,
    )
    .fetch_all(p)
    .await?;
    let rows = est.into_iter().map(|(s, t, n)| ((s, t), n)).collect();
    Ok(assemble(cols, pks, fks, enums, rows, true))
}

async fn introspect_my(p: &MySqlPool) -> Result<Vec<Table>, sqlx::Error> {
    // CAST AS CHAR: some information_schema columns come back as binary strings on MySQL 8.
    let cols: Vec<(String, String, String, String, bool, String)> = sqlx::query_as(
        r#"SELECT CAST(c.TABLE_SCHEMA AS CHAR), CAST(c.TABLE_NAME AS CHAR), CAST(c.COLUMN_NAME AS CHAR),
                  CAST(c.COLUMN_TYPE AS CHAR), c.IS_NULLABLE = 'YES', CAST(t.TABLE_TYPE AS CHAR)
           FROM information_schema.COLUMNS c
           JOIN information_schema.TABLES t ON t.TABLE_SCHEMA = c.TABLE_SCHEMA AND t.TABLE_NAME = c.TABLE_NAME
           WHERE c.TABLE_SCHEMA = DATABASE()
           ORDER BY c.TABLE_NAME, c.ORDINAL_POSITION"#,
    )
    .fetch_all(p)
    .await?;
    let pks: Vec<(String, String, String)> = sqlx::query_as(
        r#"SELECT CAST(TABLE_SCHEMA AS CHAR), CAST(TABLE_NAME AS CHAR), CAST(COLUMN_NAME AS CHAR)
           FROM information_schema.KEY_COLUMN_USAGE
           WHERE TABLE_SCHEMA = DATABASE() AND CONSTRAINT_NAME = 'PRIMARY'"#,
    )
    .fetch_all(p)
    .await?;
    let fks: Vec<(String, String, String, String, String, String)> = sqlx::query_as(
        r#"SELECT CAST(TABLE_SCHEMA AS CHAR), CAST(TABLE_NAME AS CHAR), CAST(COLUMN_NAME AS CHAR),
                  CAST(REFERENCED_TABLE_SCHEMA AS CHAR), CAST(REFERENCED_TABLE_NAME AS CHAR), CAST(REFERENCED_COLUMN_NAME AS CHAR)
           FROM information_schema.KEY_COLUMN_USAGE
           WHERE TABLE_SCHEMA = DATABASE() AND REFERENCED_TABLE_NAME IS NOT NULL"#,
    )
    .fetch_all(p)
    .await?;
    let est: Vec<(String, String, Option<i64>)> = sqlx::query_as(
        r#"SELECT CAST(TABLE_SCHEMA AS CHAR), CAST(TABLE_NAME AS CHAR), CAST(TABLE_ROWS AS SIGNED)
           FROM information_schema.TABLES WHERE TABLE_SCHEMA = DATABASE()"#,
    )
    .fetch_all(p)
    .await?;
    let rows = est.into_iter().filter_map(|(s, t, n)| n.map(|n| ((s, t), n))).collect();
    let mut tables = assemble(cols, pks, fks, HashMap::new(), rows, false);
    // Everything is in one database; drop the qualifier so names stay short.
    for t in &mut tables {
        t.schema = None;
    }
    Ok(tables)
}

async fn introspect_lite(p: &SqlitePool) -> Result<Vec<Table>, sqlx::Error> {
    let objects: Vec<(String, String)> = sqlx::query_as(
        "SELECT name, type FROM sqlite_master WHERE type IN ('table', 'view') AND name NOT LIKE 'sqlite_%' ORDER BY name",
    )
    .fetch_all(p)
    .await?;
    let mut tables = Vec::new();
    for (name, kind) in objects {
        let info: Vec<(String, String, bool, i64)> =
            sqlx::query_as("SELECT name, type, \"notnull\", pk FROM pragma_table_info(?)")
                .bind(&name)
                .fetch_all(p)
                .await?;
        let fks: Vec<(String, String, Option<String>)> =
            sqlx::query_as("SELECT \"from\", \"table\", \"to\" FROM pragma_foreign_key_list(?)")
                .bind(&name)
                .fetch_all(p)
                .await?;
        let row_estimate = if kind == "table" {
            let q = format!("SELECT COUNT(*) FROM \"{}\"", name.replace('"', "\"\""));
            sqlx::query_scalar::<_, i64>(&q).fetch_one(p).await.ok()
        } else {
            None
        };
        tables.push(Table {
            schema: None,
            name,
            kind,
            columns: info
                .into_iter()
                .map(|(n, ty, notnull, pk)| Column {
                    name: n,
                    data_type: if ty.is_empty() { "any".into() } else { ty.to_lowercase() },
                    nullable: !notnull,
                    primary_key: pk > 0,
                    enum_values: None,
                })
                .collect(),
            foreign_keys: fks
                .into_iter()
                .map(|(from, table, to)| ForeignKey {
                    column: from,
                    ref_table: table,
                    ref_column: to.unwrap_or_else(|| "rowid".into()),
                })
                .collect(),
            row_estimate,
        });
    }
    Ok(tables)
}

/// Creates the bundled demo database if it does not exist yet.
pub async fn ensure_demo(path: &std::path::Path) -> Result<(), String> {
    if path.exists() {
        return Ok(());
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(err)?;
    }
    let o = SqliteConnectOptions::new().filename(path).create_if_missing(true);
    let pool = SqlitePoolOptions::new().max_connections(1).connect_with(o).await.map_err(err)?;
    let seed = include_str!("demo.sql");
    if let Err(e) = sqlx::raw_sql(seed).execute(&pool).await {
        pool.close().await;
        let _ = std::fs::remove_file(path);
        return Err(format!("Could not create demo database: {e}"));
    }
    pool.close().await;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_read_only_statements() {
        assert!(is_read_only("select 1"));
        assert!(is_read_only("  -- top customers\nSELECT * FROM customers"));
        assert!(is_read_only("/* c */ WITH x AS (SELECT 1) SELECT * FROM x"));
        assert!(is_read_only("(SELECT 1) UNION (SELECT 2)"));
        assert!(is_read_only("PRAGMA table_info(orders)"));
        assert!(!is_read_only("PRAGMA journal_mode = WAL"));
        assert!(!is_read_only("DELETE FROM orders"));
        assert!(!is_read_only("-- select\nUPDATE orders SET status = 'paid'"));
        assert!(!is_read_only("DROP TABLE x"));
    }

    #[tokio::test]
    async fn demo_database_end_to_end() {
        let dir = std::env::temp_dir().join(format!("askdb-test-{}", uuid::Uuid::new_v4()));
        let path = dir.join("demo.sqlite");
        ensure_demo(&path).await.expect("seed");
        let cfg = ConnConfig {
            id: "t".into(),
            name: "t".into(),
            kind: Kind::Sqlite,
            host: String::new(),
            port: None,
            database: String::new(),
            user: String::new(),
            file: path.to_string_lossy().into_owned(),
            ssl: false,
        };
        let pool = connect(&cfg, None).await.expect("connect");

        let schema = introspect(&pool, Kind::Sqlite).await.expect("introspect");
        let names: Vec<&str> = schema.tables.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(names, ["categories", "customers", "order_items", "orders", "products", "reviews"]);
        let items = schema.tables.iter().find(|t| t.name == "order_items").unwrap();
        assert_eq!(items.row_estimate, Some(7500));
        assert!(items.foreign_keys.iter().any(|f| f.column == "order_id" && f.ref_table == "orders"));
        let text = crate::schema::render_full(&schema);
        assert!(text.contains("order_id integer NOT NULL -> orders.id"), "{text}");

        let r = run_query(&pool, "SELECT status, COUNT(*) AS n FROM orders GROUP BY status ORDER BY n DESC;", true)
            .await
            .expect("query");
        assert_eq!(r.columns, ["status", "n"]);
        assert_eq!(r.rows.iter().map(|row| row[1].as_i64().unwrap()).sum::<i64>(), 3000);

        // Empty result still reports column headers.
        let r = run_query(&pool, "SELECT id, email FROM customers WHERE 1 = 0", true).await.unwrap();
        assert_eq!(r.columns, ["id", "email"]);

        // Read-only mode is enforced by the database, not just the keyword check.
        assert!(run_query(&pool, "DELETE FROM reviews", true).await.is_err());
        let r = run_query(&pool, "SELECT COUNT(*) FROM reviews", true).await.unwrap();
        assert_eq!(r.rows[0][0].as_i64(), Some(1200));

        // The pooled connection is back to normal for an approved write afterwards.
        let w = run_query(&pool, "UPDATE orders SET status = 'paid' WHERE id = 1", false).await.unwrap();
        assert_eq!(w.rows_affected, Some(1));

        let big = run_query(&pool, "SELECT a.id FROM order_items a, categories b", true).await.unwrap();
        assert!(big.truncated);
        assert_eq!(big.rows.len(), MAX_ROWS);

        pool_close(pool).await;
        let _ = std::fs::remove_dir_all(dir);
    }

    async fn pool_close(p: DbPool) {
        if let DbPool::Lite(p) = p {
            p.close().await;
        }
    }
}
