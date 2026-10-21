use crate::schema::{Column, ForeignKey, Schema, Table};
use futures_util::TryStreamExt;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::Either;
use sqlx::mysql::{MySqlConnectOptions, MySqlPool, MySqlPoolOptions, MySqlRow, MySqlSslMode};
use sqlx::postgres::{
    PgConnectOptions, PgPool, PgPoolOptions, PgRow, PgSslMode, PgTypeInfo, PgTypeKind, PgValueFormat,
};
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
    Lite(LitePools),
}

/// SQLite runs every statement in a script and lets a script turn `PRAGMA query_only`
/// back off, so read-only queries get connections opened read-only instead.
#[derive(Clone)]
pub struct LitePools {
    rw: SqlitePool,
    ro: SqlitePool,
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
            let rw = SqlitePoolOptions::new()
                .max_connections(4)
                .acquire_timeout(timeout)
                .connect_with(o.clone())
                .await
                .map_err(err)?;
            let ro = SqlitePoolOptions::new()
                .max_connections(4)
                .acquire_timeout(timeout)
                .connect_with(o.read_only(true))
                .await
                .map_err(err)?;
            Ok(DbPool::Lite(LitePools { rw, ro }))
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
/// Read-only statements additionally run inside a read-only transaction (a read-only
/// connection for SQLite), so the database itself rejects anything that slips past
/// this check. Postgres and MySQL also refuse several statements in one prepared
/// statement, so `SELECT 1; COMMIT; DELETE ...` cannot end that transaction early.
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
            let mut conn = if read_only { p.ro.acquire() } else { p.rw.acquire() }.await.map_err(err)?;
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

/// `1.1f32 as f64` is 1.100000023841858; go through the shortest decimal form instead.
fn f32_to_f64(f: f32) -> f64 {
    f.to_string().parse().unwrap_or(f64::from(f))
}

/// `[-]HH:MM:SS[.ffffff]` from microseconds, trailing zeros trimmed, hours unbounded.
fn clock(us: i64) -> String {
    let a = us.unsigned_abs();
    let (h, m, s, frac) = (a / 3_600_000_000, a / 60_000_000 % 60, a / 1_000_000 % 60, a % 1_000_000);
    let mut out = format!("{}{h:02}:{m:02}:{s:02}", if us < 0 { "-" } else { "" });
    if frac != 0 {
        out.push('.');
        out.push_str(format!("{frac:06}").trim_end_matches('0'));
    }
    out
}

// Prepared statements return Postgres values in binary. They are decoded from the wire
// format directly, so arrays, enums, ranges and composites of any element type work and
// out-of-range values (huge NUMERIC, 'infinity' timestamps) cannot panic inside sqlx.

fn pg_cell(row: &PgRow, i: usize) -> Value {
    let Ok(raw) = row.try_get_raw(i) else { return Value::Null };
    if raw.is_null() {
        return Value::Null;
    }
    let ty = raw.type_info().into_owned();
    let v = match raw.format() {
        PgValueFormat::Binary => raw.as_bytes().ok().and_then(|b| pg_binary(&ty, b)),
        PgValueFormat::Text => raw.as_str().ok().map(|s| Value::String(s.to_string())),
    };
    v.unwrap_or_else(|| pg_placeholder(&ty))
}

fn pg_placeholder(ty: &PgTypeInfo) -> Value {
    Value::String(format!("<{}>", ty.name().to_lowercase()))
}

fn pg_base(mut ty: &PgTypeInfo) -> &PgTypeInfo {
    while let PgTypeKind::Domain(base) = ty.kind() {
        ty = base;
    }
    ty
}

fn pg_oid(ty: &PgTypeInfo) -> u32 {
    ty.oid().map(|o| o.0).unwrap_or(0)
}

struct Buf<'a>(&'a [u8]);

impl<'a> Buf<'a> {
    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        if self.0.len() < n {
            return None;
        }
        let (a, rest) = self.0.split_at(n);
        self.0 = rest;
        Some(a)
    }
    fn arr<const N: usize>(&mut self) -> Option<[u8; N]> {
        self.take(N)?.try_into().ok()
    }
    fn u8(&mut self) -> Option<u8> {
        Some(self.arr::<1>()?[0])
    }
    fn i16(&mut self) -> Option<i16> {
        Some(i16::from_be_bytes(self.arr()?))
    }
    fn u16(&mut self) -> Option<u16> {
        Some(u16::from_be_bytes(self.arr()?))
    }
    fn i32(&mut self) -> Option<i32> {
        Some(i32::from_be_bytes(self.arr()?))
    }
    fn u32(&mut self) -> Option<u32> {
        Some(u32::from_be_bytes(self.arr()?))
    }
    fn i64(&mut self) -> Option<i64> {
        Some(i64::from_be_bytes(self.arr()?))
    }
    fn f64(&mut self) -> Option<f64> {
        Some(f64::from_be_bytes(self.arr()?))
    }
    /// A length-prefixed field; `Some(None)` is SQL NULL.
    fn field(&mut self) -> Option<Option<&'a [u8]>> {
        let len = self.i32()?;
        if len < 0 { Some(None) } else { self.take(len as usize).map(Some) }
    }
}

fn pg_binary(ty: &PgTypeInfo, b: &[u8]) -> Option<Value> {
    let ty = pg_base(ty);
    match ty.kind() {
        PgTypeKind::Array(elem) => pg_array(elem, b),
        PgTypeKind::Enum(_) => printable(std::str::from_utf8(b).ok().map(String::from)),
        PgTypeKind::Composite(fields) => pg_record(Some(fields), b),
        PgTypeKind::Range(elem) => pg_range(elem, b),
        _ if pg_oid(ty) == 2249 => pg_record(None, b),
        _ => pg_scalar(pg_oid(ty), b).or_else(|| printable(std::str::from_utf8(b).ok().map(String::from))),
    }
}

fn pg_array(elem: &PgTypeInfo, b: &[u8]) -> Option<Value> {
    let mut r = Buf(b);
    let ndim = r.i32()?;
    let (_has_nulls, _elem_oid) = (r.i32()?, r.u32()?);
    if !(0..=6).contains(&ndim) {
        return None;
    }
    let mut dims = Vec::new();
    for _ in 0..ndim {
        dims.push(usize::try_from(r.i32()?).ok()?);
        r.i32()?; // lower bound
    }
    if dims.is_empty() {
        return Some(Value::Array(vec![]));
    }
    let total: usize = dims.iter().product();
    let mut flat = Vec::with_capacity(total.min(MAX_ROWS));
    for _ in 0..total {
        flat.push(match r.field()? {
            None => Value::Null,
            Some(e) => pg_binary(elem, e).unwrap_or_else(|| pg_placeholder(elem)),
        });
    }
    fn nest(dims: &[usize], it: &mut std::vec::IntoIter<Value>) -> Value {
        let (n, rest) = dims.split_first().unwrap();
        Value::Array((0..*n).map(|_| if rest.is_empty() { it.next().unwrap_or(Value::Null) } else { nest(rest, it) }).collect())
    }
    Some(nest(&dims, &mut flat.into_iter()))
}

/// Named composite types become objects; anonymous `ROW(...)` records become arrays.
fn pg_record(fields: Option<&[(String, PgTypeInfo)]>, b: &[u8]) -> Option<Value> {
    let mut r = Buf(b);
    let n = r.i32()?;
    let mut obj = serde_json::Map::new();
    let mut arr = Vec::new();
    for k in 0..usize::try_from(n).ok()? {
        let oid = r.u32()?;
        let field = fields.and_then(|f| f.get(k));
        let v = match r.field()? {
            None => Value::Null,
            Some(e) => match field {
                Some((_, t)) => pg_binary(t, e).unwrap_or_else(|| pg_placeholder(t)),
                None => pg_scalar(oid, e)
                    .or_else(|| printable(std::str::from_utf8(e).ok().map(String::from)))
                    .unwrap_or_else(|| Value::String(format!("<oid {oid}>"))),
            },
        };
        match field {
            Some((name, _)) => {
                obj.insert(name.clone(), v);
            }
            None => arr.push(v),
        }
    }
    Some(if fields.is_some() { Value::Object(obj) } else { Value::Array(arr) })
}

fn pg_range(elem: &PgTypeInfo, b: &[u8]) -> Option<Value> {
    let mut r = Buf(b);
    let flags = r.u8()?;
    if flags & 0x01 != 0 {
        return Some(Value::String("empty".into()));
    }
    let mut bound = |infinite: u8| -> Option<String> {
        if flags & infinite != 0 {
            return Some(String::new());
        }
        match pg_binary(elem, r.field()??)? {
            Value::String(s) => Some(s),
            v => Some(v.to_string()),
        }
    };
    let (lo, hi) = (bound(0x08)?, bound(0x10)?);
    let open = if flags & 0x02 != 0 { '[' } else { '(' };
    let close = if flags & 0x04 != 0 { ']' } else { ')' };
    Some(Value::String(format!("{open}{lo},{hi}{close}")))
}

fn pg_scalar(oid: u32, b: &[u8]) -> Option<Value> {
    use chrono::{NaiveDate, TimeDelta};
    use std::fmt::Write;
    let mut r = Buf(b);
    let epoch = NaiveDate::from_ymd_opt(2000, 1, 1)?;
    let text = || std::str::from_utf8(b).ok().map(|s| Value::String(s.to_string()));
    let timestamp = |us: i64| -> Option<String> {
        match us {
            i64::MAX => Some("infinity".into()),
            i64::MIN => Some("-infinity".into()),
            _ => {
                let t = epoch.and_hms_opt(0, 0, 0)?.checked_add_signed(TimeDelta::microseconds(us))?;
                Some(format!("{} {}", t.date(), clock(us.rem_euclid(86_400_000_000))))
            }
        }
    };
    let v = match oid {
        16 => Value::Bool(r.u8()? != 0),
        17 => bytes(Some(b.to_vec()))?,
        18 => Value::String((r.u8()? as char).to_string()),
        19 | 25 | 142 | 705 | 1042 | 1043 => text()?,
        20 => Value::from(r.i64()?),
        21 => Value::from(r.i16()?),
        23 => Value::from(r.i32()?),
        // oid, xid, cid and the reg* aliases
        24 | 26 | 28 | 29 | 2202 | 2203 | 2204 | 2205 | 2206 | 3734 | 3769 | 4089 | 4096 => Value::from(r.u32()?),
        700 => float(Some(f32_to_f64(f32::from_bits(r.u32()?))))?,
        701 => float(Some(r.f64()?))?,
        114 => serde_json::from_slice(b).ok()?,
        3802 => {
            r.u8()?; // jsonb format version
            serde_json::from_slice(r.0).ok()?
        }
        4072 => {
            r.u8()?;
            printable(std::str::from_utf8(r.0).ok().map(String::from))?
        }
        1700 => Value::String(pg_numeric(&mut r)?),
        790 => {
            let c = i128::from(r.i64()?);
            Value::String(format!("{}{}.{:02}", if c < 0 { "-" } else { "" }, c.abs() / 100, c.abs() % 100))
        }
        2950 => Value::String(uuid::Uuid::from_slice(b).ok()?.to_string()),
        1082 => Value::String(match r.i32()? {
            i32::MAX => "infinity".into(),
            i32::MIN => "-infinity".into(),
            d => epoch.checked_add_signed(TimeDelta::days(d.into()))?.to_string(),
        }),
        1083 => Value::String(clock(r.i64()?)),
        1114 => Value::String(timestamp(r.i64()?)?),
        1184 => Value::String(match timestamp(r.i64()?)? {
            t if t.ends_with("infinity") => t,
            t => format!("{t} UTC"),
        }),
        1266 => {
            let (us, west) = (r.i64()?, r.i32()?);
            let east = -west;
            let a = east.unsigned_abs();
            let mut s = format!("{}{}{:02}", clock(us), if east < 0 { '-' } else { '+' }, a / 3600);
            if a % 3600 != 0 {
                write!(s, ":{:02}", a / 60 % 60).ok()?;
            }
            if a % 60 != 0 {
                write!(s, ":{:02}", a % 60).ok()?;
            }
            Value::String(s)
        }
        1186 => {
            let (us, days, months) = (r.i64()?, r.i32()?, r.i32()?);
            Value::String(pg_interval(us, days, months))
        }
        650 | 869 => {
            let (_family, bits, _is_cidr, n) = (r.u8()?, r.u8()?, r.u8()?, r.u8()?);
            let (ip, max): (std::net::IpAddr, u8) = match n {
                4 => (std::net::Ipv4Addr::from(r.arr::<4>()?).into(), 32),
                16 => (std::net::Ipv6Addr::from(r.arr::<16>()?).into(), 128),
                _ => return None,
            };
            Value::String(if oid == 650 || bits != max { format!("{ip}/{bits}") } else { ip.to_string() })
        }
        829 | 774 => Value::String(b.iter().map(|x| format!("{x:02x}")).collect::<Vec<_>>().join(":")),
        1560 | 1562 => {
            let n = usize::try_from(r.i32()?).ok()?;
            let bits = r.0;
            let s: Option<String> = (0..n).map(|i| Some(if bits.get(i / 8)? & (0x80 >> (i % 8)) != 0 { '1' } else { '0' })).collect();
            Value::String(s?)
        }
        600 => Value::String(format!("({},{})", r.f64()?, r.f64()?)),
        3220 => {
            let v = u64::from_be_bytes(r.arr()?);
            Value::String(format!("{:X}/{:X}", v >> 32, v & 0xffff_ffff))
        }
        2278 => Value::Null,
        _ => return None,
    };
    Some(v)
}

/// Exact decimal text from Postgres' base-10000 binary NUMERIC, any size.
fn pg_numeric(r: &mut Buf) -> Option<String> {
    use std::fmt::Write;
    let n = usize::try_from(r.i16()?).ok()?;
    let weight = i32::from(r.i16()?);
    let sign = r.u16()?;
    let dscale = usize::from(r.u16()?);
    match sign {
        0xC000 => return Some("NaN".into()),
        0xD000 => return Some("Infinity".into()),
        0xF000 => return Some("-Infinity".into()),
        _ => {}
    }
    let digits: Vec<i16> = (0..n).map(|_| r.i16()).collect::<Option<_>>()?;
    // digits[0] is worth 10000^weight; digit(k) is the group k places after it.
    let digit = |k: i32| usize::try_from(k).ok().and_then(|k| digits.get(k)).copied().unwrap_or(0);
    let mut s = String::new();
    if sign == 0x4000 {
        s.push('-');
    }
    if weight < 0 {
        s.push('0');
    } else {
        for k in 0..=weight {
            if k == 0 { write!(s, "{}", digit(k)) } else { write!(s, "{:04}", digit(k)) }.ok()?;
        }
    }
    if dscale > 0 {
        let mut frac = String::new();
        let mut j = 1;
        while frac.len() < dscale {
            write!(frac, "{:04}", digit(weight + j)).ok()?;
            j += 1;
        }
        frac.truncate(dscale);
        s.push('.');
        s.push_str(&frac);
    }
    Some(s)
}

/// Same layout as Postgres' default `IntervalStyle`, e.g. `1 year 2 mons 3 days 04:05:06.5`.
fn pg_interval(us: i64, days: i32, months: i32) -> String {
    let unit = |n: i64, u: &str| format!("{n} {u}{}", if n == 1 { "" } else { "s" });
    let mut parts = Vec::new();
    let (years, months) = (months / 12, months % 12);
    if years != 0 {
        parts.push(unit(years.into(), "year"));
    }
    if months != 0 {
        parts.push(unit(months.into(), "mon"));
    }
    if days != 0 {
        parts.push(unit(days.into(), "day"));
    }
    if us != 0 || parts.is_empty() {
        let plus = us > 0 && (years < 0 || months < 0 || days < 0);
        parts.push(format!("{}{}", if plus { "+" } else { "" }, clock(us)));
    }
    parts.join(" ")
}

fn my_cell(row: &MySqlRow, i: usize) -> Value {
    let Ok(raw) = row.try_get_raw(i) else { return Value::Null };
    if raw.is_null() {
        return Value::Null;
    }
    let ty = raw.type_info().name().to_string();
    use chrono::{DateTime, NaiveDate, NaiveDateTime, Utc};
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
        "FLOAT" => float(try_as!(row, i, f32).map(f32_to_f64)),
        "DOUBLE" => float(try_as!(row, i, f64)),
        // Sent as text in both protocols; keep it exact rather than round through a Decimal.
        "DECIMAL" => row.try_get_unchecked::<Option<String>, _>(i).ok().flatten().map(Value::String),
        "DATE" => string(try_as!(row, i, NaiveDate)),
        "DATETIME" => string(try_as!(row, i, NaiveDateTime)),
        "TIMESTAMP" => string(try_as!(row, i, DateTime<Utc>)),
        // TIME is a duration in MySQL: it can be negative or longer than a day.
        "TIME" => try_as!(row, i, sqlx::mysql::types::MySqlTime).map(|t| {
            let us = i64::from(t.hours()) * 3_600_000_000
                + i64::from(t.minutes()) * 60_000_000
                + i64::from(t.seconds()) * 1_000_000
                + i64::from(t.microseconds());
            // Not `t.is_negative()`: in sqlx 0.8.6 it returns `is_positive()`.
            Value::String(clock(if t.sign().is_negative() { -us } else { us }))
        }),
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
        DbPool::Lite(p) => introspect_lite(&p.ro).await,
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
                  CASE WHEN c.data_type = 'USER-DEFINED' AND c.udt_schema NOT IN ('public', 'pg_catalog')
                       THEN c.udt_schema::text || '.' || c.udt_name::text
                       WHEN c.data_type IN ('USER-DEFINED', 'ARRAY') THEN c.udt_name::text
                       ELSE c.data_type::text END,
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
        // Keyed like the column types above, so same-named enums in different schemas stay apart.
        r#"SELECT CASE WHEN n.nspname IN ('public', 'pg_catalog') THEN t.typname::text
                       ELSE n.nspname::text || '.' || t.typname::text END,
                  e.enumlabel::text
           FROM pg_type t
           JOIN pg_namespace n ON n.oid = t.typnamespace
           JOIN pg_enum e ON e.enumtypid = t.oid ORDER BY t.oid, e.enumsortorder"#,
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
    // reltuples is -1 until the table is first vacuumed or analyzed: unknown, not empty.
    let rows = est.into_iter().filter(|(_, _, n)| *n >= 0).map(|(s, t, n)| ((s, t), n)).collect();
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
mod integration_tests;

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

        // Read-only mode is enforced by the database, not just the keyword check,
        // and a script cannot switch it off part-way through.
        assert!(run_query(&pool, "DELETE FROM reviews", true).await.is_err());
        assert!(run_query(&pool, "SELECT 1; PRAGMA query_only = OFF; DELETE FROM reviews", true).await.is_err());
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
            p.rw.close().await;
            p.ro.close().await;
        }
    }
}
