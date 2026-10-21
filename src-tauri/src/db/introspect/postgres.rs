use super::assemble;
use crate::schema::Table;
use sqlx::PgPool;
use std::collections::HashMap;

pub(super) async fn introspect(p: &PgPool) -> Result<Vec<Table>, sqlx::Error> {
    let cols: Vec<(String, String, String, String, bool, String)> = sqlx::query_as(
        r#"SELECT c.table_schema::text, c.table_name::text, c.column_name::text,
                  CASE WHEN c.data_type = 'USER-DEFINED' AND c.udt_schema NOT IN ('public', 'pg_catalog')
                       THEN c.udt_schema::text || '.' || c.udt_name::text
                       WHEN c.data_type IN ('USER-DEFINED', 'ARRAY') THEN c.udt_name::text
                       ELSE c.data_type::text END,
                  c.is_nullable = 'YES', t.table_type::text
           FROM information_schema.columns c
           JOIN information_schema.tables t ON t.table_schema = c.table_schema AND t.table_name = c.table_name
           WHERE c.table_schema NOT IN ('pg_catalog', 'information_schema') AND c.table_schema NOT LIKE 'pg_toast%'
           ORDER BY c.table_schema, c.table_name, c.ordinal_position"#,
    )
    .fetch_all(p)
    .await?;
    let pks: Vec<(String, String, String)> = sqlx::query_as(
        r#"SELECT kcu.table_schema::text, kcu.table_name::text, kcu.column_name::text
           FROM information_schema.table_constraints tc
           JOIN information_schema.key_column_usage kcu
             ON tc.constraint_name = kcu.constraint_name AND tc.table_schema = kcu.table_schema AND tc.table_name = kcu.table_name
           WHERE tc.constraint_type = 'PRIMARY KEY'"#,
    )
    .fetch_all(p)
    .await?;
    let fks: Vec<(String, String, String, String, String, String)> = sqlx::query_as(
        r#"SELECT ns.nspname::text, cl.relname::text, a.attname::text, fns.nspname::text, fcl.relname::text, fa.attname::text
           FROM pg_constraint con
           JOIN pg_class cl ON cl.oid = con.conrelid
           JOIN pg_namespace ns ON ns.oid = cl.relnamespace
           JOIN pg_class fcl ON fcl.oid = con.confrelid
           JOIN pg_namespace fns ON fns.oid = fcl.relnamespace
           CROSS JOIN LATERAL unnest(con.conkey, con.confkey) AS k(col, fcol)
           JOIN pg_attribute a ON a.attrelid = con.conrelid AND a.attnum = k.col
           JOIN pg_attribute fa ON fa.attrelid = con.confrelid AND fa.attnum = k.fcol
           WHERE con.contype = 'f'"#,
    )
    .fetch_all(p)
    .await?;
    let enum_rows: Vec<(String, String)> = sqlx::query_as(
        // Keyed like the column types above, so same-named enums in different schemas stay apart.
        r#"SELECT CASE WHEN n.nspname IN ('public', 'pg_catalog') THEN t.typname::text
                       ELSE n.nspname::text || '.' || t.typname::text END,
                  e.enumlabel::text
           FROM pg_type t
           JOIN pg_namespace n ON n.oid = t.typnamespace
           JOIN pg_enum e ON e.enumtypid = t.oid ORDER BY t.oid, e.enumsortorder"#,
    )
    .fetch_all(p)
    .await?;
    let mut enums: HashMap<String, Vec<String>> = HashMap::new();
    for (t, v) in enum_rows {
        enums.entry(t).or_default().push(v);
    }
    let est: Vec<(String, String, i64)> = sqlx::query_as(
        r#"SELECT n.nspname::text, c.relname::text, c.reltuples::bigint FROM pg_class c
           JOIN pg_namespace n ON n.oid = c.relnamespace WHERE c.relkind IN ('r', 'p')"#,
    )
    .fetch_all(p)
    .await?;
    // reltuples is -1 until the table is first vacuumed or analyzed: unknown, not empty.
    let rows = est.into_iter().filter(|(_, _, n)| *n >= 0).map(|(s, t, n)| ((s, t), n)).collect();
    Ok(assemble(cols, pks, fks, enums, rows, true))
}
