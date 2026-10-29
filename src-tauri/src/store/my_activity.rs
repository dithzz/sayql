//! Your own activity: the queries you ran in SayQL, from the editor, the Ask box or Investigate.
//! It stays on this computer, like the rest of the store.
//!
//! JSON Lines, appended by the app; past [`MAX_LOG_BYTES`] the file moves to `.1` (replacing the
//! older one), so it keeps roughly the last two files' worth. It holds the SQL and counts, never
//! rows.

use super::Store;
use crate::error::AppResult;
use serde::{Deserialize, Serialize};
use std::io::{BufRead, Write};

const LOG_FILE: &str = "my-activity.jsonl";
const OLD_LOG_FILE: &str = "my-activity.1.jsonl";
/// Size at which the log starts a new file.
const MAX_LOG_BYTES: u64 = 1024 * 1024;
/// Longest SQL kept per entry.
const MAX_SQL: usize = 4000;
/// Longest error or detail kept per entry.
const MAX_DETAIL: usize = 300;

/// One query you ran.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MyActivity {
    /// Milliseconds since the Unix epoch.
    pub at: u64,
    pub connection_id: String,
    #[serde(default)]
    pub database: Option<String>,
    /// "editor", "ask" or "investigate".
    pub source: String,
    pub sql: String,
    /// ok, error or cancelled.
    pub status: String,
    #[serde(default)]
    pub rows: Option<usize>,
    #[serde(default)]
    pub ms: Option<u64>,
    /// The error, or for Investigate what went to the AI ("sent row count only").
    #[serde(default)]
    pub detail: Option<String>,
    /// The question it answered, when there was one.
    #[serde(default)]
    pub question: Option<String>,
}

impl MyActivity {
    pub fn new(connection_id: &str, database: Option<&str>, source: &str, sql: &str) -> Self {
        MyActivity {
            at: chrono::Utc::now().timestamp_millis().max(0) as u64,
            connection_id: connection_id.to_string(),
            database: database.map(str::to_string),
            source: source.to_string(),
            sql: sql.to_string(),
            status: "ok".into(),
            rows: None,
            ms: None,
            detail: None,
            question: None,
        }
    }
}

fn cut(s: &mut String, max: usize) {
    if s.len() > max {
        let mut end = max;
        while !s.is_char_boundary(end) {
            end -= 1;
        }
        s.truncate(end);
        s.push('…');
    }
}

impl Store {
    /// Appends one entry. Errors are the caller's to ignore: a log that can't be written must
    /// not stop the query.
    pub fn log_my_activity(&self, mut entry: MyActivity) -> AppResult<()> {
        cut(&mut entry.sql, MAX_SQL);
        if let Some(d) = entry.detail.as_mut() {
            cut(d, MAX_DETAIL);
        }
        if let Some(q) = entry.question.as_mut() {
            cut(q, MAX_DETAIL);
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
        std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)?
            .write_all(line.as_bytes())?;
        Ok(())
    }

    /// The newest `limit` entries, newest first; for one connection when given. Lines that
    /// don't parse are skipped.
    pub fn my_activity(
        &self,
        connection_id: Option<&str>,
        limit: usize,
    ) -> AppResult<Vec<MyActivity>> {
        let mut all = Vec::new();
        for name in [OLD_LOG_FILE, LOG_FILE] {
            match std::fs::File::open(self.dir().join(name)) {
                Ok(f) => all.extend(
                    std::io::BufReader::new(f)
                        .lines()
                        .map_while(Result::ok)
                        .filter_map(|l| serde_json::from_str::<MyActivity>(&l).ok())
                        .filter(|a| connection_id.is_none_or(|id| a.connection_id == id)),
                ),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(e.into()),
            }
        }
        all.reverse();
        all.truncate(limit);
        Ok(all)
    }

    /// Deletes your activity log.
    pub fn clear_my_activity(&self) -> AppResult<()> {
        for name in [LOG_FILE, OLD_LOG_FILE] {
            match std::fs::remove_file(self.dir().join(name)) {
                Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e.into()),
                _ => {}
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> (Store, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("sayql-my-activity-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        (Store::new(dir.clone()), dir)
    }

    #[test]
    fn logs_newest_first_per_connection_and_clears() {
        let (s, dir) = store();
        s.log_my_activity(MyActivity::new("a", None, "editor", "SELECT 1"))
            .unwrap();
        s.log_my_activity(MyActivity::new("b", Some("shop"), "ask", "SELECT 2"))
            .unwrap();
        s.log_my_activity(MyActivity::new("a", None, "investigate", "SELECT 3"))
            .unwrap();
        std::fs::OpenOptions::new()
            .append(true)
            .open(dir.join(LOG_FILE))
            .unwrap()
            .write_all(b"not json\n")
            .unwrap();
        let all = s.my_activity(None, 10).unwrap();
        assert_eq!(
            all.iter().map(|a| a.sql.as_str()).collect::<Vec<_>>(),
            ["SELECT 3", "SELECT 2", "SELECT 1"]
        );
        let a = s.my_activity(Some("a"), 10).unwrap();
        assert_eq!(a.len(), 2);
        assert_eq!(s.my_activity(None, 1).unwrap().len(), 1);
        s.clear_my_activity().unwrap();
        assert!(s.my_activity(None, 10).unwrap().is_empty());
    }

    #[test]
    fn long_sql_and_errors_are_cut() {
        let (s, _dir) = store();
        let mut e = MyActivity::new("a", None, "editor", &"x".repeat(MAX_SQL + 50));
        e.detail = Some("é".repeat(MAX_DETAIL));
        s.log_my_activity(e).unwrap();
        let got = &s.my_activity(None, 1).unwrap()[0];
        assert!(got.sql.ends_with('…') && got.sql.len() <= MAX_SQL + 3);
        assert!(got.detail.as_ref().unwrap().ends_with('…'));
    }
}
