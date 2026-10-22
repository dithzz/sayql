//! Masks row values in database errors before they go to an AI provider. Errors about
//! duplicate keys, failed conversions or constraint checks often quote the offending
//! value; the AI only needs the error type and the names involved to fix the query.
//!
//! Values are masked as `…`. Identifiers (column, table, constraint, key names) are kept
//! where the message marks them as such; any other quoted text is masked.

const MASK: &str = "…";

/// Text before a parenthesised value list, masked up to the last `)` on the line:
/// Postgres `Key (email)=(a@b.com) already exists.` and `Failing row contains (1, x).`,
/// SQL Server `The duplicate key value is (42).`
const VALUE_LISTS: &[&str] = &[")=(", "failing row contains (", "duplicate key value is ("];

/// A single-quoted name after one of these words is an identifier, not a value.
const IDENTIFIER_WORDS: &[&str] = &[
    "alias", "column", "constraint", "database", "field", "function", "index", "key", "name", "near", "object",
    "procedure", "relation", "schema", "sequence", "table", "trigger", "view",
];

/// MySQL names parts of the query in quotes: `Unknown column 'x' in 'field list'`; MariaDB 11
/// names the clause instead: `in 'SELECT'`.
const QUERY_PARTS: &[&str] = &[
    "field list", "where clause", "on clause", "order clause", "group statement", "having clause", "select", "where",
    "on", "order by", "group by", "having", "set", "returning",
];

/// Text that follows a quoted value in MySQL and SQL Server messages, used to find where
/// a value that itself contains a quote ends (`Duplicate entry 'O'Brien' for key ...`).
const AFTER_VALUE: &[&str] = &["' for key", "' for column", "' to data type", "' at row", "' overflowed"];

/// The database error with quoted and listed values replaced by `…`.
pub(crate) fn database_error(error: &str) -> String {
    error.lines().map(|line| mask_quotes(&mask_value_lists(line))).collect::<Vec<_>>().join("\n")
}

fn mask_value_lists(line: &str) -> String {
    let mut line = line.to_string();
    let mut from = 0;
    loop {
        let lower = line.to_ascii_lowercase();
        let Some(start) = VALUE_LISTS
            .iter()
            .filter_map(|m| lower[from..].find(m).map(|i| from + i + m.len()))
            .min()
        else {
            return line;
        };
        let end = line[start..].rfind(')').map_or(line.len(), |i| start + i);
        line.replace_range(start..end, MASK);
        from = start + MASK.len();
    }
}

fn mask_quotes(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut i = 0;
    while let Some(offset) = line[i..].find(['\'', '"']) {
        let open = i + offset;
        let quote = &line[open..open + 1];
        out.push_str(&line[i..open]);
        let body = open + 1;
        let is_apostrophe = quote == "'"
            && line[..open].chars().next_back().is_some_and(char::is_alphanumeric)
            && line[body..].chars().next().is_some_and(char::is_alphanumeric);
        if is_apostrophe {
            out.push_str(quote);
            i = body;
            continue;
        }
        let next_quote = line[body..].find(quote).map(|j| body + j);
        let keep = match quote {
            "\"" => !is_value_context(&out, &["value", "entry"]),
            _ => {
                let names_query_part =
                    next_quote.is_some_and(|q| QUERY_PARTS.iter().any(|p| p.eq_ignore_ascii_case(line[body..q].trim())));
                names_query_part || !is_value_context(&out, &[]) && follows_identifier_word(&out)
            }
        };
        if keep {
            match next_quote {
                Some(close) => {
                    out.push_str(&line[open..=close]);
                    i = close + 1;
                }
                None => {
                    out.push_str(quote);
                    i = body;
                }
            }
            continue;
        }
        out.push_str(quote);
        out.push_str(MASK);
        match value_end(line, body, quote) {
            Some(close) => {
                out.push_str(quote);
                i = close + 1;
            }
            None => return out,
        }
    }
    out.push_str(&line[i..]);
    out
}

/// Text ending in `:` (`Incorrect integer value: 'abc'`) or one of `words` introduces a value.
fn is_value_context(before: &str, words: &[&str]) -> bool {
    let before = before.trim_end();
    before.ends_with(':') || words.contains(&last_word(before).to_ascii_lowercase().as_str())
}

fn follows_identifier_word(before: &str) -> bool {
    IDENTIFIER_WORDS.contains(&last_word(before.trim_end()).to_ascii_lowercase().as_str())
}

fn last_word(text: &str) -> &str {
    let start = text.rfind(|c: char| !(c.is_alphanumeric() || c == '_')).map_or(0, |i| i + 1);
    &text[start..]
}

/// Where a masked value that starts at `body` ends: before known trailing text if the
/// line has it, else at the first quote followed by a space, punctuation or the line end.
fn value_end(line: &str, body: usize, quote: &str) -> Option<usize> {
    let rest = &line[body..];
    if quote == "'" {
        let lower = rest.to_ascii_lowercase();
        if let Some(i) = AFTER_VALUE.iter().filter_map(|t| lower.find(t)).min() {
            return Some(body + i);
        }
    }
    rest.match_indices(quote)
        .find(|(i, _)| {
            rest[i + 1..].chars().next().is_none_or(|c| c.is_whitespace() || ".,;:)]".contains(c))
        })
        .map(|(i, _)| body + i)
}

#[cfg(test)]
mod tests {
    use super::database_error as r;

    #[test]
    fn postgres_values_are_masked_and_names_kept() {
        assert_eq!(
            r("duplicate key value violates unique constraint \"users_email_key\"\nDETAIL:  Key (email)=(ann@example.com) already exists."),
            "duplicate key value violates unique constraint \"users_email_key\"\nDETAIL:  Key (email)=(…) already exists."
        );
        assert_eq!(
            r("DETAIL:  Key (user_id)=(99) is not present in table \"users\"."),
            "DETAIL:  Key (user_id)=(…) is not present in table \"users\"."
        );
        assert_eq!(
            r("DETAIL:  Key (lower(email::text), org_id)=(ann@example.com, 7) already exists."),
            "DETAIL:  Key (lower(email::text), org_id)=(…) already exists."
        );
        assert_eq!(
            r("null value in column \"name\" of relation \"clients\" violates not-null constraint\nDETAIL:  Failing row contains (12, null, Ann (VIP), 2024-01-01)."),
            "null value in column \"name\" of relation \"clients\" violates not-null constraint\nDETAIL:  Failing row contains (…)."
        );
        assert_eq!(r("invalid input syntax for type integer: \"abc\""), "invalid input syntax for type integer: \"…\"");
        assert_eq!(r("value \"99999999999\" is out of range for type integer"), "value \"…\" is out of range for type integer");
        assert_eq!(r("invalid input value for enum mood: \"sad\""), "invalid input value for enum mood: \"…\"");
        assert_eq!(r("column \"nme\" does not exist"), "column \"nme\" does not exist");
        assert_eq!(r("relation \"public.clients\" does not exist"), "relation \"public.clients\" does not exist");
    }

    #[test]
    fn mysql_values_are_masked_and_names_kept() {
        assert_eq!(
            r("Duplicate entry 'ann@example.com' for key 'users.email'"),
            "Duplicate entry '…' for key 'users.email'"
        );
        assert_eq!(r("Duplicate entry 'O'Brien-42' for key 'PRIMARY'"), "Duplicate entry '…' for key 'PRIMARY'");
        assert_eq!(
            r("Incorrect integer value: 'abc' for column 'age' at row 1"),
            "Incorrect integer value: '…' for column 'age' at row 1"
        );
        assert_eq!(
            r("Incorrect datetime value: '2024-13-01' for column 'created_at' at row 1"),
            "Incorrect datetime value: '…' for column 'created_at' at row 1"
        );
        assert_eq!(r("Unknown column 'nme' in 'field list'"), "Unknown column 'nme' in 'field list'");
        assert_eq!(r("Unknown column 'nme' in 'SELECT'"), "Unknown column 'nme' in 'SELECT'");
        assert_eq!(r("Table 'shop.clients' doesn't exist"), "Table 'shop.clients' doesn't exist");
        assert_eq!(r("Column 'name' cannot be null"), "Column 'name' cannot be null");
        assert_eq!(r("Data too long for column 'name' at row 1"), "Data too long for column 'name' at row 1");
    }

    #[test]
    fn sql_server_values_are_masked_and_names_kept() {
        assert_eq!(
            r("Conversion failed when converting the varchar value 'abc' to data type int."),
            "Conversion failed when converting the varchar value '…' to data type int."
        );
        assert_eq!(
            r("Violation of PRIMARY KEY constraint 'PK_x'. Cannot insert duplicate key in object 'dbo.T'. The duplicate key value is (42)."),
            "Violation of PRIMARY KEY constraint 'PK_x'. Cannot insert duplicate key in object 'dbo.T'. The duplicate key value is (…)."
        );
        assert_eq!(
            r("String or binary data would be truncated in table 'shop.dbo.clients', column 'name'. Truncated value: 'Ann Smith-Jo'."),
            "String or binary data would be truncated in table 'shop.dbo.clients', column 'name'. Truncated value: '…'."
        );
        assert_eq!(
            r("The conversion of the varchar value '99999999999' overflowed an int column."),
            "The conversion of the varchar value '…' overflowed an int column."
        );
        assert_eq!(r("Invalid column name 'nme'."), "Invalid column name 'nme'.");
        assert_eq!(r("Invalid object name 'dbo.clients'."), "Invalid object name 'dbo.clients'.");
        assert_eq!(
            r("The INSERT statement conflicted with the FOREIGN KEY constraint \"FK_orders_clients\". The conflict occurred in database \"shop\", table \"dbo.clients\", column 'id'."),
            "The INSERT statement conflicted with the FOREIGN KEY constraint \"FK_orders_clients\". The conflict occurred in database \"shop\", table \"dbo.clients\", column 'id'."
        );
        assert_eq!(r("Incorrect syntax near 'FROM'."), "Incorrect syntax near 'FROM'.");
    }

    #[test]
    fn sqlite_messages_keep_their_names() {
        for e in [
            "UNIQUE constraint failed: users.email",
            "NOT NULL constraint failed: clients.name",
            "no such column: nme",
            "near \"SELEC\": syntax error",
            "datatype mismatch",
        ] {
            assert_eq!(r(e), e);
        }
    }

    #[test]
    fn unknown_quoted_text_is_masked() {
        assert_eq!(r("something odd with 'secret' here"), "something odd with '…' here");
        assert_eq!(r("unterminated value: 'secret"), "unterminated value: '…");
    }
}
