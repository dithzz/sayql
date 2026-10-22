//! An MCP server over stdio (`sayql-mcp`), so Claude (Claude Code, Claude Desktop) can use
//! the person's saved SayQL connections. Read-only: anything that could change data is
//! refused and has to be run in the SayQL app, where the person reviews it first.
//! Three tools hand work to the app instead (see handoff.rs): a prefilled New connection
//! dialog, where the person types the password, and SQL or a saved script in a tab, which
//! they run. `export_csv` writes a read-only query's rows to a new file in Downloads.

use crate::db::{self, DbPool, Kind, QueryResult};
use crate::handoff::{self, Action, Handoff, ScriptRef};
use crate::schema::{self, Schema, Table};
use crate::state::{AppState, Target};
use rmcp::handler::server::{router::tool::ToolRouter, wrapper::Parameters};
use rmcp::model::{Implementation, ServerCapabilities, ServerConfig};
use rmcp::service::RequestContext;
use rmcp::{RoleServer, ServerHandler, ServiceExt, tool, tool_handler, tool_router};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Map, Value, json};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tokio_util::sync::CancellationToken;

/// The Tauri app identifier: where the app keeps its config and data.
const APP_ID: &str = "com.sayql.app";
const DEFAULT_ROWS: usize = 100;
const MAX_ROWS: usize = 1000;
/// Most rows `export_csv` writes.
const EXPORT_ROWS: usize = 100_000;
/// How long a query may run before SayQL stops it. An AI app has no way to cancel one, and
/// a forgotten query would keep the database busy.
const QUERY_TIMEOUT: Duration = Duration::from_secs(120);
const EXPORT_TIMEOUT: Duration = Duration::from_secs(600);
/// A schema bigger than this is listed by table name unless a search narrows it.
const FULL_SCHEMA_CHARS: usize = 60_000;

const INSTRUCTIONS: &str = "SayQL gives read-only access to the user's saved database connections \
(PostgreSQL, MySQL, SQLite, SQL Server). Typical flow: list_connections, then list_databases for a \
server connection, then describe_schema (pass `search` terms for large databases), then run_query (page with `offset`). \
Queries run read-only and the database itself enforces it where it can; statements that could change \
data are refused, and the user must run those in the SayQL app. Keep results small (TOP/LIMIT) and \
select only the columns you need. To hand work to the user in the SayQL app: add_connection opens a \
prefilled New connection dialog (never pass a password; the user types it in SayQL), and open_query \
opens SQL in a new SayQL tab for the user to review and run (use it for statements that change data). \
list_scripts shows the user's saved scripts; open_script opens one in SayQL. export_csv saves a read-only \
query's rows (up to 100,000) to a CSV file in the user's Downloads folder without returning them.";

#[derive(Debug, Deserialize, JsonSchema)]
struct ConnectionParams {
    /// A connection id from list_connections.
    connection_id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct SchemaParams {
    /// A connection id from list_connections.
    connection_id: String,
    /// Database on the server (from list_databases). Omit to use the connection's default.
    database: Option<String>,
    /// Space-separated words; only tables whose name or columns contain one are described.
    search: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct QueryParams {
    /// A connection id from list_connections.
    connection_id: String,
    /// Database on the server (from list_databases). Omit to use the connection's default.
    database: Option<String>,
    /// One read-only statement (SELECT, WITH, EXPLAIN, SHOW, ...), in the connection's SQL dialect.
    sql: String,
    /// Most rows to return (default 100, at most 1000).
    max_rows: Option<usize>,
    /// Rows to skip, for the next page: pass `next_offset` from the previous result.
    offset: Option<usize>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct ExportParams {
    /// A connection id from list_connections.
    connection_id: String,
    /// Database on the server (from list_databases). Omit to use the connection's default.
    database: Option<String>,
    /// One read-only statement (SELECT, WITH, ...), in the connection's SQL dialect.
    sql: String,
    /// File name without folder, e.g. "orders-2026". Saved as .csv in Downloads; never overwrites.
    file_name: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct ScriptsParams {
    /// Only scripts on this connection (an id from list_connections). Omit for all.
    connection_id: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct ScriptParams {
    /// A script id from list_scripts.
    script_id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct AddConnectionParams {
    /// Database type: postgres, mysql, sqlite or sqlserver (SQL Server and Azure SQL).
    #[serde(rename = "type")]
    kind: String,
    /// Name shown in SayQL. Optional.
    name: Option<String>,
    /// Server host name or IP (not for sqlite). For SQL Server, `host,port` also works.
    host: Option<String>,
    /// Server port (not for sqlite). Omit for the default.
    port: Option<i64>,
    /// Database to open by default (not for sqlite).
    database: Option<String>,
    /// User name (not for sqlite).
    user: Option<String>,
    /// sqlite only: path to the database file.
    file: Option<String>,
    /// Require SSL (postgres, mysql) or encrypt the connection (sqlserver).
    ssl: Option<bool>,
    /// sqlserver only: trust a self-signed server certificate.
    trust_server_certificate: Option<bool>,
    /// Anything else. Checked so a password is refused with a clear message.
    #[serde(flatten)]
    #[schemars(skip)]
    extra: Map<String, Value>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct OpenQueryParams {
    /// A connection id from list_connections.
    connection_id: String,
    /// Database on the server (from list_databases). Omit to use the connection's default.
    database: Option<String>,
    /// The SQL to put in the editor, in the connection's dialect. At most 10,000 characters.
    sql: String,
    /// Short tab title. Optional.
    title: Option<String>,
    /// The user's question this SQL answers, shown above the editor. Optional.
    question: Option<String>,
}

#[derive(Clone)]
pub struct SayqlServer {
    state: Arc<AppState>,
    tool_router: ToolRouter<Self>,
}

#[tool_router]
impl SayqlServer {
    pub fn new(state: AppState) -> Self {
        SayqlServer { state: Arc::new(state), tool_router: Self::tool_router() }
    }

    #[tool(description = "List the database connections saved in SayQL (no passwords).")]
    async fn list_connections(&self) -> Result<String, String> {
        let list: Vec<_> = self
            .state
            .store
            .connections()
            .map_err(|e| e.to_string())?
            .into_iter()
            .map(|c| {
                let place = if c.kind == Kind::Sqlite { c.file.clone() } else { c.host.clone() };
                json!({ "id": c.id, "name": c.name, "kind": c.kind, "dialect": c.kind.dialect(), "location": place, "default_database": c.database })
            })
            .collect();
        Ok(json!(list).to_string())
    }

    #[tool(description = "List the databases on a connection's server, and which one it opens by default.")]
    async fn list_databases(&self, Parameters(p): Parameters<ConnectionParams>) -> Result<String, String> {
        let pool = self.state.target_pool(&Target::default_of(&p.connection_id)).await.map_err(|e| e.to_string())?;
        match db::list_databases(&pool).await.map_err(|e| e.to_string())? {
            Some(list) => Ok(json!(list).to_string()),
            None => Ok("This is a SQLite file: it is a single database; omit `database`.".into()),
        }
    }

    #[tool(description = "Describe tables and columns (types, primary keys, foreign keys, enum values, row estimates). \
                          For large databases pass `search` words, e.g. \"customer order invoice\".")]
    async fn describe_schema(&self, Parameters(p): Parameters<SchemaParams>) -> Result<String, String> {
        let schema = self.schema(&p.connection_id, p.database).await?;
        Ok(describe(&schema, p.search.as_deref()))
    }

    #[tool(description = "Run one read-only SQL statement and return the rows as JSON. \
                          Statements that could change data are refused. Results are paged: when \
                          `next_offset` is set, call again with it as `offset` for more rows.")]
    async fn run_query(&self, Parameters(p): Parameters<QueryParams>) -> Result<String, String> {
        let target = Target::new(p.connection_id, p.database);
        let pool = self.state.target_pool(&target).await.map_err(|e| e.to_string())?;
        if !db::is_read_only(&p.sql, pool.kind()) {
            return Err(WRITE_REFUSED.into());
        }
        let r = run_limited(&pool, &p.sql, db::MAX_ROWS, QUERY_TIMEOUT).await?;
        let limit = p.max_rows.unwrap_or(DEFAULT_ROWS).clamp(1, MAX_ROWS);
        let page = page(r.rows, r.truncated, p.offset.unwrap_or(0), limit);
        let mut out = json!({
            "columns": r.columns,
            "rows": page.rows,
            "row_count": page.rows.len(),
            "offset": page.offset,
            "next_offset": page.next_offset,
            "truncated": page.more,
            "elapsed_ms": r.elapsed_ms,
        });
        if page.past_cap {
            out["note"] = json!("SayQL reads at most 5,000 rows per query. For later rows, add a WHERE or \
                                LIMIT/OFFSET to the SQL; to save them all, use export_csv.");
        }
        Ok(out.to_string())
    }

    #[tool(description = "Run one read-only SQL statement and save all its rows (up to 100,000) to a new CSV \
                          file in the user's Downloads folder. Returns the file path, columns and row count, \
                          not the rows. Statements that could change data are refused.")]
    async fn export_csv(&self, Parameters(p): Parameters<ExportParams>) -> Result<String, String> {
        let target = Target::new(p.connection_id, p.database);
        let pool = self.state.target_pool(&target).await.map_err(|e| e.to_string())?;
        if !db::is_read_only(&p.sql, pool.kind()) {
            return Err(WRITE_REFUSED.into());
        }
        let dir = dirs::download_dir().or_else(dirs::home_dir).ok_or("Could not find the Downloads folder.")?;
        let r = run_limited(&pool, &p.sql, EXPORT_ROWS, EXPORT_TIMEOUT).await?;
        let (path, mut file) = new_export_file(&dir, p.file_name.as_deref())?;
        file.write_all(to_csv(&r.columns, &r.rows).as_bytes()).map_err(|e| format!("Could not write {}: {e}", path.display()))?;
        Ok(json!({
            "path": path,
            "columns": r.columns,
            "row_count": r.rows.len(),
            "truncated": r.truncated,
            "note": if r.truncated { "Stopped at 100,000 rows; narrow the query for the rest." } else { "All rows saved." },
        })
        .to_string())
    }

    #[tool(description = "List the SQL scripts the user saved in SayQL, with their SQL. Read-only ones can be run \
                          with run_query; open_script opens one in SayQL for the user.")]
    async fn list_scripts(&self, Parameters(p): Parameters<ScriptsParams>) -> Result<String, String> {
        let connections = self.state.store.connections().map_err(|e| e.to_string())?;
        let list: Vec<_> = self
            .state
            .store
            .scripts()
            .map_err(|e| e.to_string())?
            .into_iter()
            .filter(|s| p.connection_id.as_ref().is_none_or(|id| &s.connection_id == id))
            .map(|s| {
                let connection = connections.iter().find(|c| c.id == s.connection_id).map(|c| c.name.clone());
                json!({ "id": s.id, "name": s.name, "connection_id": s.connection_id, "connection": connection,
                        "database": s.database, "question": s.question, "sql": s.sql })
            })
            .collect();
        Ok(json!(list).to_string())
    }

    #[tool(description = "Open one of the user's saved scripts (from list_scripts) in SayQL, without running it. \
                          The user reviews it and clicks Run.")]
    async fn open_script(&self, Parameters(p): Parameters<ScriptParams>, ctx: RequestContext<RoleServer>) -> Result<String, String> {
        let script_id = handoff::script_id(&p.script_id)?;
        let script = self.state.store.scripts().map_err(|e| e.to_string())?.into_iter().find(|s| s.id == script_id).ok_or_else(|| {
            format!("No saved script has id {script_id:?}; use one from list_scripts.")
        })?;
        self.hand_off(Action::OpenScript(ScriptRef { script_id }), &ctx)?;
        Ok(format!("Opened the script {:?} in SayQL. Nothing ran: the user reviews it and clicks Run.", script.name))
    }

    #[tool(description = "Open SayQL's New connection dialog with these details filled in, for the user to finish. \
                          Never pass a password or any other secret: it is refused. The user types the password \
                          in SayQL, tests the connection and saves it. Returns right away; call list_connections \
                          later to use the new connection.")]
    async fn add_connection(&self, Parameters(p): Parameters<AddConnectionParams>, ctx: RequestContext<RoleServer>) -> Result<String, String> {
        let draft = connection_draft(p)?;
        self.hand_off(Action::NewConnection(draft), &ctx)?;
        Ok("Opened SayQL's New connection dialog with these details. The user enters the password there, \
            tests the connection and saves it."
            .into())
    }

    #[tool(description = "Open SQL in a new SayQL tab on a saved connection, without running it. The user reviews it \
                          and clicks Run; SayQL asks before anything that changes data. Use this for statements \
                          run_query refuses. The results stay in SayQL and are not returned.")]
    async fn open_query(&self, Parameters(p): Parameters<OpenQueryParams>, ctx: RequestContext<RoleServer>) -> Result<String, String> {
        let draft = handoff::query_draft(&p.connection_id, p.database.as_deref(), &p.sql, p.title.as_deref(), p.question.as_deref())?;
        let conn = self.state.store.connection(&draft.connection_id).map_err(|_| {
            format!("No saved connection has id {:?}; use one from list_connections.", draft.connection_id)
        })?;
        self.hand_off(Action::OpenQuery(draft), &ctx)?;
        Ok(format!(
            "Opened a new SayQL tab on {:?} with the SQL. Nothing ran: the user reviews it and clicks Run.",
            conn.name
        ))
    }
}

const WRITE_REFUSED: &str = "SayQL's MCP access is read-only, and this statement could change data. \
Use open_query to put it in a SayQL tab, where the user reviews and runs it.";

/// Runs a read-only query, stopping it on the server if it runs longer than `limit`.
async fn run_limited(pool: &DbPool, sql: &str, cap: usize, limit: Duration) -> Result<QueryResult, String> {
    let cancel = CancellationToken::new();
    let timer = tokio::spawn({
        let cancel = cancel.clone();
        async move {
            tokio::time::sleep(limit).await;
            cancel.cancel();
        }
    });
    let out = db::run_query_with(pool, sql, true, cap, &cancel).await;
    timer.abort();
    out.map_err(|e| match e.to_string() {
        e if e == db::CANCELLED => format!(
            "The query was still running after {} seconds, so SayQL stopped it. Narrow it (a WHERE clause, \
             fewer joins, LIMIT/TOP) and try again, or run it in the SayQL app.",
            limit.as_secs()
        ),
        e => e,
    })
}

/// One page of a query's rows.
struct Page {
    rows: Vec<Vec<Value>>,
    offset: usize,
    /// Where the next page starts, when there are more rows SayQL read.
    next_offset: Option<usize>,
    /// More rows exist, read or not.
    more: bool,
    /// The page reached the end of what SayQL reads, and the query has more.
    past_cap: bool,
}

/// `limit` rows from `offset` of the rows SayQL read (`cut_off`: the query had more).
fn page(rows: Vec<Vec<Value>>, cut_off: bool, offset: usize, limit: usize) -> Page {
    let total = rows.len();
    let rows: Vec<_> = rows.into_iter().skip(offset).take(limit).collect();
    let end = offset.min(total) + rows.len();
    Page { offset, next_offset: (end < total).then_some(end), more: end < total || cut_off, past_cap: end >= total && cut_off, rows }
}

/// RFC 4180 CSV, like "Copy as CSV" in the app. NULL becomes an empty field.
fn to_csv(columns: &[String], rows: &[Vec<Value>]) -> String {
    fn field(s: &str) -> String {
        if s.contains(['"', ',', '\n', '\r']) { format!("\"{}\"", s.replace('"', "\"\"")) } else { s.to_string() }
    }
    let cell = |v: &Value| match v {
        Value::Null => String::new(),
        Value::String(s) => field(s),
        other => field(&other.to_string()),
    };
    let mut out = columns.iter().map(|c| field(c)).collect::<Vec<_>>().join(",");
    for row in rows {
        out.push('\n');
        out.push_str(&row.iter().map(cell).collect::<Vec<_>>().join(","));
    }
    out.push('\n');
    out
}

/// A new file in `dir` named after `name` (letters, digits, spaces, `-`, `_`, `.`), never an existing one.
fn new_export_file(dir: &Path, name: Option<&str>) -> Result<(PathBuf, std::fs::File), String> {
    let clean: String = name.unwrap_or("").chars().filter(|c| c.is_alphanumeric() || " -_.".contains(*c)).take(80).collect();
    let stem = clean.trim().trim_end_matches(".csv").trim_matches('.').trim();
    let stem = if stem.is_empty() { "sayql-export" } else { stem };
    for n in 1..1000 {
        let path = dir.join(if n == 1 { format!("{stem}.csv") } else { format!("{stem} ({n}).csv") });
        match std::fs::OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(file) => return Ok((path, file)),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(format!("Could not create {}: {e}", path.display())),
        }
    }
    Err(format!("Too many files named {stem} in {}.", dir.display()))
}

/// The New connection fields from `add_connection`'s arguments; refuses passwords.
fn connection_draft(p: AddConnectionParams) -> Result<handoff::ConnectionDraft, String> {
    if p.extra.keys().any(|k| handoff::is_secret_field(k)) {
        return Err(handoff::secret_refused());
    }
    if let Some(k) = p.extra.keys().next() {
        return Err(format!("Unknown field `{k}`."));
    }
    let get = |k: &str| match k {
        "type" => Some(p.kind.clone()),
        "name" => p.name.clone(),
        "host" => p.host.clone(),
        "database" => p.database.clone(),
        "user" => p.user.clone(),
        "file" => p.file.clone(),
        _ => None,
    };
    let flag = |k: &str| Ok(if k == "ssl" { p.ssl } else { p.trust_server_certificate });
    handoff::connection_draft(get, p.port, flag)
}

/// The AI app's name for "From Claude (MCP)", from what it sent when it connected.
fn client_label(info: &Implementation) -> Option<String> {
    let name = info.name.to_ascii_lowercase();
    let known = if name.contains("claude-code") {
        Some("Claude Code")
    } else if name.starts_with("claude") {
        Some("Claude")
    } else if name.contains("cursor") {
        Some("Cursor")
    } else {
        None
    };
    handoff::source(known.or(info.title.as_deref()).or(Some(&info.name)))
}

impl SayqlServer {
    /// Opens `action` in the SayQL app through a one-time `sayql://` link.
    fn hand_off(&self, action: Action, ctx: &RequestContext<RoleServer>) -> Result<(), String> {
        let source = ctx.peer.peer_info().and_then(|i| client_label(&i.client_info));
        let token = handoff::issue_token(self.state.store.dir())?;
        let link = handoff::to_url(&Handoff { source, action }, &token)?;
        handoff::open_in_app(&link)
    }

    async fn schema(&self, connection_id: &str, database: Option<String>) -> Result<Schema, String> {
        let target = Target::new(connection_id.to_string(), database);
        match self.state.schema(&target) {
            Ok(loaded) => Ok(loaded.schema.clone()),
            Err(_) => self.state.load_schema(&target).await.map_err(|e| e.to_string()),
        }
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for SayqlServer {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("sayql", env!("CARGO_PKG_VERSION")))
            .with_instructions(INSTRUCTIONS)
    }
}

/// The schema as compact text: everything when small, else the tables matching `search`,
/// else just the table names with a hint to search.
fn describe(schema: &Schema, search: Option<&str>) -> String {
    let words: Vec<String> = search.unwrap_or("").split_whitespace().map(str::to_lowercase).collect();
    let matches = |t: &Table| {
        words.iter().any(|w| {
            t.qualified_name().to_lowercase().contains(w) || t.columns.iter().any(|c| c.name.to_lowercase().contains(w))
        })
    };
    let header = format!("Dialect: {}. {} tables and views.", schema.dialect, schema.tables.len());
    if !words.is_empty() {
        let found: Vec<String> = schema.tables.iter().filter(|t| matches(t)).map(|t| schema::render_table(t, &schema.dialect)).collect();
        return if found.is_empty() {
            format!("{header}\nNo table or column matches {:?}. Try other words, or omit `search` for the table list.", words.join(" "))
        } else {
            format!("{header} Showing {} matching:\n{}", found.len(), found.join("\n"))
        };
    }
    let full = schema::render_full(schema);
    if full.len() <= FULL_SCHEMA_CHARS {
        return format!("{header}\n{full}");
    }
    let names: Vec<String> = schema
        .tables
        .iter()
        .map(|t| match t.row_estimate {
            Some(n) => format!("{} (~{n} rows)", schema::render_name(t, &schema.dialect)),
            None => schema::render_name(t, &schema.dialect),
        })
        .collect();
    format!("{header} Too large to describe in full; call again with `search` words for columns.\n{}", names.join("\n"))
}

/// Where the SayQL app keeps its files; `SAYQL_CONFIG_DIR` overrides (for tests).
fn app_dirs() -> Result<(PathBuf, PathBuf), String> {
    if let Ok(dir) = std::env::var("SAYQL_CONFIG_DIR") {
        let dir = PathBuf::from(dir);
        return Ok((dir.clone(), dir));
    }
    let config = dirs::config_dir().ok_or("Could not find the user's config directory")?.join(APP_ID);
    let data = dirs::data_dir().ok_or("Could not find the user's data directory")?.join(APP_ID);
    Ok((config, data))
}

/// Runs the server on stdin/stdout until the client disconnects.
pub async fn serve_stdio() -> Result<(), Box<dyn std::error::Error>> {
    let (config_dir, data_dir) = app_dirs()?;
    let server = SayqlServer::new(AppState::new(config_dir, data_dir)?);
    server.serve(rmcp::transport::stdio()).await?.waiting().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::Column;

    fn table(name: &str, cols: &[&str]) -> Table {
        Table {
            schema: Some("dbo".into()),
            name: name.into(),
            kind: "table".into(),
            columns: cols
                .iter()
                .map(|c| Column { name: (*c).into(), data_type: "int".into(), nullable: false, primary_key: false, enum_values: None })
                .collect(),
            foreign_keys: vec![],
            row_estimate: Some(10),
        }
    }

    fn add_params(args: Value) -> Result<AddConnectionParams, serde_json::Error> {
        serde_json::from_value(args)
    }

    #[test]
    fn add_connection_refuses_passwords() {
        let p = add_params(json!({ "type": "postgres", "host": "db", "password": "hunter2" })).unwrap();
        assert_eq!(connection_draft(p).unwrap_err(), handoff::secret_refused());
        let p = add_params(json!({ "type": "mysql", "apiKey": "x" })).unwrap();
        assert_eq!(connection_draft(p).unwrap_err(), handoff::secret_refused());
        let p = add_params(json!({ "type": "mysql", "colour": "red" })).unwrap();
        assert!(connection_draft(p).unwrap_err().contains("colour"));
    }

    #[test]
    fn add_connection_validates_fields() {
        let p = add_params(json!({ "type": "sqlserver", "host": "sql.local", "port": 1433, "ssl": true, "trust_server_certificate": true })).unwrap();
        let d = connection_draft(p).unwrap();
        assert_eq!((d.kind, d.port, d.ssl, d.trust_cert), (Kind::Mssql, Some(1433), Some(true), Some(true)));
        assert!(connection_draft(add_params(json!({ "type": "postgres", "port": 0 })).unwrap()).is_err());
        assert!(connection_draft(add_params(json!({ "type": "sqlite" })).unwrap()).is_err());
        assert!(add_params(json!({ "host": "db" })).is_err(), "type is required");
        assert!(add_params(json!({ "type": "postgres", "port": "5432" })).is_err());
    }

    #[test]
    fn pages_through_rows() {
        let rows = |n: usize| (0..n).map(|i| vec![json!(i)]).collect::<Vec<_>>();
        let p = page(rows(250), false, 0, 100);
        assert_eq!((p.rows.len(), p.next_offset, p.more, p.past_cap), (100, Some(100), true, false));
        let p = page(rows(250), false, 200, 100);
        assert_eq!((p.rows[0][0].clone(), p.rows.len(), p.next_offset, p.more), (json!(200), 50, None, false));
        let p = page(rows(250), false, 900, 100);
        assert_eq!((p.rows.len(), p.next_offset, p.more), (0, None, false));
        let p = page(rows(5000), true, 4950, 100);
        assert_eq!((p.rows.len(), p.next_offset, p.more, p.past_cap), (50, None, true, true));
    }

    #[test]
    fn writes_csv_like_the_app() {
        let csv = to_csv(&["id".into(), "note".into()], &[vec![json!(1), json!("a, \"b\"")], vec![json!(2), Value::Null]]);
        assert_eq!(csv, "id,note\n1,\"a, \"\"b\"\"\"\n2,\n");
    }

    #[test]
    fn export_files_never_overwrite() {
        let dir = std::env::temp_dir().join(format!("sayql-export-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let (a, _) = new_export_file(&dir, Some("../../etc/orders.csv")).unwrap();
        let (b, _) = new_export_file(&dir, Some("....etcorders")).unwrap();
        let (c, _) = new_export_file(&dir, None).unwrap();
        assert_eq!(a, dir.join("etcorders.csv"));
        assert_eq!(b, dir.join("etcorders (2).csv"));
        assert_eq!(c, dir.join("sayql-export.csv"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn labels_known_clients() {
        let info = |name: &str| Implementation::new(name, "1");
        assert_eq!(client_label(&info("claude-ai")).as_deref(), Some("Claude"));
        assert_eq!(client_label(&info("claude-code")).as_deref(), Some("Claude Code"));
        assert_eq!(client_label(&info("cursor-vscode")).as_deref(), Some("Cursor"));
        assert_eq!(client_label(&info("Zed")).as_deref(), Some("Zed"));
    }

    #[test]
    fn describe_filters_by_search_words() {
        let s = Schema { dialect: "SQL Server".into(), tables: vec![table("Person", &["PERSON_EMAIL"]), table("Account", &["ID"])] };
        let out = describe(&s, Some("email"));
        assert!(out.contains("dbo.Person") && !out.contains("dbo.Account"), "{out}");
        assert!(describe(&s, Some("nothing")).contains("No table or column matches"));
        let all = describe(&s, None);
        assert!(all.contains("dbo.Person") && all.contains("dbo.Account"), "{all}");
    }

    /// 1000 rows, 0..999, in SQL every supported server runs.
    const THOUSAND: &str = "SELECT a.d * 100 + b.d * 10 + c.d AS v FROM \
        (SELECT 0 AS d UNION ALL SELECT 1 UNION ALL SELECT 2 UNION ALL SELECT 3 UNION ALL SELECT 4 UNION ALL SELECT 5 UNION ALL SELECT 6 UNION ALL SELECT 7 UNION ALL SELECT 8 UNION ALL SELECT 9) a CROSS JOIN \
        (SELECT 0 AS d UNION ALL SELECT 1 UNION ALL SELECT 2 UNION ALL SELECT 3 UNION ALL SELECT 4 UNION ALL SELECT 5 UNION ALL SELECT 6 UNION ALL SELECT 7 UNION ALL SELECT 8 UNION ALL SELECT 9) b CROSS JOIN \
        (SELECT 0 AS d UNION ALL SELECT 1 UNION ALL SELECT 2 UNION ALL SELECT 3 UNION ALL SELECT 4 UNION ALL SELECT 5 UNION ALL SELECT 6 UNION ALL SELECT 7 UNION ALL SELECT 8 UNION ALL SELECT 9) c \
        ORDER BY v";

    /// The tools against the demo SQLite store, and against real servers when the
    /// SAYQL_TEST_*_URL variables are set (see db/integration_tests.rs). The connection is saved
    /// without a password and its pool opened directly, so the test never touches the keychain.
    #[tokio::test]
    async fn tools_work_against_real_databases() {
        let demo_dir = std::env::temp_dir().join(format!("sayql-mcp-demo-{}", uuid::Uuid::new_v4()));
        let demo = demo_dir.join("demo.sqlite");
        db::ensure_demo(&demo).await.unwrap();
        let sqlite = db::ConnConfig {
            id: "demo".into(),
            name: "Demo".into(),
            kind: Kind::Sqlite,
            host: String::new(),
            port: None,
            database: String::new(),
            user: String::new(),
            file: demo.to_string_lossy().into_owned(),
            ssl: false,
            trust_cert: false,
        };
        let servers = [("SAYQL_TEST_PG_URL", Kind::Postgres), ("SAYQL_TEST_MYSQL_URL", Kind::Mysql), ("SAYQL_TEST_MSSQL_URL", Kind::Mssql)];
        let targets = std::iter::once(Some((sqlite, None))).chain(servers.map(|(var, kind)| db::env_config(var, kind)));
        for (cfg, password) in targets.flatten() {
            let kind = cfg.kind;
            let dir = std::env::temp_dir().join(format!("sayql-mcp-{}", uuid::Uuid::new_v4()));
            let state = AppState::new(dir.clone(), dir.clone()).unwrap();
            state.store.save_connections(&[cfg.clone()]).unwrap();
            state.set_pool(Target::default_of(&cfg.id), db::connect(&cfg, password).await.unwrap());
            let server = SayqlServer::new(state);
            let json = |s: String| serde_json::from_str::<Value>(&s).unwrap();
            let conn = || ConnectionParams { connection_id: cfg.id.clone() };

            let list = json(server.list_connections().await.unwrap());
            assert_eq!(list[0]["dialect"], json!(kind.dialect()), "{kind:?}");
            let dbs = server.list_databases(Parameters(conn())).await.unwrap();
            if kind == Kind::Sqlite {
                assert!(dbs.contains("single database"), "{dbs}");
            } else {
                assert!(json(dbs.clone())["databases"].is_array(), "{kind:?}: {dbs}");
            }
            let schema = server
                .describe_schema(Parameters(SchemaParams { connection_id: cfg.id.clone(), database: None, search: None }))
                .await
                .unwrap();
            assert!(schema.starts_with(&format!("Dialect: {}.", kind.dialect())), "{schema}");

            let run = |sql: &str, offset: Option<usize>| {
                server.run_query(Parameters(QueryParams {
                    connection_id: cfg.id.clone(),
                    database: None,
                    sql: sql.into(),
                    max_rows: Some(300),
                    offset,
                }))
            };
            // Pages through the rows, and says where the next page starts.
            let first = json(run(THOUSAND, None).await.unwrap());
            assert_eq!((first["row_count"].clone(), first["next_offset"].clone()), (json!(300), json!(300)), "{kind:?}");
            assert_eq!(first["rows"][0], json!([0]));
            let last = json(run(THOUSAND, Some(900)).await.unwrap());
            assert_eq!((last["row_count"].clone(), last["next_offset"].clone()), (json!(100), Value::Null), "{kind:?}");
            assert_eq!(last["rows"][99], json!([999]));
            assert_eq!(last["truncated"], json!(false));
            // Anything that could change data is refused, however it is dressed up.
            for sql in ["DELETE FROM t", "SELECT 1; DROP TABLE t", "WITH x AS (SELECT 1 AS a) UPDATE t SET a = 1"] {
                assert_eq!(run(sql, None).await.unwrap_err(), WRITE_REFUSED, "{kind:?}: {sql}");
            }

            // export_csv writes to Downloads; check what it writes without writing there.
            let pool = server.state.target_pool(&Target::default_of(&cfg.id)).await.unwrap();
            let r = db::run_query_with(&pool, THOUSAND, true, EXPORT_ROWS, &CancellationToken::new()).await.unwrap();
            let csv = to_csv(&r.columns, &r.rows);
            assert_eq!(csv.lines().count(), 1001, "{kind:?}");
            assert!(csv.starts_with("v\n0\n1\n") && csv.ends_with("999\n"), "{kind:?}");
            let _ = std::fs::remove_dir_all(dir);
        }
        let _ = std::fs::remove_dir_all(demo_dir);
    }

    #[test]
    fn describe_lists_names_for_huge_schemas() {
        let cols: Vec<String> = (0..60).map(|i| format!("column_{i}")).collect();
        let cols: Vec<&str> = cols.iter().map(String::as_str).collect();
        let tables = (0..200).map(|i| table(&format!("table_{i}"), &cols)).collect();
        let out = describe(&Schema { dialect: "SQL Server".into(), tables }, None);
        assert!(out.contains("Too large") && out.contains("dbo.table_199 (~10 rows)") && !out.contains("column_0"), "{}", &out[..200]);
    }
}
