//! Tests against real PostgreSQL, MySQL and SQL Server servers.
//!
//! Each test is skipped unless its URL is set, so `cargo test` passes without servers:
//!
//!   SAYQL_TEST_PG_URL=postgres://postgres:<password>@localhost:55416/sayql
//!   SAYQL_TEST_MYSQL_URL=mysql://root:<password>@localhost:33806
//!   SAYQL_TEST_MSSQL_URL=mssql://sa:<password>@localhost:14334/master
//!
//! (README.md shows how to start throwaway containers for these.)
//!
//! The Postgres test drops and recreates the schemas `sayql_it` and `sayql_it_inv`;
//! the MySQL and SQL Server tests drop and recreate the database `sayql_it`.
//! Nothing else is touched.

use super::*;
use crate::schema::Table;
use serde_json::json;

struct Target {
    host: String,
    port: Option<u16>,
    user: String,
    password: Option<String>,
    database: String,
}

/// Parses `scheme://user[:password]@host[:port][/database]`.
fn parse_url(url: &str) -> Target {
    let rest = url.split_once("://").map(|(_, r)| r).unwrap_or(url);
    let (auth, rest) = rest.rsplit_once('@').unwrap_or(("", rest));
    let (user, password) = match auth.split_once(':') {
        Some((u, p)) => (u.to_string(), Some(p.to_string())),
        None => (auth.to_string(), None),
    };
    let (hostport, database) = rest.split_once('/').unwrap_or((rest, ""));
    let (host, port) = match hostport.rsplit_once(':') {
        Some((h, p)) => (h.to_string(), Some(p.parse().expect("port"))),
        None => (hostport.to_string(), None),
    };
    let database = database.split('?').next().unwrap_or("").to_string();
    Target { host, port, user, password, database }
}

fn env_target(var: &str) -> Option<Target> {
    match std::env::var(var) {
        Ok(url) if !url.trim().is_empty() => Some(parse_url(url.trim())),
        _ => {
            eprintln!("skipping: set {var} to run this test against a real server");
            None
        }
    }
}

async fn open(kind: Kind, t: &Target, database: &str) -> DbPool {
    connect(&config(kind, t, database), t.password.clone()).await.expect("connect")
}

fn config(kind: Kind, t: &Target, database: &str) -> ConnConfig {
    ConnConfig {
        id: "it".into(),
        name: "it".into(),
        kind,
        host: t.host.clone(),
        port: t.port,
        database: database.into(),
        user: t.user.clone(),
        file: String::new(),
        ssl: false,
        // The SQL Server test container uses its auto-generated self-signed certificate.
        trust_cert: kind == Kind::Mssql,
    }
}

/// Runs a T-SQL batch outside read-only mode.
async fn mssql_batch(pool: &MssqlPool, sql: &str) -> Result<(), String> {
    let mut conn = pool.acquire().await.map_err(|e| e.to_string())?;
    let stream = conn.client().simple_query(sql).await.map_err(|e| e.to_string())?;
    stream.into_results().await.map(|_| ()).map_err(|e| e.to_string())
}

/// Runs a multi-statement setup script outside read-only mode.
async fn exec(pool: &DbPool, sql: &str) {
    let r = match pool {
        DbPool::Pg(p) => sqlx::raw_sql(sql).execute(p).await.map(|_| ()).map_err(|e| e.to_string()),
        DbPool::My(p) => sqlx::raw_sql(sql).execute(p).await.map(|_| ()).map_err(|e| e.to_string()),
        DbPool::Ms(p) => mssql_batch(p, sql).await,
        DbPool::Lite(_) => unreachable!("SQLite is covered by the unit tests"),
    };
    r.unwrap_or_else(|e| panic!("setup failed: {e}\n{sql}"));
}

async fn query(pool: &DbPool, sql: &str, read_only: bool) -> QueryResult {
    run_query(pool, sql, read_only).await.unwrap_or_else(|e| panic!("{sql}: {e}"))
}

async fn scalar(pool: &DbPool, sql: &str) -> Value {
    query(pool, sql, true).await.rows.remove(0).remove(0)
}

fn table<'a>(tables: &'a [Table], schema: Option<&str>, name: &str) -> &'a Table {
    tables
        .iter()
        .find(|t| t.schema.as_deref() == schema && t.name == name)
        .unwrap_or_else(|| panic!("table {schema:?}.{name} missing"))
}

fn pk_columns(t: &Table) -> Vec<&str> {
    t.columns.iter().filter(|c| c.primary_key).map(|c| c.name.as_str()).collect()
}

fn fks(t: &Table) -> Vec<(String, String, String)> {
    let mut v: Vec<_> =
        t.foreign_keys.iter().map(|f| (f.column.clone(), f.ref_table.clone(), f.ref_column.clone())).collect();
    v.sort();
    v
}

fn fk(c: &str, t: &str, rc: &str) -> (String, String, String) {
    (c.into(), t.into(), rc.into())
}

fn column<'a>(t: &'a Table, name: &str) -> &'a crate::schema::Column {
    t.columns.iter().find(|c| c.name == name).unwrap_or_else(|| panic!("column {}.{name} missing", t.name))
}

fn is_placeholder(v: &Value) -> bool {
    matches!(v, Value::String(s) if s.starts_with('<') && s.ends_with('>') && !s.contains("</"))
}

/// Compares one result row against `expected` (column name -> value) and reports every mismatch at once.
fn check_row(label: &str, r: &QueryResult, row: usize, expected: &Value) {
    let expected = expected.as_object().unwrap();
    let mut bad = Vec::new();
    for (i, name) in r.columns.iter().enumerate() {
        let got = &r.rows[row][i];
        match expected.get(name) {
            Some(want) if want != got => bad.push(format!("  {name}: got {got}, want {want}")),
            None => bad.push(format!("  {name}: got {got}, no expectation")),
            _ => {}
        }
    }
    for name in expected.keys() {
        if !r.columns.contains(name) {
            bad.push(format!("  {name}: column missing from result"));
        }
    }
    assert!(bad.is_empty(), "{label} row {row} mismatches:\n{}", bad.join("\n"));
}

/// Read-only mode must be enforced by the database, survive multi-statement tricks,
/// and leave pooled connections usable for normal writes afterwards.
async fn check_read_only(pool: &DbPool, count_sql: &str, writes: &[&str], show_ro: &str, approved_write: &str) {
    let before = scalar(pool, count_sql).await;
    for _ in 0..3 {
        for sql in writes {
            let e = run_query(pool, sql, true).await;
            assert!(e.is_err(), "read-only mode allowed: {sql}");
        }
    }
    assert_eq!(scalar(pool, count_sql).await, before, "a read-only statement changed data");
    // Run more queries than the pool has connections so every pooled connection is checked.
    for _ in 0..8 {
        let r = query(pool, show_ro, false).await;
        let v = r.rows[0].last().unwrap().clone();
        assert!(v == json!("off") || v == json!("OFF") || v == json!(0), "connection left read-only: {v}");
    }
    let w = query(pool, approved_write, false).await;
    assert_eq!(w.rows_affected, Some(1), "{approved_write}");
    assert!(w.columns.is_empty());
}

// ---------------------------------------------------------------- PostgreSQL

const PG_SEED: &str = r#"
DROP SCHEMA IF EXISTS sayql_it CASCADE;
DROP SCHEMA IF EXISTS sayql_it_inv CASCADE;
CREATE SCHEMA sayql_it;
CREATE SCHEMA sayql_it_inv;

CREATE TYPE sayql_it.order_status AS ENUM ('pending', 'paid', 'shipped', 'cancelled');
-- Same type name in another schema with different labels.
CREATE TYPE sayql_it_inv.order_status AS ENUM ('in_stock', 'backordered');
CREATE TYPE sayql_it.addr AS (city text, zip int);

CREATE TABLE sayql_it.customers (
  id serial PRIMARY KEY,
  email text NOT NULL UNIQUE,
  external_id uuid NOT NULL,
  tags text[] NOT NULL DEFAULT '{}',
  profile jsonb,
  signed_up_at timestamptz NOT NULL
);

CREATE TABLE sayql_it.orders (
  id bigserial PRIMARY KEY,
  customer_id int NOT NULL REFERENCES sayql_it.customers(id),
  status sayql_it.order_status NOT NULL,
  total numeric(12, 2) NOT NULL,
  ordered_at timestamptz NOT NULL
);

CREATE TABLE sayql_it_inv.warehouses (code text PRIMARY KEY, name text NOT NULL);

CREATE TABLE sayql_it_inv.stock (
  warehouse_code text NOT NULL REFERENCES sayql_it_inv.warehouses(code),
  product_id int NOT NULL,
  qty int NOT NULL,
  state sayql_it_inv.order_status NOT NULL,
  PRIMARY KEY (warehouse_code, product_id)
);

CREATE TABLE sayql_it_inv.movements (
  id int PRIMARY KEY,
  wh text NOT NULL,
  pid int NOT NULL,
  delta int NOT NULL,
  FOREIGN KEY (wh, pid) REFERENCES sayql_it_inv.stock (warehouse_code, product_id)
);

CREATE TABLE sayql_it.shipments (
  id int PRIMARY KEY,
  order_id bigint NOT NULL REFERENCES sayql_it.orders(id),
  warehouse_code text NOT NULL REFERENCES sayql_it_inv.warehouses(code)
);

CREATE VIEW sayql_it.paid_orders AS SELECT * FROM sayql_it.orders WHERE status = 'paid';

INSERT INTO sayql_it.customers (email, external_id, tags, profile, signed_up_at)
SELECT 'c' || i || '@example.com', gen_random_uuid(), ARRAY['t' || (i % 3)], jsonb_build_object('n', i),
       timestamptz '2024-01-01 00:00:00+00' + i * interval '1 day'
FROM generate_series(1, 200) i;

INSERT INTO sayql_it.orders (customer_id, status, total, ordered_at)
SELECT 1 + i % 200, (ARRAY['pending', 'paid', 'shipped', 'cancelled'])[1 + i % 4]::sayql_it.order_status,
       (i % 500) + 0.99, timestamptz '2024-01-01 00:00:00+00' + i * interval '1 hour'
FROM generate_series(1, 1000) i;

CREATE TABLE sayql_it.all_types (
  id int PRIMARY KEY,
  b bool, i2 smallint, i4 int, i8 bigint, f4 real, f8 double precision,
  n numeric(12, 2), n_big numeric, n_neg numeric, n_nan numeric,
  t text, vc varchar(20), c char(3), nm name, u uuid, j json, jb jsonb,
  ts timestamp(3), tstz timestamptz, ts_inf timestamptz, d date, tm time, tmtz timetz,
  iv interval, m money, by bytea, st sayql_it.order_status,
  ta text[], ia int[], ia_null int[], ia2 int[], ba bool[], na numeric[], ua uuid[],
  tsa timestamptz[], ea sayql_it.order_status[], ja jsonb[],
  ip inet, ip6 inet, cn cidr, mac macaddr, bits bit(4), vb varbit, x xml, o oid, ch "char",
  pt point, r int4range, dr daterange, comp sayql_it.addr
);

INSERT INTO sayql_it.all_types VALUES (
  1, true, -2, 40000, 9000000000, 1.1, 2.5,
  1234.5, 123456789012345678901234567890.123456789, -0.00012, 'NaN',
  'hello', 'world', 'ab', 'pg_name', 'a0eebc99-9c0b-4ef8-bb6d-6bb9bd380a11', '{"a": 1}', '{"b": [1, 2]}',
  '2024-03-01 12:34:56.789', '2024-03-01 12:34:56+00', 'infinity', '2024-03-01', '12:34:56', '12:34:56+02',
  '1 year 2 mons 3 days 04:05:06.5', 12.34, '\xdeadbeef', 'paid',
  '{a,b}', '{1,2}', '{1,NULL}', '{{1,2},{3,4}}', '{t,f}', '{1.50,2}', '{a0eebc99-9c0b-4ef8-bb6d-6bb9bd380a11}',
  '{"2024-03-01 12:34:56+00"}', '{paid,shipped}', ARRAY['{"k": 1}'::jsonb],
  '192.168.0.1', '::1', '10.0.0.0/8', '08:00:2b:01:02:03', B'1010', B'101', '<a>1</a>', 42, 'a',
  '(1,2.5)', '[1,10)', '[2024-01-01,2024-02-01)', ROW('Paris', 75001)
);
INSERT INTO sayql_it.all_types (id) VALUES (2);
"#;

fn pg_expected() -> Value {
    serde_json::from_str(
        r#"{
        "id": 1, "b": true, "i2": -2, "i4": 40000, "i8": 9000000000, "f4": 1.1, "f8": 2.5,
        "n": "1234.50", "n_big": "123456789012345678901234567890.123456789", "n_neg": "-0.00012", "n_nan": "NaN",
        "t": "hello", "vc": "world", "c": "ab ", "nm": "pg_name", "u": "a0eebc99-9c0b-4ef8-bb6d-6bb9bd380a11",
        "j": {"a": 1}, "jb": {"b": [1, 2]},
        "ts": "2024-03-01 12:34:56.789", "tstz": "2024-03-01 12:34:56 UTC", "ts_inf": "infinity",
        "d": "2024-03-01", "tm": "12:34:56", "tmtz": "12:34:56+02",
        "iv": "1 year 2 mons 3 days 04:05:06.5", "m": "12.34", "by": "0xdeadbeef", "st": "paid",
        "ta": ["a", "b"], "ia": [1, 2], "ia_null": [1, null], "ia2": [[1, 2], [3, 4]], "ba": [true, false],
        "na": ["1.50", "2"], "ua": ["a0eebc99-9c0b-4ef8-bb6d-6bb9bd380a11"], "tsa": ["2024-03-01 12:34:56 UTC"],
        "ea": ["paid", "shipped"], "ja": [{"k": 1}],
        "ip": "192.168.0.1", "ip6": "::1", "cn": "10.0.0.0/8", "mac": "08:00:2b:01:02:03",
        "bits": "1010", "vb": "101", "x": "<a>1</a>", "o": 42, "ch": "a",
        "pt": "(1,2.5)", "r": "[1,10)", "dr": "[2024-01-01,2024-02-01)", "comp": {"city": "Paris", "zip": 75001}
    }"#,
    )
    .unwrap()
}

#[tokio::test]
async fn postgres_integration() {
    let Some(t) = env_target("SAYQL_TEST_PG_URL") else { return };
    let db = if t.database.is_empty() { "postgres".to_string() } else { t.database.clone() };
    let pool = open(Kind::Postgres, &t, &db).await;
    let list = list_databases(&pool).await.expect("list").expect("a server has databases");
    assert!(list.databases.contains(&db) && !list.databases.iter().any(|d| d.starts_with("template")), "{list:?}");
    assert_eq!(list.current.as_deref(), Some(db.as_str()));
    exec(&pool, PG_SEED).await;

    // ---- schema reading
    let schema = introspect(&pool, Kind::Postgres).await.expect("introspect");
    let ours: Vec<String> = schema
        .tables
        .iter()
        .filter(|t| t.schema.as_deref().is_some_and(|s| s.starts_with("sayql_it")))
        .map(|t| t.qualified_name())
        .collect();
    assert_eq!(
        ours,
        [
            "sayql_it.all_types",
            "sayql_it.customers",
            "sayql_it.orders",
            "sayql_it.paid_orders",
            "sayql_it.shipments",
            "sayql_it_inv.movements",
            "sayql_it_inv.stock",
            "sayql_it_inv.warehouses",
        ]
    );
    let tb = &schema.tables;
    let customers = table(tb, Some("sayql_it"), "customers");
    let orders = table(tb, Some("sayql_it"), "orders");
    let stock = table(tb, Some("sayql_it_inv"), "stock");
    let view = table(tb, Some("sayql_it"), "paid_orders");
    assert_eq!(customers.kind, "table");
    assert_eq!(view.kind, "view");
    assert_eq!(pk_columns(customers), ["id"]);
    assert_eq!(pk_columns(stock), ["warehouse_code", "product_id"]);
    assert!(pk_columns(view).is_empty());
    assert_eq!(fks(orders), [fk("customer_id", "sayql_it.customers", "id")]);
    assert_eq!(
        fks(table(tb, Some("sayql_it"), "shipments")),
        [fk("order_id", "sayql_it.orders", "id"), fk("warehouse_code", "sayql_it_inv.warehouses", "code")]
    );
    assert_eq!(
        fks(table(tb, Some("sayql_it_inv"), "movements")),
        [fk("pid", "sayql_it_inv.stock", "product_id"), fk("wh", "sayql_it_inv.stock", "warehouse_code")]
    );
    let status = column(orders, "status");
    // Non-public types are schema-qualified, as they must be written in a cast.
    assert_eq!(status.data_type, "sayql_it.order_status");
    assert_eq!(status.enum_values.as_deref(), Some(&["pending", "paid", "shipped", "cancelled"].map(String::from)[..]));
    assert_eq!(
        column(stock, "state").enum_values.as_deref(),
        Some(&["in_stock", "backordered"].map(String::from)[..]),
        "enum labels must come from the column's own schema"
    );
    assert_eq!(column(customers, "tags").data_type, "_text");
    assert_eq!(column(orders, "total").data_type, "numeric");
    assert!(!column(orders, "total").nullable);
    assert!(column(customers, "profile").nullable);
    // Never-analyzed tables have no statistics yet: report "unknown", not a negative count.
    for t in tb.iter().filter(|t| t.schema.as_deref().is_some_and(|s| s.starts_with("sayql_it"))) {
        assert!(t.row_estimate.is_none_or(|n| n >= 0), "{}: row estimate {:?}", t.name, t.row_estimate);
    }
    exec(&pool, "ANALYZE sayql_it.customers; ANALYZE sayql_it.orders;").await;
    let schema = introspect(&pool, Kind::Postgres).await.expect("introspect");
    assert_eq!(table(&schema.tables, Some("sayql_it"), "customers").row_estimate, Some(200));
    assert_eq!(table(&schema.tables, Some("sayql_it"), "orders").row_estimate, Some(1000));
    assert_eq!(table(&schema.tables, Some("sayql_it"), "paid_orders").row_estimate, None);

    // ---- value decoding
    let r = query(&pool, "SELECT * FROM sayql_it.all_types ORDER BY id", true).await;
    check_row("pg read-only", &r, 0, &pg_expected());
    assert!(r.rows[1][1..].iter().all(Value::is_null), "{:?}", r.rows[1]);
    assert!(!r.rows[0].iter().any(is_placeholder));
    // Approved writes that return rows decode the same way.
    let w = query(&pool, "UPDATE sayql_it.all_types SET i4 = i4 WHERE id = 1 RETURNING *", false).await;
    check_row("pg RETURNING", &w, 0, &pg_expected());
    let r = query(&pool, "SELECT status, count(*) AS n, sum(total) AS s FROM sayql_it.orders GROUP BY 1 ORDER BY 1", true).await;
    assert_eq!(r.columns, ["status", "n", "s"]);
    assert_eq!(r.rows.iter().map(|row| row[1].as_i64().unwrap()).sum::<i64>(), 1000);
    assert_eq!(r.rows[0][0], json!("pending"));

    // ---- other read-only statement kinds still work
    assert_eq!(scalar(&pool, "SHOW transaction_read_only").await, json!("on"));
    assert_eq!(scalar(&pool, "VALUES (1)").await, json!(1));
    assert_eq!(query(&pool, "TABLE sayql_it_inv.warehouses", true).await.columns, ["code", "name"]);
    assert!(!query(&pool, "EXPLAIN SELECT * FROM sayql_it.orders", true).await.rows.is_empty());
    assert_eq!(
        scalar(&pool, "WITH x AS (SELECT 41 AS v) SELECT v + 1 FROM x").await,
        json!(42)
    );

    // ---- empty results keep their headers
    for ro in [true, false] {
        let r = query(&pool, "SELECT id, email FROM sayql_it.customers WHERE false", ro).await;
        assert_eq!(r.columns, ["id", "email"]);
        assert!(r.rows.is_empty());
        assert_eq!(r.rows_affected, None);
    }

    // ---- read-only enforcement
    check_read_only(
        &pool,
        "SELECT count(*) FROM sayql_it.orders WHERE status = 'paid'",
        &[
            "UPDATE sayql_it.orders SET status = 'paid'",
            "DELETE FROM sayql_it.orders",
            "WITH d AS (DELETE FROM sayql_it.orders RETURNING id) SELECT count(*) FROM d",
            "SELECT 1; COMMIT; UPDATE sayql_it.orders SET status = 'paid'",
            "SELECT 1; ROLLBACK; BEGIN; UPDATE sayql_it.orders SET status = 'paid'; COMMIT",
            "SELECT nextval('sayql_it.orders_id_seq')",
        ],
        "SHOW transaction_read_only",
        "UPDATE sayql_it.orders SET status = 'paid' WHERE id = 1",
    )
    .await;

    exec(&pool, "DROP SCHEMA sayql_it CASCADE; DROP SCHEMA sayql_it_inv CASCADE;").await;
}

// ---------------------------------------------------------------- MySQL

const MY_SEED: &str = r#"
DROP DATABASE IF EXISTS sayql_it;
CREATE DATABASE sayql_it;
USE sayql_it;

CREATE TABLE customers (
  id INT UNSIGNED AUTO_INCREMENT PRIMARY KEY,
  email VARCHAR(200) NOT NULL UNIQUE,
  tier ENUM('free', 'pro', 'enterprise') NOT NULL DEFAULT 'free',
  signed_up_at DATETIME NOT NULL
);

CREATE TABLE orders (
  id BIGINT UNSIGNED AUTO_INCREMENT PRIMARY KEY,
  customer_id INT UNSIGNED NOT NULL,
  status ENUM('pending', 'paid', 'shipped', 'it''s late') NOT NULL,
  total DECIMAL(12, 2) NOT NULL,
  meta JSON NULL,
  ordered_at TIMESTAMP NOT NULL,
  FOREIGN KEY (customer_id) REFERENCES customers (id)
);

CREATE TABLE order_items (
  order_id BIGINT UNSIGNED NOT NULL,
  line SMALLINT UNSIGNED NOT NULL,
  qty INT NOT NULL,
  PRIMARY KEY (order_id, line),
  FOREIGN KEY (order_id) REFERENCES orders (id)
);

CREATE VIEW paid_orders AS SELECT * FROM orders WHERE status = 'paid';

INSERT INTO customers (email, tier, signed_up_at)
WITH RECURSIVE n (i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM n WHERE i < 200)
SELECT CONCAT('c', i, '@example.com'), ELT(1 + i % 3, 'free', 'pro', 'enterprise'), '2024-01-01' + INTERVAL i DAY FROM n;

INSERT INTO orders (customer_id, status, total, meta, ordered_at)
WITH RECURSIVE n (i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM n WHERE i < 1000)
SELECT 1 + i % 200, ELT(1 + i % 4, 'pending', 'paid', 'shipped', 'it''s late'), (i % 500) + 0.99,
       JSON_OBJECT('n', i), '2024-01-01' + INTERVAL i HOUR FROM n;

CREATE TABLE all_types (
  id INT PRIMARY KEY,
  b BOOL, ti TINYINT, tiu TINYINT UNSIGNED, si SMALLINT, siu SMALLINT UNSIGNED, mi MEDIUMINT,
  i INT, iu INT UNSIGNED, bi BIGINT, biu BIGINT UNSIGNED, f FLOAT, d DOUBLE,
  dc DECIMAL(12, 2), dc_big DECIMAL(65, 30),
  dt DATETIME, dt6 DATETIME(6), ts TIMESTAMP NULL, da DATE, tm TIME, tm_neg TIME, tm_big TIME, yr YEAR,
  ch CHAR(3), vc VARCHAR(20), tx TEXT, js JSON, en ENUM('a', 'b'), st SET('x', 'y', 'z'),
  bin BINARY(4), vb VARBINARY(8), bl BLOB, bt BIT(4)
);

INSERT INTO all_types VALUES (
  1, TRUE, -5, 250, -300, 60000, -8000000,
  -2000000000, 4000000000, -9000000000, 18446744073709551615, 1.1, 2.5,
  1234.5, 12345678901234567890.123456789,
  '2024-03-01 12:34:56', '2024-03-01 12:34:56.123456', '2024-03-01 12:34:56', '2024-03-01', '12:34:56', '-01:30:00', '100:00:00', 2024,
  'ab', 'world', 'hello', '{"a": 1}', 'b', 'x,z',
  'ab', x'DEADBEEF', 'blob text', b'1010'
);
INSERT INTO all_types (id) VALUES (2);
"#;

fn my_expected() -> Value {
    serde_json::from_str(
        r#"{
        "id": 1, "b": true, "ti": -5, "tiu": 250, "si": -300, "siu": 60000, "mi": -8000000,
        "i": -2000000000, "iu": 4000000000, "bi": -9000000000, "biu": 18446744073709551615,
        "f": 1.1, "d": 2.5, "dc": "1234.50", "dc_big": "12345678901234567890.123456789000000000000000000000",
        "dt": "2024-03-01 12:34:56", "dt6": "2024-03-01 12:34:56.123456", "ts": "2024-03-01 12:34:56 UTC",
        "da": "2024-03-01", "tm": "12:34:56", "tm_neg": "-01:30:00", "tm_big": "100:00:00", "yr": 2024,
        "ch": "ab", "vc": "world", "tx": "hello", "js": {"a": 1}, "en": "b", "st": "x,z",
        "bin": "0x61620000", "vb": "0xdeadbeef", "bl": "blob text", "bt": 10
    }"#,
    )
    .unwrap()
}

#[tokio::test]
async fn mysql_integration() {
    let Some(t) = env_target("SAYQL_TEST_MYSQL_URL") else { return };
    let admin = open(Kind::Mysql, &t, &t.database).await;
    exec(&admin, MY_SEED).await;
    let pool = open(Kind::Mysql, &t, "sayql_it").await;
    let list = list_databases(&pool).await.expect("list").expect("a server has databases");
    assert!(list.databases.contains(&"sayql_it".to_string()) && !list.databases.contains(&"mysql".to_string()), "{list:?}");
    assert_eq!(list.current.as_deref(), Some("sayql_it"));

    // ---- schema reading
    let schema = introspect(&pool, Kind::Mysql).await.expect("introspect");
    let names: Vec<&str> = schema.tables.iter().map(|t| t.name.as_str()).collect();
    assert_eq!(names, ["all_types", "customers", "order_items", "orders", "paid_orders"]);
    let tb = &schema.tables;
    let customers = table(tb, None, "customers");
    let orders = table(tb, None, "orders");
    let view = table(tb, None, "paid_orders");
    assert_eq!(view.kind, "view");
    assert_eq!(customers.kind, "table");
    assert_eq!(pk_columns(customers), ["id"]);
    assert_eq!(pk_columns(table(tb, None, "order_items")), ["order_id", "line"]);
    assert!(pk_columns(view).is_empty());
    assert_eq!(fks(orders), [fk("customer_id", "customers", "id")]);
    assert_eq!(fks(table(tb, None, "order_items")), [fk("order_id", "orders", "id")]);
    let status = column(orders, "status");
    assert_eq!(status.data_type, "enum");
    assert_eq!(status.enum_values.as_deref(), Some(&["pending", "paid", "shipped", "it's late"].map(String::from)[..]));
    assert_eq!(column(customers, "tier").enum_values.as_deref(), Some(&["free", "pro", "enterprise"].map(String::from)[..]));
    assert_eq!(column(orders, "total").data_type, "decimal(12,2)");
    assert_eq!(column(customers, "id").data_type, "int unsigned");
    assert!(column(orders, "meta").nullable);
    assert!(!column(orders, "total").nullable);
    exec(&pool, "ANALYZE TABLE customers, orders").await;
    let schema = introspect(&pool, Kind::Mysql).await.expect("introspect");
    // InnoDB row counts are estimates; they should be in the right ballpark.
    let est = |name: &str| table(&schema.tables, None, name).row_estimate;
    assert!(est("customers").is_some_and(|n| (150..=250).contains(&n)), "customers ~200: {:?}", est("customers"));
    assert!(est("orders").is_some_and(|n| (800..=1200).contains(&n)), "orders ~1000: {:?}", est("orders"));
    assert_eq!(est("paid_orders"), None);

    // ---- value decoding, inside and outside the read-only transaction
    let r = query(&pool, "SELECT * FROM all_types ORDER BY id", true).await;
    check_row("mysql read-only", &r, 0, &my_expected());
    assert!(r.rows[1][1..].iter().all(Value::is_null), "{:?}", r.rows[1]);
    assert!(!r.rows[0].iter().any(is_placeholder));
    let w = query(&pool, "SELECT * FROM all_types ORDER BY id", false).await;
    check_row("mysql read-write", &w, 0, &my_expected());
    let r = query(&pool, "SELECT status, COUNT(*) AS n, SUM(total) AS s FROM orders GROUP BY status ORDER BY status", true).await;
    assert_eq!(r.columns, ["status", "n", "s"]);
    assert_eq!(r.rows.iter().map(|row| row[1].as_i64().unwrap()).sum::<i64>(), 1000);
    assert_eq!(r.rows[0][0], json!("pending"));
    assert_eq!(r.rows[0][2], json!("62247.50"));

    // ---- other read-only statement kinds still work
    assert!(!query(&pool, "SHOW TABLES", true).await.rows.is_empty());
    assert!(!query(&pool, "DESCRIBE orders", true).await.rows.is_empty());
    assert!(!query(&pool, "EXPLAIN SELECT * FROM orders", true).await.rows.is_empty());
    assert_eq!(scalar(&pool, "WITH x AS (SELECT 41 AS v) SELECT v + 1 FROM x").await, json!(42));

    // ---- empty results keep their headers
    for ro in [true, false] {
        let r = query(&pool, "SELECT id, email FROM customers WHERE 1 = 0", ro).await;
        assert_eq!(r.columns, ["id", "email"]);
        assert!(r.rows.is_empty());
        assert_eq!(r.rows_affected, None);
    }

    // ---- read-only enforcement
    check_read_only(
        &pool,
        "SELECT COUNT(*) FROM orders WHERE status = 'paid'",
        &[
            "UPDATE orders SET status = 'paid'",
            "DELETE FROM order_items",
            "SELECT 1; COMMIT; UPDATE orders SET status = 'paid'",
            "SELECT 1; ROLLBACK; UPDATE orders SET status = 'paid'",
        ],
        "SELECT @@transaction_read_only",
        "UPDATE orders SET status = 'paid' WHERE id = 1",
    )
    .await;

    exec(&admin, "DROP DATABASE sayql_it").await;
}

const MS_SEED: &[&str] = &[
    "CREATE SCHEMA sales",
    "CREATE TABLE dbo.customers (id int IDENTITY PRIMARY KEY, name nvarchar(100) NOT NULL, email varchar(200) NULL)",
    "CREATE TABLE sales.orders (id int PRIMARY KEY, customer_id int NOT NULL REFERENCES dbo.customers(id),
       total decimal(12,2) NOT NULL, status varchar(20) NOT NULL)",
    "CREATE VIEW sales.paid_orders AS SELECT id, total FROM sales.orders WHERE status = 'paid'",
    "CREATE TABLE dbo.all_types (id int PRIMARY KEY, t_tiny tinyint, t_small smallint, t_big bigint, t_bit bit,
       t_real real, t_float float, t_dec decimal(38,10), t_money money, t_nvarchar nvarchar(max), t_char char(3),
       t_guid uniqueidentifier, t_bin varbinary(8), t_date date, t_time time(3), t_dt datetime, t_dt2 datetime2(7),
       t_sdt smalldatetime, t_dto datetimeoffset(0), t_xml xml)",
    "INSERT INTO dbo.customers (name, email) VALUES (N'Zoë', 'zoe@example.com'), (N'李雷', NULL), (N'O''Brien', 'ob@example.com')",
    "INSERT INTO sales.orders VALUES (1, 1, 10.50, 'paid'), (2, 1, 99.99, 'pending'), (3, 2, 0.01, 'paid'),
       (4, 3, 1234567890.12, 'shipped'), (5, 3, 5.00, 'paid')",
    "INSERT INTO dbo.all_types VALUES (1, 255, -32768, 9007199254740993, 1, 1.1, 2.5,
       12345678901234567890.0123456789, 12.34, N'héllo 世界', 'ab', '6F9619FF-8B86-D011-B42D-00C04FC964FF',
       0xDEADBEEF, '2026-10-21', '13:45:30.123', '2026-10-21T13:45:30.997', '2026-10-21T13:45:30.1234567',
       '2026-10-21T13:45:29', '2026-10-21T13:45:30+05:30', '<a>1</a>'),
       (2, NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL)",
];

#[tokio::test]
async fn mssql_integration() {
    let Some(t) = env_target("SAYQL_TEST_MSSQL_URL") else { return };
    let admin = open(Kind::Mssql, &t, &t.database).await;
    exec(&admin, "IF DB_ID('sayql_it') IS NOT NULL BEGIN ALTER DATABASE sayql_it SET SINGLE_USER WITH ROLLBACK IMMEDIATE; DROP DATABASE sayql_it; END").await;
    exec(&admin, "CREATE DATABASE sayql_it").await;
    let pool = open(Kind::Mssql, &t, "sayql_it").await;
    for sql in MS_SEED {
        exec(&pool, sql).await;
    }

    // ---- browsing databases: list them from the default connection, then switch with USE
    let list = list_databases(&admin).await.expect("list").expect("a server has databases");
    assert!(list.databases.contains(&"sayql_it".to_string()) && list.databases.contains(&"master".to_string()), "{list:?}");
    assert_eq!(list.current.as_deref(), Some(t.database.as_str()));
    let switched = open_database(&admin, &config(Kind::Mssql, &t, &t.database), t.password.clone(), "sayql_it").await.expect("open sayql_it");
    assert_eq!(scalar(&switched, "SELECT DB_NAME()").await, json!("sayql_it"));
    assert_eq!(introspect(&switched, Kind::Mssql).await.expect("introspect").tables.len(), 4);
    drop(switched);

    // ---- schema reading
    let schema = introspect(&pool, Kind::Mssql).await.expect("introspect");
    assert_eq!(schema.dialect, "SQL Server");
    let tb = &schema.tables;
    let names: Vec<String> = tb.iter().map(|t| t.qualified_name()).collect();
    assert_eq!(names, ["dbo.all_types", "dbo.customers", "sales.orders", "sales.paid_orders"]);
    let orders = table(tb, Some("sales"), "orders");
    assert_eq!(pk_columns(orders), ["id"]);
    assert_eq!(fks(orders), [fk("customer_id", "dbo.customers", "id")]);
    assert_eq!(column(orders, "total").data_type, "decimal(12,2)");
    assert_eq!(column(table(tb, Some("dbo"), "customers"), "name").data_type, "nvarchar(100)");
    assert_eq!(column(table(tb, Some("dbo"), "all_types"), "t_nvarchar").data_type, "nvarchar(max)");
    assert!(column(table(tb, Some("dbo"), "customers"), "email").nullable);
    assert!(!column(orders, "status").nullable);
    assert_eq!(table(tb, Some("sales"), "paid_orders").kind, "view");
    assert_eq!(orders.row_estimate, Some(5));
    assert_eq!(table(tb, Some("sales"), "paid_orders").row_estimate, None);

    // ---- value decoding
    let r = query(&pool, "SELECT * FROM dbo.all_types ORDER BY id", true).await;
    check_row(
        "mssql",
        &r,
        0,
        &json!({
            "id": 1, "t_tiny": 255, "t_small": -32768, "t_big": 9007199254740993_i64, "t_bit": true,
            "t_real": 1.1, "t_float": 2.5, "t_dec": "12345678901234567890.0123456789", "t_money": 12.34,
            "t_nvarchar": "héllo 世界", "t_char": "ab ", "t_guid": "6f9619ff-8b86-d011-b42d-00c04fc964ff",
            "t_bin": "0xdeadbeef", "t_date": "2026-10-21", "t_time": "13:45:30.123",
            "t_dt": "2026-10-21 13:45:30.997", "t_dt2": "2026-10-21 13:45:30.123456700",
            "t_sdt": "2026-10-21 13:45:00", "t_dto": "2026-10-21 13:45:30 +05:30", "t_xml": "<a>1</a>",
        }),
    );
    assert!(r.rows[1][1..].iter().all(Value::is_null), "{:?}", r.rows[1]);
    let r = query(&pool, "SELECT status, COUNT(*) AS n, SUM(total) AS s FROM sales.orders GROUP BY status ORDER BY status", true).await;
    assert_eq!(r.columns, ["status", "n", "s"]);
    assert_eq!(r.rows[0], [json!("paid"), json!(3), json!("15.51")]);
    let r = query(&pool, "SELECT id, name FROM dbo.customers WHERE 1 = 0", true).await;
    assert_eq!(r.columns, ["id", "name"]);
    assert!(r.rows.is_empty());

    // ---- read-only mode: never changes data, however the statement is phrased
    let count = || scalar(&pool, "SELECT COUNT(*) FROM sales.orders");
    let before = count().await;
    for sql in [
        "DELETE FROM sales.orders",
        "SELECT 1; DELETE FROM sales.orders",
        "SELECT 1; COMMIT; DELETE FROM sales.orders",
        "SELECT * INTO dbo.copied FROM sales.orders",
        "WITH x AS (SELECT 1 AS a) DELETE FROM sales.orders",
    ] {
        assert!(run_query(&pool, sql, true).await.is_err(), "read-only mode allowed: {sql}");
    }
    assert_eq!(count().await, before, "a read-only statement changed data");
    // 1 is the read-only wrapper's own transaction; a leaked one from earlier would make it 2.
    assert_eq!(scalar(&pool, "SELECT @@TRANCOUNT").await, json!(1), "transaction left open");

    // ---- row cap: cut off at 5000 rows, and the connection still works afterwards
    let big = query(&pool, "SELECT a.object_id FROM sys.all_objects a CROSS JOIN sys.all_objects b", true).await;
    assert!(big.truncated);
    assert_eq!(big.rows.len(), MAX_ROWS);
    assert_eq!(scalar(&pool, "SELECT 40 + 2").await, json!(42));

    // ---- an approved write
    let w = query(&pool, "UPDATE sales.orders SET status = 'paid' WHERE status = 'pending'", false).await;
    assert_eq!(w.rows_affected, Some(1));

    drop(pool);
    exec(&admin, "ALTER DATABASE sayql_it SET SINGLE_USER WITH ROLLBACK IMMEDIATE; DROP DATABASE sayql_it").await;
}

#[tokio::test]
async fn mssql_connection_errors() {
    let Some(t) = env_target("SAYQL_TEST_MSSQL_URL") else { return };

    // ---- TLS: the login is always encrypted, so an untrusted certificate fails with the hint
    let untrusted = ConnConfig { trust_cert: false, ..config(Kind::Mssql, &t, &t.database) };
    let err = connect(&untrusted, t.password.clone()).await.err().expect("untrusted certificate accepted").to_string();
    assert!(err.contains("Trust server certificate"), "{err}");

    // ---- an encrypted connection works end to end once the certificate is trusted
    let encrypted = ConnConfig { ssl: true, ..config(Kind::Mssql, &t, &t.database) };
    let pool = connect(&encrypted, t.password.clone()).await.expect("connect with encryption");
    assert_eq!(scalar(&pool, "SELECT 40 + 2").await, json!(42));

    // ---- a wrong password gets the credentials hint
    let err = connect(&config(Kind::Mssql, &t, &t.database), Some("wrong".into())).await.err().expect("wrong password accepted");
    assert!(err.to_string().contains("didn't accept this user and password"), "{err}");
}
