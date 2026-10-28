use super::Kind;

/// Keywords that can change data, structure, permissions, session or transaction state.
/// `INTO` covers `SELECT ... INTO new_table` and `INSERT INTO`; `USE` and `SET` change
/// the session (SQL Server's `USE` would move a pooled connection to another database).
/// `OUTFILE`/`DUMPFILE` write files on MySQL; the function names below have side effects
/// that a read-only transaction doesn't stop (or that leave the database).
const WRITE_KEYWORDS: &[&str] = &[
    "INSERT",
    "UPDATE",
    "DELETE",
    "MERGE",
    "UPSERT",
    "INTO",
    "DROP",
    "ALTER",
    "CREATE",
    "TRUNCATE",
    "RENAME",
    "GRANT",
    "REVOKE",
    "DENY",
    "EXEC",
    "EXECUTE",
    "CALL",
    "COMMIT",
    "ROLLBACK",
    "SAVEPOINT",
    "BEGIN",
    "BACKUP",
    "RESTORE",
    "DBCC",
    "KILL",
    "SHUTDOWN",
    "RECONFIGURE",
    "BULK",
    "OPENROWSET",
    "OPENQUERY",
    "OPENDATASOURCE",
    "ATTACH",
    "DETACH",
    "VACUUM",
    "REINDEX",
    "COPY",
    "LOCK",
    "USE",
    "SET",
    "CHECKPOINT",
    "RECEIVE",
    "OUTFILE",
    "DUMPFILE",
    "LOAD_EXTENSION",
    "DBLINK",
    "DBLINK_EXEC",
    "LO_IMPORT",
    "LO_EXPORT",
    "LO_UNLINK",
    "NEXTVAL",
    "SETVAL",
    "PG_TERMINATE_BACKEND",
    "PG_CANCEL_BACKEND",
    "PG_RELOAD_CONF",
    "PG_ROTATE_LOGFILE",
    "PG_PROMOTE",
    "PG_SWITCH_WAL",
    // Effects that a read-only transaction allows and a rollback doesn't undo. Session-level
    // locks stay held by the pooled connection; replication slots keep WAL until the disk fills.
    "PG_ADVISORY_LOCK",
    "PG_ADVISORY_LOCK_SHARED",
    "PG_TRY_ADVISORY_LOCK",
    "PG_TRY_ADVISORY_LOCK_SHARED",
    "GET_LOCK",
    "PG_CREATE_RESTORE_POINT",
    "PG_BACKUP_START",
    "PG_BACKUP_STOP",
    "PG_START_BACKUP",
    "PG_STOP_BACKUP",
    "PG_WAL_REPLAY_PAUSE",
    "PG_WAL_REPLAY_RESUME",
    "PG_LOGICAL_EMIT_MESSAGE",
    "PG_FILE_WRITE",
    "PG_FILE_RENAME",
    "PG_FILE_UNLINK",
];

/// Function families with the same kind of lasting effect: `pg_stat_reset_shared`,
/// `pg_create_logical_replication_slot`, `pg_replication_origin_drop`, ...
const WRITE_PREFIXES: &[&str] = &[
    "PG_STAT_RESET",
    "PG_STAT_STATEMENTS_RESET",
    "PG_CREATE_PHYSICAL_REPLICATION_SLOT",
    "PG_CREATE_LOGICAL_REPLICATION_SLOT",
    "PG_COPY_PHYSICAL_REPLICATION_SLOT",
    "PG_COPY_LOGICAL_REPLICATION_SLOT",
    "PG_DROP_REPLICATION_SLOT",
    "PG_REPLICATION_SLOT_ADVANCE",
    "PG_REPLICATION_ORIGIN_",
];

/// True when the statement only reads data: it starts with a reading keyword and
/// contains no write keyword outside strings, quoted identifiers and comments, read the
/// way `kind`'s database reads them.
///
/// The database enforces this too, where it can: read-only transactions on Postgres and
/// MySQL, a read-only connection on SQLite, and an always-rolled-back transaction on
/// SQL Server (which has no read-only mode, so there this check matters most).
pub fn is_read_only(sql: &str, kind: Kind) -> bool {
    // Whether `\` escapes a quote in a plain string depends on server settings
    // (MySQL's NO_BACKSLASH_ESCAPES, Postgres' standard_conforming_strings), so the
    // statement must read as read-only both ways.
    let backslash: &[bool] = match kind {
        Kind::Mysql | Kind::Postgres => &[true, false],
        Kind::Sqlite | Kind::Mssql => &[false],
    };
    backslash
        .iter()
        .all(|&backslash| reads_only(sql, Lexer { kind, backslash }))
}

fn reads_only(sql: &str, lexer: Lexer) -> bool {
    let first = lexer
        .tokens(sql)
        .find(|t| !matches!(t, Tok::Sym(b'(' | b';')));
    let reads = match first {
        Some(Tok::Word(w)) => match w.to_ascii_uppercase().as_str() {
            "SELECT" | "WITH" | "EXPLAIN" | "SHOW" | "DESCRIBE" | "DESC" | "VALUES" | "TABLE" => {
                true
            }
            // `PRAGMA x = y` changes settings; `PRAGMA x` / `PRAGMA x(y)` reads them.
            "PRAGMA" => !sql.contains('='),
            _ => false,
        },
        _ => false,
    };
    if !reads {
        return false;
    }
    let words: Vec<&str> = lexer
        .tokens(sql)
        .filter_map(|t| if let Tok::Word(w) = t { Some(w) } else { None })
        .collect();
    let is = |w: &str, k: &str| w.eq_ignore_ascii_case(k);
    // SQL Server's `NEXT VALUE FOR seq` advances a sequence, and a rollback doesn't undo it.
    let next_value = words
        .windows(3)
        .any(|w| is(w[0], "NEXT") && is(w[1], "VALUE") && is(w[2], "FOR"));
    let writes = |i: usize, w: &str| {
        // MySQL's `SHOW CREATE TABLE t` only shows the statement.
        let shown = is(w, "CREATE") && i > 0 && is(words[i - 1], "SHOW");
        let upper = w.to_ascii_uppercase();
        !shown
            && (WRITE_KEYWORDS.contains(&upper.as_str())
                || WRITE_PREFIXES.iter().any(|p| upper.starts_with(p)))
    };
    !next_value && !words.iter().enumerate().any(|(i, w)| writes(i, w))
}

#[derive(Debug, PartialEq)]
enum Tok<'a> {
    /// A bare word: keyword, unquoted identifier or function name.
    Word(&'a str),
    /// Any other character outside strings and comments; a whole string or quoted
    /// identifier is one `Sym` of its opening quote.
    Sym(u8),
}

#[derive(Clone, Copy)]
struct Lexer {
    kind: Kind,
    /// Plain `'...'` strings (and MySQL `"..."` strings) treat `\` as an escape.
    backslash: bool,
}

impl Lexer {
    /// Tokens of the statement, skipping whitespace and comments.
    fn tokens(self, sql: &str) -> impl Iterator<Item = Tok<'_>> {
        let (mysql, postgres) = (self.kind == Kind::Mysql, self.kind == Kind::Postgres);
        let bytes = sql.as_bytes();
        let mut i = 0;
        // Index just past the next `end` at or after `from`, or the end of the text.
        let skip_to = |from: usize, end: &str| {
            sql[from..]
                .find(end)
                .map_or(sql.len(), |p| from + p + end.len())
        };
        // Index just past the closing `q` of a quoted run starting at `from`, where `\`
        // escapes the next byte and a doubled `q` is a literal one.
        let skip_escaped = |mut j: usize, q: u8| {
            while j < bytes.len() {
                match bytes[j] {
                    b'\\' => j += 2,
                    c if c == q && bytes.get(j + 1) == Some(&q) => j += 2,
                    c if c == q => return j + 1,
                    _ => j += 1,
                }
            }
            bytes.len()
        };
        // MySQL and Postgres allow `$` inside identifiers (`a$b`); elsewhere a `$` ends a word.
        let word_byte = move |c: u8| {
            c.is_ascii_alphanumeric() || c == b'_' || (c == b'$' && (mysql || postgres))
        };
        std::iter::from_fn(move || {
            while i < bytes.len() {
                let c = bytes[i];
                let next = bytes.get(i + 1).copied();
                let start = i;
                match c {
                    b'\'' => {
                        // Postgres E'...' strings always take backslash escapes.
                        let e_string = postgres
                            && i > 0
                            && bytes[i - 1].eq_ignore_ascii_case(&b'e')
                            && (i == 1 || !word_byte(bytes[i - 2]));
                        i = if self.backslash || e_string {
                            skip_escaped(i + 1, b'\'')
                        } else {
                            skip_to(i + 1, "'")
                        };
                        // '' escapes re-enter as an empty string in the non-escaped case.
                    }
                    // In MySQL `"` quotes a string (unless ANSI_QUOTES) and takes `\` escapes.
                    b'"' if mysql && self.backslash => i = skip_escaped(i + 1, b'"'),
                    b'"' => i = skip_to(i + 1, "\""),
                    // Backticks quote identifiers in MySQL and SQLite; elsewhere they aren't quotes.
                    b'`' if mysql || self.kind == Kind::Sqlite => i = skip_to(i + 1, "`"),
                    // Only SQL Server and SQLite quote identifiers with brackets; in
                    // Postgres `[` indexes arrays, and in MySQL it is not valid.
                    b'[' if !mysql && !postgres => i = skip_to(i + 1, "]"),
                    // Postgres dollar quoting: $$...$$ or $tag$...$tag$ (but not $1).
                    b'$' if postgres => {
                        let tag_end = bytes[i + 1..]
                            .iter()
                            .position(|&b| !(b.is_ascii_alphanumeric() || b == b'_'))
                            .map(|p| i + 1 + p);
                        match tag_end {
                            Some(t)
                                if bytes[t] == b'$'
                                    && !bytes.get(i + 1).is_some_and(u8::is_ascii_digit) =>
                            {
                                let tag = &sql[i..=t];
                                i = skip_to(t + 1, tag);
                            }
                            _ => i += 1,
                        }
                    }
                    // MySQL needs whitespace after `--` for a comment: `1--1` is arithmetic.
                    b'-' if next == Some(b'-')
                        && (!mysql
                            || bytes.get(i + 2).is_none_or(|b| {
                                b.is_ascii_whitespace() || b.is_ascii_control()
                            })) =>
                    {
                        i = skip_to(i, "\n");
                        continue;
                    }
                    b'#' if mysql => {
                        i = skip_to(i, "\n");
                        continue;
                    }
                    b'/' if next == Some(b'*') => {
                        // MySQL runs the inside of `/*! ... */` (and MariaDB `/*M! ... */`) as SQL.
                        let rest = &bytes[i + 2..];
                        let marker = if rest.starts_with(b"!") {
                            1
                        } else if rest.starts_with(b"M!") {
                            2
                        } else {
                            0
                        };
                        if mysql && marker > 0 {
                            i += 2 + marker;
                            while i < bytes.len() && bytes[i].is_ascii_digit() {
                                i += 1;
                            }
                        } else {
                            i = skip_to(i + 2, "*/");
                        }
                        continue;
                    }
                    c if c.is_ascii_alphabetic() || c == b'_' => {
                        while i < bytes.len() && word_byte(bytes[i]) {
                            i += 1;
                        }
                        return Some(Tok::Word(&sql[start..i]));
                    }
                    c if c.is_ascii_whitespace() => {
                        i += 1;
                        continue;
                    }
                    _ => i += 1,
                }
                return Some(Tok::Sym(c));
            }
            None
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: [Kind; 4] = [Kind::Postgres, Kind::Mysql, Kind::Sqlite, Kind::Mssql];

    fn ro_everywhere(sql: &str) -> bool {
        ALL.iter().all(|&k| is_read_only(sql, k))
    }

    fn rw_everywhere(sql: &str) -> bool {
        ALL.iter().all(|&k| !is_read_only(sql, k))
    }

    #[test]
    fn classifies_read_only_statements() {
        assert!(ro_everywhere("select 1"));
        assert!(ro_everywhere("  -- top customers\nSELECT * FROM customers"));
        assert!(ro_everywhere(
            "/* c */ WITH x AS (SELECT 1) SELECT * FROM x"
        ));
        assert!(ro_everywhere("(SELECT 1) UNION (SELECT 2)"));
        assert!(ro_everywhere("PRAGMA table_info(orders)"));
        assert!(rw_everywhere("PRAGMA journal_mode = WAL"));
        assert!(rw_everywhere("DELETE FROM orders"));
        assert!(rw_everywhere(
            "-- select\nUPDATE orders SET status = 'paid'"
        ));
        assert!(rw_everywhere("DROP TABLE x"));
    }

    #[test]
    fn write_keywords_anywhere_need_confirmation() {
        assert!(rw_everywhere("SELECT 1; DELETE FROM orders"));
        assert!(rw_everywhere("SELECT * INTO backup_orders FROM orders"));
        assert!(rw_everywhere(
            "WITH gone AS (DELETE FROM t RETURNING *) SELECT * FROM gone"
        ));
        assert!(rw_everywhere("SELECT 1\nEXEC sp_configure"));
        assert!(rw_everywhere("select 1; commit; drop table x"));
        assert!(rw_everywhere("SELECT 1; USE master"));
        assert!(rw_everywhere("SELECT 1\nSET ROLE admin"));
    }

    #[test]
    fn an_explanation_commented_out_before_the_query_is_fine() {
        assert!(ro_everywhere(
            "-- The user's email is on the Person table, so join those.\n\nSELECT TOP 100 email FROM dbo.Person"
        ));
        // Prose that isn't commented out is not SQL: it must not count as read-only.
        assert!(rw_everywhere(
            "The email is on Person.\nSELECT email FROM dbo.Person"
        ));
    }

    #[test]
    fn write_words_inside_strings_identifiers_and_comments_are_fine() {
        assert!(ro_everywhere("SELECT * FROM audit WHERE action = 'DELETE'"));
        assert!(is_read_only(
            "SELECT [Update], \"insert\", `drop` FROM t",
            Kind::Sqlite
        ));
        assert!(is_read_only(
            "SELECT [Update], \"insert\" FROM t",
            Kind::Mssql
        ));
        assert!(is_read_only(
            "SELECT `drop`, \"insert\" FROM t",
            Kind::Mysql
        ));
        assert!(is_read_only("SELECT \"insert\" FROM t", Kind::Postgres));
        assert!(ro_everywhere(
            "SELECT updated_at, created_by FROM t -- delete later\n/* drop */"
        ));
        assert!(ro_everywhere(
            "SELECT REPLACE(name, 'a', 'b') FROM t WHERE note = 'it''s DELETE'"
        ));
    }

    #[test]
    fn mysql_backslash_escapes_and_hash_comments_cannot_hide_writes() {
        // The audit's two bypasses.
        assert!(!is_read_only(
            r"SELECT '\'' INTO OUTFILE '/tmp/x' -- '",
            Kind::Mysql
        ));
        assert!(!is_read_only(
            "SELECT 1 # [\nINTO OUTFILE '/tmp/x' -- ]",
            Kind::Mysql
        ));
        // With NO_BACKSLASH_ESCAPES, `'\'` is a whole string.
        assert!(!is_read_only(
            r"SELECT '\' INTO OUTFILE '/tmp/x' -- '",
            Kind::Mysql
        ));
        assert!(!is_read_only(
            r#"SELECT "\"" INTO OUTFILE '/tmp/x' -- ""#,
            Kind::Mysql
        ));
        // `/*! ... */` runs as SQL on MySQL, and `--` without a space isn't a comment.
        assert!(!is_read_only(
            "SELECT 1 /*! INTO OUTFILE '/tmp/x' */",
            Kind::Mysql
        ));
        assert!(!is_read_only(
            "SELECT 1 /*!50000 INTO DUMPFILE '/tmp/x' */",
            Kind::Mysql
        ));
        assert!(!is_read_only(
            "SELECT 1 /*M! INTO OUTFILE '/tmp/x' */",
            Kind::Mysql
        ));
        assert!(!is_read_only(
            "SELECT 1--1 INTO OUTFILE '/tmp/x'",
            Kind::Mysql
        ));
        // Brackets don't quote in MySQL.
        assert!(!is_read_only("SELECT [delete] FROM t", Kind::Mysql));
    }

    #[test]
    fn postgres_escape_and_dollar_strings_cannot_hide_writes() {
        assert!(!is_read_only(
            r"SELECT E'\'' ; DROP TABLE t -- '",
            Kind::Postgres
        ));
        assert!(!is_read_only(
            r"SELECT e'\''; DROP TABLE t -- '",
            Kind::Postgres
        ));
        // With standard_conforming_strings off, plain strings take backslash escapes too.
        assert!(!is_read_only(
            r"SELECT '\''; DROP TABLE t -- '",
            Kind::Postgres
        ));
        assert!(!is_read_only(
            "SELECT $$'$$; DROP TABLE t; -- '",
            Kind::Postgres
        ));
        assert!(!is_read_only(
            "SELECT $q$ ' $q$; DELETE FROM t -- '",
            Kind::Postgres
        ));
        // A backtick is an operator character in Postgres, not a quote.
        assert!(!is_read_only("SELECT 1 ` DELETE FROM t `", Kind::Postgres));
    }

    #[test]
    fn file_writes_new_tables_and_side_effects_need_confirmation() {
        assert!(rw_everywhere("SELECT * FROM t INTO OUTFILE '/tmp/x'"));
        assert!(rw_everywhere("SELECT * FROM t INTO DUMPFILE '/tmp/x'"));
        assert!(rw_everywhere("SELECT * INTO TEMP t2 FROM t"));
        assert!(rw_everywhere("SELECT * INTO #t2 FROM t"));
        assert!(rw_everywhere("COPY t TO '/tmp/x'"));
        assert!(rw_everywhere("COPY (SELECT * FROM t) TO PROGRAM 'sh'"));
        assert!(rw_everywhere("SELECT pg_terminate_backend(123)"));
        assert!(rw_everywhere("SELECT nextval('orders_id_seq')"));
        assert!(rw_everywhere(
            "SELECT * FROM dblink('dbname=x', 'DELETE FROM t') AS r(n int)"
        ));
        assert!(rw_everywhere("SELECT lo_export(16409, '/tmp/x')"));
        assert!(rw_everywhere("SELECT load_extension('evil')"));
        assert!(rw_everywhere(
            "SELECT * FROM OPENDATASOURCE('SQLNCLI', 'Data Source=x').db.dbo.t"
        ));
        assert!(rw_everywhere("SELECT NEXT VALUE FOR dbo.order_seq"));
        // Reading a file is a read.
        assert!(is_read_only("SELECT LOAD_FILE('/etc/hosts')", Kind::Mysql));
    }

    #[test]
    fn effects_a_rollback_does_not_undo_need_confirmation() {
        for sql in [
            "SELECT pg_create_physical_replication_slot('s')",
            "SELECT * FROM pg_create_logical_replication_slot('s', 'pgoutput')",
            "SELECT pg_drop_replication_slot('s')",
            "SELECT pg_stat_reset()",
            "SELECT pg_stat_reset_shared('bgwriter')",
            "SELECT pg_stat_statements_reset()",
            "SELECT pg_advisory_lock(42)",
            "SELECT pg_try_advisory_lock_shared(42)",
            "SELECT pg_replication_origin_create('o')",
            "SELECT pg_create_restore_point('r')",
            "SELECT GET_LOCK('sayql', 10)",
        ] {
            assert!(rw_everywhere(sql), "{sql}");
        }
        // Transaction-level advisory locks end with the read-only transaction.
        assert!(ro_everywhere("SELECT pg_try_advisory_xact_lock(42)"));
        assert!(ro_everywhere("SELECT * FROM pg_stat_activity"));
        assert!(ro_everywhere("SELECT slot_name FROM pg_replication_slots"));
    }

    #[test]
    fn show_create_is_a_read() {
        assert!(is_read_only("SHOW CREATE TABLE `orders`", Kind::Mysql));
        assert!(is_read_only("show create view v", Kind::Mysql));
        assert!(!is_read_only(
            "SHOW TABLES; CREATE TABLE x (a int)",
            Kind::Mysql
        ));
        assert!(!is_read_only(
            "SELECT 1; CREATE TABLE x (a int)",
            Kind::Mysql
        ));
    }

    #[test]
    fn normal_queries_stay_read_only_in_each_dialect() {
        let mysql = [
            r#"SELECT * FROM t WHERE name = 'O\'Brien' AND note = "say \"hi\"""#,
            "SELECT id FROM t # delete later\nWHERE a = 1",
            "# top customers\nSELECT * FROM customers",
            "SELECT 5--1",
            "SELECT `order`, a$b FROM `my db`.t LIMIT 10",
            "SELECT /*+ MAX_EXECUTION_TIME(1000) */ * FROM t",
            r"SELECT 'C:\\path\\update' AS p",
        ];
        for sql in mysql {
            assert!(is_read_only(sql, Kind::Mysql), "{sql}");
        }
        let postgres = [
            r"SELECT E'it\'s', E'line\nnext' FROM t",
            "SELECT $$it's DELETE$$, $fn$ drop $fn$",
            "SELECT a$b, arr[1], arr[1:2] FROM t WHERE c = $1 AND d = $2",
            "SELECT 'x' || '[' || ']' FROM t",
            "SELECT 5 # 3, x::text[] FROM t",
            "WITH r AS (SELECT * FROM orders) SELECT count(*) FROM r",
        ];
        for sql in postgres {
            assert!(is_read_only(sql, Kind::Postgres), "{sql}");
        }
        let mssql = [
            "SELECT TOP 10 [Order Date], [Delete] FROM dbo.[Orders] WHERE n = N'O''Brien'",
            "SELECT next_value FROM t",
            "SELECT 'a\\' AS p, 1 FROM t",
        ];
        for sql in mssql {
            assert!(is_read_only(sql, Kind::Mssql), "{sql}");
        }
        let sqlite = [
            "SELECT [update], `drop`, \"insert\" FROM t WHERE x = 'a\\'",
            "PRAGMA table_info('t')",
        ];
        for sql in sqlite {
            assert!(is_read_only(sql, Kind::Sqlite), "{sql}");
        }
    }
}
