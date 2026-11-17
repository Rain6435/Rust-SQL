use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExecutionType {
    #[serde(rename = "LISTING")]
    Listing,
    #[serde(rename = "MODIFICATION")]
    Modification,
    #[serde(rename = "INFORMATION")]
    Information,
    #[serde(rename = "UNKNOWN")]
    Unknown,
}

impl ExecutionType {
    pub fn as_str(&self) -> &'static str {
        match self {
            ExecutionType::Listing => "LISTING",
            ExecutionType::Modification => "MODIFICATION",
            ExecutionType::Information => "INFORMATION",
            ExecutionType::Unknown => "UNKNOWN",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IdentifiedStatement {
    pub start: usize,
    pub end: usize,
    pub text: String,
    #[serde(rename = "type")]
    pub statement_type: String,
    pub execution_type: ExecutionType,
    pub parameters: Vec<String>,
}

/// Splits and identifies SQL statements in `query_text`, faithfully replicating `sql-query-identifier`.
pub fn identify(query_text: &str, strict: bool) -> Result<Vec<IdentifiedStatement>, String> {
    let raw_statements = split_sql_statements(query_text);
    let mut results = Vec::new();

    for (start, end, raw_stmt) in raw_statements {
        let stripped = strip_comments(raw_stmt).trim().to_string();
        if stripped.is_empty() {
            continue;
        }
        let (stmt_type, exec_type) = classify_statement(&stripped, strict)?;
        let parameters = extract_parameters(raw_stmt);
        results.push(IdentifiedStatement {
            start,
            end,
            text: raw_stmt.trim().to_string(),
            statement_type: stmt_type,
            execution_type: exec_type,
            parameters,
        });
    }

    Ok(results)
}

fn split_sql_statements(sql: &str) -> Vec<(usize, usize, &str)> {
    let bytes = sql.as_bytes();
    let len = bytes.len();
    let mut statements = Vec::new();
    let mut stmt_start = 0;
    let mut i = 0;

    let mut in_single_quote = false;
    let mut in_double_quote = false;
    let mut in_backtick = false;
    let mut in_bracket = false;
    let mut in_line_comment = false;
    let mut in_block_comment = false;
    let mut dollar_tag: Option<&str> = None;
    let mut paren_depth: usize = 0;
    let mut begin_depth: usize = 0;

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

        if let Some(tag) = dollar_tag {
            if sql[i..].starts_with(tag) {
                dollar_tag = None;
                i += tag.len();
                continue;
            }
            i += 1;
            continue;
        }

        if in_single_quote {
            if c == b'\\' && next.is_some() {
                i += 2;
                continue;
            }
            if c == b'\'' {
                if next == Some(b'\'') {
                    i += 2;
                    continue;
                }
                in_single_quote = false;
            }
            i += 1;
            continue;
        }

        if in_double_quote {
            if c == b'"' {
                if next == Some(b'"') {
                    i += 2;
                    continue;
                }
                in_double_quote = false;
            }
            i += 1;
            continue;
        }

        if in_backtick {
            if c == b'`' {
                if next == Some(b'`') {
                    i += 2;
                    continue;
                }
                in_backtick = false;
            }
            i += 1;
            continue;
        }

        if in_bracket {
            if c == b']' {
                in_bracket = false;
            }
            i += 1;
            continue;
        }

        // Check comment starts
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

        // Check dollar quote start: $$ or $tag$
        if c == b'$' {
            if let Some(tag) = parse_dollar_tag(&sql[i..]) {
                dollar_tag = Some(tag);
                i += tag.len();
                continue;
            }
        }

        if c == b'\'' {
            in_single_quote = true;
            i += 1;
            continue;
        }
        if c == b'"' {
            in_double_quote = true;
            i += 1;
            continue;
        }
        if c == b'`' {
            in_backtick = true;
            i += 1;
            continue;
        }
        if c == b'[' {
            in_bracket = true;
            i += 1;
            continue;
        }
        if c == b'(' {
            paren_depth += 1;
            i += 1;
            continue;
        }
        if c == b')' {
            paren_depth = paren_depth.saturating_sub(1);
            i += 1;
            continue;
        }

        // Track BEGIN ... END blocks (e.g. CREATE PROCEDURE ... BEGIN SELECT ...; END;)
        if c.is_ascii_alphabetic() {
            let prev_is_word =
                i > 0 && (bytes[i - 1].is_ascii_alphanumeric() || bytes[i - 1] == b'_');
            if !prev_is_word {
                let word_end = sql[i..]
                    .find(|ch: char| !ch.is_ascii_alphanumeric() && ch != '_')
                    .map(|off| i + off)
                    .unwrap_or(len);
                let word = &sql[i..word_end];
                if word.eq_ignore_ascii_case("BEGIN") {
                    // Check if followed by `;` or `TRANSACTION` / `WORK` / `DEFERRED` / `IMMEDIATE` / `EXCLUSIVE`
                    let after = sql[word_end..].trim_start();
                    let is_tx = after.starts_with(';')
                        || after.is_empty()
                        || after
                            .split_whitespace()
                            .next()
                            .map(|w| {
                                let w = w.trim_end_matches(';');
                                w.eq_ignore_ascii_case("TRANSACTION")
                                    || w.eq_ignore_ascii_case("WORK")
                                    || w.eq_ignore_ascii_case("DEFERRED")
                                    || w.eq_ignore_ascii_case("IMMEDIATE")
                                    || w.eq_ignore_ascii_case("EXCLUSIVE")
                            })
                            .unwrap_or(false);
                    if !is_tx {
                        begin_depth += 1;
                    }
                } else if word.eq_ignore_ascii_case("END") && begin_depth > 0 {
                    begin_depth = begin_depth.saturating_sub(1);
                }
                i = word_end;
                continue;
            }
        }

        if c == b';' && paren_depth == 0 && begin_depth == 0 {
            let slice = &sql[stmt_start..=i];
            if !slice.trim().is_empty() {
                statements.push((stmt_start, i + 1, slice));
            }
            stmt_start = i + 1;
        }

        i += 1;
    }

    if stmt_start < len {
        let slice = &sql[stmt_start..len];
        if !slice.trim().is_empty() {
            statements.push((stmt_start, len, slice));
        }
    }

    statements
}

fn parse_dollar_tag(s: &str) -> Option<&str> {
    let bytes = s.as_bytes();
    if bytes.first() != Some(&b'$') {
        return None;
    }
    for (idx, &b) in bytes[1..].iter().enumerate() {
        if b == b'$' {
            return Some(&s[..=idx + 1]);
        }
        if !(b.is_ascii_alphanumeric() || b == b'_') {
            return None;
        }
    }
    None
}

fn strip_comments(sql: &str) -> String {
    let mut out = String::with_capacity(sql.len());
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

        if !in_single && !in_double {
            if c == b'-' && next == Some(b'-') {
                i += 2;
                while i < len && bytes[i] != b'\n' {
                    i += 1;
                }
                continue;
            }
            if c == b'/' && next == Some(b'*') {
                i += 2;
                while i + 1 < len && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                    i += 1;
                }
                i = (i + 2).min(len);
                out.push(' ');
                continue;
            }
        }

        if c == b'\'' && !in_double {
            in_single = !in_single;
        } else if c == b'"' && !in_single {
            in_double = !in_double;
        }

        out.push(c as char);
        i += 1;
    }

    out
}

fn classify_statement(sql: &str, strict: bool) -> Result<(String, ExecutionType), String> {
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
        "SET" | "USE" | "ALTER" if first != "ALTER" => {
            Ok((first.to_string(), ExecutionType::Modification))
        }
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

fn extract_parameters(sql: &str) -> Vec<String> {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_identify_multiple_statements() {
        let sql = "
            SELECT * FROM users WHERE name = 'a;b';
            INSERT INTO users (name) VALUES ('max');
            UPDATE users SET name = 'dev' WHERE id = 1;
            DELETE FROM users WHERE id = 1;
            CREATE DATABASE test_db;
            DROP DATABASE test_db;
            CREATE TABLE t1 (id INT);
            DROP TABLE t1;
        ";
        let res = identify(sql, true).unwrap();
        let types: Vec<&str> = res.iter().map(|s| s.statement_type.as_str()).collect();
        assert_eq!(
            types,
            vec![
                "SELECT",
                "INSERT",
                "UPDATE",
                "DELETE",
                "CREATE_DATABASE",
                "DROP_DATABASE",
                "CREATE_TABLE",
                "DROP_TABLE",
            ]
        );
    }

    #[test]
    fn test_identify_comments_only() {
        let res = identify("-- my comment", true).unwrap();
        assert!(res.is_empty());
    }
}
