//! Database notes AI apps suggest over MCP (`suggest_note`). A suggestion waits here until the
//! person accepts it, which saves it as an ordinary note, or dismisses it. Like notes, a
//! suggestion is schema-level only: its text and evidence are refused when they look like
//! values from the data.

use super::Store;
use super::agent::{Note, clean_note};
use crate::error::AppResult;
use serde::{Deserialize, Serialize};

const FILE: &str = "note-suggestions.json";
/// Longest evidence an AI app may give.
pub const MAX_EVIDENCE: usize = 600;
/// Most suggestions waiting per connection.
pub const MAX_PENDING: usize = 50;

/// A note an AI app suggested, waiting for the person.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NoteSuggestion {
    pub id: String,
    pub connection_id: String,
    #[serde(default)]
    pub database: Option<String>,
    /// The note as it would be saved.
    pub text: String,
    /// Why the app thinks it's true (what it checked).
    #[serde(default)]
    pub evidence: String,
    /// The AI app's name, as it gave it when it connected.
    pub app: String,
    /// Milliseconds since the Unix epoch.
    pub created_at: u64,
}

/// Lowercase, single spaces: two notes that differ only in that are the same note.
fn same(a: &str, b: &str) -> bool {
    let norm = |s: &str| s.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase();
    norm(a) == norm(b)
}

/// The evidence tidied up, or why it can't be kept.
fn clean_evidence(text: &str) -> AppResult<String> {
    let text = text.trim().to_string();
    if text.chars().count() > MAX_EVIDENCE {
        return Err(format!("Keep the evidence to {MAX_EVIDENCE} characters.").into());
    }
    if text.contains('‹') || crate::ai::agent::mask::value_kind(&serde_json::Value::String(text.clone())).is_some() {
        return Err("The evidence looks like it holds a value from the data (an email, number, name or long id). \
                    Describe what you checked instead, and use short commit hashes."
            .into());
    }
    Ok(text)
}

impl Store {
    /// Every waiting suggestion, oldest first.
    pub fn note_suggestions(&self) -> AppResult<Vec<NoteSuggestion>> {
        self.read(FILE)
    }

    /// Stores a suggestion for the person to accept or dismiss. Refuses values from the data,
    /// a note already saved or suggested, and more than [`MAX_PENDING`] per connection.
    pub fn suggest_note(&self, connection_id: &str, database: Option<&str>, text: &str, evidence: &str, app: &str) -> AppResult<NoteSuggestion> {
        let text = clean_note(text)?;
        let evidence = clean_evidence(evidence)?;
        let database = database.map(str::trim).filter(|d| !d.is_empty()).map(str::to_string);
        if self.notes(connection_id, database.as_deref())?.iter().any(|n| same(&n.text, &text)) {
            return Err("The user already saved this note; it's in describe_schema.".into());
        }
        let mut all = self.note_suggestions()?;
        if all.iter().any(|s| s.connection_id == connection_id && s.database == database && same(&s.text, &text)) {
            return Err("This note was already suggested and is waiting for the user.".into());
        }
        if all.iter().filter(|s| s.connection_id == connection_id).count() >= MAX_PENDING {
            return Err(format!(
                "{MAX_PENDING} suggested notes are already waiting for the user on this connection. Ask them to review them in SayQL first."
            )
            .into());
        }
        let suggestion = NoteSuggestion {
            id: uuid::Uuid::new_v4().to_string(),
            connection_id: connection_id.to_string(),
            database,
            text,
            evidence,
            app: app.to_string(),
            created_at: chrono::Utc::now().timestamp_millis().max(0) as u64,
        };
        all.push(suggestion.clone());
        self.write(FILE, &all)?;
        Ok(suggestion)
    }

    /// Saves the suggestion as a note through the usual checks, and removes it.
    pub fn accept_note_suggestion(&self, id: &str) -> AppResult<Note> {
        let all = self.note_suggestions()?;
        let s = all.iter().find(|s| s.id == id).ok_or("This suggestion is no longer waiting.")?;
        let note = self.save_note(Note {
            id: uuid::Uuid::new_v4().to_string(),
            connection_id: s.connection_id.clone(),
            database: s.database.clone(),
            text: s.text.clone(),
            updated_at: 0,
        })?;
        self.dismiss_note_suggestion(id)?;
        Ok(note)
    }

    pub fn dismiss_note_suggestion(&self, id: &str) -> AppResult<()> {
        let mut all = self.note_suggestions()?;
        let before = all.len();
        all.retain(|s| s.id != id);
        if all.len() != before {
            self.write(FILE, &all)?;
        }
        Ok(())
    }

    /// Forgets a deleted connection's suggestions.
    pub(super) fn forget_note_suggestions(&self, connection_id: &str) -> AppResult<()> {
        let mut all = self.note_suggestions()?;
        let before = all.len();
        all.retain(|s| s.connection_id != connection_id);
        if all.len() != before {
            self.write(FILE, &all)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> (std::path::PathBuf, Store) {
        let dir = std::env::temp_dir().join(format!("sayql-suggest-{}", uuid::Uuid::new_v4()));
        (dir.clone(), Store::new(dir))
    }

    const EVIDENCE: &str = "no references in api/ since commit a1b2c3 (2026-03-12); pg stats: 0 writes since 2026-01-01";

    #[test]
    fn suggestions_wait_until_accepted() {
        let (dir, s) = store();
        let sug = s.suggest_note("c1", Some(" "), "orders_legacy is unused since March; use orders_v2", EVIDENCE, "Claude Code").unwrap();
        assert_eq!((sug.database.clone(), sug.app.as_str()), (None, "Claude Code"));
        assert!(s.notes("c1", None).unwrap().is_empty(), "not a note until accepted");
        assert!(s.notes_text("c1", None).is_empty());

        let note = s.accept_note_suggestion(&sug.id).unwrap();
        assert_eq!(note.text, sug.text);
        assert_eq!(s.notes("c1", None).unwrap(), vec![note]);
        assert!(s.notes_text("c1", None).contains("orders_legacy is unused"));
        assert!(s.note_suggestions().unwrap().is_empty());
        assert!(s.accept_note_suggestion(&sug.id).is_err(), "only once");
        // A saved note can't be suggested again.
        assert!(s.suggest_note("c1", None, "orders_legacy is  UNUSED since March; use orders_v2", "", "Cursor").unwrap_err().to_string().contains("already saved"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn dismissing_drops_it() {
        let (dir, s) = store();
        let sug = s.suggest_note("c1", Some("shop"), "status 3 = cancelled", "", "Cursor").unwrap();
        s.dismiss_note_suggestion(&sug.id).unwrap();
        assert!(s.note_suggestions().unwrap().is_empty());
        assert!(s.notes("c1", Some("shop")).unwrap().is_empty());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn refuses_duplicates_values_and_too_many() {
        let (dir, s) = store();
        s.suggest_note("c1", None, "status 3 = cancelled", EVIDENCE, "Cursor").unwrap();
        assert!(s.suggest_note("c1", None, "Status 3 = cancelled", EVIDENCE, "Claude Code").unwrap_err().to_string().contains("already suggested"));
        s.suggest_note("c1", Some("other"), "status 3 = cancelled", EVIDENCE, "Cursor").unwrap();
        for (text, evidence) in [
            ("owner is dana@example.com", ""),
            ("x".repeat(301).as_str(), ""),
            ("customers.vip marks paying customers", "row for dana@example.com has vip = true"),
            ("customers.vip marks paying customers", &"y".repeat(601)),
        ] {
            assert!(s.suggest_note("c1", None, text, evidence, "Cursor").is_err(), "{text} / {evidence}");
        }
        for i in 2..MAX_PENDING {
            s.suggest_note("c1", None, &format!("table t{i} is unused"), "", "Cursor").unwrap();
        }
        assert!(s.suggest_note("c1", None, "one more", "", "Cursor").unwrap_err().to_string().contains("already waiting"));
        assert!(s.suggest_note("c2", None, "one more", "", "Cursor").is_ok(), "the cap is per connection");
        s.forget_agent("c1").unwrap();
        assert!(s.note_suggestions().unwrap().iter().all(|x| x.connection_id == "c2"));
        let _ = std::fs::remove_dir_all(dir);
    }
}
