use serde::Serialize;
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, Serialize)]
pub struct Schema {
    pub dialect: String,
    pub tables: Vec<Table>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Table {
    /// Postgres schema / MySQL database. `None` for SQLite.
    pub schema: Option<String>,
    pub name: String,
    /// "table" or "view"
    pub kind: String,
    pub columns: Vec<Column>,
    pub foreign_keys: Vec<ForeignKey>,
    pub row_estimate: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Column {
    pub name: String,
    pub data_type: String,
    pub nullable: bool,
    pub primary_key: bool,
    /// Allowed values for enum-typed columns, when the database exposes them.
    pub enum_values: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ForeignKey {
    pub column: String,
    /// Qualified like the referencing table (e.g. `public.customers`).
    pub ref_table: String,
    pub ref_column: String,
}

impl Table {
    pub fn qualified_name(&self) -> String {
        match &self.schema {
            Some(s) if s != "public" => format!("{s}.{}", self.name),
            _ => self.name.clone(),
        }
    }
}

/// Above this size we send only the tables relevant to the question instead of the
/// whole schema. Below it, sending everything keeps the prompt prefix identical across
/// keystrokes so it stays in the prompt cache, which is what keeps generation fast.
const FULL_SCHEMA_CHAR_BUDGET: usize = 300_000;
const RELEVANT_TABLE_LIMIT: usize = 30;

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

fn tokenize(s: &str) -> Vec<String> {
    s.split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.len() > 1)
        .map(|w| w.to_lowercase())
        .collect()
}

fn singular(w: &str) -> String {
    w.strip_suffix('s').unwrap_or(w).to_string()
}
