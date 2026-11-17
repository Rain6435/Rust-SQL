//! Multi-statement SQL parser, statement splitter, and command classifier.

pub mod classifier;
pub mod kind;
pub mod splitter;

use classifier::{classify_statement, extract_parameters};
pub use kind::StatementKind;
use serde::{Deserialize, Serialize};
use splitter::{split_sql_statements, strip_comments};

/// High-level execution behavior classification for a SQL statement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExecutionType {
    /// Query that returns a tabular dataset (`SELECT`, `SHOW`, `PRAGMA`, `EXEC`).
    #[serde(rename = "LISTING")]
    Listing,
    /// Query that mutates data or schema (`INSERT`, `UPDATE`, `DELETE`, `CREATE`, `DROP`, `ALTER`, `TRUNCATE`).
    #[serde(rename = "MODIFICATION")]
    Modification,
    /// Query that returns execution plan or metadata (`EXPLAIN`).
    #[serde(rename = "INFORMATION")]
    Information,
    /// Unrecognized or dialect-specific statement.
    #[serde(rename = "UNKNOWN")]
    Unknown,
}

impl ExecutionType {
    /// Returns the uppercase string representation of this [`ExecutionType`].
    pub fn as_str(&self) -> &'static str {
        match self {
            ExecutionType::Listing => "LISTING",
            ExecutionType::Modification => "MODIFICATION",
            ExecutionType::Information => "INFORMATION",
            ExecutionType::Unknown => "UNKNOWN",
        }
    }
}

/// Metadata for a single parsed SQL statement extracted from a multi-statement script.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IdentifiedStatement {
    /// Byte offset where the statement starts in the input script.
    pub start: usize,
    /// Byte offset where the statement ends in the input script.
    pub end: usize,
    /// Trimmed raw SQL text of the statement.
    pub text: String,
    /// Statement command classification (serialized as `SELECT`, `INSERT`, `CREATE_TABLE`, etc.).
    #[serde(rename = "type")]
    pub statement_type: StatementKind,
    /// High-level execution category (`LISTING`, `MODIFICATION`, `INFORMATION`, `UNKNOWN`).
    pub execution_type: ExecutionType,
    /// Positional placeholders (`?`, `$1`) found inside the statement.
    pub parameters: Vec<String>,
}

/// Splits `query_text` into individual statements and classifies each statement's command and execution type.
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
