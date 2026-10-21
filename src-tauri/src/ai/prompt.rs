//! The English → SQL prompt.

use serde::Deserialize;
use serde_json::{Value, json};

const INSTRUCTIONS: &str = "You turn a person's plain-English request into one {DIALECT} query for the database described below. \
The person is usually not a developer, so infer what they mean.

Rules:
- Reply with the SQL only: no Markdown fences, no explanation before or after. Your reply must begin with \
the SQL itself or a `-- ` comment line, never with prose; put any note you need in a `-- ` comment.
- Write a single statement in {DIALECT} syntax, using only tables and columns from the schema. Quote identifiers only when needed.
- Prefer readable results: pick useful columns instead of SELECT * when joining, use clear aliases, and join to show names rather than bare IDs.
- When the request lists rows without a limit, return at most 100 rows using {DIALECT} syntax (TOP 100 in SQL Server, LIMIT 100 elsewhere). Never limit aggregates the person asked for in full.
- Interpret relative dates (\"last week\", \"this year\") relative to the current date in SQL, not a hard-coded date.
- Write INSERT, UPDATE, DELETE or DDL only when the person explicitly asks to change data or structure.
- If something is ambiguous, make the most reasonable choice and state the assumption in one leading `-- ` comment line.
- If the request cannot be answered from this schema, reply with only a `-- ` comment saying what is missing.

Follow-ups: the message may include the query the person ran last. When the new request reads as a change to it \
(\"remove the middle name\", \"only active ones\", \"sort by date\", \"add their email\", \"show 50\"), return that query \
with the change applied and everything else kept as it was. When the new request is a different question, ignore it \
and write a fresh query.";

/// The query the person ran last in the same tab, for follow-up requests.
#[derive(Debug, Clone, Deserialize)]
pub struct PreviousQuery {
    pub question: String,
    pub sql: String,
}

/// What the model is asked to do with the request.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Task {
    /// Write SQL while the person types, building on the previous query for follow-ups.
    Draft { previous: Option<PreviousQuery> },
    /// Correct a query the database rejected.
    Fix { sql: String, error: String },
    /// Review the current query and return the best version of it.
    Improve { sql: String },
}

impl Task {
    /// Fixes and improvements go to the stronger model; drafts to the fast one.
    pub fn needs_expert(&self) -> bool {
        !matches!(self, Task::Draft { .. })
    }

    /// Drafts must be fast; fixes and reviews are worth a little more thought.
    pub(super) fn effort(&self) -> &'static str {
        if self.needs_expert() { "medium" } else { "low" }
    }

    /// SQL that comes with the request, for picking the relevant tables of a large schema.
    pub fn context_sql(&self) -> &str {
        match self {
            Task::Draft { previous } => previous.as_ref().map_or("", |p| p.sql.as_str()),
            Task::Fix { sql, .. } | Task::Improve { sql } => sql,
        }
    }
}

fn tagged(tag: &str, body: &str) -> String {
    format!("<{tag}>\n{}\n</{tag}>", body.trim())
}

/// The user turn for a task.
fn user_message(question: &str, task: &Task) -> String {
    let request = if question.trim().is_empty() { "(not stated; infer it from the query)" } else { question.trim() };
    match task {
        Task::Draft { previous } => match previous.as_ref().filter(|p| !p.sql.trim().is_empty()) {
            None => question.to_string(),
            Some(p) => {
                let asked = if p.question.trim().is_empty() { String::new() } else { format!("Request: {}\n", p.question.trim()) };
                format!("<previous_query>\n{asked}SQL:\n{}\n</previous_query>\n\nNew request: {question}", p.sql.trim())
            }
        },
        Task::Fix { sql, error } => format!(
            "{}\n\n{}\n\n{}\n\nThe database rejected this query. Find the cause in the error and the schema, and return \
             the corrected query for the request. Keep everything that was not part of the problem.",
            tagged("request", request),
            tagged("failed_query", sql),
            tagged("database_error", error)
        ),
        Task::Improve { sql } => format!(
            "{}\n\n{}\n\nReview this query against the request and the schema: joins, filters, date logic, grouping, \
             NULL handling and the columns shown. Return the best query for the request. If it is already right, \
             return it unchanged.",
            tagged("request", request),
            tagged("current_query", sql)
        ),
    }
}

/// The Messages API body (without model-specific options) for one request.
pub(super) fn request_body(model: &str, dialect: &str, schema_text: &str, question: &str, task: &Task) -> Value {
    json!({
        "model": model,
        "max_tokens": 16000,
        "stream": true,
        "system": [
            { "type": "text", "text": INSTRUCTIONS.replace("{DIALECT}", dialect) },
            // The schema is identical across keystrokes for a connection, so it is cached
            // and only the short request is processed fresh each time.
            {
                "type": "text",
                "text": format!("<schema dialect=\"{dialect}\">\n{schema_text}\n</schema>"),
                "cache_control": { "type": "ephemeral", "ttl": "1h" }
            }
        ],
        "messages": [{ "role": "user", "content": user_message(question, task) }],
        "output_config": { "effort": task.effort() },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn draft(previous: Option<PreviousQuery>) -> Task {
        Task::Draft { previous }
    }

    #[test]
    fn a_new_question_is_sent_as_is() {
        assert_eq!(user_message("top 10 customers", &draft(None)), "top 10 customers");
        let empty = PreviousQuery { question: "x".into(), sql: "  ".into() };
        assert_eq!(user_message("top 10 customers", &draft(Some(empty))), "top 10 customers");
    }

    #[test]
    fn a_follow_up_carries_the_previous_query() {
        let prev = PreviousQuery {
            question: "list a few clients from account 42".into(),
            sql: "SELECT TOP 10 first_name, middle_name, last_name FROM clients WHERE account_id = 42;".into(),
        };
        let m = user_message("remove the middle name", &draft(Some(prev)));
        assert!(m.starts_with("<previous_query>\nRequest: list a few clients from account 42\nSQL:\nSELECT TOP 10"), "{m}");
        assert!(m.ends_with("</previous_query>\n\nNew request: remove the middle name"), "{m}");
    }

    #[test]
    fn fixes_carry_the_error_and_go_to_the_expert() {
        let t = Task::Fix { sql: "SELECT nme FROM clients".into(), error: "Invalid column name 'nme'.".into() };
        let m = user_message("list clients", &t);
        assert!(m.contains("<failed_query>\nSELECT nme FROM clients\n</failed_query>"), "{m}");
        assert!(m.contains("<database_error>\nInvalid column name 'nme'.\n</database_error>"), "{m}");
        assert!(t.needs_expert());
        assert_eq!(t.effort(), "medium");
        assert!(!draft(None).needs_expert());
    }

    #[test]
    fn tasks_deserialize_from_the_ui() {
        let t: Task = serde_json::from_str(r#"{"kind":"improve","sql":"SELECT 1"}"#).unwrap();
        assert!(matches!(t, Task::Improve { .. }));
        let t: Task = serde_json::from_str(r#"{"kind":"draft","previous":null}"#).unwrap();
        assert_eq!(user_message("", &Task::Improve { sql: "SELECT 1".into() }).contains("(not stated"), true);
        assert!(matches!(t, Task::Draft { previous: None }));
    }
}
