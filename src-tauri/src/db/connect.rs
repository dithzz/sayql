use super::{ConnConfig, DbPool, Kind, LitePools, MssqlPool};
use crate::error::{AppError, AppResult};
use sqlx::mysql::{MySqlConnectOptions, MySqlPoolOptions, MySqlSslMode};
use sqlx::postgres::{PgConnectOptions, PgPoolOptions, PgSslMode};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::{Connection, MySqlConnection, PgConnection};
use std::time::Duration;

const TIMEOUT: Duration = Duration::from_secs(10);

pub async fn connect(cfg: &ConnConfig, password: Option<String>) -> AppResult<DbPool> {
    let pw = password.unwrap_or_default();
    match cfg.kind {
        Kind::Postgres => {
            let host = or(&cfg.host, "localhost");
            let port = cfg.port.unwrap_or(5432);
            let mut o = PgConnectOptions::new()
                .host(host)
                .port(port)
                .username(&cfg.user)
                .database(or(&cfg.database, "postgres"))
                .ssl_mode(if cfg.ssl { PgSslMode::Require } else { PgSslMode::Prefer })
                .application_name("sayql");
            if !pw.is_empty() {
                o = o.password(&pw);
            }
            // "Prefer" means encrypt when the server can, else connect without, as in psql.
            let o = match try_connect::<PgConnection>(&o).await {
                Err(e) if !cfg.ssl && is_tls_failure(&e) => o.ssl_mode(PgSslMode::Disable),
                r => r.map(|()| o).map_err(|e| explain(e, host, port, cfg.ssl))?,
            };
            let pool = PgPoolOptions::new()
                .max_connections(4)
                .acquire_timeout(TIMEOUT)
                .connect_with(o)
                .await
                .map_err(|e| explain(e, host, port, cfg.ssl))?;
            Ok(DbPool::Pg(pool))
        }
        Kind::Mysql => {
            let host = or(&cfg.host, "localhost");
            let port = cfg.port.unwrap_or(3306);
            let mut o = MySqlConnectOptions::new()
                .host(host)
                .port(port)
                .username(&cfg.user)
                .ssl_mode(if cfg.ssl { MySqlSslMode::Required } else { MySqlSslMode::Preferred });
            if !cfg.database.is_empty() {
                o = o.database(&cfg.database);
            }
            if !pw.is_empty() {
                o = o.password(&pw);
            }
            // MySQL's PREFERRED falls back to an unencrypted connection when TLS can't be set up
            // (MySQL 5.7 offers no cipher suite rustls accepts); sqlx's stops with an error.
            let o = match try_connect::<MySqlConnection>(&o).await {
                Err(e) if !cfg.ssl && is_tls_failure(&e) => o.ssl_mode(MySqlSslMode::Disabled),
                r => r.map(|()| o).map_err(|e| explain(e, host, port, cfg.ssl))?,
            };
            let pool = MySqlPoolOptions::new()
                .max_connections(4)
                .acquire_timeout(TIMEOUT)
                .connect_with(o)
                .await
                .map_err(|e| explain(e, host, port, cfg.ssl))?;
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
                .acquire_timeout(TIMEOUT)
                .connect_with(o.clone())
                .await
                ?;
            let ro = SqlitePoolOptions::new()
                .max_connections(4)
                .acquire_timeout(TIMEOUT)
                .connect_with(o.read_only(true))
                .await
                ?;
            Ok(DbPool::Lite(LitePools { rw, ro }))
        }
        Kind::Mssql => Ok(DbPool::Ms(MssqlPool::connect(cfg, Some(pw)).await?)),
    }
}

/// Opens and closes one connection, so a bad setting fails with its own error. A pool
/// retries until its timeout and then reports only "pool timed out".
async fn try_connect<C: Connection>(o: &<C as Connection>::Options) -> Result<(), sqlx::Error> {
    let conn = tokio::time::timeout(TIMEOUT, C::connect_with(o)).await.map_err(|_| sqlx::Error::PoolTimedOut)??;
    conn.close().await
}

fn is_tls_failure(e: &sqlx::Error) -> bool {
    match e {
        sqlx::Error::Tls(_) => true,
        // rustls reports a refused handshake as an I/O error.
        sqlx::Error::Io(io) => {
            let text = io.to_string().to_ascii_lowercase();
            ["tls", "alert", "handshake", "certificate"].iter().any(|w| text.contains(w))
        }
        _ => false,
    }
}

/// Says what went wrong while connecting in words that point at the fix.
fn explain(e: sqlx::Error, host: &str, port: u16, ssl: bool) -> AppError {
    let addr = format!("{host}:{port}");
    if is_tls_failure(&e) {
        let reason = match &e {
            sqlx::Error::Tls(inner) => inner.to_string(),
            sqlx::Error::Io(io) => io.to_string(),
            other => other.to_string(),
        };
        let hint = if ssl {
            " The server may not support SSL, or only older TLS versions SayQL can't use. Turn off \"Require SSL\" to \
             connect without encryption."
        } else {
            ""
        };
        return format!("Could not set up an encrypted connection to {addr}: {reason}.{hint}").into();
    }
    match e {
        sqlx::Error::Io(io) => format!("Could not reach {addr}: {io}").into(),
        sqlx::Error::PoolTimedOut => format!(
            "Timed out reaching {addr}. Check the host and port, and that you're on the right network or VPN."
        )
        .into(),
        other => other.into(),
    }
}

fn or<'a>(s: &'a str, default: &'a str) -> &'a str {
    if s.trim().is_empty() { default } else { s.trim() }
}
