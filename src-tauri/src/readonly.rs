//! Read-only access for AI clients: the pieces `sayql-mcp` and agent mode share, so both
//! keep the same safety rules. Schema search, the write refusal, and queries that SayQL
//! stops on the server when they run too long.

use crate::db::{self, DbPool, QueryResult};
use crate::schema::{self, Schema, Table};
use std::time::Duration;
use tokio_util::sync::CancellationToken;

/// Rows a query tool returns when the caller doesn't say.
pub const DEFAULT_ROWS: usize = 100;
/// Most rows a query tool returns at once.
pub const MAX_ROWS: usize = 1000;
/// A schema bigger than this is listed by table name unless a search narrows it.
const FULL_SCHEMA_CHARS: usize = 60_000;

pub const WRITE_REFUSED: &str = "SayQL's MCP access is read-only, and this statement could change data. \
Use open_query to put it in a SayQL tab, where the user reviews and runs it.";

/// Runs a read-only query, stopping it on the server if it runs longer than `limit`.
pub async fn run_limited(pool: &DbPool, sql: &str, cap: usize, limit: Duration) -> Result<QueryResult, String> {
    let cancel = CancellationToken::new();
    let timer = tokio::spawn({
        let cancel = cancel.clone();
        async move {
            tokio::time::sleep(limit).await;
            cancel.cancel();
        }
    });
    let out = db::run_query_with(pool, sql, true, cap, &cancel).await;
    timer.abort();
    out.map_err(|e| match e.to_string() {
        e if e == db::CANCELLED => format!(
            "The query was still running after {} seconds, so SayQL stopped it. Narrow it (a WHERE clause, \
             fewer joins, LIMIT/TOP) and try again, or run it in the SayQL app.",
            limit.as_secs()
        ),
        e => e,
    })
}

/// The schema as compact text: everything when small, else the tables matching `search`,
/// else just the table names with a hint to search.
pub fn describe(schema: &Schema, search: Option<&str>) -> String {
    let words: Vec<String> = search.unwrap_or("").split_whitespace().map(str::to_lowercase).collect();
    let matches = |t: &Table| {
        words.iter().any(|w| {
            t.qualified_name().to_lowercase().contains(w) || t.columns.iter().any(|c| c.name.to_lowercase().contains(w))
        })
    };
    let header = format!("Dialect: {}. {} tables and views.", schema.dialect, schema.tables.len());
    if !words.is_empty() {
        let found: Vec<String> = schema.tables.iter().filter(|t| matches(t)).map(|t| schema::render_table(t, &schema.dialect)).collect();
        return if found.is_empty() {
            format!("{header}\nNo table or column matches {:?}. Try other words, or omit `search` for the table list.", words.join(" "))
        } else {
            format!("{header} Showing {} matching:\n{}", found.len(), found.join("\n"))
        };
    }
    let full = schema::render_full(schema);
    if full.len() <= FULL_SCHEMA_CHARS {
        return format!("{header}\n{full}");
    }
    let names: Vec<String> = schema
        .tables
        .iter()
        .map(|t| match t.row_estimate {
            Some(n) => format!("{} (~{n} rows)", schema::render_name(t, &schema.dialect)),
            None => schema::render_name(t, &schema.dialect),
        })
        .collect();
    format!("{header} Too large to describe in full; call again with `search` words for columns.\n{}", names.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::Column;

    fn table(name: &str, cols: &[&str]) -> Table {
        Table {
            schema: Some("dbo".into()),
            name: name.into(),
            kind: "table".into(),
            columns: cols
                .iter()
                .map(|c| Column { name: (*c).into(), data_type: "int".into(), nullable: false, primary_key: false, enum_values: None })
                .collect(),
            foreign_keys: vec![],
            row_estimate: Some(10),
        }
    }

    #[test]
    fn describe_filters_by_search_words() {
        let s = Schema { dialect: "SQL Server".into(), tables: vec![table("Person", &["PERSON_EMAIL"]), table("Account", &["ID"])] };
        let out = describe(&s, Some("email"));
        assert!(out.contains("dbo.Person") && !out.contains("dbo.Account"), "{out}");
        assert!(describe(&s, Some("nothing")).contains("No table or column matches"));
        let all = describe(&s, None);
        assert!(all.contains("dbo.Person") && all.contains("dbo.Account"), "{all}");
    }

    #[test]
    fn describe_lists_names_for_huge_schemas() {
        let cols: Vec<String> = (0..60).map(|i| format!("column_{i}")).collect();
        let cols: Vec<&str> = cols.iter().map(String::as_str).collect();
        let tables = (0..200).map(|i| table(&format!("table_{i}"), &cols)).collect();
        let out = describe(&Schema { dialect: "SQL Server".into(), tables }, None);
        assert!(out.contains("Too large") && out.contains("dbo.table_199 (~10 rows)") && !out.contains("column_0"), "{}", &out[..200]);
    }
}
