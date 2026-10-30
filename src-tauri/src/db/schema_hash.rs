//! A cheap fingerprint of a database's schema, in one read-only query, so a cached schema
//! can be confirmed current without reading it all again. It changes when a table, column,
//! column type, enum value or key is added, removed or changed.

use super::DbPool;
use std::time::Duration;

const TIMEOUT: Duration = Duration::from_secs(5);

const PG_SQL: &str = "SELECT count(*)::text || ':' || coalesce(sum(hashtext(c.table_schema || '.' || c.table_name || '.' || c.column_name || ':' || c.data_type || ':' || c.udt_name || ':' || c.is_nullable)), 0)::text || ':' || (SELECT count(*)::text || ':' || coalesce(sum(hashtext(t.typname || '.' || e.enumlabel)), 0)::text FROM pg_enum e JOIN pg_type t ON t.oid = e.enumtypid) || ':' || (SELECT count(*)::text FROM pg_constraint WHERE contype IN ('p', 'f')) || ':' || (SELECT count(*)::text FROM pg_class WHERE relkind IN ('r', 'v', 'm', 'p')) FROM information_schema.columns c WHERE c.table_schema NOT IN ('pg_catalog', 'information_schema')";

const MYSQL_SQL: &str = "SELECT CONCAT(COUNT(*), ':', COALESCE(SUM(CRC32(CONCAT_WS('.', TABLE_NAME, COLUMN_NAME, COLUMN_TYPE, IS_NULLABLE, COLUMN_KEY))), 0), ':', (SELECT COUNT(*) FROM information_schema.KEY_COLUMN_USAGE k WHERE k.TABLE_SCHEMA = DATABASE() AND k.REFERENCED_TABLE_NAME IS NOT NULL)) FROM information_schema.COLUMNS WHERE TABLE_SCHEMA = DATABASE()";

const MSSQL_SQL: &str = "SELECT CONCAT(COUNT_BIG(*), ':', COALESCE(SUM(CAST(CHECKSUM(TABLE_SCHEMA, TABLE_NAME, COLUMN_NAME, DATA_TYPE, IS_NULLABLE, CHARACTER_MAXIMUM_LENGTH) AS bigint)), 0), ':', (SELECT COUNT_BIG(*) FROM sys.foreign_keys)) FROM INFORMATION_SCHEMA.COLUMNS";

/// SQLite counts every change to its schema.
const SQLITE_SQL: &str = "SELECT schema_version FROM pragma_schema_version";

/// The pool's schema fingerprint, or `None` when it can't be had (a team connection, or the
/// query failed): then the caller reads the whole schema to be sure.
pub async fn read(pool: &DbPool) -> Option<String> {
    let sql = match pool {
        DbPool::Pg(_) => PG_SQL,
        DbPool::My(_) => MYSQL_SQL,
        DbPool::Ms(_) => MSSQL_SQL,
        DbPool::Lite(_) => SQLITE_SQL,
        DbPool::Team(_) => return None,
    };
    let r = crate::readonly::run_limited(pool, sql, 1, TIMEOUT)
        .await
        .ok()?;
    let v = r.rows.first()?.first()?;
    Some(match v {
        serde_json::Value::String(s) => s.clone(),
        other => other.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{Kind, connect, run_query};

    #[tokio::test]
    async fn sqlite_fingerprint_moves_with_the_schema() {
        let path = std::env::temp_dir().join(format!("sayql-hash-{}.sqlite", uuid::Uuid::new_v4()));
        let pool = crate::db::tenant_fixture::sqlite(&path).await;
        let before = read(&pool).await.unwrap();
        assert_eq!(
            read(&pool).await.unwrap(),
            before,
            "stable when nothing changed"
        );
        run_query(&pool, "ALTER TABLE tenants ADD COLUMN region TEXT", false)
            .await
            .unwrap();
        assert_ne!(read(&pool).await.unwrap(), before);
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn fingerprints_on_real_servers() {
        // Runs against the servers in SAYQL_TEST_*_URL when set (see db/integration_tests.rs).
        for (var, kind) in [
            ("SAYQL_TEST_PG_URL", Kind::Postgres),
            ("SAYQL_TEST_MYSQL_URL", Kind::Mysql),
            ("SAYQL_TEST_MSSQL_URL", Kind::Mssql),
        ] {
            let Some((cfg, password)) = crate::db::env_config(var, kind) else {
                continue;
            };
            let pool = connect(&cfg, password).await.unwrap();
            let a = read(&pool)
                .await
                .unwrap_or_else(|| panic!("{kind:?} gave no fingerprint"));
            assert_eq!(read(&pool).await.unwrap(), a, "{kind:?} stable");
        }
    }
}
