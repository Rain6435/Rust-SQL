//! Live PostgreSQL / Redshift metadata introspection using the `pg_sql` catalog templates.
//!
//! Every function queries the connected server and propagates its errors. None of them
//! substitute emulated or placeholder data.

use crate::adapters::dialect_adapters::live_pg::describe_pg_error;
use crate::adapters::dialect_adapters::sql_templates::pg_sql;
use crate::common::types::{
    DatabaseFilter, ListRoutineResult, ListTableColumnsResult, ListTableResult, ListViewResult,
    SchemaFilter, TableKeysResult,
};
use crate::core::errors::DbCoreError;
use tokio_postgres::types::ToSql;
use tokio_postgres::{Client, Row};

fn db_err(e: tokio_postgres::Error) -> DbCoreError {
    DbCoreError::Message(describe_pg_error(&e))
}

async fn rows(
    client: &Client,
    sql: &str,
    params: &[&(dyn ToSql + Sync)],
) -> Result<Vec<Row>, DbCoreError> {
    client.query(sql, params).await.map_err(db_err)
}

fn text(row: &Row, idx: usize) -> Result<Option<String>, DbCoreError> {
    row.try_get::<_, Option<String>>(idx).map_err(db_err)
}

fn required_text(row: &Row, idx: usize) -> Result<String, DbCoreError> {
    Ok(text(row, idx)?.unwrap_or_default())
}

async fn first_column(
    client: &Client,
    sql: &str,
    params: &[&(dyn ToSql + Sync)],
) -> Result<Vec<String>, DbCoreError> {
    rows(client, sql, params)
        .await?
        .iter()
        .map(|r| required_text(r, 0))
        .collect()
}

/// Lists non-template databases on the server.
pub async fn live_list_databases(
    client: &Client,
    filter: Option<&DatabaseFilter>,
) -> Result<Vec<String>, DbCoreError> {
    first_column(client, &pg_sql::list_databases(filter), &[&false]).await
}

/// Lists schemas in the connected database.
pub async fn live_list_schemas(
    client: &Client,
    filter: Option<&SchemaFilter>,
) -> Result<Vec<String>, DbCoreError> {
    first_column(client, &pg_sql::list_schemas(filter), &[]).await
}

async fn schema_and_name(client: &Client, sql: &str) -> Result<Vec<ListTableResult>, DbCoreError> {
    rows(client, sql, &[])
        .await?
        .iter()
        .map(|r| {
            Ok(ListTableResult {
                schema: text(r, 0)?,
                name: required_text(r, 1)?,
            })
        })
        .collect()
}

/// Lists tables in the connected database.
pub async fn live_list_tables(
    client: &Client,
    filter: Option<&SchemaFilter>,
) -> Result<Vec<ListTableResult>, DbCoreError> {
    schema_and_name(client, &pg_sql::list_tables(filter)).await
}

/// Lists views in the connected database.
pub async fn live_list_views(
    client: &Client,
    filter: Option<&SchemaFilter>,
) -> Result<Vec<ListViewResult>, DbCoreError> {
    schema_and_name(client, &pg_sql::list_views(filter)).await
}

/// Lists functions and procedures in the connected database.
pub async fn live_list_routines(
    client: &Client,
    filter: Option<&SchemaFilter>,
) -> Result<Vec<ListRoutineResult>, DbCoreError> {
    rows(client, &pg_sql::list_routines(filter), &[])
        .await?
        .iter()
        .map(|r| {
            Ok(ListRoutineResult {
                schema: text(r, 0)?,
                routine_name: required_text(r, 1)?,
                routine_type: required_text(r, 2)?,
            })
        })
        .collect()
}

/// Lists columns of `schema.table`.
pub async fn live_list_columns(
    client: &Client,
    schema: &str,
    table: &str,
) -> Result<Vec<ListTableColumnsResult>, DbCoreError> {
    rows(client, pg_sql::LIST_TABLE_COLUMNS, &[&schema, &table])
        .await?
        .iter()
        .map(|r| {
            Ok(ListTableColumnsResult {
                column_name: required_text(r, 0)?,
                data_type: required_text(r, 1)?,
            })
        })
        .collect()
}

/// Lists trigger names on `schema.table`.
pub async fn live_list_triggers(
    client: &Client,
    schema: &str,
    table: &str,
) -> Result<Vec<String>, DbCoreError> {
    first_column(client, pg_sql::LIST_TABLE_TRIGGERS, &[&schema, &table]).await
}

/// Lists index names on `schema.table`.
pub async fn live_list_indexes(
    client: &Client,
    schema: &str,
    table: &str,
) -> Result<Vec<String>, DbCoreError> {
    first_column(client, pg_sql::LIST_TABLE_INDEXES, &[&schema, &table]).await
}

/// Lists tables referenced by foreign keys on `schema.table`.
pub async fn live_table_references(
    client: &Client,
    schema: &str,
    table: &str,
) -> Result<Vec<String>, DbCoreError> {
    first_column(client, pg_sql::GET_TABLE_REFERENCES, &[&table, &schema]).await
}

/// Lists primary and foreign keys on `schema.table`.
pub async fn live_table_keys(
    client: &Client,
    schema: &str,
    table: &str,
) -> Result<Vec<TableKeysResult>, DbCoreError> {
    rows(client, pg_sql::GET_TABLE_KEYS, &[&table, &schema])
        .await?
        .iter()
        .map(|r| {
            Ok(TableKeysResult {
                constraint_name: text(r, 0)?,
                column_name: required_text(r, 1)?,
                referenced_table: text(r, 2)?,
                key_type: required_text(r, 3)?,
            })
        })
        .collect()
}
