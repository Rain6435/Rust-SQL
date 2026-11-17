//! Live PostgreSQL / Redshift query execution and cancellation via `tokio-postgres`.

use crate::adapters::dialect_adapters::live_pg::describe_pg_error;
use crate::adapters::dialect_adapters::live_pg_params::execute_live_query_with_params;
use crate::adapters::CancellableQuery;
use crate::common::types::{FieldInfo, QueryRowResult};
use crate::core::errors::{CanceledByUserError, DbCoreError};
use crate::core::params::QueryParams;
use crate::core::sql_identifier::identify;
use async_trait::async_trait;
use serde_json::{Map, Value};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio_postgres::{Client, NoTls, SimpleQueryMessage};

pub(crate) fn finish_statement(
    label: Option<String>,
    fields: &mut Vec<FieldInfo>,
    rows: &mut Vec<Map<String, Value>>,
    returned_rows: bool,
    affected: u64,
) -> QueryRowResult {
    if returned_rows {
        let rows = std::mem::take(rows);
        QueryRowResult {
            command: label.unwrap_or_else(|| "SELECT".to_string()),
            row_count: Some(rows.len()),
            rows,
            fields: std::mem::take(fields),
            affected_rows: None,
        }
    } else {
        QueryRowResult {
            command: label.unwrap_or_else(|| "UNKNOWN".to_string()),
            rows: Vec::new(),
            fields: Vec::new(),
            row_count: None,
            affected_rows: Some(affected as usize),
        }
    }
}

/// Executes `query_text` (one or more statements) on the live server.
///
/// Values are returned as text, exactly as the server renders them, or `null`.
pub async fn execute_live_query(
    client: &Client,
    query_text: &str,
) -> Result<Vec<QueryRowResult>, DbCoreError> {
    if query_text.trim().is_empty() {
        return Ok(Vec::new());
    }
    let mut labels = identify(query_text, false)
        .map(|stmts| stmts.into_iter().map(|s| s.statement_type.to_string()))
        .into_iter()
        .flatten();
    let messages = client
        .simple_query(query_text)
        .await
        .map_err(|e| DbCoreError::Message(describe_pg_error(&e)))?;

    let mut results = Vec::new();
    let mut fields: Vec<FieldInfo> = Vec::new();
    let mut rows: Vec<Map<String, Value>> = Vec::new();
    let mut returned_rows = false;
    for msg in messages {
        match msg {
            SimpleQueryMessage::RowDescription(cols) => {
                returned_rows = true;
                fields = cols
                    .iter()
                    .map(|c| FieldInfo {
                        name: c.name().to_string(),
                    })
                    .collect();
            }
            SimpleQueryMessage::Row(row) => {
                returned_rows = true;
                let mut map = Map::new();
                for (idx, col) in row.columns().iter().enumerate() {
                    let val = row.get(idx).map(|v| Value::String(v.to_string()));
                    map.insert(col.name().to_string(), val.unwrap_or(Value::Null));
                }
                rows.push(map);
            }
            SimpleQueryMessage::CommandComplete(affected) => {
                let label = labels.next();
                results.push(finish_statement(
                    label,
                    &mut fields,
                    &mut rows,
                    returned_rows,
                    affected,
                ));
                returned_rows = false;
            }
            _ => {}
        }
    }
    Ok(results)
}

/// Cancellable query handle for a live PostgreSQL / Redshift connection.
pub struct PgCancellableQuery {
    /// Connected client the query runs on.
    pub client: Arc<Client>,
    /// Raw SQL query text.
    pub query_text: String,
    /// Values for the query's placeholders, bound through prepared statements.
    pub params: QueryParams,
    /// Set once the user cancels.
    pub canceled: Arc<AtomicBool>,
}

#[async_trait]
impl CancellableQuery for PgCancellableQuery {
    async fn execute(&self) -> Result<Vec<QueryRowResult>, DbCoreError> {
        if self.canceled.load(Ordering::SeqCst) {
            return Err(DbCoreError::CanceledByUser(CanceledByUserError::new()));
        }
        let res =
            execute_live_query_with_params(&self.client, &self.query_text, &self.params).await;
        if res.is_err() && self.canceled.load(Ordering::SeqCst) {
            return Err(DbCoreError::CanceledByUser(CanceledByUserError::new()));
        }
        res
    }

    async fn cancel(&self) -> Result<(), DbCoreError> {
        self.canceled.store(true, Ordering::SeqCst);
        self.client
            .cancel_token()
            .cancel_query(NoTls)
            .await
            .map_err(|e| DbCoreError::Message(format!("Could not cancel query: {e}")))
    }
}
