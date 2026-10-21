use super::{bytes, f32_to_f64, float, num, string};
use chrono::{DateTime, FixedOffset, NaiveDate, NaiveDateTime, NaiveTime, SubsecRound};
use serde_json::Value;
use tiberius::{ColumnData, Row};

pub(crate) fn ms_cell(row: &Row, i: usize) -> Value {
    let Some((_, data)) = row.cells().nth(i) else { return Value::Null };
    let v = match data {
        ColumnData::U8(v) => num(*v),
        ColumnData::I16(v) => num(*v),
        ColumnData::I32(v) => num(*v),
        ColumnData::I64(v) => num(*v),
        ColumnData::F32(v) => float(v.map(f32_to_f64)),
        ColumnData::F64(v) => float(*v),
        ColumnData::Bit(v) => v.map(Value::Bool),
        ColumnData::String(v) => string(v.as_deref()),
        ColumnData::Guid(v) => string(v.as_ref()),
        ColumnData::Binary(v) => bytes(v.as_ref().map(|b| b.to_vec())),
        // Exact decimal text, no float rounding.
        ColumnData::Numeric(v) => string(v.as_ref()),
        ColumnData::Xml(v) => string(v.as_deref()),
        // `datetime` counts in 1/300 s; SQL Server's own tools show it rounded to milliseconds.
        ColumnData::DateTime(_) => string(row.try_get::<NaiveDateTime, _>(i).ok().flatten().map(|t| t.round_subsecs(3))),
        ColumnData::SmallDateTime(_) | ColumnData::DateTime2(_) => {
            string(row.try_get::<NaiveDateTime, _>(i).ok().flatten())
        }
        ColumnData::Date(_) => string(row.try_get::<NaiveDate, _>(i).ok().flatten()),
        ColumnData::Time(_) => string(row.try_get::<NaiveTime, _>(i).ok().flatten()),
        ColumnData::DateTimeOffset(_) => string(row.try_get::<DateTime<FixedOffset>, _>(i).ok().flatten()),
    };
    v.unwrap_or(Value::Null)
}
