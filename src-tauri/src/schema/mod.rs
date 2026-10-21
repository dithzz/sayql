//! The schema model sent to the UI and, rendered compactly, to the AI.

mod render;

pub use render::{render_full, render_table, schema_for_prompt, schema_with_focus};

use serde::Serialize;

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
