//! Display-only parameter substitution for history and previews.
//!
//! The output of this module is **display only, not executed**. Queries run through the
//! driver's prepared-statement API ([`super::prepare`]); nothing here is sent to a server.

use crate::core::params::prepare::lookup;
use crate::core::params::{scan_placeholders, ParamValue, QueryParams};
use serde_json::Value;
use std::collections::HashMap;

/// First line of every [`bind_for_display`] result.
pub const DISPLAY_ONLY_LABEL: &str = "-- display only, not executed";

/// Whether `adapter_key`'s string literals treat backslash as an escape character.
fn backslash_escapes(adapter_key: &str) -> bool {
    matches!(adapter_key, "mysql" | "mariadb")
}

/// Formats a typed value as a literal for `adapter_key`, for display only.
///
/// MySQL and MariaDB escape backslashes as well as quotes, so a value such as `\'` cannot end
/// the literal early. SQL Server shows booleans as `1` / `0`.
pub fn display_literal(value: &ParamValue, adapter_key: &str) -> String {
    match value {
        ParamValue::Null => "NULL".to_string(),
        ParamValue::Bool(b) if adapter_key == "sqlserver" => u8::from(*b).to_string(),
        ParamValue::Bool(b) => if *b { "TRUE" } else { "FALSE" }.to_string(),
        ParamValue::Int(i) => i.to_string(),
        ParamValue::Float(f) if f.is_finite() => f.to_string(),
        ParamValue::Float(f) => format!("'{f}'"),
        ParamValue::Text(s) => {
            let s = if backslash_escapes(adapter_key) {
                s.replace('\\', "\\\\")
            } else {
                s.clone()
            };
            format!("'{}'", s.replace('\'', "''"))
        }
    }
}

/// Substitutes `params` into `sql` for `adapter_key`, skipping literals and comments exactly
/// as execution does. Placeholders without a value are left as written.
fn substitute(sql: &str, params: &QueryParams, adapter_key: &str) -> String {
    let mut out = String::with_capacity(sql.len() + 32);
    let mut last = 0;
    for p in scan_placeholders(sql) {
        out.push_str(&sql[last..p.start]);
        match lookup(params, &p, sql) {
            Some(v) => out.push_str(&display_literal(v, adapter_key)),
            None => out.push_str(p.token(sql)),
        }
        last = p.end;
    }
    out.push_str(&sql[last..]);
    out
}

/// Returns `sql` with `params` substituted, prefixed with [`DISPLAY_ONLY_LABEL`].
///
/// Use this for history and previews. Never execute the result.
pub fn bind_for_display(sql: &str, params: &QueryParams, adapter_key: &str) -> String {
    format!(
        "{DISPLAY_ONLY_LABEL}\n{}",
        substitute(sql, params, adapter_key)
    )
}

/// Display only, not executed: substitutes named (`:name`, `@name`) and positional (`$1`)
/// parameters into `sql` as generic SQL literals.
///
/// JSON strings stay quoted text even when they look numeric. Execution never uses this;
/// see [`super::prepare_statement`].
pub fn bind_query_parameters(sql: &str, params: &HashMap<String, Value>) -> String {
    let typed: QueryParams = params
        .iter()
        .map(|(k, v)| {
            let value =
                ParamValue::from_json(v).unwrap_or_else(|_| ParamValue::Text(v.to_string()));
            (k.clone(), value)
        })
        .collect();
    substitute(sql, &typed, "generic")
}

/// Formats a JSON `Value` as a safely escaped SQL literal (`NULL`, `TRUE`, `FALSE`, numeric, or single-quoted string).
///
/// Used by result export, where values arrive as text from some drivers; it is not used for
/// query parameters.
pub fn format_sql_literal(val: &Value) -> String {
    match val {
        Value::Null => "NULL".to_string(),
        Value::Bool(b) => if *b { "TRUE" } else { "FALSE" }.to_string(),
        Value::Number(n) => n.to_string(),
        Value::String(s) => {
            if let Ok(n) = s.parse::<i64>() {
                return n.to_string();
            }
            if let Ok(f) = s.parse::<f64>() {
                if f.is_finite() {
                    return f.to_string();
                }
            }
            format!("'{}'", s.replace('\'', "''"))
        }
        other => format!("'{}'", other.to_string().replace('\'', "''")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn binds_named_and_positional_values_for_display() {
        let sql = "SELECT * FROM users WHERE username = :username AND role_id = $1 AND active = @active AND createdat::date > '2020-01-01'";
        let mut map = HashMap::new();
        map.insert("username".to_string(), Value::from("O'Reilly"));
        map.insert("$1".to_string(), Value::from(42));
        map.insert("active".to_string(), Value::Bool(true));
        assert_eq!(
            bind_query_parameters(sql, &map),
            "SELECT * FROM users WHERE username = 'O''Reilly' AND role_id = 42 AND active = TRUE AND createdat::date > '2020-01-01'"
        );
    }

    #[test]
    fn placeholders_in_comments_are_not_substituted() {
        let mut map = HashMap::new();
        map.insert("id".to_string(), Value::from(1));
        assert_eq!(
            bind_query_parameters("SELECT :id -- :id\n/* :id */", &map),
            "SELECT 1 -- :id\n/* :id */"
        );
    }

    #[test]
    fn numeric_looking_strings_stay_text() {
        let mut map = HashMap::new();
        map.insert("zip".to_string(), Value::from("02134"));
        assert_eq!(bind_query_parameters("SELECT :zip", &map), "SELECT '02134'");
    }

    #[test]
    fn mysql_display_escapes_backslash_quote() {
        let v = ParamValue::Text("a\\' OR 1=1 -- ".into());
        assert_eq!(display_literal(&v, "mysql"), "'a\\\\'' OR 1=1 -- '");
        assert_eq!(display_literal(&v, "mariadb"), "'a\\\\'' OR 1=1 -- '");
        assert_eq!(display_literal(&v, "postgresql"), "'a\\'' OR 1=1 -- '");
    }

    #[test]
    fn display_output_is_labeled() {
        let p: QueryParams = [("a".to_string(), ParamValue::Int(1))]
            .into_iter()
            .collect();
        let shown = bind_for_display("SELECT :a", &p, "sqlite");
        assert_eq!(shown, format!("{DISPLAY_ONLY_LABEL}\nSELECT 1"));
    }
}
