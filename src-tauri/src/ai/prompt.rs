//! The English → SQL prompt.

use serde::Deserialize;
use serde_json::{Value, json};

const INSTRUCTIONS: &str = "You turn a person's plain-English request into one {DIALECT} query for the database described below. \
The person is usually not a developer, so infer what they mean.

Rules:
- Reply with the SQL only: no Markdown fences, no explanation before or after.
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

/// The user turn: the new request, preceded by the previous query when there is one.
fn user_message(question: &str, previous: Option<&PreviousQuery>) -> String {
    match previous.filter(|p| !p.sql.trim().is_empty()) {
        None => question.to_string(),
        Some(p) => {
            let asked = if p.question.trim().is_empty() { String::new() } else { format!("Request: {}\n", p.question.trim()) };
            format!("<previous_query>\n{asked}SQL:\n{}\n</previous_query>\n\nNew request: {question}", p.sql.trim())
        }
    }
}

/// The Messages API body (without model-specific options) for one request.
pub(super) fn request_body(
    model: &str,
    dialect: &str,
    schema_text: &str,
    question: &str,
    previous: Option<&PreviousQuery>,
) -> Value {
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
        "messages": [{ "role": "user", "content": user_message(question, previous) }],
        "output_config": { "effort": "low" },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_question_is_sent_as_is() {
        assert_eq!(user_message("top 10 customers", None), "top 10 customers");
        let empty = PreviousQuery { question: "x".into(), sql: "  ".into() };
        assert_eq!(user_message("top 10 customers", Some(&empty)), "top 10 customers");
    }

    #[test]
    fn a_follow_up_carries_the_previous_query() {
        let prev = PreviousQuery {
            question: "list a few clients from account 42".into(),
            sql: "SELECT TOP 10 first_name, middle_name, last_name FROM clients WHERE account_id = 42;".into(),
        };
        let m = user_message("remove the middle name", Some(&prev));
        assert!(m.starts_with("<previous_query>\nRequest: list a few clients from account 42\nSQL:\nSELECT TOP 10"), "{m}");
        assert!(m.ends_with("</previous_query>\n\nNew request: remove the middle name"), "{m}");
    }
}
