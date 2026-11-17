//! Parameterized SQL query placeholder extraction (`:param`, `@param`, `$1`) and safe literal binding.

use serde_json::Value;
use std::collections::HashMap;

/// Extracts all unique named (`:param_name`, `@param_name`) and positional (`$1`) parameters
/// outside of string literals and comments, skipping PostgreSQL `::type` casts and `@@system` variables.
pub fn extract_query_parameters(sql: &str) -> Vec<String> {
    let mut params = Vec::new();
    let bytes = sql.as_bytes();
    let len = bytes.len();
    let mut i = 0;
    let mut in_single = false;
    let mut in_double = false;
    let mut in_line_comment = false;
    let mut in_block_comment = false;

    while i < len {
        let c = bytes[i];
        let next = if i + 1 < len {
            Some(bytes[i + 1])
        } else {
            None
        };

        if in_line_comment {
            if c == b'\n' {
                in_line_comment = false;
            }
            i += 1;
            continue;
        }
        if in_block_comment {
            if c == b'*' && next == Some(b'/') {
                in_block_comment = false;
                i += 2;
                continue;
            }
            i += 1;
            continue;
        }

        if !in_single && !in_double {
            if c == b'-' && next == Some(b'-') {
                in_line_comment = true;
                i += 2;
                continue;
            }
            if c == b'/' && next == Some(b'*') {
                in_block_comment = true;
                i += 2;
                continue;
            }
        }

        if c == b'\'' && !in_double {
            in_single = !in_single;
            i += 1;
            continue;
        }
        if c == b'"' && !in_single {
            in_double = !in_double;
            i += 1;
            continue;
        }

        if !in_single && !in_double {
            if c == b':' && next == Some(b':') {
                i += 2;
                continue;
            }
            if c == b'@' && next == Some(b'@') {
                i += 2;
                continue;
            }

            if (c == b':' || c == b'@')
                && next
                    .map(|n| n.is_ascii_alphabetic() || n == b'_')
                    .unwrap_or(false)
            {
                let start = i + 1;
                let mut end = start;
                while end < len && (bytes[end].is_ascii_alphanumeric() || bytes[end] == b'_') {
                    end += 1;
                }
                let name = sql[start..end].to_string();
                if !params.contains(&name) {
                    params.push(name);
                }
                i = end;
                continue;
            }

            if c == b'$' && next.map(|n| n.is_ascii_digit()).unwrap_or(false) {
                let start = i;
                let mut end = start + 1;
                while end < len && bytes[end].is_ascii_digit() {
                    end += 1;
                }
                let name = sql[start..end].to_string();
                if !params.contains(&name) {
                    params.push(name);
                }
                i = end;
                continue;
            }
        }

        i += 1;
    }

    params
}

/// Safely binds named (`:name`, `@name`) and positional (`$1`) parameters into `sql`
/// with proper SQL literal escaping.
pub fn bind_query_parameters(sql: &str, params: &HashMap<String, Value>) -> String {
    if params.is_empty() {
        return sql.to_string();
    }

    let mut out = String::with_capacity(sql.len() + 32);
    let bytes = sql.as_bytes();
    let len = bytes.len();
    let mut i = 0;
    let mut in_single = false;
    let mut in_double = false;

    while i < len {
        let c = bytes[i];
        let next = if i + 1 < len {
            Some(bytes[i + 1])
        } else {
            None
        };

        if c == b'\'' && !in_double {
            in_single = !in_single;
            out.push('\'');
            i += 1;
            continue;
        }
        if c == b'"' && !in_single {
            in_double = !in_double;
            out.push('"');
            i += 1;
            continue;
        }

        if !in_single && !in_double {
            if c == b':' && next == Some(b':') {
                out.push_str("::");
                i += 2;
                continue;
            }
            if c == b'@' && next == Some(b'@') {
                out.push_str("@@");
                i += 2;
                continue;
            }

            if (c == b':' || c == b'@')
                && next
                    .map(|n| n.is_ascii_alphabetic() || n == b'_')
                    .unwrap_or(false)
            {
                let start = i + 1;
                let mut end = start;
                while end < len && (bytes[end].is_ascii_alphanumeric() || bytes[end] == b'_') {
                    end += 1;
                }
                let name = &sql[start..end];
                if let Some(val) = params.get(name).or_else(|| params.get(&sql[i..end])) {
                    out.push_str(&format_sql_literal(val));
                } else {
                    out.push_str(&sql[i..end]);
                }
                i = end;
                continue;
            }

            if c == b'$' && next.map(|n| n.is_ascii_digit()).unwrap_or(false) {
                let start = i;
                let mut end = start + 1;
                while end < len && bytes[end].is_ascii_digit() {
                    end += 1;
                }
                let token = &sql[start..end];
                let num_only = &sql[start + 1..end];
                if let Some(val) = params.get(token).or_else(|| params.get(num_only)) {
                    out.push_str(&format_sql_literal(val));
                } else {
                    out.push_str(token);
                }
                i = end;
                continue;
            }
        }

        out.push(c as char);
        i += 1;
    }

    out
}

/// Formats a JSON `Value` as a safely escaped SQL literal (`NULL`, `TRUE`, `FALSE`, numeric, or single-quoted string).
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
    fn test_extract_and_bind_parameters() {
        let sql = "SELECT * FROM users WHERE username = :username AND role_id = $1 AND active = @active AND createdat::date > '2020-01-01'";
        let extracted = extract_query_parameters(sql);
        assert_eq!(extracted, vec!["username", "$1", "active"]);

        let mut map = HashMap::new();
        map.insert(
            "username".to_string(),
            Value::String("O'Reilly".to_string()),
        );
        map.insert("$1".to_string(), Value::from(42));
        map.insert("active".to_string(), Value::Bool(true));

        let bound = bind_query_parameters(sql, &map);
        assert_eq!(
            bound,
            "SELECT * FROM users WHERE username = 'O''Reilly' AND role_id = 42 AND active = TRUE AND createdat::date > '2020-01-01'"
        );
    }
}
