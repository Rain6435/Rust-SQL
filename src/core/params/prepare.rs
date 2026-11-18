//! Rewrites a statement's placeholders into the driver's native positional form for
//! prepared-statement execution. Values are never written into the SQL text.

use crate::core::errors::DbCoreError;
use crate::core::params::{scan_placeholders, ParamValue, Placeholder, QueryParams};

/// The positional placeholder syntax a driver accepts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaceholderStyle {
    /// SQLite numbered parameters: `?1`, `?2`, ...
    Question,
    /// PostgreSQL parameters: `$1`, `$2`, ...
    Dollar,
}

/// A statement ready for a driver's prepared-statement API.
#[derive(Debug, Clone, PartialEq)]
pub struct PreparedSql {
    /// SQL with every placeholder replaced by a numbered driver placeholder.
    pub sql: String,
    /// Values for `?1`/`$1`, `?2`/`$2`, ... in order. A name used twice shares one slot.
    pub values: Vec<ParamValue>,
}

/// Finds the value for a placeholder: by name, by the token as written, or (for `$N`) by `N`.
pub(crate) fn lookup<'a>(
    params: &'a QueryParams,
    p: &Placeholder,
    sql: &str,
) -> Option<&'a ParamValue> {
    params
        .get(&p.name)
        .or_else(|| params.get(p.token(sql)))
        .or_else(|| p.name.strip_prefix('$').and_then(|n| params.get(n)))
}

/// Rewrites the placeholders of one statement into `style` and collects their values in order.
///
/// Every placeholder must have a value. A missing one is an error that names it, rather than
/// being sent to the server as text or bound as `NULL`.
pub fn prepare_statement(
    sql: &str,
    params: &QueryParams,
    style: PlaceholderStyle,
) -> Result<PreparedSql, DbCoreError> {
    let mut out = String::with_capacity(sql.len());
    let mut names: Vec<&str> = Vec::new();
    let mut values = Vec::new();
    let mut last = 0;

    for p in scan_placeholders(sql) {
        let value = lookup(params, &p, sql).ok_or_else(|| {
            DbCoreError::Message(format!(
                "No value supplied for query parameter {}",
                p.token(sql)
            ))
        })?;
        let key = p.name.trim_start_matches('$');
        let slot = match names.iter().position(|n| *n == key) {
            Some(idx) => idx + 1,
            None => {
                names.push(&sql[p.end - key.len()..p.end]);
                values.push(value.clone());
                values.len()
            }
        };
        out.push_str(&sql[last..p.start]);
        out.push(match style {
            PlaceholderStyle::Question => '?',
            PlaceholderStyle::Dollar => '$',
        });
        out.push_str(&slot.to_string());
        last = p.end;
    }
    out.push_str(&sql[last..]);
    Ok(PreparedSql { sql: out, values })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params(pairs: &[(&str, ParamValue)]) -> QueryParams {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.clone()))
            .collect()
    }

    #[test]
    fn rewrites_named_and_positional_placeholders() {
        let p = params(&[
            ("name", ParamValue::Text("x".into())),
            ("$2", ParamValue::Int(7)),
        ]);
        let out = prepare_statement(
            "SELECT :name, @name, $2, ':name' -- :name",
            &p,
            PlaceholderStyle::Dollar,
        )
        .unwrap();
        assert_eq!(out.sql, "SELECT $1, $1, $2, ':name' -- :name");
        assert_eq!(
            out.values,
            vec![ParamValue::Text("x".into()), ParamValue::Int(7)]
        );
    }

    #[test]
    fn values_are_never_written_into_the_sql() {
        let evil = "x'); DROP TABLE t; --\\'";
        let p = params(&[("v", ParamValue::Text(evil.into()))]);
        let out = prepare_statement("SELECT :v", &p, PlaceholderStyle::Question).unwrap();
        assert_eq!(out.sql, "SELECT ?1");
        assert!(!out.sql.contains("DROP"));
    }

    #[test]
    fn missing_values_are_errors_naming_the_placeholder() {
        let err = prepare_statement("SELECT :a", &QueryParams::new(), PlaceholderStyle::Question)
            .unwrap_err()
            .to_string();
        assert!(err.contains(":a"), "{err}");
    }

    #[test]
    fn dollar_placeholders_accept_bare_numbers() {
        let p = params(&[("1", ParamValue::Bool(true))]);
        let out = prepare_statement("SELECT $1", &p, PlaceholderStyle::Question).unwrap();
        assert_eq!(out.sql, "SELECT ?1");
        assert_eq!(out.values, vec![ParamValue::Bool(true)]);
    }
}
