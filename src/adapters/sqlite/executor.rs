//! SQLite statement execution, row/value conversion, and `sqlite3_interrupt` cancellable query support.

use crate::adapters::sqlite::connection::open_sqlite;
use crate::adapters::CancellableQuery;
use crate::common::types::{FieldInfo, QueryRowResult};
use crate::core::errors::{CanceledByUserError, DbCoreError};
use crate::core::sql_identifier::{identify, ExecutionType, StatementKind};
use async_trait::async_trait;
use rusqlite::{types::ValueRef, Connection, InterruptHandle};
use serde_json::{Map, Number, Value};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

/// Executes a single SQL statement against a `rusqlite::Connection` and constructs a [`QueryRowResult`].
pub fn execute_sqlite_statement(
    conn: &Connection,
    stmt_text: &str,
    stmt_type: StatementKind,
    exec_type: ExecutionType,
) -> Result<QueryRowResult, DbCoreError> {
    let is_mod = exec_type == ExecutionType::Modification
        && !matches!(
            stmt_type,
            StatementKind::Select | StatementKind::Pragma | StatementKind::Explain
        );

    if is_mod {
        let changes = conn.execute(stmt_text, []).map_err(map_sqlite_err)?;
        Ok(QueryRowResult {
            command: stmt_type.to_string(),
            rows: Vec::new(),
            fields: Vec::new(),
            row_count: None,
            affected_rows: Some(changes),
        })
    } else {
        let mut stmt = conn.prepare(stmt_text).map_err(map_sqlite_err)?;
        let col_names: Vec<String> = stmt
            .column_names()
            .into_iter()
            .map(|s| s.to_string())
            .collect();
        let mut rows_out = Vec::new();

        let mut rows = stmt.query([]).map_err(map_sqlite_err)?;
        while let Some(row) = rows.next().map_err(map_sqlite_err)? {
            let mut map = Map::new();
            for (idx, col_name) in col_names.iter().enumerate() {
                let val_ref = row.get_ref(idx).map_err(map_sqlite_err)?;
                let json_val = match val_ref {
                    ValueRef::Null => Value::Null,
                    ValueRef::Integer(i) => Value::Number(Number::from(i)),
                    ValueRef::Real(f) => Number::from_f64(f)
                        .map(Value::Number)
                        .unwrap_or(Value::Null),
                    ValueRef::Text(t) => Value::String(String::from_utf8_lossy(t).into_owned()),
                    ValueRef::Blob(b) => {
                        if b.len() == 1 {
                            Value::String(b[0].to_string())
                        } else if let Ok(s) = std::str::from_utf8(b) {
                            Value::String(s.to_string())
                        } else {
                            Value::Array(b.iter().map(|&byte| Value::from(byte)).collect())
                        }
                    }
                };
                map.insert(col_name.clone(), json_val);
            }
            rows_out.push(map);
        }

        let fields = if !rows_out.is_empty() {
            col_names
                .into_iter()
                .map(|name| FieldInfo { name })
                .collect()
        } else {
            Vec::new()
        };

        let row_count = rows_out.len();
        Ok(QueryRowResult {
            command: stmt_type.to_string(),
            rows: rows_out,
            fields,
            row_count: Some(row_count),
            affected_rows: Some(0),
        })
    }
}

/// Maps a `rusqlite::Error` into [`DbCoreError`], translating `SQLITE_INTERRUPT` into [`DbCoreError::CanceledByUser`].
pub fn map_sqlite_err(err: rusqlite::Error) -> DbCoreError {
    if let rusqlite::Error::SqliteFailure(ffi_err, _) = &err {
        if ffi_err.extended_code == 9 || ffi_err.code == rusqlite::ErrorCode::OperationInterrupted {
            return DbCoreError::CanceledByUser(CanceledByUserError::new());
        }
    }
    DbCoreError::Message(err.to_string())
}

/// Cancellable SQLite query handle backed by `rusqlite::InterruptHandle`.
pub struct SqliteCancellableQuery {
    /// Shared SQLite session connection.
    pub shared_conn: Arc<Mutex<Option<Connection>>>,
    /// Path to the SQLite database file or `":memory:"`.
    pub db_path: String,
    /// Read-Only flag shared with the owning adapter, used if the connection must be opened.
    pub read_only: Arc<AtomicBool>,
    /// SQL script to execute.
    pub query_text: String,
    /// Native SQLite interrupt handle populated when execution begins.
    pub interrupt_handle: Mutex<Option<InterruptHandle>>,
    /// Atomic flag tracking whether [`cancel`](CancellableQuery::cancel) has been invoked.
    pub canceled: AtomicBool,
}

#[async_trait]
impl CancellableQuery for SqliteCancellableQuery {
    async fn execute(&self) -> Result<Vec<QueryRowResult>, DbCoreError> {
        let statements = identify(&self.query_text, false).unwrap_or_default();
        if statements.is_empty() {
            return Ok(Vec::new());
        }

        let mut guard = self
            .shared_conn
            .lock()
            .map_err(|e| DbCoreError::Message(e.to_string()))?;
        if guard.is_none() {
            *guard = Some(open_sqlite(
                &self.db_path,
                self.read_only.load(Ordering::SeqCst),
            )?);
        }
        let Some(conn) = guard.as_ref() else {
            return Err(DbCoreError::NoConnectionAvailable);
        };
        if let Ok(mut ih) = self.interrupt_handle.lock() {
            *ih = Some(conn.get_interrupt_handle());
        }

        if self.canceled.load(Ordering::SeqCst) {
            return Err(DbCoreError::CanceledByUser(CanceledByUserError::new()));
        }

        let mut results = Vec::with_capacity(statements.len());
        for stmt in statements {
            if self.canceled.load(Ordering::SeqCst) {
                return Err(DbCoreError::CanceledByUser(CanceledByUserError::new()));
            }
            results.push(execute_sqlite_statement(
                conn,
                &stmt.text,
                stmt.statement_type,
                stmt.execution_type,
            )?);
        }
        Ok(results)
    }

    async fn cancel(&self) -> Result<(), DbCoreError> {
        self.canceled.store(true, Ordering::SeqCst);
        let guard = self
            .interrupt_handle
            .lock()
            .map_err(|e| DbCoreError::Message(e.to_string()))?;
        if let Some(ref handle) = *guard {
            handle.interrupt();
            Ok(())
        } else {
            Err(DbCoreError::QueryNotReadyToCancel)
        }
    }
}
