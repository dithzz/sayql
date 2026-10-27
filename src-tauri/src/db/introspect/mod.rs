//! Reads tables, columns, keys, enum values and row estimates into a [`Schema`].

mod mssql;
mod mysql;
mod postgres;
mod sqlite;

use super::{DbPool, Kind};
use crate::error::AppResult;
use crate::schema::{Column, ForeignKey, Schema, Table};
use std::collections::{HashMap, HashSet};

pub async fn introspect(pool: &DbPool, kind: Kind) -> AppResult<Schema> {
    let tables = match pool {
        DbPool::Pg(p) => postgres::introspect(p).await?,
        DbPool::My(p) => mysql::introspect(p).await?,
        DbPool::Lite(p) => sqlite::introspect(&p.ro).await?,
        DbPool::Ms(p) => mssql::introspect(p).await?,
        DbPool::Team(t) => return t.schema().await,
    };
    Ok(Schema { dialect: kind.dialect().to_string(), tables })
}

/// (schema, table)
type Key = (String, String);

fn assemble(
    cols: Vec<(String, String, String, String, bool, String)>,
    pks: Vec<(String, String, String)>,
    fks: Vec<(String, String, String, String, String, String)>,
    enums: HashMap<String, Vec<String>>,
    rows: HashMap<Key, i64>,
    qualify_refs: bool,
) -> Vec<Table> {
    let pk: HashSet<(String, String, String)> = pks.into_iter().collect();
    let mut fk_map: HashMap<Key, Vec<ForeignKey>> = HashMap::new();
    for (s, t, c, rs, rt, rc) in fks {
        let ref_table = if qualify_refs && rs != "public" { format!("{rs}.{rt}") } else { rt };
        fk_map.entry((s, t)).or_default().push(ForeignKey { column: c, ref_table, ref_column: rc });
    }
    let mut order: Vec<Key> = Vec::new();
    let mut map: HashMap<Key, Table> = HashMap::new();
    for (s, t, c, ty, nullable, kind) in cols {
        let key = (s.clone(), t.clone());
        let table = map.entry(key.clone()).or_insert_with(|| {
            order.push(key.clone());
            Table {
                schema: Some(s.clone()),
                name: t.clone(),
                kind: if kind.contains("MATERIALIZED") {
                    "materialized view".into()
                } else if kind.contains("VIEW") {
                    "view".into()
                } else {
                    "table".into()
                },
                columns: vec![],
                foreign_keys: fk_map.remove(&key).unwrap_or_default(),
                row_estimate: rows.get(&key).copied(),
            }
        });
        let enum_values = enums.get(&ty).cloned().or_else(|| parse_mysql_enum(&ty));
        table.columns.push(Column {
            primary_key: pk.contains(&(s, t, c.clone())),
            name: c,
            data_type: if enum_values.is_some() && ty.starts_with("enum(") { "enum".into() } else { ty },
            nullable,
            enum_values,
        });
    }
    order.into_iter().filter_map(|k| map.remove(&k)).collect()
}

fn parse_mysql_enum(ty: &str) -> Option<Vec<String>> {
    let inner = ty.strip_prefix("enum(")?.strip_suffix(')')?;
    Some(
        inner
            .split("','")
            .map(|v| v.trim_matches('\'').replace("''", "'"))
            .collect(),
    )
}
