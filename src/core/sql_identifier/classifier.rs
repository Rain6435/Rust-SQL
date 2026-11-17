//! SQL statement type classifier and positional parameter extractor.

use crate::core::sql_identifier::ExecutionType;

/// Classifies a comment-stripped SQL statement into `(statement_type, ExecutionType)`.
pub fn classify_statement(sql: &str, strict: bool) -> Result<(String, ExecutionType), String> {
    let tokens: Vec<String> = sql
        .split(|c: char| c.is_whitespace() || c == '(' || c == ';')
        .filter(|s| !s.is_empty())
        .take(5)
        .map(|s| s.to_uppercase())
        .collect();

    let Some(first) = tokens.first().map(|s| s.as_str()) else {
        return Ok(("UNKNOWN".to_string(), ExecutionType::Unknown));
    };

    let second = tokens.get(1).map(|s| s.as_str()).unwrap_or("");
    let third = tokens.get(2).map(|s| s.as_str()).unwrap_or("");
    let fourth = tokens.get(3).map(|s| s.as_str()).unwrap_or("");

    let obj = if second == "OR" && third == "REPLACE" {
        fourth
    } else if second == "TEMP" || second == "TEMPORARY" || second == "UNIQUE" {
        third
    } else {
        second
    };

    match first {
        "SELECT" | "WITH" | "VALUES" => Ok(("SELECT".to_string(), ExecutionType::Listing)),
        "INSERT" => Ok(("INSERT".to_string(), ExecutionType::Modification)),
        "UPDATE" => Ok(("UPDATE".to_string(), ExecutionType::Modification)),
        "DELETE" => Ok(("DELETE".to_string(), ExecutionType::Modification)),
        "TRUNCATE" => Ok(("TRUNCATE".to_string(), ExecutionType::Modification)),
        "EXPLAIN" => Ok(("EXPLAIN".to_string(), ExecutionType::Information)),
        "SHOW" => Ok(("SHOW".to_string(), ExecutionType::Listing)),
        "PRAGMA" => Ok(("PRAGMA".to_string(), ExecutionType::Listing)),
        "EXEC" | "EXECUTE" | "CALL" => Ok(("EXEC".to_string(), ExecutionType::Listing)),
        "BEGIN" | "DECLARE" => Ok(("ANON_BLOCK".to_string(), ExecutionType::Modification)),
        "SET" | "USE" => Ok((first.to_string(), ExecutionType::Modification)),
        "CREATE" => match obj {
            "DATABASE" | "KEYSPACE" => {
                Ok(("CREATE_DATABASE".to_string(), ExecutionType::Modification))
            }
            "SCHEMA" => Ok(("CREATE_SCHEMA".to_string(), ExecutionType::Modification)),
            "TABLE" => Ok(("CREATE_TABLE".to_string(), ExecutionType::Modification)),
            "VIEW" => Ok(("CREATE_VIEW".to_string(), ExecutionType::Modification)),
            "TRIGGER" => Ok(("CREATE_TRIGGER".to_string(), ExecutionType::Modification)),
            "FUNCTION" => Ok(("CREATE_FUNCTION".to_string(), ExecutionType::Modification)),
            "PROCEDURE" => Ok(("CREATE_PROCEDURE".to_string(), ExecutionType::Modification)),
            "INDEX" => Ok(("CREATE_INDEX".to_string(), ExecutionType::Modification)),
            _ => Ok(("CREATE_TABLE".to_string(), ExecutionType::Modification)),
        },
        "DROP" => match obj {
            "DATABASE" | "KEYSPACE" => {
                Ok(("DROP_DATABASE".to_string(), ExecutionType::Modification))
            }
            "SCHEMA" => Ok(("DROP_SCHEMA".to_string(), ExecutionType::Modification)),
            "TABLE" => Ok(("DROP_TABLE".to_string(), ExecutionType::Modification)),
            "VIEW" => Ok(("DROP_VIEW".to_string(), ExecutionType::Modification)),
            "TRIGGER" => Ok(("DROP_TRIGGER".to_string(), ExecutionType::Modification)),
            "FUNCTION" => Ok(("DROP_FUNCTION".to_string(), ExecutionType::Modification)),
            "PROCEDURE" => Ok(("DROP_PROCEDURE".to_string(), ExecutionType::Modification)),
            "INDEX" => Ok(("DROP_INDEX".to_string(), ExecutionType::Modification)),
            _ => Ok(("DROP_TABLE".to_string(), ExecutionType::Modification)),
        },
        "ALTER" => match obj {
            "DATABASE" => Ok(("ALTER_DATABASE".to_string(), ExecutionType::Modification)),
            "SCHEMA" => Ok(("ALTER_SCHEMA".to_string(), ExecutionType::Modification)),
            "TABLE" => Ok(("ALTER_TABLE".to_string(), ExecutionType::Modification)),
            "VIEW" => Ok(("ALTER_VIEW".to_string(), ExecutionType::Modification)),
            "TRIGGER" => Ok(("ALTER_TRIGGER".to_string(), ExecutionType::Modification)),
            "FUNCTION" => Ok(("ALTER_FUNCTION".to_string(), ExecutionType::Modification)),
            "PROCEDURE" => Ok(("ALTER_PROCEDURE".to_string(), ExecutionType::Modification)),
            "INDEX" => Ok(("ALTER_INDEX".to_string(), ExecutionType::Modification)),
            _ => Ok(("ALTER_TABLE".to_string(), ExecutionType::Modification)),
        },
        _ => {
            if strict {
                Err(format!("Invalid statement parser \"{first}\""))
            } else {
                Ok(("UNKNOWN".to_string(), ExecutionType::Unknown))
            }
        }
    }
}

/// Extracts positional parameter tokens (`?` and `$1`, `$2`, ...) from a SQL statement.
pub fn extract_parameters(sql: &str) -> Vec<String> {
    let mut params = Vec::new();
    let bytes = sql.as_bytes();
    let len = bytes.len();
    let mut i = 0;
    let mut in_single = false;
    let mut in_double = false;

    while i < len {
        let c = bytes[i];
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
            if c == b'?' {
                params.push("?".to_string());
            } else if c == b'$' && i + 1 < len && bytes[i + 1].is_ascii_digit() {
                let start = i;
                i += 1;
                while i < len && bytes[i].is_ascii_digit() {
                    i += 1;
                }
                params.push(sql[start..i].to_string());
                continue;
            }
        }
        i += 1;
    }

    params
}
