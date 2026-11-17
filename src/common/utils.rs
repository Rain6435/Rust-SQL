//! String formatting, regular expression escaping, version comparison, and value conversion utilities.

use serde_json::{Map, Value};

/// Capitalizes the first character of `s` and lowercases the remainder.
pub fn titlize(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        None => String::new(),
        Some(first) => first.to_uppercase().collect::<String>() + &chars.as_str().to_lowercase(),
    }
}

/// Escapes regular expression metacharacters in `s` so it can be safely embedded in a regex pattern.
pub fn escape_regexp_string(s: &str) -> String {
    let special = [
        '\\', '^', '$', '*', '+', '?', '.', '(', ')', '|', '{', '}', '[', ']',
    ];
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if special.contains(&c) {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// Converts a JSON `Value` from a database row into its display and export string representation.
pub fn value_to_string(value: &Value) -> String {
    match value {
        Value::Null => "NULL".to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        Value::String(s) => s.clone(),
        Value::Array(arr) => {
            if !arr.is_empty()
                && arr
                    .iter()
                    .all(|v| v.as_u64().map(|n| n <= 255).unwrap_or(false))
            {
                let bytes: Vec<u8> = arr.iter().map(|v| v.as_u64().unwrap() as u8).collect();
                if bytes.len() == 1 {
                    return bytes[0].to_string();
                }
                if let Ok(utf8) = String::from_utf8(bytes) {
                    return utf8;
                }
            }
            serde_json::to_string(value).unwrap_or_default()
        }
        Value::Object(obj) => {
            if obj.get("type").and_then(|v| v.as_str()) == Some("Buffer") {
                if let Some(Value::Array(data)) = obj.get("data") {
                    let bytes: Vec<u8> = data
                        .iter()
                        .filter_map(|v| v.as_u64().map(|n| n as u8))
                        .collect();
                    if bytes.len() == 1 {
                        return bytes[0].to_string();
                    }
                    return String::from_utf8_lossy(&bytes).into_owned();
                }
            }
            serde_json::to_string(value).unwrap_or_default()
        }
    }
}

/// Converts every cell in a single row map into a `Value::String`.
pub fn row_values_to_string(row: &Map<String, Value>) -> Map<String, Value> {
    let mut parsed = Map::new();
    for (k, v) in row {
        parsed.insert(k.clone(), Value::String(value_to_string(v)));
    }
    parsed
}

/// Converts every cell across a slice of rows into `Value::String` representations.
pub fn rows_values_to_string(rows: &[Map<String, Value>]) -> Vec<Map<String, Value>> {
    rows.iter().map(row_values_to_string).collect()
}

/// Compares two dot-separated version strings component by component up to the shorter prefix length.
///
/// Returns `1` if `a > b`, `-1` if `a < b` (or if a segment is non-numeric), and `0` if equal.
pub fn version_compare(a: &str, b: &str) -> i32 {
    let a_parts: Vec<&str> = a.split('.').collect();
    let b_parts: Vec<&str> = b.split('.').collect();
    let len = std::cmp::min(a_parts.len(), b_parts.len());

    for i in 0..len {
        let a_num = a_parts[i].parse::<i64>();
        let b_num = b_parts[i].parse::<i64>();
        match (a_num, b_num) {
            (Ok(an), Ok(bn)) => {
                if an > bn {
                    return 1;
                }
                if an < bn {
                    return -1;
                }
            }
            _ => return -1,
        }
    }

    0
}

/// Trims leading and trailing newlines from `sql` and ensures it ends with a semicolon (`;`).
pub fn append_semi_colon(sql: &str) -> String {
    let trimmed = sql.trim_matches('\n');
    if trimmed.ends_with(';') {
        trimmed.to_string()
    } else {
        format!("{trimmed};")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_titlize() {
        assert_eq!(titlize("table"), "Table");
        assert_eq!(titlize("VIEW"), "View");
        assert_eq!(titlize("fUnCtIoN"), "Function");
        assert_eq!(titlize(""), "");
    }

    #[test]
    fn test_escape_regexp_string() {
        assert_eq!(escape_regexp_string("a.b*c?"), r"a\.b\*c\?");
        assert_eq!(escape_regexp_string("[test](1)"), r"\[test\]\(1\)");
    }

    #[test]
    fn test_version_compare() {
        let cases = [
            ("8.0.2", "8.0.1", 1),
            ("8.0.2", "8.0.3", -1),
            ("8.0.2.", "8.1", -1),
            ("8.0.2", "8", 0),
            ("8.0", "8", 0),
            ("8", "8", 0),
            ("8", "8.0.2", 0),
            ("8", "8.0", 0),
            ("8.0.2", "12.3", -1),
            ("12.3", "8", 1),
            ("12", "8", 1),
            ("8", "12", -1),
        ];
        for (a, b, expected) in cases {
            assert_eq!(version_compare(a, b), expected);
        }
    }

    #[test]
    fn test_append_semi_colon() {
        let inputs = ["test", "test;", "\ntest", "test\n", "\ntest\n", "\ntest;\n"];
        for input in inputs {
            assert_eq!(append_semi_colon(input), "test;");
        }
    }
}
