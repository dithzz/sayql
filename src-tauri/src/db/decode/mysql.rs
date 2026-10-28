use super::{bytes, clock, f32_to_f64, float, num, printable, string};
use serde_json::Value;
use sqlx::mysql::MySqlRow;
use sqlx::{Row, TypeInfo, ValueRef};

pub(crate) fn my_cell(row: &MySqlRow, i: usize) -> Value {
    let Ok(raw) = row.try_get_raw(i) else {
        return Value::Null;
    };
    let ty = raw.type_info().name().to_string();
    let is_date = matches!(ty.as_str(), "DATE" | "DATETIME" | "TIMESTAMP");
    if raw.is_null() {
        // sqlx reads MySQL's zero dates ('0000-00-00') as NULL, but they are stored values.
        return match row.try_get_unchecked::<&[u8], _>(i) {
            Ok(b) if is_date => binary_datetime(b, ty != "DATE").map_or(Value::Null, Value::String),
            _ => Value::Null,
        };
    }
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
        "DECIMAL" => row
            .try_get_unchecked::<Option<String>, _>(i)
            .ok()
            .flatten()
            .map(Value::String),
        "DATE" => string(try_as!(row, i, NaiveDate)),
        "DATETIME" => string(try_as!(row, i, NaiveDateTime)),
        "TIMESTAMP" => string(try_as!(row, i, DateTime<Utc>)),
        "GEOMETRY" => row
            .try_get_unchecked::<&[u8], _>(i)
            .ok()
            .and_then(geometry)
            .map(Value::String),
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
    // Dates with a zero month or day ('2024-00-15') are valid in MySQL but not in chrono.
    let v = if is_date {
        v.or_else(|| {
            row.try_get_unchecked::<&[u8], _>(i)
                .ok()
                .and_then(|b| binary_datetime(b, ty != "DATE"))
                .map(Value::String)
        })
    } else {
        v
    };
    v.or_else(|| printable(row.try_get_unchecked::<Option<String>, _>(i).ok().flatten()))
        .unwrap_or_else(|| Value::String(format!("<{}>", ty.to_lowercase())))
}

/// A binary-protocol DATE/DATETIME/TIMESTAMP as MySQL writes it, zero parts included:
/// a length byte, then year (2 bytes), month, day, and optionally hour, minute, second and
/// microseconds (4 bytes).
fn binary_datetime(b: &[u8], with_time: bool) -> Option<String> {
    let (&len, rest) = b.split_first()?;
    let p = |i: usize| rest.get(i).copied().unwrap_or(0);
    if rest.len() < usize::from(len) {
        return None;
    }
    let year = u16::from_le_bytes([p(0), p(1)]);
    let mut out = format!("{year:04}-{:02}-{:02}", p(2), p(3));
    if with_time {
        out.push_str(&format!(" {:02}:{:02}:{:02}", p(4), p(5), p(6)));
        let micros = u32::from_le_bytes([p(7), p(8), p(9), p(10)]);
        if micros > 0 {
            out.push_str(&format!(".{micros:06}"));
        }
    }
    Some(out)
}

/// MySQL stores geometry as a 4-byte SRID and then WKB; shown as WKT, the way
/// `ST_AsText` writes it, with `SRID=n;` in front when it isn't 0.
fn geometry(b: &[u8]) -> Option<String> {
    let srid = u32::from_le_bytes(b.get(..4)?.try_into().ok()?);
    let mut wkb = Wkb(&b[4..]);
    let wkt = wkb.geometry(0)?;
    Some(if srid == 0 {
        wkt
    } else {
        format!("SRID={srid};{wkt}")
    })
}

struct Wkb<'a>(&'a [u8]);

impl Wkb<'_> {
    fn take<const N: usize>(&mut self) -> Option<[u8; N]> {
        let (head, rest) = self.0.split_at_checked(N)?;
        self.0 = rest;
        head.try_into().ok()
    }
    fn u32(&mut self, le: bool) -> Option<u32> {
        let b = self.take::<4>()?;
        Some(if le {
            u32::from_le_bytes(b)
        } else {
            u32::from_be_bytes(b)
        })
    }
    fn point(&mut self, le: bool) -> Option<String> {
        let mut xy = [0f64; 2];
        for v in &mut xy {
            let b = self.take::<8>()?;
            *v = if le {
                f64::from_le_bytes(b)
            } else {
                f64::from_be_bytes(b)
            };
        }
        Some(format!("{} {}", xy[0], xy[1]))
    }
    fn list(&mut self, le: bool, item: impl Fn(&mut Self) -> Option<String>) -> Option<String> {
        let n = self.u32(le)?;
        let items: Option<Vec<String>> = (0..n.min(1_000_000)).map(|_| item(self)).collect();
        Some(items?.join(","))
    }
    /// One WKB geometry as WKT; collections nest at most a few levels.
    fn geometry(&mut self, depth: u8) -> Option<String> {
        let le = self.take::<1>()?[0] == 1;
        let kind = self.u32(le)?;
        let ring = |w: &mut Self| Some(format!("({})", w.list(le, |w| w.point(le))?));
        let nested = |w: &mut Self| {
            if depth < 4 {
                w.geometry(depth + 1)
            } else {
                None
            }
        };
        // Members of MULTI* collections are written without their own type name.
        let bare = |s: String| s.find('(').map(|i| s[i..].to_string());
        Some(match kind {
            1 => format!("POINT({})", self.point(le)?),
            2 => format!("LINESTRING({})", self.list(le, |w| w.point(le))?),
            3 => format!("POLYGON({})", self.list(le, ring)?),
            4 => format!("MULTIPOINT({})", self.list(le, |w| bare(nested(w)?))?),
            5 => format!("MULTILINESTRING({})", self.list(le, |w| bare(nested(w)?))?),
            6 => format!("MULTIPOLYGON({})", self.list(le, |w| bare(nested(w)?))?),
            7 => format!("GEOMETRYCOLLECTION({})", self.list(le, nested)?),
            _ => return None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_dates_keep_their_zeros() {
        assert_eq!(binary_datetime(&[0], false).as_deref(), Some("0000-00-00"));
        assert_eq!(
            binary_datetime(&[0], true).as_deref(),
            Some("0000-00-00 00:00:00")
        );
        assert_eq!(
            binary_datetime(&[4, 0xe8, 0x07, 0, 15], false).as_deref(),
            Some("2024-00-15")
        );
        assert_eq!(
            binary_datetime(&[7, 0xe8, 0x07, 2, 0, 10, 0, 5], true).as_deref(),
            Some("2024-02-00 10:00:05")
        );
        assert_eq!(
            binary_datetime(&[11, 0xe8, 0x07, 2, 0, 10, 0, 5, 0x40, 0xe2, 0x01, 0], true)
                .as_deref(),
            Some("2024-02-00 10:00:05.123456")
        );
        assert_eq!(binary_datetime(&[7, 0xe8], true), None);
    }

    fn wkb_point(x: f64, y: f64) -> Vec<u8> {
        let mut b = vec![1, 1, 0, 0, 0];
        b.extend(x.to_le_bytes());
        b.extend(y.to_le_bytes());
        b
    }

    #[test]
    fn geometry_is_shown_as_wkt() {
        let mut point = vec![0, 0, 0, 0];
        point.extend(wkb_point(1.5, -2.0));
        assert_eq!(geometry(&point).as_deref(), Some("POINT(1.5 -2)"));
        let mut multi = vec![0xe6, 0x10, 0, 0, 1, 4, 0, 0, 0, 2, 0, 0, 0];
        multi.extend(wkb_point(1.0, 2.0));
        multi.extend(wkb_point(3.0, 4.0));
        assert_eq!(
            geometry(&multi).as_deref(),
            Some("SRID=4326;MULTIPOINT((1 2),(3 4))")
        );
        assert_eq!(geometry(&[0, 0, 0, 0, 1, 1, 0]), None);
    }
}
