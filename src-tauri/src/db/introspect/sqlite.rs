use crate::schema::{Column, ForeignKey, Table};
use sqlx::SqlitePool;

pub(super) async fn introspect(p: &SqlitePool) -> Result<Vec<Table>, sqlx::Error> {
    let objects: Vec<(String, String)> = sqlx::query_as(
        "SELECT name, type FROM sqlite_master WHERE type IN ('table', 'view') AND name NOT LIKE 'sqlite_%' ORDER BY name",
    )
    .fetch_all(p)
    .await?;
    let mut tables = Vec::new();
    for (name, kind) in objects {
        let info: Vec<(String, String, bool, i64)> =
            sqlx::query_as("SELECT name, type, \"notnull\", pk FROM pragma_table_info(?)")
                .bind(&name)
                .fetch_all(p)
                .await?;
        let fks: Vec<(String, String, Option<String>)> =
            sqlx::query_as("SELECT \"from\", \"table\", \"to\" FROM pragma_foreign_key_list(?)")
                .bind(&name)
                .fetch_all(p)
                .await?;
        let row_estimate = if kind == "table" {
            let q = format!("SELECT COUNT(*) FROM \"{}\"", name.replace('"', "\"\""));
            sqlx::query_scalar::<_, i64>(sqlx::AssertSqlSafe(q)).fetch_one(p).await.ok()
        } else {
            None
        };
        tables.push(Table {
            schema: None,
            name,
            kind,
            columns: info
                .into_iter()
                .map(|(n, ty, notnull, pk)| Column {
                    name: n,
                    data_type: if ty.is_empty() { "any".into() } else { ty.to_lowercase() },
                    nullable: !notnull,
                    primary_key: pk > 0,
                    enum_values: None,
                })
                .collect(),
            foreign_keys: fks
                .into_iter()
                .map(|(from, table, to)| ForeignKey {
                    column: from,
                    ref_table: table,
                    ref_column: to.unwrap_or_else(|| "rowid".into()),
                })
                .collect(),
            row_estimate,
        });
    }
    Ok(tables)
}
