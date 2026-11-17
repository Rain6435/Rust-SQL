//! SQL statement type classifier and positional parameter extractor.

use crate::core::sql_identifier::{ExecutionType, StatementKind};

/// Classifies a comment-stripped SQL statement into `(StatementKind, ExecutionType)`.
///
/// This is a fast keyword classifier used to label results and route execution. It is not a
/// safety check: [`crate::core::safety`] parses statements into an AST for that.
pub fn classify_statement(
    sql: &str,
    strict: bool,
) -> Result<(StatementKind, ExecutionType), String> {
    use ExecutionType::{Information, Listing, Modification, Unknown as Unk};
    use StatementKind as K;

    let tokens: Vec<String> = sql
        .split(|c: char| c.is_whitespace() || c == '(' || c == ';')
        .filter(|s| !s.is_empty())
        .take(5)
        .map(|s| s.to_uppercase())
        .collect();

    let Some(first) = tokens.first().map(|s| s.as_str()) else {
        return Ok((K::Unknown, Unk));
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

    let classified = match first {
        "SELECT" | "WITH" | "VALUES" => (K::Select, Listing),
        "INSERT" => (K::Insert, Modification),
        "REPLACE" => (K::Replace, Modification),
        "UPDATE" => (K::Update, Modification),
        "DELETE" => (K::Delete, Modification),
        "MERGE" => (K::Merge, Modification),
        "TRUNCATE" => (K::Truncate, Modification),
        "COPY" => (K::Copy, Modification),
        "GRANT" => (K::Grant, Modification),
        "REVOKE" => (K::Revoke, Modification),
        "EXPLAIN" => (K::Explain, Information),
        "SHOW" => (K::Show, Listing),
        "PRAGMA" => (K::Pragma, Listing),
        "EXEC" | "EXECUTE" | "CALL" => (K::Exec, Listing),
        "BEGIN" | "DECLARE" => (K::AnonBlock, Modification),
        "SET" => (K::Set, Modification),
        "USE" => (K::Use, Modification),
        "CREATE" => match obj {
            "DATABASE" | "KEYSPACE" => (K::CreateDatabase, Modification),
            "SCHEMA" => (K::CreateSchema, Modification),
            "VIEW" => (K::CreateView, Modification),
            "TRIGGER" => (K::CreateTrigger, Modification),
            "FUNCTION" => (K::CreateFunction, Modification),
            "PROCEDURE" => (K::CreateProcedure, Modification),
            "INDEX" => (K::CreateIndex, Modification),
            _ => (K::CreateTable, Modification),
        },
        "DROP" => match obj {
            "DATABASE" | "KEYSPACE" => (K::DropDatabase, Modification),
            "SCHEMA" => (K::DropSchema, Modification),
            "VIEW" => (K::DropView, Modification),
            "TRIGGER" => (K::DropTrigger, Modification),
            "FUNCTION" => (K::DropFunction, Modification),
            "PROCEDURE" => (K::DropProcedure, Modification),
            "INDEX" => (K::DropIndex, Modification),
            _ => (K::DropTable, Modification),
        },
        "ALTER" => match obj {
            "DATABASE" => (K::AlterDatabase, Modification),
            "SCHEMA" => (K::AlterSchema, Modification),
            "VIEW" => (K::AlterView, Modification),
            "TRIGGER" => (K::AlterTrigger, Modification),
            "FUNCTION" => (K::AlterFunction, Modification),
            "PROCEDURE" => (K::AlterProcedure, Modification),
            "INDEX" => (K::AlterIndex, Modification),
            _ => (K::AlterTable, Modification),
        },
        _ if strict => return Err(format!("Invalid statement parser \"{first}\"")),
        _ => (K::Unknown, Unk),
    };
    Ok(classified)
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
