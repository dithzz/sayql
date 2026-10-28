use super::{bytes, float, num};
use serde_json::Value;
use sqlx::sqlite::SqliteRow;
use sqlx::{Row, TypeInfo, ValueRef};

pub(crate) fn lite_cell(row: &SqliteRow, i: usize) -> Value {
    let Ok(raw) = row.try_get_raw(i) else {
        return Value::Null;
    };
    if raw.is_null() {
        return Value::Null;
    }
    // SQLite is dynamically typed: decode by the stored value's type, not the declared one.
    let ty = raw.type_info().name().to_string();
    let v = match ty.as_str() {
        "INTEGER" | "BOOLEAN" => num(row.try_get_unchecked::<Option<i64>, _>(i).ok().flatten()),
        "REAL" | "NUMERIC" => float(row.try_get_unchecked::<Option<f64>, _>(i).ok().flatten()),
        "BLOB" => bytes(
            row.try_get_unchecked::<Option<Vec<u8>>, _>(i)
                .ok()
                .flatten(),
        ),
        _ => row
            .try_get_unchecked::<Option<String>, _>(i)
            .ok()
            .flatten()
            .map(Value::String),
    };
    v.unwrap_or_else(|| Value::String(format!("<{}>", ty.to_lowercase())))
}
