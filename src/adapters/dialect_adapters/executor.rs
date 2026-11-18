//! Multi-statement dialect query executor and cancellable query handle implementation.

use crate::adapters::{execute_sqlite_statement_with_params, CancellableQuery};
use crate::common::types::{FieldInfo, QueryRowResult};
use crate::core::errors::{CanceledByUserError, DbCoreError};
use crate::core::params::QueryParams;
use crate::core::sql_identifier::{identify, StatementKind};
use async_trait::async_trait;
use rusqlite::Connection;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

/// Cancellable query handle for an emulated [`super::DialectAdapter`] session.
///
/// Emulated queries finish immediately. Cancellation is honored if requested before execution.
pub struct DialectCancellableQuery {
    /// Target database adapter key.
    pub adapter_key: String,
    /// Shared SQLite backing connection for emulated execution.
    pub emulated_conn: Arc<Mutex<Connection>>,
    /// Tracked databases list for `CREATE DATABASE` / `DROP DATABASE` statements.
    pub extra_databases: Arc<Mutex<Vec<String>>>,
    /// Raw SQL query text.
    pub query_text: String,
    /// Values for the query's placeholders, bound through SQLite prepared statements.
    pub params: QueryParams,
    /// Atomic cancellation flag.
    pub canceled: Arc<AtomicBool>,
}

#[async_trait]
impl CancellableQuery for DialectCancellableQuery {
    async fn execute(&self) -> Result<Vec<QueryRowResult>, DbCoreError> {
        if self.canceled.load(Ordering::SeqCst) {
            return Err(DbCoreError::CanceledByUser(CanceledByUserError::new()));
        }

        execute_dialect_query_internal(
            &self.adapter_key,
            &self.emulated_conn,
            &self.extra_databases,
            &self.query_text,
            &self.params,
        )
    }

    async fn cancel(&self) -> Result<(), DbCoreError> {
        self.canceled.store(true, Ordering::SeqCst);
        Ok(())
    }
}

/// Executes `query_text` on the in-memory emulator, imitating the result shapes of `adapter_key`.
///
/// Only called for opted-in emulated sessions. Results are not from any real server.
/// Parameters are bound through SQLite prepared statements, like a live adapter.
pub fn execute_dialect_query_internal(
    adapter_key: &str,
    emulated_conn: &Arc<Mutex<Connection>>,
    extra_databases: &Arc<Mutex<Vec<String>>>,
    query_text: &str,
    params: &QueryParams,
) -> Result<Vec<QueryRowResult>, DbCoreError> {
    let trimmed = query_text.trim();

    if adapter_key == "cassandra" {
        if trimmed.is_empty() {
            return Err(DbCoreError::Message(
                "line 0:-1 no viable alternative at input '<EOF>'".to_string(),
            ));
        }
        if trimmed.starts_with("--") && !trimmed.contains('\n') {
            return Err(DbCoreError::Message(
                "line 1:13 mismatched character '<EOF>' expecting set null".to_string(),
            ));
        }
    } else if trimmed.is_empty() {
        return Ok(Vec::new());
    }

    let strict = matches!(
        adapter_key,
        "postgresql" | "redshift" | "sqlserver" | "cassandra"
    );
    let statements = match identify(query_text, strict) {
        Ok(s) => s,
        Err(_) => identify(query_text, false).unwrap_or_default(),
    };

    if adapter_key == "cassandra" && statements.len() > 1 {
        let second_cmd = statements[1].statement_type.as_str().to_lowercase();
        return Err(DbCoreError::Message(format!(
            "mismatched input '{second_cmd}' expecting EOF"
        )));
    }

    if statements.is_empty() {
        if matches!(adapter_key, "mysql" | "mariadb") && trimmed.starts_with("--") {
            return Ok(vec![QueryRowResult {
                command: "UNKNOWN".to_string(),
                rows: Vec::new(),
                fields: Vec::new(),
                row_count: None,
                affected_rows: Some(0),
            }]);
        }
        return Ok(Vec::new());
    }

    let conn = emulated_conn
        .lock()
        .map_err(|e| DbCoreError::Message(e.to_string()))?;

    let mut results = Vec::new();
    let mut sqlserver_non_select_affected = 0usize;
    let mut sqlserver_non_select_cmd: Option<String> = None;

    for stmt in &statements {
        let kind = stmt.statement_type;
        if kind == StatementKind::CreateDatabase {
            let db_name = stmt
                .text
                .split_whitespace()
                .last()
                .unwrap_or("")
                .trim_end_matches(';')
                .to_string();
            if !db_name.is_empty() {
                if let Ok(mut dbs) = extra_databases.lock() {
                    if !dbs.contains(&db_name) {
                        dbs.push(db_name);
                    }
                }
            }
            if adapter_key == "sqlserver" {
                continue;
            }
            results.push(QueryRowResult {
                command: "CREATE_DATABASE".to_string(),
                rows: Vec::new(),
                fields: Vec::new(),
                row_count: None,
                affected_rows: Some(1),
            });
            continue;
        }

        if kind == StatementKind::DropDatabase {
            let db_name = stmt
                .text
                .split_whitespace()
                .last()
                .unwrap_or("")
                .trim_end_matches(';')
                .to_string();
            if let Ok(mut dbs) = extra_databases.lock() {
                dbs.retain(|d| d != &db_name);
            }
            if adapter_key == "sqlserver" {
                continue;
            }
            results.push(QueryRowResult {
                command: "DROP_DATABASE".to_string(),
                rows: Vec::new(),
                fields: Vec::new(),
                row_count: None,
                affected_rows: Some(0),
            });
            continue;
        }

        let mut res = execute_sqlite_statement_with_params(
            &conn,
            &stmt.text,
            kind,
            stmt.execution_type,
            params,
        )?;

        if res.command == "SELECT" {
            if res.rows.is_empty() && adapter_key != "sqlserver" && adapter_key != "sqlite" {
                if let Ok(prepared) = conn.prepare(&stmt.text) {
                    res.fields = prepared
                        .column_names()
                        .map(|n| FieldInfo {
                            name: n.to_string(),
                        })
                        .collect();
                }
            }
            if adapter_key == "sqlserver" {
                res.affected_rows = Some(res.rows.len());
            } else {
                res.affected_rows = None;
            }
            results.push(res);
        } else if adapter_key == "cassandra" {
            res.affected_rows = None;
            res.row_count = None;
            results.push(res);
        } else if adapter_key == "sqlserver" {
            sqlserver_non_select_affected += res.affected_rows.unwrap_or(0);
            if sqlserver_non_select_cmd.is_none() {
                sqlserver_non_select_cmd = Some(res.command.clone());
            }
        } else {
            res.row_count = None;
            results.push(res);
        }
    }

    if adapter_key == "sqlserver" {
        if let Some(cmd) = sqlserver_non_select_cmd {
            results.push(QueryRowResult {
                command: cmd,
                rows: Vec::new(),
                fields: Vec::new(),
                row_count: Some(0),
                affected_rows: Some(sqlserver_non_select_affected),
            });
        }
    }

    Ok(results)
}
