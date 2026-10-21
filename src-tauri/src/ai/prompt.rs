//! The English → SQL prompt.

use serde_json::{Value, json};

const INSTRUCTIONS: &str = "You turn a person's plain-English request into one {DIALECT} query for the database described below. \
The person is usually not a developer, so infer what they mean.

Rules:
- Reply with the SQL only: no Markdown fences, no explanation before or after.
- Write a single statement in {DIALECT} syntax, using only tables and columns from the schema. Quote identifiers only when needed.
- Prefer readable results: pick useful columns instead of SELECT * when joining, use clear aliases, and join to show names rather than bare IDs.
- When the request lists rows without a limit, add LIMIT 100. Never add a limit to aggregates the person asked for in full.
- Interpret relative dates (\"last week\", \"this year\") relative to the current date in SQL, not a hard-coded date.
- Write INSERT, UPDATE, DELETE or DDL only when the person explicitly asks to change data or structure.
- If something is ambiguous, make the most reasonable choice and state the assumption in one leading `-- ` comment line.
- If the request cannot be answered from this schema, reply with only a `-- ` comment saying what is missing.";

/// The Messages API body (without model-specific options) for one question.
pub(super) fn request_body(model: &str, dialect: &str, schema_text: &str, question: &str) -> Value {
    json!({
        "model": model,
        "max_tokens": 16000,
        "stream": true,
        "system": [
            { "type": "text", "text": INSTRUCTIONS.replace("{DIALECT}", dialect) },
            // The schema is identical across keystrokes for a connection, so it is cached
            // and only the short question is processed fresh on each request.
            {
                "type": "text",
                "text": format!("<schema dialect=\"{dialect}\">\n{schema_text}\n</schema>"),
                "cache_control": { "type": "ephemeral", "ttl": "1h" }
            }
        ],
        "messages": [{ "role": "user", "content": question }],
        "output_config": { "effort": "low" },
    })
}
