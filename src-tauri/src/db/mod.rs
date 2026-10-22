//! Database access for PostgreSQL, MySQL, SQLite and SQL Server: connecting, running queries
//! safely, decoding values for display and reading the schema.

mod classify;
mod connect;
mod databases;
mod decode;
mod demo;
mod execute;
mod introspect;
mod mssql_pool;

pub use classify::is_read_only;
pub use connect::connect;
pub use databases::{DatabaseList, list_databases, open_database};
pub use demo::ensure_demo;
pub use execute::{run_query, run_query_capped};
pub use introspect::introspect;
pub use mssql_pool::MssqlPool;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::{MySqlPool, PgPool, SqlitePool};

/// Rows beyond this are not sent to the UI.
pub(crate) const MAX_ROWS: usize = 5000;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Postgres,
    Mysql,
    Sqlite,
    /// Microsoft SQL Server and Azure SQL.
    Mssql,
}

impl Kind {
    pub fn dialect(&self) -> &'static str {
        match self {
            Kind::Postgres => "PostgreSQL",
            Kind::Mysql => "MySQL",
            Kind::Sqlite => "SQLite",
            Kind::Mssql => "SQL Server",
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
    /// Require an encrypted connection.
    #[serde(default)]
    pub ssl: bool,
    /// Accept the server's TLS certificate without verifying it (self-signed internal servers).
    #[serde(default)]
    pub trust_cert: bool,
}

#[derive(Clone)]
pub enum DbPool {
    Pg(PgPool),
    My(MySqlPool),
    Lite(LitePools),
    Ms(MssqlPool),
}

impl DbPool {
    pub fn kind(&self) -> Kind {
        match self {
            DbPool::Pg(_) => Kind::Postgres,
            DbPool::My(_) => Kind::Mysql,
            DbPool::Lite(_) => Kind::Sqlite,
            DbPool::Ms(_) => Kind::Mssql,
        }
    }
}

/// SQLite runs every statement in a script and lets a script turn `PRAGMA query_only`
/// back off, so read-only queries get connections opened read-only instead.
#[derive(Clone)]
pub struct LitePools {
    pub(crate) rw: SqlitePool,
    pub(crate) ro: SqlitePool,
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

#[cfg(test)]
mod integration_tests;
#[cfg(test)]
pub(crate) use integration_tests::env_config;
#[cfg(test)]
mod tests;
