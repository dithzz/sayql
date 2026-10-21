use super::{bytes, clock, f32_to_f64, float, num, printable, string};
use serde_json::Value;
use sqlx::mysql::MySqlRow;
use sqlx::{Row, TypeInfo, ValueRef};

pub(crate) fn my_cell(row: &MySqlRow, i: usize) -> Value {
    let Ok(raw) = row.try_get_raw(i) else { return Value::Null };
    if raw.is_null() {
        return Value::Null;
    }
    let ty = raw.type_info().name().to_string();
    use chrono::{DateTime, NaiveDate, NaiveDateTime, Utc};
    let v = match ty.as_str() {
        "BOOLEAN" => try_as!(row, i, bool).map(Value::Bool),
        "TINYINT" => num(try_as!(row, i, i8)),
        "SMALLINT" => num(try_as!(row, i, i16)),
        "MEDIUMINT" | "INT" => num(try_as!(row, i, i32)),
        "BIGINT" => num(try_as!(row, i, i64)),
        "TINYINT UNSIGNED" => num(try_as!(row, i, u8)),
        "SMALLINT UNSIGNED" => num(try_as!(row, i, u16)),
        "MEDIUMINT UNSIGNED" | "INT UNSIGNED" => num(try_as!(row, i, u32)),
        "BIGINT UNSIGNED" | "BIT" => num(try_as!(row, i, u64)),
        "FLOAT" => float(try_as!(row, i, f32).map(f32_to_f64)),
        "DOUBLE" => float(try_as!(row, i, f64)),
        // Sent as text in both protocols; keep it exact rather than round through a Decimal.
        "DECIMAL" => row.try_get_unchecked::<Option<String>, _>(i).ok().flatten().map(Value::String),
        "DATE" => string(try_as!(row, i, NaiveDate)),
        "DATETIME" => string(try_as!(row, i, NaiveDateTime)),
        "TIMESTAMP" => string(try_as!(row, i, DateTime<Utc>)),
        // TIME is a duration in MySQL: it can be negative or longer than a day.
        "TIME" => try_as!(row, i, sqlx::mysql::types::MySqlTime).map(|t| {
            let us = i64::from(t.hours()) * 3_600_000_000
                + i64::from(t.minutes()) * 60_000_000
                + i64::from(t.seconds()) * 1_000_000
                + i64::from(t.microseconds());
            // Not `t.is_negative()`: in sqlx 0.8.6 it returns `is_positive()`.
            Value::String(clock(if t.sign().is_negative() { -us } else { us }))
        }),
        "YEAR" => num(try_as!(row, i, u16)),
        "JSON" => try_as!(row, i, Value),
        "BINARY" | "VARBINARY" | "TINYBLOB" | "BLOB" | "MEDIUMBLOB" | "LONGBLOB" => {
            printable(row.try_get_unchecked::<Option<String>, _>(i).ok().flatten())
                .or_else(|| bytes(try_as!(row, i, Vec<u8>)))
        }
        _ => try_as!(row, i, String).map(Value::String),
    };
    v.or_else(|| printable(row.try_get_unchecked::<Option<String>, _>(i).ok().flatten()))
        .unwrap_or_else(|| Value::String(format!("<{}>", ty.to_lowercase())))
}
