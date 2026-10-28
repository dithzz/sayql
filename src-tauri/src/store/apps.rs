//! What AI apps did over MCP (the activity log) and which apps the person blocked.
//!
//! The log is JSON Lines, appended by the MCP server (`sayql-mcp`, one process per AI app) and
//! read by the app. One small `write` per line in append mode keeps lines whole when two AI apps
//! write at once. Past [`MAX_LOG_BYTES`] the file is moved to `.1` (replacing the older one), so
//! the log keeps roughly the last two files' worth. It holds the SQL and counts, never rows.

use super::Store;
use crate::error::AppResult;
use crate::store::agent::AppResults;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io::{BufRead, Write};

const LOG_FILE: &str = "mcp-activity.jsonl";
const OLD_LOG_FILE: &str = "mcp-activity.1.jsonl";
const APPS_FILE: &str = "mcp-apps.json";
/// Size at which the log starts a new file.
const MAX_LOG_BYTES: u64 = 1024 * 1024;
/// Longest SQL kept per entry.
const MAX_SQL: usize = 4000;
/// The tools counted as queries in an app's summary.
const QUERY_TOOLS: &[&str] = &["run_query", "export_csv"];
/// What an AI app that doesn't say its name is called.
pub const UNKNOWN_APP: &str = "Unknown app";

/// One thing an AI app did, or tried, over MCP.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Activity {
    /// Milliseconds since the Unix epoch.
    pub at: u64,
    /// The AI app's name, as it gave it (Claude Code, Cursor, ...).
    pub app: String,
    /// The MCP tool: run_query, export_csv, ...
    pub tool: String,
    #[serde(default)]
    pub connection_id: String,
    #[serde(default)]
    pub connection: String,
    #[serde(default)]
    pub sql: String,
    /// ok, refused or error.
    pub status: String,
    /// Why it was refused or failed, in a few words.
    #[serde(default)]
    pub detail: Option<String>,
    /// Rows the query returned (the app may have seen fewer, or only a summary).
    #[serde(default)]
    pub rows: Option<usize>,
    /// What the connection let AI apps see at the time.
    #[serde(default)]
    pub may_see: Option<AppResults>,
}

impl Activity {
    pub fn new(app: &str, tool: &str) -> Self {
        Activity {
            at: chrono::Utc::now().timestamp_millis().max(0) as u64,
            app: app.to_string(),
            tool: tool.to_string(),
            connection_id: String::new(),
            connection: String::new(),
            sql: String::new(),
            status: "ok".into(),
            detail: None,
            rows: None,
            may_see: None,
        }
    }
}

/// An AI app SayQL has seen, or that the person blocked.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSummary {
    pub name: String,
    pub blocked: bool,
    /// Queries and exports in the log.
    pub queries: usize,
    pub last_used: Option<u64>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Apps {
    #[serde(default)]
    blocked: Vec<String>,
}

impl Store {
    /// Appends one entry to the activity log. Errors are the caller's to ignore: a log that
    /// can't be written must not stop the query.
    pub fn log_app_activity(&self, mut entry: Activity) -> AppResult<()> {
        if entry.sql.len() > MAX_SQL {
            let mut end = MAX_SQL;
            while !entry.sql.is_char_boundary(end) {
                end -= 1;
            }
            entry.sql.truncate(end);
            entry.sql.push('…');
        }
        let path = self.dir().join(LOG_FILE);
        if std::fs::metadata(&path)
            .map(|m| m.len() > MAX_LOG_BYTES)
            .unwrap_or(false)
        {
            let _ = std::fs::rename(&path, self.dir().join(OLD_LOG_FILE));
        }
        let mut line = serde_json::to_string(&entry)?;
        line.push('\n');
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)?;
        file.write_all(line.as_bytes())?;
        Ok(())
    }

    /// The newest `limit` entries, newest first. Lines that don't parse are skipped.
    pub fn app_activity(&self, limit: usize) -> AppResult<Vec<Activity>> {
        let mut all = Vec::new();
        for name in [OLD_LOG_FILE, LOG_FILE] {
            match std::fs::File::open(self.dir().join(name)) {
                Ok(f) => all.extend(
                    std::io::BufReader::new(f)
                        .lines()
                        .map_while(Result::ok)
                        .filter_map(|l| serde_json::from_str::<Activity>(&l).ok()),
                ),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(e.into()),
            }
        }
        all.reverse();
        all.truncate(limit);
        Ok(all)
    }

    /// Deletes the activity log. The blocked apps stay.
    pub fn clear_app_activity(&self) -> AppResult<()> {
        for name in [LOG_FILE, OLD_LOG_FILE] {
            match std::fs::remove_file(self.dir().join(name)) {
                Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e.into()),
                _ => {}
            }
        }
        Ok(())
    }

    /// Whether the person blocked `app` (by the name the app reports). An unreadable list is
    /// an error, never "not blocked".
    pub fn is_app_blocked(&self, app: &str) -> AppResult<bool> {
        Ok(self
            .read::<Apps>(APPS_FILE)?
            .blocked
            .iter()
            .any(|b| b.eq_ignore_ascii_case(app)))
    }

    pub fn set_app_blocked(&self, app: &str, blocked: bool) -> AppResult<()> {
        let mut apps: Apps = self.read(APPS_FILE)?;
        apps.blocked.retain(|b| !b.eq_ignore_ascii_case(app));
        if blocked {
            apps.blocked.push(app.to_string());
        }
        self.write(APPS_FILE, &apps)
    }

    /// Every app in the log or on the blocked list, most recently used first.
    pub fn app_summaries(&self) -> AppResult<Vec<AppSummary>> {
        let mut by_name: BTreeMap<String, AppSummary> = BTreeMap::new();
        for a in self.app_activity(usize::MAX)? {
            // Reads (list_*, describe_schema) and hand-offs are logged too; count queries.
            if !QUERY_TOOLS.contains(&a.tool.as_str()) {
                continue;
            }
            let s = by_name.entry(a.app.clone()).or_insert(AppSummary {
                name: a.app.clone(),
                blocked: false,
                queries: 0,
                last_used: None,
            });
            s.queries += 1;
            s.last_used = s.last_used.max(Some(a.at));
        }
        let apps: Apps = self.read(APPS_FILE)?;
        for b in &apps.blocked {
            by_name
                .entry(b.clone())
                .or_insert(AppSummary {
                    name: b.clone(),
                    blocked: true,
                    queries: 0,
                    last_used: None,
                })
                .blocked = true;
        }
        let mut list: Vec<AppSummary> = by_name.into_values().collect();
        list.sort_by(|a, b| b.last_used.cmp(&a.last_used).then(a.name.cmp(&b.name)));
        Ok(list)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> (Store, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("sayql-apps-{}", uuid::Uuid::new_v4()));
        (Store::new(dir.clone()), dir)
    }

    fn entry(app: &str, at: u64, sql: &str) -> Activity {
        Activity {
            at,
            sql: sql.into(),
            ..Activity::new(app, "run_query")
        }
    }

    #[test]
    fn logs_newest_first_and_survives_a_bad_line() {
        let (s, dir) = store();
        assert!(s.app_activity(10).unwrap().is_empty());
        s.log_app_activity(entry("Claude Code", 1, "SELECT 1"))
            .unwrap();
        std::fs::OpenOptions::new()
            .append(true)
            .open(dir.join(LOG_FILE))
            .unwrap()
            .write_all(b"not json\n")
            .unwrap();
        s.log_app_activity(entry("Cursor", 2, "SELECT 2")).unwrap();
        let log = s.app_activity(10).unwrap();
        assert_eq!(
            log.iter().map(|a| a.app.as_str()).collect::<Vec<_>>(),
            ["Cursor", "Claude Code"]
        );
        assert_eq!(s.app_activity(1).unwrap().len(), 1);
        s.clear_app_activity().unwrap();
        assert!(s.app_activity(10).unwrap().is_empty());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn long_sql_is_cut_and_big_logs_roll_over() {
        let (s, dir) = store();
        s.log_app_activity(entry("A", 1, &"x".repeat(10_000)))
            .unwrap();
        assert!(s.app_activity(1).unwrap()[0].sql.chars().count() <= MAX_SQL + 1);
        std::fs::write(dir.join(LOG_FILE), vec![b' '; MAX_LOG_BYTES as usize + 1]).unwrap();
        s.log_app_activity(entry("B", 2, "SELECT 1")).unwrap();
        assert!(dir.join(OLD_LOG_FILE).exists());
        assert_eq!(s.app_activity(10).unwrap()[0].app, "B");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn blocking_is_per_app_and_listed() {
        let (s, dir) = store();
        s.log_app_activity(entry("Claude Code", 5, "SELECT 1"))
            .unwrap();
        s.log_app_activity(entry("Claude Code", 9, "SELECT 2"))
            .unwrap();
        s.set_app_blocked("Cursor", true).unwrap();
        assert!(s.is_app_blocked("cursor").unwrap() && !s.is_app_blocked("Claude Code").unwrap());
        let apps = s.app_summaries().unwrap();
        assert_eq!(
            apps[0],
            AppSummary {
                name: "Claude Code".into(),
                blocked: false,
                queries: 2,
                last_used: Some(9)
            }
        );
        assert_eq!(
            apps[1],
            AppSummary {
                name: "Cursor".into(),
                blocked: true,
                queries: 0,
                last_used: None
            }
        );
        s.set_app_blocked("Cursor", false).unwrap();
        assert!(!s.is_app_blocked("Cursor").unwrap());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn an_unreadable_block_list_fails_closed_and_stays_put() {
        let (s, dir) = store();
        std::fs::write(dir.join(APPS_FILE), r#"{"blocked": ["Cursor""#).unwrap();
        let s = s.without_repair();
        assert!(s.is_app_blocked("Claude Code").is_err());
        assert!(s.set_app_blocked("Cursor", false).is_err());
        assert_eq!(
            std::fs::read_to_string(dir.join(APPS_FILE)).unwrap(),
            r#"{"blocked": ["Cursor""#,
            "the file is left for the app to repair"
        );
        let _ = std::fs::remove_dir_all(dir);
    }
}
