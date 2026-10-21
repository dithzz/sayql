//! End-to-end tests against the bundled demo SQLite database.

use super::*;

#[tokio::test]
async fn demo_database_end_to_end() {
    let dir = std::env::temp_dir().join(format!("askdb-test-{}", uuid::Uuid::new_v4()));
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
    };
    let pool = connect(&cfg, None).await.expect("connect");

    let schema = introspect(&pool, Kind::Sqlite).await.expect("introspect");
    let names: Vec<&str> = schema.tables.iter().map(|t| t.name.as_str()).collect();
    assert_eq!(names, ["categories", "customers", "order_items", "orders", "products", "reviews"]);
    let items = schema.tables.iter().find(|t| t.name == "order_items").unwrap();
    assert_eq!(items.row_estimate, Some(7500));
    assert!(items.foreign_keys.iter().any(|f| f.column == "order_id" && f.ref_table == "orders"));
    let text = crate::schema::render_full(&schema);
    assert!(text.contains("order_id integer NOT NULL -> orders.id"), "{text}");

    let r = run_query(&pool, "SELECT status, COUNT(*) AS n FROM orders GROUP BY status ORDER BY n DESC;", true)
        .await
        .expect("query");
    assert_eq!(r.columns, ["status", "n"]);
    assert_eq!(r.rows.iter().map(|row| row[1].as_i64().unwrap()).sum::<i64>(), 3000);

    // Empty result still reports column headers.
    let r = run_query(&pool, "SELECT id, email FROM customers WHERE 1 = 0", true).await.unwrap();
    assert_eq!(r.columns, ["id", "email"]);

    // Read-only mode is enforced by the database, not just the keyword check,
    // and a script cannot switch it off part-way through.
    assert!(run_query(&pool, "DELETE FROM reviews", true).await.is_err());
    assert!(run_query(&pool, "SELECT 1; PRAGMA query_only = OFF; DELETE FROM reviews", true).await.is_err());
    let r = run_query(&pool, "SELECT COUNT(*) FROM reviews", true).await.unwrap();
    assert_eq!(r.rows[0][0].as_i64(), Some(1200));

    // The pooled connection is back to normal for an approved write afterwards.
    let w = run_query(&pool, "UPDATE orders SET status = 'paid' WHERE id = 1", false).await.unwrap();
    assert_eq!(w.rows_affected, Some(1));

    let big = run_query(&pool, "SELECT a.id FROM order_items a, categories b", true).await.unwrap();
    assert!(big.truncated);
    assert_eq!(big.rows.len(), MAX_ROWS);

    pool_close(pool).await;
    let _ = std::fs::remove_dir_all(dir);
}

async fn pool_close(p: DbPool) {
    if let DbPool::Lite(p) = p {
        p.rw.close().await;
        p.ro.close().await;
    }
}
