//! What agent mode may do on each connection, the run limits, and database notes.

use super::Store;
use crate::error::{AppError, AppResult};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

const ACCESS_FILE: &str = "agent.json";
const NOTES_FILE: &str = "notes.json";
pub const MAX_NOTE: usize = 300;
pub const MAX_NEVER_SEND: usize = 200;

/// What the AI may see of query results on one connection.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Results {
    /// Only how many rows a query returned, and its column names.
    #[default]
    Off,
    /// Per result: row count, and per column its type, nulls, distinct count, min and max.
    Counts,
    /// Up to 20 rows per query, with likely personal data masked.
    Rows,
}

/// What AI apps using SayQL over MCP (Claude Code, Cursor, ...) may see of query results on
/// one connection. Separate from agent mode's setting; counts and summaries until the person
/// chooses otherwise.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AppResults {
    /// Only how many rows a query returned, and its column names.
    Off,
    /// Per result: row count, and per column its type, nulls, distinct count, min and max.
    #[default]
    Counts,
    /// Rows, with likely personal data replaced by tokens.
    Masked,
    /// Rows as they are (never-send columns still withheld), and CSV export.
    All,
}

/// Agent mode's access to one connection, which is off until the person turns it on, and
/// what AI apps may see of it.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentAccess {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub results: Results,
    /// What AI apps over MCP may see. Never-send columns apply to them too.
    #[serde(default)]
    pub apps: AppResults,
    /// Pause before each send to the AI, with Send and Skip.
    #[serde(default = "yes")]
    pub ask_before_send: bool,
    /// Columns never sent (`column` or `table.column`), whatever the setting.
    #[serde(default)]
    pub never_send: Vec<String>,
}

fn yes() -> bool {
    true
}

/// Run limits and agent preferences, in Settings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentSettings {
    #[serde(default = "default_steps")]
    pub max_steps: u32,
    #[serde(default = "default_queries")]
    pub max_queries: u32,
    #[serde(default = "default_query_secs")]
    pub query_seconds: u64,
    #[serde(default = "default_run_secs")]
    pub run_seconds: u64,
    /// Prompt tokens at the full rate plus output tokens, per run.
    #[serde(default = "default_tokens")]
    pub max_tokens: u64,
    /// Start right away instead of showing the plan first.
    #[serde(default)]
    pub skip_plan: bool,
}

fn default_steps() -> u32 {
    12
}
fn default_queries() -> u32 {
    10
}
fn default_query_secs() -> u64 {
    30
}
fn default_run_secs() -> u64 {
    180
}
fn default_tokens() -> u64 {
    400_000
}

impl Default for AgentSettings {
    fn default() -> Self {
        AgentSettings {
            max_steps: default_steps(),
            max_queries: default_queries(),
            query_seconds: default_query_secs(),
            run_seconds: default_run_secs(),
            max_tokens: default_tokens(),
            skip_plan: false,
        }
    }
}

impl AgentSettings {
    /// The limits within sensible bounds.
    pub fn validated(self) -> AppResult<AgentSettings> {
        let check = |ok: bool, m: &str| {
            if ok {
                Ok(())
            } else {
                Err(AppError::User(m.to_string()))
            }
        };
        check(
            (1..=50).contains(&self.max_steps),
            "Steps per run must be between 1 and 50.",
        )?;
        check(
            (1..=50).contains(&self.max_queries),
            "Queries per run must be between 1 and 50.",
        )?;
        check(
            (5..=600).contains(&self.query_seconds),
            "Time per query must be between 5 and 600 seconds.",
        )?;
        check(
            (30..=3600).contains(&self.run_seconds),
            "Time per run must be between 30 seconds and an hour.",
        )?;
        check(
            (10_000..=5_000_000).contains(&self.max_tokens),
            "Tokens per run must be between 10,000 and 5,000,000.",
        )?;
        Ok(self)
    }
}

/// A fact about a database that helps answer questions on it ("owner = memberships.role
/// = 'owner'"). Schema-level only: notes never hold row values.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Note {
    pub id: String,
    pub connection_id: String,
    #[serde(default)]
    pub database: Option<String>,
    pub text: String,
    /// Milliseconds since the Unix epoch.
    #[serde(default)]
    pub updated_at: u64,
}

impl Store {
    pub fn agent_access(&self, connection_id: &str) -> AppResult<AgentAccess> {
        let all: BTreeMap<String, AgentAccess> = self.read(ACCESS_FILE)?;
        Ok(all.get(connection_id).cloned().unwrap_or_default())
    }

    pub fn save_agent_access(&self, connection_id: &str, access: AgentAccess) -> AppResult<()> {
        let mut all: BTreeMap<String, AgentAccess> = self.read(ACCESS_FILE)?;
        all.insert(connection_id.to_string(), clean_access(access)?);
        self.write(ACCESS_FILE, &all)
    }

    /// Forgets a deleted connection's agent access, notes and suggested notes.
    pub fn forget_agent(&self, connection_id: &str) -> AppResult<()> {
        let mut all: BTreeMap<String, AgentAccess> = self.read(ACCESS_FILE)?;
        if all.remove(connection_id).is_some() {
            self.write(ACCESS_FILE, &all)?;
        }
        let mut notes = self.all_notes()?;
        let before = notes.len();
        notes.retain(|n| n.connection_id != connection_id);
        if notes.len() != before {
            self.write(NOTES_FILE, &notes)?;
        }
        self.forget_note_suggestions(connection_id)
    }

    pub fn all_notes(&self) -> AppResult<Vec<Note>> {
        self.read(NOTES_FILE)
    }

    /// The notes for one database (`None`: the connection's default database).
    pub fn notes(&self, connection_id: &str, database: Option<&str>) -> AppResult<Vec<Note>> {
        Ok(self
            .all_notes()?
            .into_iter()
            .filter(|n| n.connection_id == connection_id && n.database.as_deref() == database)
            .collect())
    }

    /// Adds or replaces (by id) a note, after checking it holds no row values.
    pub fn save_note(&self, note: Note) -> AppResult<Note> {
        let note = Note {
            text: clean_note(&note.text)?,
            updated_at: now_ms(),
            ..note
        };
        let mut all = self.all_notes()?;
        match all.iter_mut().find(|n| n.id == note.id) {
            Some(existing) => *existing = note.clone(),
            None => all.push(note.clone()),
        }
        self.write(NOTES_FILE, &all)?;
        Ok(note)
    }

    /// The notes as text to add after a schema, or nothing when there are none.
    pub fn notes_text(&self, connection_id: &str, database: Option<&str>) -> String {
        let notes = self.notes(connection_id, database).unwrap_or_default();
        if notes.is_empty() {
            return String::new();
        }
        let lines: Vec<String> = notes.iter().map(|n| format!("- {}", n.text)).collect();
        format!(
            "\n\nNotes saved for this database (facts learned earlier):\n{}",
            lines.join("\n")
        )
    }

    pub fn delete_note(&self, id: &str) -> AppResult<()> {
        let mut all = self.all_notes()?;
        all.retain(|n| n.id != id);
        self.write(NOTES_FILE, &all)
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn clean_access(a: AgentAccess) -> AppResult<AgentAccess> {
    let mut never_send: Vec<String> = a
        .never_send
        .iter()
        .map(|c| c.trim().to_string())
        .filter(|c| !c.is_empty())
        .collect();
    never_send.sort_by_key(|c| c.to_lowercase());
    never_send.dedup_by_key(|c| c.to_lowercase());
    if never_send.len() > MAX_NEVER_SEND {
        return Err(format!("Keep the never-send list to {MAX_NEVER_SEND} columns.").into());
    }
    Ok(AgentAccess { never_send, ..a })
}

/// The note tidied up, or why it can't be saved. A note that looks like it holds a value
/// from the data (an email, a phone number, a masked token) is refused.
pub fn clean_note(text: &str) -> AppResult<String> {
    let text = text.trim().to_string();
    if text.is_empty() {
        return Err("Write the note.".into());
    }
    if text.chars().count() > MAX_NOTE {
        return Err(format!("Keep a note to {MAX_NOTE} characters.").into());
    }
    if text.contains('‹')
        || crate::ai::agent::mask::value_kind(&serde_json::Value::String(text.clone())).is_some()
    {
        return Err("A note describes the database, not the data in it. Remove the value (an email, number or name) from it.".into());
    }
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> (std::path::PathBuf, Store) {
        let dir = std::env::temp_dir().join(format!("sayql-agent-store-{}", uuid::Uuid::new_v4()));
        (dir.clone(), Store::new(dir))
    }

    #[test]
    fn access_is_off_until_turned_on() {
        let (dir, s) = store();
        let a = s.agent_access("c1").unwrap();
        assert_eq!(
            (a.enabled, a.results, a.ask_before_send),
            (false, Results::Off, false)
        );
        let saved: AgentAccess =
            serde_json::from_str(r#"{"enabled":true,"results":"counts"}"#).unwrap();
        assert!(
            saved.ask_before_send,
            "Ask before send defaults to on once saved"
        );
        s.save_agent_access(
            "c1",
            AgentAccess {
                never_send: vec![" email ".into(), "EMAIL".into(), "".into()],
                ..saved
            },
        )
        .unwrap();
        assert_eq!(s.agent_access("c1").unwrap().never_send, ["email"]);
        assert!(!s.agent_access("c2").unwrap().enabled);
        s.forget_agent("c1").unwrap();
        assert!(!s.agent_access("c1").unwrap().enabled);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn notes_hold_no_values() {
        let (dir, s) = store();
        let note = |text: &str| Note {
            id: "n1".into(),
            connection_id: "c1".into(),
            database: None,
            text: text.into(),
            updated_at: 0,
        };
        s.save_note(note("owner = memberships.role = 'owner'"))
            .unwrap();
        s.save_note(note("status 3 = cancelled (appointment_statuses)"))
            .unwrap();
        assert_eq!(s.notes("c1", None).unwrap().len(), 1, "same id replaces");
        assert!(s.notes("c1", Some("other")).unwrap().is_empty());
        for bad in [
            "owner is dana@example.com",
            "call +1 (555) 014-2233",
            "owner is ‹name-1›",
            " ",
        ] {
            assert!(s.save_note(note(bad)).is_err(), "{bad}");
        }
        s.delete_note("n1").unwrap();
        assert!(s.all_notes().unwrap().is_empty());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn limits_are_checked() {
        assert!(AgentSettings::default().validated().is_ok());
        assert!(
            AgentSettings {
                max_steps: 0,
                ..Default::default()
            }
            .validated()
            .is_err()
        );
        assert!(
            AgentSettings {
                query_seconds: 9999,
                ..Default::default()
            }
            .validated()
            .is_err()
        );
    }
}
