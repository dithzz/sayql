use super::assemble;
use crate::schema::Table;
use sqlx::MySqlPool;
use std::collections::HashMap;

pub(super) async fn introspect(p: &MySqlPool) -> Result<Vec<Table>, sqlx::Error> {
    // CAST AS CHAR: some information_schema columns come back as binary strings on MySQL 8.
    let cols: Vec<(String, String, String, String, bool, String)> = sqlx::query_as(
        r#"SELECT CAST(c.TABLE_SCHEMA AS CHAR), CAST(c.TABLE_NAME AS CHAR), CAST(c.COLUMN_NAME AS CHAR),
                  CAST(c.COLUMN_TYPE AS CHAR), c.IS_NULLABLE = 'YES', CAST(t.TABLE_TYPE AS CHAR)
           FROM information_schema.COLUMNS c
           JOIN information_schema.TABLES t ON t.TABLE_SCHEMA = c.TABLE_SCHEMA AND t.TABLE_NAME = c.TABLE_NAME
           WHERE c.TABLE_SCHEMA = DATABASE()
           ORDER BY c.TABLE_NAME, c.ORDINAL_POSITION"#,
    )
    .fetch_all(p)
    .await?;
    let pks: Vec<(String, String, String)> = sqlx::query_as(
        r#"SELECT CAST(TABLE_SCHEMA AS CHAR), CAST(TABLE_NAME AS CHAR), CAST(COLUMN_NAME AS CHAR)
           FROM information_schema.KEY_COLUMN_USAGE
           WHERE TABLE_SCHEMA = DATABASE() AND CONSTRAINT_NAME = 'PRIMARY'"#,
    )
    .fetch_all(p)
    .await?;
    let fks: Vec<(String, String, String, String, String, String)> = sqlx::query_as(
        r#"SELECT CAST(TABLE_SCHEMA AS CHAR), CAST(TABLE_NAME AS CHAR), CAST(COLUMN_NAME AS CHAR),
                  CAST(REFERENCED_TABLE_SCHEMA AS CHAR), CAST(REFERENCED_TABLE_NAME AS CHAR), CAST(REFERENCED_COLUMN_NAME AS CHAR)
           FROM information_schema.KEY_COLUMN_USAGE
           WHERE TABLE_SCHEMA = DATABASE() AND REFERENCED_TABLE_NAME IS NOT NULL"#,
    )
    .fetch_all(p)
    .await?;
    let est: Vec<(String, String, Option<i64>)> = sqlx::query_as(
        r#"SELECT CAST(TABLE_SCHEMA AS CHAR), CAST(TABLE_NAME AS CHAR), CAST(TABLE_ROWS AS SIGNED)
           FROM information_schema.TABLES WHERE TABLE_SCHEMA = DATABASE()"#,
    )
    .fetch_all(p)
    .await?;
    let rows = est
        .into_iter()
        .filter_map(|(s, t, n)| n.map(|n| ((s, t), n)))
        .collect();
    let mut tables = assemble(cols, pks, fks, HashMap::new(), rows, false);
    // Everything is in one database; drop the qualifier so names stay short.
    for t in &mut tables {
        t.schema = None;
    }
    Ok(tables)
}
