use super::{ConnConfig, DbPool, Kind, LitePools};
use crate::error::AppResult;
use sqlx::mysql::{MySqlConnectOptions, MySqlPoolOptions, MySqlSslMode};
use sqlx::postgres::{PgConnectOptions, PgPoolOptions, PgSslMode};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use std::time::Duration;

pub async fn connect(cfg: &ConnConfig, password: Option<String>) -> AppResult<DbPool> {
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
                ?;
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
                ?;
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
                ?;
            let ro = SqlitePoolOptions::new()
                .max_connections(4)
                .acquire_timeout(timeout)
                .connect_with(o.read_only(true))
                .await
                ?;
            Ok(DbPool::Lite(LitePools { rw, ro }))
        }
    }
}

fn or<'a>(s: &'a str, default: &'a str) -> &'a str {
    if s.trim().is_empty() { default } else { s.trim() }
}
