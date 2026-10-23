//! Checks SayQL runs on the agent's work in code, not by asking the model: joins that repeat
//! rows before they are summed, and numbers in the answer that no step's result contains.

use crate::schema::Schema;
use serde_json::Value;
use std::collections::HashMap;

/// Warnings for joins that repeat the rows of a table whose columns the query then sums,
/// averages or counts. Uses primary keys from the schema: joining a table on its key to
/// another table's non-key column repeats the first table's rows once per match.
pub fn join_fan_out(sql: &str, schema: &Schema) -> Vec<String> {
    let words = tokens(sql);
    let lower: Vec<String> = words.iter().map(|w| w.to_lowercase()).collect();
    // alias → table, from `FROM t [AS] a` and `JOIN t [AS] a`.
    let mut aliases: HashMap<String, String> = HashMap::new();
    let mut joins = 0;
    for (i, w) in lower.iter().enumerate() {
        if w == "from" || w == "join" {
            if w == "join" {
                joins += 1;
            }
            let Some(table) = words.get(i + 1).filter(|t| t.as_str() != "(") else { continue };
            let table = unquote(table);
            let short = table.rsplit('.').next().unwrap_or(&table).to_lowercase();
            let mut alias = short.clone();
            let next = lower.get(i + 2).map(String::as_str);
            let after = match next {
                Some("as") => lower.get(i + 3),
                Some(n) if !KEYWORDS.contains(&n) && n != "," && n != "(" && n != ")" => lower.get(i + 2),
                _ => None,
            };
            if let Some(a) = after {
                alias = unquote(a).to_lowercase();
            }
            aliases.insert(alias, short.clone());
            aliases.entry(short.clone()).or_insert(short);
        }
    }
    if joins == 0 {
        return Vec::new();
    }
    let unique = |alias: &str, column: &str| -> Option<bool> {
        let table = aliases.get(alias)?;
        let t = schema.tables.iter().find(|t| t.name.eq_ignore_ascii_case(table))?;
        let keys: Vec<&str> = t.columns.iter().filter(|c| c.primary_key).map(|c| c.name.as_str()).collect();
        Some(keys.len() == 1 && keys[0].eq_ignore_ascii_case(column))
    };
    // `a.x = b.y` pairs after ON.
    let mut repeated: Vec<(String, String)> = Vec::new();
    for i in 0..lower.len().saturating_sub(2) {
        if lower[i + 1] != "=" {
            continue;
        }
        let (Some((a, x)), Some((b, y))) = (qualified(&lower[i]), qualified(&lower[i + 2])) else { continue };
        match (unique(&a, &x), unique(&b, &y)) {
            (Some(true), Some(false)) => repeated.push((a.clone(), format!("{b}.{y}"))),
            (Some(false), Some(true)) => repeated.push((b.clone(), format!("{a}.{x}"))),
            _ => {}
        }
    }
    let mut out = Vec::new();
    for (alias, by) in repeated {
        let table = aliases.get(&alias).cloned().unwrap_or_else(|| alias.clone());
        for (i, w) in lower.iter().enumerate() {
            let agg = matches!(w.as_str(), "sum" | "avg" | "count") && lower.get(i + 1).is_some_and(|p| p == "(");
            let distinct = lower.get(i + 2).is_some_and(|d| d == "distinct");
            let target = lower.get(i + 2).and_then(|t| qualified(t));
            if agg && !distinct && target.is_some_and(|(a, _)| a == alias) {
                out.push(format!(
                    "{table} rows repeat once per matching {by} in this join, so {}(...) over {table} counts each of them \
                     several times. Aggregate {table} in a subquery first, or use DISTINCT.",
                    w.to_uppercase()
                ));
                break;
            }
        }
    }
    out
}

/// A table with more rows than this is big: reading all of it needs a filter or a plan check.
pub const BIG_TABLE_ROWS: i64 = 1_000_000;

/// The big table `sql` reads without any WHERE clause, with its row estimate.
pub fn unfiltered_big_scan(sql: &str, schema: &Schema) -> Option<(String, i64)> {
    let words: Vec<String> = tokens(sql).into_iter().map(|w| w.to_lowercase()).collect();
    if words.iter().any(|w| w == "where") {
        return None;
    }
    words.iter().enumerate().filter(|(_, w)| *w == "from" || *w == "join").find_map(|(i, _)| {
        let name = unquote(words.get(i + 1)?);
        let short = name.rsplit('.').next().unwrap_or(&name).to_string();
        let t = schema.tables.iter().find(|t| t.name.eq_ignore_ascii_case(&short))?;
        let rows = t.row_estimate?;
        (rows > BIG_TABLE_ROWS).then(|| (t.name.clone(), rows))
    })
}

/// How many statements `sql` holds, ignoring `;` in strings, comments and at the end.
pub fn statements(sql: &str) -> usize {
    let t = tokens(sql);
    let end = t.iter().rposition(|w| w != ";").map_or(0, |i| i + 1);
    1 + t[..end].iter().filter(|w| *w == ";").count()
}

const KEYWORDS: &[&str] = &[
    "on", "where", "join", "inner", "left", "right", "full", "cross", "outer", "group", "order", "limit", "having", "union",
    "using", "natural", "offset", "fetch", "window", "for", "with",
];

/// Words, quoted names and single symbols of the SQL, with strings and comments dropped.
fn tokens(sql: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut chars = sql.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\'' => {
                while let Some(d) = chars.next() {
                    if d == '\'' {
                        if chars.peek() == Some(&'\'') {
                            chars.next();
                        } else {
                            break;
                        }
                    }
                }
            }
            '-' if chars.peek() == Some(&'-') => {
                for d in chars.by_ref() {
                    if d == '\n' {
                        break;
                    }
                }
            }
            c if c.is_alphanumeric() || "_\"`[].$".contains(c) => {
                let mut w = c.to_string();
                while let Some(&d) = chars.peek() {
                    if d.is_alphanumeric() || "_\"`[].$".contains(d) {
                        w.push(d);
                        chars.next();
                    } else {
                        break;
                    }
                }
                out.push(w);
            }
            c if c.is_whitespace() => {}
            c => out.push(c.to_string()),
        }
    }
    out
}

fn unquote(s: &str) -> String {
    s.chars().filter(|c| !"\"`[]".contains(*c)).collect()
}

/// `alias.column` as (alias, column), quotes removed.
fn qualified(word: &str) -> Option<(String, String)> {
    let w = unquote(word);
    let mut parts = w.rsplitn(2, '.');
    let column = parts.next()?.to_string();
    let alias = parts.next()?.rsplit('.').next()?.to_string();
    (!alias.is_empty() && !column.is_empty()).then_some((alias, column))
}

/// The numbers in `text` no step backs: not in any result, row count, the question or the
/// SQL that ran. Rounded figures count when a result rounds to them.
pub fn unbacked_numbers(text: &str, results: &[&[Vec<Value>]], other: &[f64], context: &str) -> Vec<String> {
    let mut known: Vec<f64> = other.to_vec();
    for rows in results {
        for row in rows.iter() {
            for v in row {
                match v {
                    Value::Number(n) => known.extend(n.as_f64()),
                    Value::String(s) => known.extend(numbers(s).into_iter().map(|(_, n, _)| n)),
                    _ => {}
                }
            }
        }
    }
    known.extend(numbers(context).into_iter().map(|(_, n, _)| n));
    numbers(text)
        .into_iter()
        .filter(|(_, n, decimals)| {
            let tolerance = 0.5 * 10f64.powi(-(*decimals as i32)) + 1e-9;
            // 12% may come from 0.12.
            !known.iter().any(|k| (k - n).abs() <= tolerance || (k * 100.0 - n).abs() <= tolerance)
        })
        .map(|(s, _, _)| s)
        .collect()
}

/// The numbers written in `text`: (as written, value, decimal places). Thousands separators
/// are allowed; digits inside words (`q3`, `‹email-3›`) are not numbers.
fn numbers(text: &str) -> Vec<(String, f64, usize)> {
    let chars: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let starts = chars[i].is_ascii_digit() && (i == 0 || !(chars[i - 1].is_alphanumeric() || chars[i - 1] == '-' || chars[i - 1] == '_'));
        if !starts {
            i += 1;
            continue;
        }
        let mut j = i;
        let mut written = String::new();
        while j < chars.len() {
            let c = chars[j];
            let next_digit = chars.get(j + 1).is_some_and(char::is_ascii_digit);
            if c.is_ascii_digit() || ((c == ',' || c == '.') && next_digit) {
                written.push(c);
                j += 1;
            } else {
                break;
            }
        }
        let ends_in_word = chars.get(j).is_some_and(|c| c.is_alphabetic() || *c == '›' || *c == '_');
        if !ends_in_word {
            let plain = written.replace(',', "");
            if let Ok(n) = plain.parse::<f64>() {
                let decimals = plain.split_once('.').map_or(0, |(_, d)| d.len());
                out.push((written, n, decimals));
            }
        }
        i = j.max(i + 1);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::{Column, Table};
    use serde_json::json;

    fn schema() -> Schema {
        let table = |name: &str, cols: &[(&str, bool)]| Table {
            schema: None,
            name: name.into(),
            kind: "table".into(),
            columns: cols
                .iter()
                .map(|(c, pk)| Column { name: (*c).into(), data_type: "int".into(), nullable: false, primary_key: *pk, enum_values: None })
                .collect(),
            foreign_keys: vec![],
            row_estimate: None,
        };
        Schema {
            dialect: "SQLite".into(),
            tables: vec![
                table("orders", &[("id", true), ("total", false), ("customer_id", false)]),
                table("order_items", &[("id", true), ("order_id", false), ("quantity", false)]),
            ],
        }
    }

    #[test]
    fn flags_sums_over_repeated_rows() {
        let bad = "SELECT SUM(o.total) FROM orders o JOIN order_items oi ON oi.order_id = o.id";
        let w = join_fan_out(bad, &schema());
        assert_eq!(w.len(), 1, "{w:?}");
        assert!(w[0].contains("orders rows repeat") && w[0].contains("oi.order_id"), "{w:?}");
        let quoted = "SELECT AVG(\"o\".\"total\") FROM \"orders\" AS \"o\" INNER JOIN order_items AS oi ON o.id = oi.order_id";
        assert_eq!(join_fan_out(quoted, &schema()).len(), 1);
        for fine in [
            "SELECT SUM(oi.quantity) FROM orders o JOIN order_items oi ON oi.order_id = o.id",
            "SELECT COUNT(DISTINCT o.id) FROM orders o JOIN order_items oi ON oi.order_id = o.id",
            "SELECT SUM(total) FROM orders",
            "SELECT o.id FROM orders o JOIN order_items oi ON oi.order_id = o.id WHERE o.total > 'SUM(o.total)'",
        ] {
            assert!(join_fan_out(fine, &schema()).is_empty(), "{fine}");
        }
    }

    #[test]
    fn finds_unfiltered_scans_of_big_tables() {
        let mut s = schema();
        s.tables[1].row_estimate = Some(5_000_000);
        assert_eq!(unfiltered_big_scan("SELECT SUM(quantity) FROM order_items", &s), Some(("order_items".into(), 5_000_000)));
        assert_eq!(unfiltered_big_scan("SELECT * FROM orders o JOIN \"order_items\" oi ON oi.order_id = o.id", &s).map(|x| x.0), Some("order_items".into()));
        assert!(unfiltered_big_scan("SELECT COUNT(*) FROM order_items WHERE order_id = 3", &s).is_none());
        assert!(unfiltered_big_scan("SELECT COUNT(*) FROM orders", &s).is_none());
    }

    #[test]
    fn counts_statements() {
        assert_eq!(statements("SELECT 1;"), 1);
        assert_eq!(statements("SELECT ';' -- ;\n;"), 1);
        assert_eq!(statements("SELECT 1; SELECT 2"), 2);
    }

    #[test]
    fn finds_numbers_no_step_backs() {
        let rows = vec![vec![json!("Pinecrest Physio"), json!(0), json!("8")], vec![json!("total"), json!(1234.567), json!("2026-08-24")]];
        let results: Vec<&[Vec<Value>]> = vec![&rows];
        let text = "Pinecrest Physio went from 8 to 0 appointments in the week of 2026-08-24, about 1,234.6 in total, \
                    40% of the drop, per ‹name-3› and step 2. It was 17 last month.";
        let bad = unbacked_numbers(text, &results, &[0.4, 2.0], "");
        assert_eq!(bad, ["17"], "{bad:?}");
        assert!(unbacked_numbers("Top 5 customers.", &results, &[], "top 5 customers").is_empty());
    }
}
