//! Decodes binary result rows from PostgreSQL prepared statements into JSON values.
//!
//! Numbers and booleans become JSON numbers and booleans. `numeric`, dates, and times become
//! strings in PostgreSQL's text form, so no precision is lost. A column type with no decoder
//! is an error that tells the user to cast it to text, never a guessed value.

use crate::common::types::FieldInfo;
use crate::core::errors::DbCoreError;
use serde_json::{Map, Number, Value};
use std::error::Error;
use std::fmt::Write;
use tokio_postgres::types::{FromSql, Kind, Type};
use tokio_postgres::{Column, Row};

type BoxErr = Box<dyn Error + Sync + Send>;

/// PostgreSQL `numeric`, decoded from its binary form into exact decimal text.
pub(crate) struct PgNumeric(pub String);

impl<'a> FromSql<'a> for PgNumeric {
    fn from_sql(_: &Type, raw: &'a [u8]) -> Result<Self, BoxErr> {
        let word = |i: usize| -> Result<u16, BoxErr> {
            raw.get(i..i + 2)
                .map(|b| u16::from_be_bytes([b[0], b[1]]))
                .ok_or_else(|| "truncated numeric value".into())
        };
        let ndigits = word(0)? as usize;
        let weight = word(2)? as i16 as i64;
        let sign = word(4)?;
        let dscale = word(6)? as usize;
        let digits = (0..ndigits)
            .map(|i| word(8 + 2 * i))
            .collect::<Result<Vec<_>, _>>()?;
        let digit = |i: i64| -> u16 {
            usize::try_from(i)
                .ok()
                .and_then(|i| digits.get(i).copied())
                .unwrap_or(0)
        };
        let prefix = match sign {
            0x0000 => "",
            0x4000 => "-",
            0xC000 => return Ok(Self("NaN".into())),
            0xD000 => return Ok(Self("Infinity".into())),
            0xF000 => return Ok(Self("-Infinity".into())),
            other => return Err(format!("invalid numeric sign {other:#x}").into()),
        };
        let mut out = prefix.to_string();
        if weight < 0 {
            out.push('0');
        } else {
            for i in 0..=weight {
                if i == 0 {
                    write!(out, "{}", digit(i))?;
                } else {
                    write!(out, "{:04}", digit(i))?;
                }
            }
        }
        if dscale > 0 {
            let mut frac = String::new();
            let mut i = weight + 1;
            while frac.len() < dscale {
                write!(frac, "{:04}", digit(i))?;
                i += 1;
            }
            frac.truncate(dscale);
            out.push('.');
            out.push_str(&frac);
        }
        Ok(Self(out))
    }

    fn accepts(ty: &Type) -> bool {
        *ty == Type::NUMERIC
    }
}

/// Enum labels arrive as their UTF-8 text in binary format.
struct EnumLabel(String);

impl<'a> FromSql<'a> for EnumLabel {
    fn from_sql(_: &Type, raw: &'a [u8]) -> Result<Self, BoxErr> {
        Ok(Self(std::str::from_utf8(raw)?.to_string()))
    }

    fn accepts(ty: &Type) -> bool {
        matches!(ty.kind(), Kind::Enum(_))
    }
}

fn get<'a, T: FromSql<'a>>(row: &'a Row, idx: usize) -> Result<Option<T>, tokio_postgres::Error> {
    row.try_get::<_, Option<T>>(idx)
}

fn two(n: impl Into<u32>) -> String {
    format!("{:02}", n.into())
}

fn fmt_time(t: time::Time) -> String {
    let mut s = format!("{}:{}:{}", two(t.hour()), two(t.minute()), two(t.second()));
    if t.microsecond() > 0 {
        let frac = format!("{:06}", t.microsecond());
        s.push('.');
        s.push_str(frac.trim_end_matches('0'));
    }
    s
}

fn fmt_date(d: time::Date) -> String {
    format!(
        "{:04}-{}-{}",
        d.year(),
        two(u8::from(d.month())),
        two(d.day())
    )
}

fn decode_value(row: &Row, idx: usize, col: &Column) -> Result<Value, DbCoreError> {
    let ty = col.type_();
    let float = |f: Option<f64>| {
        f.map(|f| Number::from_f64(f).map_or_else(|| Value::String(f.to_string()), Value::Number))
    };
    let decoded = match *ty {
        Type::BOOL => get::<bool>(row, idx).map(|v| v.map(Value::Bool)),
        Type::INT2 => get::<i16>(row, idx).map(|v| v.map(Value::from)),
        Type::INT4 => get::<i32>(row, idx).map(|v| v.map(Value::from)),
        Type::INT8 => get::<i64>(row, idx).map(|v| v.map(Value::from)),
        Type::OID => get::<u32>(row, idx).map(|v| v.map(Value::from)),
        Type::FLOAT4 => get::<f32>(row, idx).map(|v| float(v.map(f64::from))),
        Type::FLOAT8 => get::<f64>(row, idx).map(float),
        Type::NUMERIC => get::<PgNumeric>(row, idx).map(|v| v.map(|n| Value::String(n.0))),
        Type::JSON | Type::JSONB => get::<Value>(row, idx),
        Type::UUID => get::<uuid::Uuid>(row, idx).map(|v| v.map(|u| Value::String(u.to_string()))),
        Type::BYTEA => get::<Vec<u8>>(row, idx).map(|v| {
            v.map(|b| {
                Value::String(b.iter().fold("\\x".to_string(), |mut s, x| {
                    let _ = write!(s, "{x:02x}");
                    s
                }))
            })
        }),
        Type::DATE => get::<time::Date>(row, idx).map(|v| v.map(|d| Value::String(fmt_date(d)))),
        Type::TIME => get::<time::Time>(row, idx).map(|v| v.map(|t| Value::String(fmt_time(t)))),
        Type::TIMESTAMP => get::<time::PrimitiveDateTime>(row, idx).map(|v| {
            v.map(|t| Value::String(format!("{} {}", fmt_date(t.date()), fmt_time(t.time()))))
        }),
        Type::TIMESTAMPTZ => get::<time::OffsetDateTime>(row, idx).map(|v| {
            v.map(|t| {
                let t = t.to_offset(time::UtcOffset::UTC);
                Value::String(format!("{} {}+00", fmt_date(t.date()), fmt_time(t.time())))
            })
        }),
        _ if <&str as FromSql>::accepts(ty) => {
            get::<&str>(row, idx).map(|v| v.map(|s| Value::String(s.to_string())))
        }
        _ if EnumLabel::accepts(ty) => {
            get::<EnumLabel>(row, idx).map(|v| v.map(|e| Value::String(e.0)))
        }
        _ => {
            return Err(DbCoreError::Message(format!(
                "Column \"{name}\" has PostgreSQL type {ty}, which parameterized queries cannot \
                 return yet. Cast it to text in the query, for example {name}::text.",
                name = col.name()
            )))
        }
    };
    decoded
        .map(|v| v.unwrap_or(Value::Null))
        .map_err(|e| DbCoreError::Message(format!("Could not read column \"{}\": {e}", col.name())))
}

/// Converts prepared-statement rows into result fields and JSON row maps.
pub(crate) fn decode_rows(
    columns: &[Column],
    rows: &[Row],
) -> Result<(Vec<FieldInfo>, Vec<Map<String, Value>>), DbCoreError> {
    let fields = columns
        .iter()
        .map(|c| FieldInfo {
            name: c.name().to_string(),
        })
        .collect();
    let decoded = rows
        .iter()
        .map(|row| {
            columns
                .iter()
                .enumerate()
                .map(|(idx, col)| decode_value(row, idx, col).map(|v| (col.name().to_string(), v)))
                .collect::<Result<Map<_, _>, _>>()
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok((fields, decoded))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn numeric(ndigits: u16, weight: i16, sign: u16, dscale: u16, digits: &[u16]) -> String {
        let mut raw = Vec::new();
        for w in [ndigits, weight as u16, sign, dscale].iter().chain(digits) {
            raw.extend_from_slice(&w.to_be_bytes());
        }
        PgNumeric::from_sql(&Type::NUMERIC, &raw).unwrap().0
    }

    #[test]
    fn decodes_numeric_binary_exactly() {
        assert_eq!(numeric(2, 0, 0, 2, &[123, 4500]), "123.45");
        assert_eq!(numeric(1, -1, 0, 4, &[12]), "0.0012");
        assert_eq!(numeric(1, 1, 0, 0, &[1]), "10000");
        assert_eq!(numeric(1, -2, 0x4000, 5, &[1000]), "-0.00001");
        assert_eq!(numeric(0, 0, 0, 0, &[]), "0");
        assert_eq!(numeric(0, 0, 0xC000, 0, &[]), "NaN");
        assert_eq!(numeric(3, 2, 0, 1, &[12, 3456, 7890]), "1234567890.0");
        assert!(PgNumeric::from_sql(&Type::NUMERIC, &[0, 1]).is_err());
    }

    #[test]
    fn formats_times_like_postgres_text_output() {
        let t = time::Time::from_hms_micro(3, 4, 5, 120_000).unwrap();
        assert_eq!(fmt_time(t), "03:04:05.12");
        let d = time::Date::from_calendar_date(2024, time::Month::January, 2).unwrap();
        assert_eq!(fmt_date(d), "2024-01-02");
    }
}
