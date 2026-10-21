/// True when the statement only reads data, judged by its first keyword.
/// Read-only statements additionally run inside a read-only transaction (a read-only
/// connection for SQLite), so the database itself rejects anything that slips past
/// this check. Postgres and MySQL also refuse several statements in one prepared
/// statement, so `SELECT 1; COMMIT; DELETE ...` cannot end that transaction early.
pub fn is_read_only(sql: &str) -> bool {
    let s = strip_leading_comments(sql);
    let first = s
        .split(|c: char| c.is_whitespace() || c == '(' || c == ';')
        .find(|w| !w.is_empty())
        .unwrap_or("")
        .to_ascii_uppercase();
    match first.as_str() {
        "SELECT" | "WITH" | "EXPLAIN" | "SHOW" | "DESCRIBE" | "DESC" | "VALUES" | "TABLE" => true,
        // `PRAGMA x = y` changes settings; `PRAGMA x` / `PRAGMA x(y)` reads them.
        "PRAGMA" => !s.contains('='),
        _ => false,
    }
}

fn strip_leading_comments(sql: &str) -> &str {
    let mut s = sql.trim_start();
    loop {
        if let Some(rest) = s.strip_prefix("--") {
            s = rest.split_once('\n').map(|(_, r)| r).unwrap_or("").trim_start();
        } else if let Some(rest) = s.strip_prefix("/*") {
            s = rest.split_once("*/").map(|(_, r)| r).unwrap_or("").trim_start();
        } else {
            return s;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_read_only_statements() {
        assert!(is_read_only("select 1"));
        assert!(is_read_only("  -- top customers\nSELECT * FROM customers"));
        assert!(is_read_only("/* c */ WITH x AS (SELECT 1) SELECT * FROM x"));
        assert!(is_read_only("(SELECT 1) UNION (SELECT 2)"));
        assert!(is_read_only("PRAGMA table_info(orders)"));
        assert!(!is_read_only("PRAGMA journal_mode = WAL"));
        assert!(!is_read_only("DELETE FROM orders"));
        assert!(!is_read_only("-- select\nUPDATE orders SET status = 'paid'"));
        assert!(!is_read_only("DROP TABLE x"));
    }
}
