//! End-to-end tests against the bundled demo SQLite database.

use super::*;

/// Connections saved before the production tag existed load untagged, and the tag round-trips.
#[test]
fn production_tag_defaults_off_and_round_trips() {
    let old: ConnConfig =
        serde_json::from_str(r#"{"id":"a","name":"a","kind":"postgres","host":"h"}"#).unwrap();
    assert!(!old.production);
    let tagged: ConnConfig = serde_json::from_str(
        r#"{"id":"a","name":"a","kind":"postgres","host":"h","production":true}"#,
    )
    .unwrap();
    assert!(tagged.production);
    assert_eq!(
        serde_json::to_value(&tagged).unwrap()["production"],
        serde_json::json!(true)
    );
}

#[tokio::test]
async fn demo_database_end_to_end() {
    let dir = std::env::temp_dir().join(format!("sayql-test-{}", uuid::Uuid::new_v4()));
    let path = dir.join("demo.sqlite");
    ensure_demo(&path).await.expect("seed");
    let cfg = ConnConfig {
        id: "t".into(),
        name: "t".into(),
        kind: Kind::Sqlite,
        host: String::new(),
        port: None,
        database: String::new(),
        user: String::new(),
        file: path.to_string_lossy().into_owned(),
        ssl: false,
        trust_cert: false,
        production: false,
    };
    let pool = connect(&cfg, None).await.expect("connect");

    let schema = introspect(&pool, Kind::Sqlite).await.expect("introspect");
    let names: Vec<&str> = schema.tables.iter().map(|t| t.name.as_str()).collect();
    assert_eq!(
        names,
        [
            "categories",
            "customers",
            "order_items",
            "orders",
            "products",
            "reviews"
        ]
    );
    let items = schema
        .tables
        .iter()
        .find(|t| t.name == "order_items")
        .unwrap();
    assert_eq!(items.row_estimate, Some(7500));
    assert!(
        items
            .foreign_keys
            .iter()
            .any(|f| f.column == "order_id" && f.ref_table == "orders")
    );
    let text = crate::schema::render_full(&schema);
    assert!(
        text.contains("order_id integer NOT NULL -> orders.id"),
        "{text}"
    );

    let r = run_query(
        &pool,
        "SELECT status, COUNT(*) AS n FROM orders GROUP BY status ORDER BY n DESC;",
        true,
    )
    .await
    .expect("query");
    assert_eq!(r.columns, ["status", "n"]);
    assert_eq!(
        r.rows
            .iter()
            .map(|row| row[1].as_i64().unwrap())
            .sum::<i64>(),
        3000
    );

    // Empty result still reports column headers.
    let r = run_query(&pool, "SELECT id, email FROM customers WHERE 1 = 0", true)
        .await
        .unwrap();
    assert_eq!(r.columns, ["id", "email"]);

    // Read-only mode is enforced by the database, not just the keyword check,
    // and a script cannot switch it off part-way through.
    assert!(run_query(&pool, "DELETE FROM reviews", true).await.is_err());
    assert!(
        run_query(
            &pool,
            "SELECT 1; PRAGMA query_only = OFF; DELETE FROM reviews",
            true
        )
        .await
        .is_err()
    );
    let r = run_query(&pool, "SELECT COUNT(*) FROM reviews", true)
        .await
        .unwrap();
    assert_eq!(r.rows[0][0].as_i64(), Some(1200));

    // The pooled connection is back to normal for an approved write afterwards.
    let w = run_query(
        &pool,
        "UPDATE orders SET status = 'paid' WHERE id = 1",
        false,
    )
    .await
    .unwrap();
    assert_eq!(w.rows_affected, Some(1));

    let big = run_query(&pool, "SELECT a.id FROM order_items a, categories b", true)
        .await
        .unwrap();
    assert!(big.truncated);
    assert_eq!(big.rows.len(), MAX_ROWS);

    pool_close(pool).await;
    let _ = std::fs::remove_dir_all(dir);
}

/// SQLite runs in-process; a cancelled query is interrupted mid-statement, and each script
/// shows its first result set only.
#[tokio::test]
async fn sqlite_queries_cancel_and_show_the_first_result() {
    let dir = std::env::temp_dir().join(format!("sayql-test-{}", uuid::Uuid::new_v4()));
    let path = dir.join("demo.sqlite");
    ensure_demo(&path).await.expect("seed");
    let cfg = ConnConfig {
        id: "t".into(),
        name: "t".into(),
        kind: Kind::Sqlite,
        host: String::new(),
        port: None,
        database: String::new(),
        user: String::new(),
        file: path.to_string_lossy().into_owned(),
        ssl: false,
        trust_cert: false,
        production: false,
    };
    let pool = connect(&cfg, None).await.expect("connect");

    let r = run_query(&pool, "SELECT 1 AS a; SELECT 'x' AS b, 2 AS c", true)
        .await
        .unwrap();
    assert_eq!(
        (r.columns.clone(), r.rows.clone()),
        (vec!["a".to_string()], vec![vec![serde_json::json!(1)]])
    );

    let endless =
        "WITH RECURSIVE r(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM r) SELECT count(*) FROM r";
    for read_only in [true, false] {
        let cancel = tokio_util::sync::CancellationToken::new();
        let timer = tokio::spawn({
            let cancel = cancel.clone();
            async move {
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                cancel.cancel();
            }
        });
        let started = std::time::Instant::now();
        let r = run_query_with(&pool, endless, read_only, MAX_ROWS, &cancel).await;
        timer.await.unwrap();
        assert_eq!(r.err().map(|e| e.to_string()).as_deref(), Some(CANCELLED));
        assert!(
            started.elapsed() < std::time::Duration::from_secs(5),
            "took {:?}",
            started.elapsed()
        );
    }
    let r = run_query(&pool, "SELECT COUNT(*) FROM reviews", true)
        .await
        .unwrap();
    assert_eq!(r.rows[0][0].as_i64(), Some(1200));

    pool_close(pool).await;
    let _ = std::fs::remove_dir_all(dir);
}

/// A 500-table SQLite file reads quickly, with row counts.
#[tokio::test]
async fn sqlite_large_schema() {
    let dir = std::env::temp_dir().join(format!("sayql-test-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("big.sqlite");
    let setup = sqlx::SqlitePool::connect_with(
        sqlx::sqlite::SqliteConnectOptions::new()
            .filename(&path)
            .create_if_missing(true),
    )
    .await
    .unwrap();
    let ddl: String = (0..500)
        .map(|i| {
            let cols: Vec<String> = (0..10).map(|c| format!("c{c} TEXT")).collect();
            format!("CREATE TABLE t{i} (id INTEGER PRIMARY KEY, {}, ref INT REFERENCES t0(id)); INSERT INTO t{i} (id) VALUES (1), (2);", cols.join(", "))
        })
        .collect();
    sqlx::raw_sql(sqlx::AssertSqlSafe(ddl))
        .execute(&setup)
        .await
        .unwrap();
    setup.close().await;
    let cfg = ConnConfig {
        id: "t".into(),
        name: "t".into(),
        kind: Kind::Sqlite,
        host: String::new(),
        port: None,
        database: String::new(),
        user: String::new(),
        file: path.to_string_lossy().into_owned(),
        ssl: false,
        trust_cert: false,
        production: false,
    };
    let pool = connect(&cfg, None).await.expect("connect");
    let started = std::time::Instant::now();
    let schema = introspect(&pool, Kind::Sqlite).await.expect("introspect");
    assert!(
        started.elapsed() < std::time::Duration::from_secs(5),
        "took {:?}",
        started.elapsed()
    );
    assert_eq!(schema.tables.len(), 500);
    assert!(
        schema
            .tables
            .iter()
            .all(|t| t.columns.len() == 12 && t.row_estimate == Some(2))
    );
    pool_close(pool).await;
    let _ = std::fs::remove_dir_all(dir);
}

async fn pool_close(p: DbPool) {
    if let DbPool::Lite(p) = p {
        p.rw.close().await;
        p.ro.close().await;
    }
}
