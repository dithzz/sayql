//! Compact one-line-per-table text for the AI prompt.

use super::{ForeignKey, Schema, Table};
use std::collections::{HashMap, HashSet};

/// Above this size we send only the tables relevant to the question instead of the
/// whole schema. Below it, sending everything keeps the prompt prefix identical across
/// keystrokes so it stays in the prompt cache, which is what keeps generation fast.
const FULL_SCHEMA_CHAR_BUDGET: usize = 300_000;
const RELEVANT_TABLE_LIMIT: usize = 30;
/// Tables that reference a focus table, included so the model can join to them.
const REFERENCING_TABLE_LIMIT: usize = 15;

/// One line per table, e.g.
/// `orders [table, ~1000 rows](id integer PK, customer_id integer -> customers.id, status text)`
pub fn render_table(t: &Table) -> String {
    let fk: HashMap<&str, &ForeignKey> =
        t.foreign_keys.iter().map(|f| (f.column.as_str(), f)).collect();
    let cols: Vec<String> = t
        .columns
        .iter()
        .map(|c| {
            let mut s = format!("{} {}", c.name, c.data_type);
            if c.primary_key {
                s.push_str(" PK");
            }
            if !c.nullable && !c.primary_key {
                s.push_str(" NOT NULL");
            }
            if let Some(f) = fk.get(c.name.as_str()) {
                s.push_str(&format!(" -> {}.{}", f.ref_table, f.ref_column));
            }
            if let Some(vals) = &c.enum_values {
                s.push_str(&format!(" {{{}}}", vals.join("|")));
            }
            s
        })
        .collect();
    let rows = t
        .row_estimate
        .filter(|n| *n >= 0)
        .map(|n| format!(", ~{n} rows"))
        .unwrap_or_default();
    format!("{} [{}{}]({})", t.qualified_name(), t.kind, rows, cols.join(", "))
}

pub fn render_full(schema: &Schema) -> String {
    schema.tables.iter().map(render_table).collect::<Vec<_>>().join("\n")
}

/// The schema text to put in the prompt for this question.
/// Returns the full schema when it fits, otherwise the most relevant tables
/// plus the tables they reference.
pub fn schema_for_prompt(schema: &Schema, full: &str, question: &str) -> String {
    if full.len() <= FULL_SCHEMA_CHAR_BUDGET {
        return full.to_string();
    }
    let words: HashSet<String> = tokenize(question)
        .into_iter()
        .flat_map(|w| [singular(&w), w])
        .collect();
    let mut scored: Vec<(usize, i64)> = schema
        .tables
        .iter()
        .enumerate()
        .map(|(i, t)| {
            let mut score = 0i64;
            for w in tokenize(&t.name) {
                if words.contains(&w) || words.contains(&singular(&w)) {
                    score += 10;
                }
            }
            for c in &t.columns {
                for w in tokenize(&c.name) {
                    if words.contains(&w) {
                        score += 2;
                    }
                }
            }
            (i, score)
        })
        .filter(|(_, s)| *s > 0)
        .collect();
    scored.sort_by(|a, b| b.1.cmp(&a.1));

    let mut picked: Vec<usize> = scored.iter().take(RELEVANT_TABLE_LIMIT).map(|(i, _)| *i).collect();
    // Pull in referenced tables so joins are possible.
    let by_name: HashMap<String, usize> = schema
        .tables
        .iter()
        .enumerate()
        .map(|(i, t)| (t.qualified_name(), i))
        .collect();
    let mut seen: HashSet<usize> = picked.iter().copied().collect();
    for i in picked.clone() {
        for f in &schema.tables[i].foreign_keys {
            if let Some(&j) = by_name.get(&f.ref_table) {
                if seen.insert(j) {
                    picked.push(j);
                }
            }
        }
    }
    if picked.is_empty() {
        // Nothing matched: send the table list without columns so the model can still orient.
        let names: Vec<String> = schema.tables.iter().map(|t| t.qualified_name()).collect();
        return format!("(schema too large to include in full; table names only)\n{}", names.join(", "));
    }
    picked.sort();
    let mut out = String::from("(only the tables most relevant to the question are listed)\n");
    out.push_str(
        &picked
            .iter()
            .map(|&i| render_table(&schema.tables[i]))
            .collect::<Vec<_>>()
            .join("\n"),
    );
    out
}

/// The schema text when the person picked tables for the request: those tables, the tables
/// they reference, and (a few of) the tables that reference them. `None` when none of the
/// names exist, so the caller falls back to the usual schema.
pub fn schema_with_focus(schema: &Schema, focus: &[String]) -> Option<String> {
    let wanted: HashSet<&str> = focus.iter().map(String::as_str).collect();
    let focused: Vec<usize> = (0..schema.tables.len()).filter(|&i| wanted.contains(schema.tables[i].qualified_name().as_str())).collect();
    if focused.is_empty() {
        return None;
    }
    let by_name: HashMap<String, usize> = schema.tables.iter().enumerate().map(|(i, t)| (t.qualified_name(), i)).collect();
    let mut related: Vec<usize> = Vec::new();
    let mut seen: HashSet<usize> = focused.iter().copied().collect();
    for &i in &focused {
        for f in &schema.tables[i].foreign_keys {
            if let Some(&j) = by_name.get(&f.ref_table) {
                if seen.insert(j) {
                    related.push(j);
                }
            }
        }
    }
    let focus_names: HashSet<String> = focused.iter().map(|&i| schema.tables[i].qualified_name()).collect();
    let referencing = (0..schema.tables.len())
        .filter(|j| !seen.contains(j))
        .filter(|&j| schema.tables[j].foreign_keys.iter().any(|f| focus_names.contains(&f.ref_table)))
        .take(REFERENCING_TABLE_LIMIT)
        .collect::<Vec<_>>();
    related.extend(referencing);

    let render = |ids: &[usize]| ids.iter().map(|&i| render_table(&schema.tables[i])).collect::<Vec<_>>().join("\n");
    let mut out = format!(
        "(The person selected these tables for this request: build the query on them.)\n{}",
        render(&focused)
    );
    if !related.is_empty() {
        out.push_str(&format!("\n\n(Related tables, for joins only when the request needs them.)\n{}", render(&related)));
    }
    Some(out)
}

fn tokenize(s: &str) -> Vec<String> {
    s.split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.len() > 1)
        .map(|w| w.to_lowercase())
        .collect()
}

fn singular(w: &str) -> String {
    w.strip_suffix('s').unwrap_or(w).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::Column;

    fn table(name: &str, fks: &[(&str, &str)]) -> Table {
        Table {
            schema: None,
            name: name.into(),
            kind: "table".into(),
            columns: vec![Column { name: "id".into(), data_type: "int".into(), nullable: false, primary_key: true, enum_values: None }],
            foreign_keys: fks
                .iter()
                .map(|(col, to)| ForeignKey { column: (*col).into(), ref_table: (*to).into(), ref_column: "id".into() })
                .collect(),
            row_estimate: None,
        }
    }

    #[test]
    fn focus_keeps_picked_tables_and_their_neighbours() {
        let schema = Schema {
            dialect: "SQLite".into(),
            tables: vec![
                table("person", &[]),
                table("app_user", &[("person_id", "person")]),
                table("login", &[("user_id", "app_user")]),
                table("invoice", &[]),
            ],
        };
        let text = schema_with_focus(&schema, &["app_user".into()]).unwrap();
        let (picked, related) = text.split_once("(Related tables").unwrap();
        assert!(picked.contains("app_user [table]") && !picked.contains("person [table]"), "{text}");
        // Referenced (person) and referencing (login) tables come along; unrelated ones don't.
        assert!(related.contains("person [table]") && related.contains("login [table]"), "{text}");
        assert!(!text.contains("invoice"), "{text}");
        assert!(schema_with_focus(&schema, &["nope".into()]).is_none());
        assert!(schema_with_focus(&schema, &[]).is_none());
    }
}
