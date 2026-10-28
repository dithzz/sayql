//! Ported from the SayQL Team server (sayql-server src/sql_lex.rs).
//!
//! A small SQL lexer that knows where each engine's strings, quoted identifiers and comments
//! start and end. The classifier and the never-send checks read its tokens, so a write keyword
//! or a column name can't hide inside something one engine treats as a string and another
//! doesn't.
//!
//! Some engine settings change how text is split (MySQL `NO_BACKSLASH_ESCAPES` and
//! `ANSI_QUOTES`, Postgres `standard_conforming_strings`). The server can't know them, so
//! [`lex`] returns one token list per plausible setting and callers check every one.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dialect {
    Postgres,
    MySql,
    Sqlite,
    /// SQL Server (T-SQL): `'…'` and `N'…'` strings, `"…"` and `[…]` identifiers (`]]` is an
    /// escaped `]`), `--` and nested `/* */` comments, `0x…` binary literals.
    Mssql,
}

impl Dialect {
    /// Settings under which the same text splits differently.
    fn variants(self) -> &'static [Variant] {
        const PLAIN: Variant = Variant {
            bs_single: false,
            bs_double: false,
        };
        match self {
            // standard_conforming_strings on (default) and off.
            Dialect::Postgres => &[
                PLAIN,
                Variant {
                    bs_single: true,
                    bs_double: false,
                },
            ],
            // Default mode, ANSI_QUOTES, and NO_BACKSLASH_ESCAPES (with or without ANSI_QUOTES).
            Dialect::MySql => &[
                Variant {
                    bs_single: true,
                    bs_double: true,
                },
                Variant {
                    bs_single: true,
                    bs_double: false,
                },
                PLAIN,
            ],
            // T-SQL has no backslash escapes. QUOTED_IDENTIFIER OFF makes `"…"` a string, but
            // it ends at the same place either way.
            Dialect::Sqlite | Dialect::Mssql => &[PLAIN],
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct Variant {
    /// Backslash escapes inside `'…'`.
    bs_single: bool,
    /// Backslash escapes inside `"…"` (MySQL without ANSI_QUOTES, where `"…"` is a string).
    bs_double: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Tok {
    /// An unquoted word (keyword or identifier) as written.
    Word(String),
    /// A quoted identifier, unescaped: `"x"`, `` `x` ``, `[x]`, Postgres `U&"x"`. On MySQL
    /// `"x"` is a string unless ANSI_QUOTES is on; it is reported here either way.
    Quoted(String),
    /// A string literal, unescaped as far as it matters here. SQLite also accepts `'x'` as an
    /// identifier in some places (`FROM 'patients'`, `p.'ssn'`).
    Str(String),
    /// Digits and dots.
    Num,
    /// Any other character.
    Punct(char),
}

impl Tok {
    /// Word or quoted identifier, lowercased.
    pub fn ident(&self) -> Option<String> {
        match self {
            Tok::Word(w) | Tok::Quoted(w) => Some(w.to_lowercase()),
            _ => None,
        }
    }

    pub fn is_word(&self, kw: &str) -> bool {
        matches!(self, Tok::Word(w) if w.eq_ignore_ascii_case(kw))
    }

    pub fn is_punct(&self, c: char) -> bool {
        *self == Tok::Punct(c)
    }
}

#[derive(Debug, Clone, Default)]
pub struct Lexed {
    pub toks: Vec<Tok>,
    /// MySQL `/*! … */`, `/*M! … */` (run as SQL) or `/*+ … */` (optimizer hints).
    pub exec_comment: bool,
}

/// The statement's tokens under each plausible engine setting.
pub fn lex(sql: &str, dialect: Dialect) -> Vec<Lexed> {
    dialect
        .variants()
        .iter()
        .map(|v| lex_one(sql, dialect, *v))
        .collect()
}

fn is_ident_start(b: u8) -> bool {
    b.is_ascii_alphabetic() || b == b'_' || b >= 0x80
}

fn is_ident_char(b: u8, dialect: Dialect) -> bool {
    b.is_ascii_alphanumeric()
        || b == b'_'
        || b >= 0x80
        || (b == b'$' && matches!(dialect, Dialect::Postgres | Dialect::MySql))
}

fn lex_one(sql: &str, dialect: Dialect, v: Variant) -> Lexed {
    let b = sql.as_bytes();
    let n = b.len();
    let mut out = Lexed::default();
    let mut i = 0;
    // End of the previous token, to spot prefixes written right before a quote (E'…', U&"…").
    let mut prev_end = usize::MAX;
    while i < n {
        let c = b[i];
        let start = i;
        match c {
            _ if c.is_ascii_whitespace() => {
                i += 1;
                continue;
            }
            b'-' if b.get(i + 1) == Some(&b'-') => {
                // MySQL needs whitespace (or the end) after `--`; `1--1` is arithmetic there.
                let comment = dialect != Dialect::MySql
                    || b.get(i + 2)
                        .is_none_or(|x| x.is_ascii_whitespace() || *x < 0x20);
                if comment {
                    i = line_end(b, i);
                    continue;
                }
                out.toks.push(Tok::Punct('-'));
                i += 1;
            }
            b'#' if dialect == Dialect::MySql => {
                i = line_end(b, i);
                continue;
            }
            b'/' if b.get(i + 1) == Some(&b'*') => {
                if dialect == Dialect::MySql
                    && (matches!(b.get(i + 2), Some(b'!' | b'+'))
                        || (b.get(i + 2) == Some(&b'M') && b.get(i + 3) == Some(&b'!')))
                {
                    out.exec_comment = true;
                }
                // Postgres and SQL Server comments nest.
                i = comment_end(b, i, matches!(dialect, Dialect::Postgres | Dialect::Mssql));
                continue;
            }
            b'\'' => {
                // Postgres E'…' strings take backslash escapes whatever the setting.
                let e_prefix = dialect == Dialect::Postgres
                    && prev_end == i
                    && matches!(out.toks.last(), Some(Tok::Word(w)) if w.eq_ignore_ascii_case("e"));
                if e_prefix {
                    out.toks.pop();
                }
                // T-SQL N'…' (Unicode) strings: drop the N so it isn't read as a name.
                if dialect == Dialect::Mssql
                    && prev_end == i
                    && matches!(out.toks.last(), Some(Tok::Word(w)) if w.eq_ignore_ascii_case("n"))
                {
                    out.toks.pop();
                }
                let bs = v.bs_single || e_prefix;
                let (close, end) = quoted_end(b, i + 1, b'\'', bs);
                let inner = sql.get(i + 1..close).unwrap_or("");
                out.toks.push(Tok::Str(unescape_quoted(inner, '\'', bs)));
                i = end;
            }
            b'"' => {
                let bs = dialect == Dialect::MySql && v.bs_double;
                let (close, end) = quoted_end(b, i + 1, b'"', bs);
                let inner = sql.get(i + 1..close).unwrap_or("");
                let mut text = unescape_quoted(inner, '"', bs);
                // Postgres U&"d\0061ta": Unicode escapes.
                if dialect == Dialect::Postgres
                    && prev_end == i
                    && out.toks.len() >= 2
                    && out.toks[out.toks.len() - 1] == Tok::Punct('&')
                    && out.toks[out.toks.len() - 2].is_word("u")
                {
                    out.toks.truncate(out.toks.len() - 2);
                    text = unicode_unescape(&text);
                }
                out.toks.push(Tok::Quoted(text));
                i = end;
            }
            b'`' => {
                let (close, end) = quoted_end(b, i + 1, b'`', false);
                let inner = sql.get(i + 1..close).unwrap_or("");
                out.toks
                    .push(Tok::Quoted(unescape_quoted(inner, '`', false)));
                i = end;
            }
            b'[' if dialect == Dialect::Mssql => {
                // `]]` inside brackets is an escaped `]`.
                let (close, end) = quoted_end(b, i + 1, b']', false);
                let inner = sql.get(i + 1..close).unwrap_or("");
                out.toks
                    .push(Tok::Quoted(unescape_quoted(inner, ']', false)));
                i = end;
            }
            b'[' if dialect == Dialect::Sqlite => {
                let close = sql[i + 1..].find(']').map_or(n, |p| i + 1 + p);
                let inner = sql.get(i + 1..close).unwrap_or("");
                let end = (close + 1).min(n);
                out.toks.push(Tok::Quoted(inner.to_string()));
                i = end;
            }
            b'$' if dialect == Dialect::Postgres => {
                // $tag$ … $tag$ (tag empty or an identifier); `$1` is a parameter.
                let mut j = i + 1;
                while j < n && (is_ident_start(b[j]) || (j > i + 1 && b[j].is_ascii_digit())) {
                    j += 1;
                }
                if j < n && b[j] == b'$' {
                    let tag = &sql[i..=j];
                    let close = sql[j + 1..].find(tag).map_or(n, |p| j + 1 + p);
                    out.toks.push(Tok::Str(sql[j + 1..close].to_string()));
                    i = (close + tag.len()).min(n);
                } else {
                    out.toks.push(Tok::Punct('$'));
                    i += 1;
                }
            }
            b'0' if dialect == Dialect::Mssql && matches!(b.get(i + 1), Some(b'x' | b'X')) => {
                // 0x1F binary literal.
                i += 2;
                while i < n && b[i].is_ascii_alphanumeric() {
                    i += 1;
                }
                out.toks.push(Tok::Num);
            }
            _ if c.is_ascii_digit() => {
                while i < n && (b[i].is_ascii_digit() || b[i] == b'.') {
                    i += 1;
                }
                out.toks.push(Tok::Num);
            }
            _ if is_ident_start(c) => {
                while i < n && is_ident_char(b[i], dialect) {
                    i += 1;
                }
                out.toks.push(Tok::Word(sql[start..i].to_string()));
            }
            _ => {
                // `c` may be the first byte of a multi-byte char only if >= 0x80, handled above.
                out.toks.push(Tok::Punct(c as char));
                i += 1;
            }
        }
        prev_end = i;
    }
    out
}

/// Index of the newline ending a `--` / `#` comment (or the end).
fn line_end(b: &[u8], from: usize) -> usize {
    b[from..]
        .iter()
        .position(|&x| x == b'\n')
        .map_or(b.len(), |p| from + p)
}

/// Index just past a block comment starting at `from` (`/*`). Postgres comments nest.
fn comment_end(b: &[u8], from: usize, nested: bool) -> usize {
    let mut depth = 0usize;
    let mut i = from;
    while i + 1 < b.len() {
        if b[i] == b'/' && b[i + 1] == b'*' {
            if depth == 0 || nested {
                depth += 1;
            }
            i += 2;
        } else if b[i] == b'*' && b[i + 1] == b'/' {
            depth -= 1;
            i += 2;
            if depth == 0 {
                return i;
            }
        } else {
            i += 1;
        }
    }
    b.len()
}

/// For a quoted run starting at `from` (just after the opening quote): the index of the
/// closing `q` and the index just past it (both the end of the text if it never closes).
/// A doubled `qq` is an escaped quote; with `backslash`, so is `\q` (and `\\`).
fn quoted_end(b: &[u8], from: usize, q: u8, backslash: bool) -> (usize, usize) {
    let mut i = from;
    while i < b.len() {
        if backslash && b[i] == b'\\' {
            i += 2;
        } else if b[i] == q {
            if b.get(i + 1) == Some(&q) {
                i += 2;
            } else {
                return (i, i + 1);
            }
        } else {
            i += 1;
        }
    }
    (b.len(), b.len())
}

fn unescape_quoted(inner: &str, q: char, backslash: bool) -> String {
    let mut out = String::with_capacity(inner.len());
    let mut chars = inner.chars().peekable();
    while let Some(c) = chars.next() {
        if backslash && c == '\\' {
            if let Some(n) = chars.next() {
                out.push(n);
            }
        } else if c == q && chars.peek() == Some(&q) {
            chars.next();
            out.push(q);
        } else {
            out.push(c);
        }
    }
    out
}

/// Postgres `U&"…"` escapes with the default escape character: `\XXXX`, `\+XXXXXX`, `\\`.
fn unicode_unescape(s: &str) -> String {
    let mut out = String::new();
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '\\' {
            if chars.get(i + 1) == Some(&'\\') {
                out.push('\\');
                i += 2;
                continue;
            }
            let (start, len) = if chars.get(i + 1) == Some(&'+') {
                (i + 2, 6)
            } else {
                (i + 1, 4)
            };
            let hex: String = chars.iter().skip(start).take(len).collect();
            if hex.len() == len
                && let Some(ch) = u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32)
            {
                out.push(ch);
                i = start + len;
                continue;
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(sql: &str, d: Dialect) -> Vec<Vec<String>> {
        lex(sql, d)
            .into_iter()
            .map(|l| {
                l.toks
                    .iter()
                    .filter_map(|t| match t {
                        Tok::Word(w) => Some(w.to_uppercase()),
                        _ => None,
                    })
                    .collect()
            })
            .collect()
    }

    #[test]
    fn mysql_backslash_escapes_can_not_hide_keywords() {
        let w = words("SELECT 'a\\'' INTO OUTFILE '/tmp/x' -- '", Dialect::MySql);
        assert!(w[0].contains(&"INTO".to_string()), "{w:?}");
        // `1--1` is arithmetic on MySQL, not a comment.
        let w = words("SELECT 1--1\nFROM t", Dialect::MySql);
        assert_eq!(w[0], vec!["SELECT", "FROM", "T"]);
        let w = words("SELECT 1 --x INTO OUTFILE", Dialect::MySql);
        assert!(w[0].contains(&"INTO".to_string()));
        assert!(words("SELECT 1 # INTO\n", Dialect::MySql)[0] == vec!["SELECT"]);
        assert!(lex("SELECT /*!50000 1 */", Dialect::MySql)[0].exec_comment);
        assert!(lex("SELECT /*+ MAX_EXECUTION_TIME(1) */ 1", Dialect::MySql)[0].exec_comment);
    }

    #[test]
    fn postgres_dollar_quotes_e_strings_and_nested_comments() {
        assert_eq!(
            words(
                "SELECT $$ DELETE $$, $tag$ x $tag$ FROM t",
                Dialect::Postgres
            )[0],
            vec!["SELECT", "FROM", "T"]
        );
        // E'' takes backslashes: the string ends at the last quote, so DELETE is code.
        let w = words("SELECT E'\\'' , 1 DELETE '", Dialect::Postgres);
        assert!(w[0].contains(&"DELETE".to_string()), "{w:?}");
        // Nested comment: Postgres runs what follows the second */.
        let w = words(
            "SELECT 1 /* /* */ ' */ DELETE FROM x; -- '",
            Dialect::Postgres,
        );
        assert!(w[0].contains(&"DELETE".to_string()), "{w:?}");
        // standard_conforming_strings on: the backslash is a plain character; off: it escapes.
        let w = words("SELECT 'a\\' DELETE '", Dialect::Postgres);
        assert!(w[0].contains(&"DELETE".to_string()));
        assert!(!w[1].contains(&"DELETE".to_string()));
        // a$b is one identifier; $1 is a parameter.
        assert_eq!(
            words("SELECT a$b, $1", Dialect::Postgres)[0],
            vec!["SELECT", "A$B"]
        );
    }

    #[test]
    fn quoted_identifiers_are_unescaped() {
        let l = &lex(
            r#"SELECT U&"\0073sn", "a""b", `c``d` FROM t"#,
            Dialect::Postgres,
        )[0];
        let q: Vec<_> = l
            .toks
            .iter()
            .filter_map(|t| match t {
                Tok::Quoted(s) => Some(s.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(q, vec!["ssn", "a\"b", "c`d"]);
        let l = &lex("SELECT [x y] FROM t", Dialect::Sqlite)[0];
        assert!(l.toks.contains(&Tok::Quoted("x y".into())));
    }

    #[test]
    fn tsql_brackets_unicode_strings_and_nested_comments() {
        let d = Dialect::Mssql;
        // `]]` is an escaped `]`: the identifier runs to the last `]`.
        let l = &lex("SELECT [a]]' DELETE ] FROM t", d)[0];
        assert!(l.toks.contains(&Tok::Quoted("a]' DELETE ".into())), "{l:?}");
        assert_eq!(
            words("SELECT [a]]' DELETE ] FROM t", d)[0],
            vec!["SELECT", "FROM", "T"]
        );
        // N'…' is one string, not the name N.
        assert_eq!(
            words("SELECT N'it''s' AS n", d)[0],
            vec!["SELECT", "AS", "N"]
        );
        // Comments nest: SQL Server runs what follows the second */.
        let w = words("SELECT 1 /* /* */ '*/ DELETE FROM x --'", d);
        assert!(w[0].contains(&"DELETE".to_string()), "{w:?}");
        assert_eq!(words("SELECT 0x1F2e AS b", d)[0], vec!["SELECT", "AS", "B"]);
    }
}
