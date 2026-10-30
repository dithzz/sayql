//! The tools the agent's model may call. They only ask; `run.rs` does the work, through the
//! read-only gate and the connection's result setting.

use super::model::ToolSpec;
use serde_json::json;

pub const PLAN: &str = "propose_plan";
pub const SEARCH: &str = "search_schema";
pub const DESCRIBE: &str = "describe_table";
pub const PROFILE: &str = "profile_column";
pub const QUERY: &str = "run_query";
pub const EXPLAIN: &str = "explain_query";
pub const ASK: &str = "ask_user";
pub const FINISH: &str = "finish";
pub const SHOW: &str = "show_value";
pub const CHECK_NOTE: &str = "check_note";

pub fn all() -> Vec<ToolSpec> {
    vec![
        ToolSpec {
            name: CHECK_NOTE,
            description: "Runs a saved note's check against the data and says whether the note still holds. Use it before \
                          relying on a note marked \"may be out of date\", when the note has a check.",
            schema: json!({
                "type": "object",
                "properties": { "note": { "type": "string", "description": "The note's id, e.g. n_1a2b3c4d." } },
                "required": ["note"]
            }),
        },
        ToolSpec {
            name: PLAN,
            description: "Before anything else: the three to six steps you will take. The person may change them.",
            schema: json!({
                "type": "object",
                "properties": { "steps": { "type": "array", "items": { "type": "string" }, "description": "One short line each." } },
                "required": ["steps"]
            }),
        },
        ToolSpec {
            name: SEARCH,
            description: "Tables and columns whose names contain any of the words, with keys, row estimates and saved notes. \
                          Reads the cached schema; no database call.",
            schema: json!({
                "type": "object",
                "properties": { "words": { "type": "string", "description": "Space-separated words, e.g. \"owner role member\"." } },
                "required": ["words"]
            }),
        },
        ToolSpec {
            name: DESCRIBE,
            description: "All columns, keys, enum values and saved notes of one table.",
            schema: json!({
                "type": "object",
                "properties": { "table": { "type": "string", "description": "The table's name, schema-qualified if needed." } },
                "required": ["table"]
            }),
        },
        ToolSpec {
            name: PROFILE,
            description: "One column's row count, nulls, distinct values, min and max, and its most common values with counts \
                          (as far as this connection's setting allows). Computed in the database.",
            schema: json!({
                "type": "object",
                "properties": { "table": { "type": "string" }, "column": { "type": "string" }, "purpose": { "type": "string", "description": "One line for the person: why." } },
                "required": ["table", "column"]
            }),
        },
        ToolSpec {
            name: QUERY,
            description: "Run one read-only SQL statement. Returns the result as far as this connection's setting allows.",
            schema: json!({
                "type": "object",
                "properties": {
                    "sql": { "type": "string" },
                    "purpose": { "type": "string", "description": "One line for the person, e.g. \"Counting appointments per tenant last week\"." }
                },
                "required": ["sql", "purpose"]
            }),
        },
        ToolSpec {
            name: EXPLAIN,
            description: "The database's estimated plan and cost for a query, without running it. Use before a query that \
                          could scan a big table. Not available on SQL Server.",
            schema: json!({ "type": "object", "properties": { "sql": { "type": "string" } }, "required": ["sql"] }),
        },
        ToolSpec {
            name: SHOW,
            description: "Look up values to name in your answer, such as the full_name of the user with id 9, or of \
                          the user with the most claims. You get tokens (like ‹name-3›), never the values; put them in the \
                          answer and the person sees the real values. Give either `sql` (one read-only query whose first \
                          column is what to show, at most 5 rows) or `table`, `column`, `key_column` and `key`.",
            schema: json!({
                "type": "object",
                "properties": {
                    "sql": { "type": "string", "description": "e.g. SELECT full_name FROM users WHERE id = (SELECT user_id FROM access GROUP BY user_id ORDER BY COUNT(*) DESC LIMIT 1)" },
                    "table": { "type": "string" },
                    "column": { "type": "string", "description": "The column to show, e.g. full_name." },
                    "key_column": { "type": "string", "description": "The column that picks the row, e.g. id." },
                    "key": { "description": "Its value, e.g. 9, or a token from an earlier result." }
                },
                "required": []
            }),
        },
        ToolSpec {
            name: ASK,
            description: "Ask the person a question and wait for the answer. Use when the question has two reasonable \
                          meanings that lead to different answers.",
            schema: json!({
                "type": "object",
                "properties": {
                    "question": { "type": "string" },
                    "choices": { "type": "array", "items": { "type": "string" }, "description": "Two to four short options." }
                },
                "required": ["question"]
            }),
        },
        ToolSpec {
            name: FINISH,
            description: "End the run with the answer.",
            schema: json!({
                "type": "object",
                "properties": {
                    "answer": { "type": "string", "description": "A short plain-English answer." },
                    "citations": {
                        "type": "array",
                        "description": "Each sentence of the answer with the step numbers it rests on.",
                        "items": {
                            "type": "object",
                            "properties": { "sentence": { "type": "string" }, "steps": { "type": "array", "items": { "type": "integer" } } },
                            "required": ["sentence", "steps"]
                        }
                    },
                    "confidence": { "type": "string", "enum": ["high", "medium", "low"] },
                    "assumptions": { "type": "array", "items": { "type": "string" } },
                    "notes": {
                        "type": "array",
                        "description": "Up to three notes: new facts about the database's codes and meanings, or fixes to saved notes that turned out wrong.",
                        "items": {
                            "type": "object",
                            "properties": {
                                "text": { "type": "string", "description": "The note, up to 300 characters. For a removal: why." },
                                "replaces": { "type": "string", "description": "A saved note's id (n_…) this one corrects." },
                                "remove": { "type": "boolean", "description": "True with replaces: the saved note is wrong and should go." },
                                "check": { "type": "string", "description": "Optional read-only SQL returning one row, one column: true while the note holds." },
                                "evidence": { "type": "string", "description": "What you found that shows it (step numbers, tables)." }
                            },
                            "required": ["text"]
                        }
                    },
                    "proposed_sql": { "type": "string", "description": "A statement that changes data, for the person to review. Never run." },
                    "cannot_answer": { "type": "boolean", "description": "True when the data can't answer the question." }
                },
                "required": ["answer"]
            }),
        },
    ]
}

/// Only `finish`: offered when the run reaches its step or query limit.
pub fn finish_only() -> Vec<ToolSpec> {
    all().into_iter().filter(|t| t.name == FINISH).collect()
}
