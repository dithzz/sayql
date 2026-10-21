//! SQL Server / Azure SQL connections (tiberius; sqlx has no SQL Server driver).
//! A desktop client runs one statement at a time, so this keeps one connection and
//! reopens it whenever it was discarded.

use super::ConnConfig;
use crate::error::{AppError, AppResult};
use std::sync::Arc;
use std::time::Duration;
use tiberius::{AuthMethod, Client, Config, EncryptionLevel};
use tokio::net::TcpStream;
use tokio::sync::{Mutex, MutexGuard};
use tokio_util::compat::{Compat, TokioAsyncWriteCompatExt};

pub type MssqlClient = Client<Compat<TcpStream>>;

const DEFAULT_PORT: u16 = 1433;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Clone)]
pub struct MssqlPool {
    config: Config,
    /// Switched to with `USE` after login, so browsing another database never changes
    /// how the login itself happens (the same way DBeaver browses databases).
    database: Option<String>,
    client: Arc<Mutex<Option<MssqlClient>>>,
}

/// A checked-out connection. `discard` it when it may hold an open transaction or
/// unread results, so the next use starts from a fresh connection.
pub struct MssqlConn<'a> {
    guard: MutexGuard<'a, Option<MssqlClient>>,
}

impl MssqlConn<'_> {
    pub fn client(&mut self) -> &mut MssqlClient {
        self.guard.as_mut().expect("acquire() always opens a connection")
    }

    pub fn discard(mut self) {
        *self.guard = None;
    }
}

impl MssqlPool {
    /// Opens a first connection so bad settings fail here, not on the first query.
    pub async fn connect(cfg: &ConnConfig, password: Option<String>) -> AppResult<Self> {
        let pool = MssqlPool { config: config(cfg, password)?, database: None, client: Arc::new(Mutex::new(None)) };
        drop(pool.acquire().await?);
        Ok(pool)
    }

    /// A separate connection, with the same login, that works in `database`.
    pub async fn for_database(&self, database: &str) -> AppResult<Self> {
        let pool = MssqlPool { config: self.config.clone(), database: Some(database.to_string()), client: Arc::new(Mutex::new(None)) };
        drop(pool.acquire().await?);
        Ok(pool)
    }

    pub async fn acquire(&self) -> AppResult<MssqlConn<'_>> {
        let mut guard = self.client.lock().await;
        if guard.is_none() {
            let mut client = open(&self.config).await?;
            if let Some(db) = &self.database {
                let use_db = format!("USE [{}]", db.replace(']', "]]"));
                client.simple_query(use_db).await?.into_results().await?;
            }
            *guard = Some(client);
        }
        Ok(MssqlConn { guard })
    }
}

fn config(cfg: &ConnConfig, password: Option<String>) -> AppResult<Config> {
    let (host, port) = split_host(&cfg.host, cfg.port)?;
    let mut c = Config::new();
    c.host(host);
    c.port(port);
    if !cfg.database.trim().is_empty() {
        c.database(cfg.database.trim());
    }
    c.application_name("askdb");
    c.authentication(AuthMethod::sql_server(cfg.user.trim(), password.unwrap_or_default()));
    c.encryption(if cfg.ssl { EncryptionLevel::Required } else { EncryptionLevel::Off });
    if cfg.trust_cert {
        c.trust_cert();
    }
    Ok(c)
}

/// Accepts `host`, `host:port` and SQL Server's own `host,port` (as in connection strings).
fn split_host(raw: &str, port: Option<u16>) -> AppResult<(String, u16)> {
    let raw = raw.trim().strip_prefix("tcp:").unwrap_or(raw.trim());
    if raw.contains('\\') {
        return Err("Named instances (server\\instance) aren't supported yet. Use the server name and the instance's port.".into());
    }
    let (host, explicit) = match raw.rsplit_once([',', ':']) {
        Some((h, p)) if !h.is_empty() => (h, Some(p.trim().parse::<u16>().map_err(|_| format!("Invalid port in host: {p}"))?)),
        _ => (raw, None),
    };
    let host = if host.is_empty() { "localhost" } else { host };
    Ok((host.to_string(), explicit.or(port).unwrap_or(DEFAULT_PORT)))
}

async fn open(config: &Config) -> AppResult<MssqlClient> {
    let mut config = config.clone();
    // Azure SQL may redirect the client to the node that serves the database.
    for _ in 0..3 {
        let tcp = tokio::time::timeout(CONNECT_TIMEOUT, TcpStream::connect(config.get_addr()))
            .await
            .map_err(|_| "Timed out reaching the server. Check the host and port, and that you're on the right network or VPN.")?
            .map_err(|e| format!("Could not reach {}: {e}", config.get_addr()))?;
        tcp.set_nodelay(true)?;
        match Client::connect(config.clone(), tcp.compat_write()).await {
            Ok(client) => return Ok(client),
            Err(tiberius::error::Error::Routing { host, port }) => {
                config.host(host);
                config.port(port);
            }
            Err(e) => return Err(explain(e)),
        }
    }
    Err("SQL Server redirected the connection too many times.".into())
}

/// Adds the likely fix to errors people hit when setting up a connection.
fn explain(e: tiberius::error::Error) -> AppError {
    let text = e.to_string();
    if text.contains("certificate") || text.contains("UnknownIssuer") {
        format!(
            "{text}\nThe server's certificate isn't trusted. SQL Server always encrypts the login, so this applies even \
             with \"Encrypt connection\" off. If this is an internal server, turn on \"Trust server certificate\"."
        )
        .into()
    } else if text.contains("code: 18456") {
        // SQL Server reports every credential problem the same way; the real reason is only in its log.
        format!(
            "{text}\nThe server was reached but didn't accept this user and password. Check them, or try them in another \
             SQL client. It can also mean the login is disabled, or the server only allows Windows/Entra sign-in."
        )
        .into()
    } else if text.contains("code: 4060") {
        format!("{text}\nThe login works but can't open this database. Check the database name, or leave it empty.").into()
    } else {
        AppError::SqlServer(e)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_sql_server_host_formats() {
        assert_eq!(split_host("myserver.database.windows.net,1433", None).unwrap(), ("myserver.database.windows.net".into(), 1433));
        assert_eq!(split_host("tcp:db.example.com,1500", Some(1433)).unwrap(), ("db.example.com".into(), 1500));
        assert_eq!(split_host("db.internal:1444", None).unwrap(), ("db.internal".into(), 1444));
        assert_eq!(split_host(" db.internal ", Some(1435)).unwrap(), ("db.internal".into(), 1435));
        assert_eq!(split_host("", None).unwrap(), ("localhost".into(), 1433));
        assert!(split_host(r"db\SQLEXPRESS", None).is_err());
        assert!(split_host("db,notaport", None).is_err());
    }
}
