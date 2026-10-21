//! Turns driver values into JSON for the results grid. Values are decoded for display:
//! exact (no float rounding of decimals), and never panicking on unusual types.

use serde_json::Value;

/// `row.try_get::<Option<T>>(i)`, with decode errors treated as "no value".
macro_rules! try_as {
    ($row:expr, $i:expr, $t:ty) => {
        $row.try_get::<Option<$t>, _>($i).ok().flatten()
    };
}


mod mssql;
mod mysql;
mod postgres;
mod sqlite;

pub(crate) use mssql::ms_cell;
pub(crate) use mysql::my_cell;
pub(crate) use postgres::pg_cell;
pub(crate) use sqlite::lite_cell;

fn num<T: Into<serde_json::Number>>(v: Option<T>) -> Option<Value> {
    v.map(|n| Value::Number(n.into()))
}

fn float(v: Option<f64>) -> Option<Value> {
    v.map(|f| serde_json::Number::from_f64(f).map(Value::Number).unwrap_or(Value::String(f.to_string())))
}

fn string<T: ToString>(v: Option<T>) -> Option<Value> {
    v.map(|s| Value::String(s.to_string()))
}

fn bytes(v: Option<Vec<u8>>) -> Option<Value> {
    v.map(|b| {
        let hex: String = b.iter().take(64).map(|x| format!("{x:02x}")).collect();
        let more = if b.len() > 64 { format!("… ({} bytes)", b.len()) } else { String::new() };
        Value::String(format!("0x{hex}{more}"))
    })
}

/// Last resort for types we don't decode explicitly (enums, domains, ...):
/// show the raw bytes as text if they are printable.
fn printable(v: Option<String>) -> Option<Value> {
    v.filter(|s| s.chars().all(|c| !c.is_control() || c == '\n' || c == '\t'))
        .map(Value::String)
}

/// `1.1f32 as f64` is 1.100000023841858; go through the shortest decimal form instead.
fn f32_to_f64(f: f32) -> f64 {
    f.to_string().parse().unwrap_or(f64::from(f))
}

/// `[-]HH:MM:SS[.ffffff]` from microseconds, trailing zeros trimmed, hours unbounded.
fn clock(us: i64) -> String {
    let a = us.unsigned_abs();
    let (h, m, s, frac) = (a / 3_600_000_000, a / 60_000_000 % 60, a / 1_000_000 % 60, a % 1_000_000);
    let mut out = format!("{}{h:02}:{m:02}:{s:02}", if us < 0 { "-" } else { "" });
    if frac != 0 {
        out.push('.');
        out.push_str(format!("{frac:06}").trim_end_matches('0'));
    }
    out
}
