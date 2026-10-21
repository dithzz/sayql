//! An MCP server over stdio (`askdb-mcp`), so Claude (Claude Code, Claude Desktop) can use
//! the person's saved AskDB connections. Read-only: anything that could change data is
//! refused and has to be run in the AskDB app, where the person reviews it first.

use crate::commands::connections::{load_schema, target_pool};
use crate::db::{self, Kind};
use crate::schema::{self, Schema, Table};
use crate::state::{AppState, Target};
use rmcp::handler::server::{router::tool::ToolRouter, wrapper::Parameters};
use rmcp::model::{Implementation, ServerCapabilities, ServerConfig};
use rmcp::{ServerHandler, ServiceExt, tool, tool_handler, tool_router};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;
use std::path::PathBuf;
use std::sync::Arc;

/// The Tauri app identifier: where the app keeps its config and data.
const APP_ID: &str = "com.askdb.app";
const DEFAULT_ROWS: usize = 100;
const MAX_ROWS: usize = 1000;
/// A schema bigger than this is listed by table name unless a search narrows it.
const FULL_SCHEMA_CHARS: usize = 60_000;

const INSTRUCTIONS: &str = "AskDB gives read-only access to the user's saved database connections \
(PostgreSQL, MySQL, SQLite, SQL Server). Typical flow: list_connections, then list_databases for a \
server connection, then describe_schema (pass `search` terms for large databases), then run_query. \
Queries run read-only and the database itself enforces it where it can; statements that could change \
data are refused, and the user must run those in the AskDB app. Keep results small (TOP/LIMIT) and \
select only the columns you need.";

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
}

#[derive(Clone)]
pub struct AskDbServer {
    state: Arc<AppState>,
    tool_router: ToolRouter<Self>,
}

#[tool_router]
impl AskDbServer {
    pub fn new(state: AppState) -> Self {
        AskDbServer { state: Arc::new(state), tool_router: Self::tool_router() }
    }

    #[tool(description = "List the database connections saved in AskDB (no passwords).")]
    async fn list_connections(&self) -> String {
        let list: Vec<_> = self
            .state
            .store
            .connections()
            .into_iter()
            .map(|c| {
                let place = if c.kind == Kind::Sqlite { c.file.clone() } else { c.host.clone() };
                json!({ "id": c.id, "name": c.name, "kind": c.kind, "dialect": c.kind.dialect(), "location": place, "default_database": c.database })
            })
            .collect();
        json!(list).to_string()
    }

    #[tool(description = "List the databases on a connection's server, and which one it opens by default.")]
    async fn list_databases(&self, Parameters(p): Parameters<ConnectionParams>) -> Result<String, String> {
        let pool = target_pool(&self.state, &Target::default_of(&p.connection_id)).await.map_err(|e| e.to_string())?;
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
                          Statements that could change data are refused.")]
    async fn run_query(&self, Parameters(p): Parameters<QueryParams>) -> Result<String, String> {
        if !db::is_read_only(&p.sql) {
            return Err("AskDB's MCP access is read-only, and this statement could change data. \
                        Show it to the user to run in the AskDB app, where they can review it."
                .into());
        }
        let target = Target::new(p.connection_id, p.database);
        let pool = target_pool(&self.state, &target).await.map_err(|e| e.to_string())?;
        let mut r = db::run_query(&pool, &p.sql, true).await.map_err(|e| e.to_string())?;
        let limit = p.max_rows.unwrap_or(DEFAULT_ROWS).clamp(1, MAX_ROWS);
        let truncated = r.truncated || r.rows.len() > limit;
        r.rows.truncate(limit);
        Ok(json!({
            "columns": r.columns,
            "rows": r.rows,
            "row_count": r.rows.len(),
            "truncated": truncated,
            "elapsed_ms": r.elapsed_ms,
        })
        .to_string())
    }
}

impl AskDbServer {
    async fn schema(&self, connection_id: &str, database: Option<String>) -> Result<Schema, String> {
        let target = Target::new(connection_id.to_string(), database);
        match self.state.schema(&target) {
            Ok(loaded) => Ok(loaded.schema.clone()),
            Err(_) => load_schema(&self.state, &target).await.map_err(|e| e.to_string()),
        }
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for AskDbServer {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("askdb", env!("CARGO_PKG_VERSION")))
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
        let found: Vec<String> = schema.tables.iter().filter(|t| matches(t)).map(schema::render_table).collect();
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
            Some(n) => format!("{} (~{n} rows)", t.qualified_name()),
            None => t.qualified_name(),
        })
        .collect();
    format!("{header} Too large to describe in full; call again with `search` words for columns.\n{}", names.join("\n"))
}

/// Where the AskDB app keeps its files; `ASKDB_CONFIG_DIR` overrides (for tests).
fn app_dirs() -> Result<(PathBuf, PathBuf), String> {
    if let Ok(dir) = std::env::var("ASKDB_CONFIG_DIR") {
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
    let server = AskDbServer::new(AppState::new(config_dir, data_dir)?);
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

    #[test]
    fn describe_filters_by_search_words() {
        let s = Schema { dialect: "SQL Server".into(), tables: vec![table("Person", &["PERSON_EMAIL"]), table("Account", &["ID"])] };
        let out = describe(&s, Some("email"));
        assert!(out.contains("dbo.Person") && !out.contains("dbo.Account"), "{out}");
        assert!(describe(&s, Some("nothing")).contains("No table or column matches"));
        let all = describe(&s, None);
        assert!(all.contains("dbo.Person") && all.contains("dbo.Account"), "{all}");
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
