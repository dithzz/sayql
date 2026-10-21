/// Keywords that can change data, structure, permissions, session or transaction state.
/// `INTO` covers `SELECT ... INTO new_table` and `INSERT INTO`; `USE` and `SET` change
/// the session (SQL Server's `USE` would move a pooled connection to another database).
const WRITE_KEYWORDS: &[&str] = &[
    "INSERT", "UPDATE", "DELETE", "MERGE", "UPSERT", "INTO", "DROP", "ALTER", "CREATE", "TRUNCATE", "RENAME",
    "GRANT", "REVOKE", "DENY", "EXEC", "EXECUTE", "CALL", "COMMIT", "ROLLBACK", "SAVEPOINT", "BEGIN", "BACKUP",
    "RESTORE", "DBCC", "KILL", "SHUTDOWN", "RECONFIGURE", "BULK", "OPENROWSET", "OPENQUERY", "ATTACH", "DETACH",
    "VACUUM", "REINDEX", "COPY", "LOCK", "USE", "SET", "CHECKPOINT", "RECEIVE",
];

/// True when the statement only reads data: it starts with a reading keyword and
/// contains no write keyword outside strings, quoted identifiers and comments.
///
/// The database enforces this too, where it can: read-only transactions on Postgres and
/// MySQL, a read-only connection on SQLite, and an always-rolled-back transaction on
/// SQL Server (which has no read-only mode, so there this check matters most).
pub fn is_read_only(sql: &str) -> bool {
    let s = strip_leading_comments(sql);
    let first = s
        .split(|c: char| c.is_whitespace() || c == '(' || c == ';')
        .find(|w| !w.is_empty())
        .unwrap_or("")
        .to_ascii_uppercase();
    let reads = match first.as_str() {
        "SELECT" | "WITH" | "EXPLAIN" | "SHOW" | "DESCRIBE" | "DESC" | "VALUES" | "TABLE" => true,
        // `PRAGMA x = y` changes settings; `PRAGMA x` / `PRAGMA x(y)` reads them.
        "PRAGMA" => !s.contains('='),
        _ => false,
    };
    reads && !keywords(sql).any(|w| WRITE_KEYWORDS.iter().any(|k| w.eq_ignore_ascii_case(k)))
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

/// Bare words of the statement, skipping string literals, quoted identifiers
/// (`"x"`, `[x]`, `` `x` ``) and comments.
fn keywords(sql: &str) -> impl Iterator<Item = &str> {
    let bytes = sql.as_bytes();
    let mut i = 0;
    // Index just past the next `end` at or after `from`, or the end of the text.
    let skip_to = |from: usize, end: &str| sql[from..].find(end).map_or(sql.len(), |p| from + p + end.len());
    std::iter::from_fn(move || {
        while i < bytes.len() {
            match bytes[i] {
                b'\'' => i = skip_to(i + 1, "'"), // '' escapes re-enter as an empty string
                b'"' => i = skip_to(i + 1, "\""),
                b'`' => i = skip_to(i + 1, "`"),
                b'[' => i = skip_to(i + 1, "]"),
                b'-' if bytes.get(i + 1) == Some(&b'-') => i = skip_to(i, "\n"),
                b'/' if bytes.get(i + 1) == Some(&b'*') => i = skip_to(i + 2, "*/"),
                c if c.is_ascii_alphabetic() || c == b'_' => {
                    let start = i;
                    while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                        i += 1;
                    }
                    return Some(&sql[start..i]);
                }
                _ => i += 1,
            }
        }
        None
    })
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

    #[test]
    fn write_keywords_anywhere_need_confirmation() {
        assert!(!is_read_only("SELECT 1; DELETE FROM orders"));
        assert!(!is_read_only("SELECT * INTO backup_orders FROM orders"));
        assert!(!is_read_only("WITH gone AS (DELETE FROM t RETURNING *) SELECT * FROM gone"));
        assert!(!is_read_only("SELECT 1\nEXEC sp_configure"));
        assert!(!is_read_only("select 1; commit; drop table x"));
        assert!(!is_read_only("SELECT 1; USE master"));
        assert!(!is_read_only("SELECT 1\nSET ROLE admin"));
    }

    #[test]
    fn an_explanation_commented_out_before_the_query_is_fine() {
        assert!(is_read_only("-- The user's email is on the Person table, so join those.\n\nSELECT TOP 100 email FROM dbo.Person"));
        // Prose that isn't commented out is not SQL: it must not count as read-only.
        assert!(!is_read_only("The email is on Person.\nSELECT email FROM dbo.Person"));
    }

    #[test]
    fn write_words_inside_strings_identifiers_and_comments_are_fine() {
        assert!(is_read_only("SELECT * FROM audit WHERE action = 'DELETE'"));
        assert!(is_read_only("SELECT [Update], \"insert\", `drop` FROM t"));
        assert!(is_read_only("SELECT updated_at, created_by FROM t -- delete later\n/* drop */"));
        assert!(is_read_only("SELECT REPLACE(name, 'a', 'b') FROM t WHERE note = 'it''s DELETE'"));
    }
}
