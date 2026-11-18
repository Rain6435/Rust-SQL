//! Running an import: prepared-statement inserts inside a single transaction.

use crate::adapters::DatabaseAdapter;
use crate::core::errors::DbCoreError;
use crate::core::import::{build_insert_sql, row_params, ColumnMapping, ImportData};
use std::collections::HashSet;

/// What to import and where.
#[derive(Debug, Clone, Copy)]
pub struct ImportRequest<'a> {
    /// Target table (must already exist).
    pub table: &'a str,
    /// Target schema, if any.
    pub schema: Option<&'a str>,
    /// Source-to-target column mapping. Unmapped source columns are skipped.
    pub mappings: &'a [ColumnMapping],
    /// Parsed rows.
    pub data: &'a ImportData,
    /// The user explicitly confirmed a large import (see [`ImportData::requires_confirmation`]).
    pub confirmed: bool,
}

/// Outcome of a committed import.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportReport {
    /// Rows inserted (all of them, since the import is all-or-nothing).
    pub rows_inserted: usize,
    /// The parameterized `INSERT` statement each row ran through.
    pub sql: String,
}

fn msg(text: String) -> DbCoreError {
    DbCoreError::Message(text)
}

/// Checks the mapping against the table's real columns before anything is written.
async fn validate(
    adapter: &dyn DatabaseAdapter,
    adapter_key: &str,
    req: &ImportRequest<'_>,
) -> Result<(), DbCoreError> {
    if adapter_key == "cassandra" {
        return Err(msg(
            "Import is not supported for Cassandra (no transactions)".into(),
        ));
    }
    if req.mappings.is_empty() {
        return Err(msg("Map at least one column before importing".into()));
    }
    let columns: Vec<String> = adapter
        .list_table_columns(req.table, req.schema)
        .await?
        .into_iter()
        .map(|c| c.column_name)
        .collect();
    if columns.is_empty() {
        return Err(msg(format!(
            "Table {} was not found or has no columns",
            req.table
        )));
    }
    let mut seen = HashSet::new();
    for m in req.mappings {
        if m.source >= req.data.columns.len() {
            return Err(msg(format!(
                "Source column #{} does not exist",
                m.source + 1
            )));
        }
        if !columns.contains(&m.target) {
            return Err(msg(format!(
                "Table {} has no column {}",
                req.table, m.target
            )));
        }
        if !seen.insert(&m.target) {
            return Err(msg(format!("Column {} is mapped more than once", m.target)));
        }
    }
    if req.data.requires_confirmation() && !req.confirmed {
        return Err(msg(format!(
            "This import has {} rows. Large imports must be confirmed explicitly \
             (the confirmation checkbox, or --yes on the command line).",
            req.data.rows.len()
        )));
    }
    Ok(())
}

/// Inserts every row of `req.data` into `req.table` in one transaction.
///
/// Values go through the adapter's prepared statements. If any row fails, the transaction is
/// rolled back and the error names the row (1-based, excluding the header).
pub async fn run_import(
    adapter: &dyn DatabaseAdapter,
    adapter_key: &str,
    req: ImportRequest<'_>,
) -> Result<ImportReport, DbCoreError> {
    validate(adapter, adapter_key, &req).await?;
    let sql = build_insert_sql(adapter_key, req.table, req.schema, req.mappings);
    if req.data.rows.is_empty() {
        return Ok(ImportReport {
            rows_inserted: 0,
            sql,
        });
    }
    adapter.execute_query("BEGIN").await?;
    for (i, row) in req.data.rows.iter().enumerate() {
        let params = row_params(row, req.mappings);
        if let Err(e) = adapter.execute_query_with_params(&sql, &params).await {
            let rollback = adapter.execute_query("ROLLBACK").await;
            return Err(msg(format!(
                "Row {}: {e}. The import was rolled back{}.",
                i + 1,
                match rollback {
                    Ok(_) => "; no rows were inserted".to_string(),
                    Err(r) => format!(", but ROLLBACK also failed: {r}"),
                }
            )));
        }
    }
    if let Err(e) = adapter.execute_query("COMMIT").await {
        let _ = adapter.execute_query("ROLLBACK").await;
        return Err(msg(format!("COMMIT failed, nothing was imported: {e}")));
    }
    Ok(ImportReport {
        rows_inserted: req.data.rows.len(),
        sql,
    })
}
