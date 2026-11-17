//! Database adapter traits, client registry, connection pooling, and multi-dialect facade.
//!
//! Provides the [`DatabaseAdapter`] and [`CancellableQuery`] traits, built-in client adapters
//! (`sqlite`, `postgresql`, `redshift`, `mysql`, `mariadb`, `sqlserver`, `cassandra`), and
//! high-level session management via [`DatabaseFacade`] and [`ServerSession`].

pub mod dialect_adapters;
pub mod emulation;
pub mod facade;
pub mod registry;
pub mod sqlite;

pub use dialect_adapters::DialectAdapter;
pub use emulation::{emulation_enabled, has_live_driver, set_emulation_enabled};
pub use facade::*;
pub use registry::*;
pub use sqlite::{
    execute_sqlite_statement, execute_sqlite_statement_with_params, map_sqlite_err, SqliteAdapter,
    SqliteCancellableQuery,
};

use crate::common::types::{
    AdapterVersion, ConnectionMode, DatabaseFilter, ListRoutineResult, ListTableColumnsResult,
    ListTableResult, ListViewResult, QueryRowResult, SchemaFilter, Server, TableKeysResult,
};
use crate::core::errors::DbCoreError;
use crate::core::params::QueryParams;
use async_trait::async_trait;
use std::sync::Arc;

/// Handle representing an in-flight or prepared SQL query that can be executed or canceled asynchronously.
#[async_trait]
pub trait CancellableQuery: Send + Sync {
    /// Executes the SQL query and returns the statement results.
    async fn execute(&self) -> Result<Vec<QueryRowResult>, DbCoreError>;
    /// Signals the database backend to abort the running query.
    async fn cancel(&self) -> Result<(), DbCoreError>;
}

/// Unified asynchronous interface implemented by all database client adapters.
#[async_trait]
pub trait DatabaseAdapter: Send + Sync {
    /// Establishes a connection to the target database server and queries its version metadata.
    async fn connect(&mut self) -> Result<(), DbCoreError>;
    /// Closes all active connections in the adapter's pool or session.
    async fn disconnect(&mut self) -> Result<(), DbCoreError>;
    /// Returns the server's [`AdapterVersion`] discovered during [`connect`](Self::connect).
    fn get_version(&self) -> AdapterVersion;
    /// Reports whether results come from the real server or from the in-memory emulator.
    fn connection_mode(&self) -> ConnectionMode;
    /// Puts the session into (or out of) a read-only session enforced by the database itself.
    ///
    /// This backs up the Read-Only analyzer in [`crate::core::safety`]. It is required: an
    /// adapter that cannot enforce read-only sessions must return an error, never `Ok(())`.
    async fn set_read_only(&self, read_only: bool) -> Result<(), DbCoreError>;
    /// Quotes a SQL identifier (`"ident"`, `` `ident` ``, or `[ident]`) for this adapter's dialect.
    fn wrap_identifier(&self, value: &str) -> String;

    /// Lists all accessible databases or keyspaces on the server, optionally filtered by `filter`.
    async fn list_databases(
        &self,
        filter: Option<&DatabaseFilter>,
    ) -> Result<Vec<String>, DbCoreError>;

    /// Lists all schemas in the current database, optionally filtered by `filter`.
    async fn list_schemas(
        &self,
        _filter: Option<&SchemaFilter>,
    ) -> Result<Vec<String>, DbCoreError> {
        Ok(Vec::new())
    }

    /// Lists all user tables in the current database, optionally filtered by `filter`.
    async fn list_tables(
        &self,
        filter: Option<&SchemaFilter>,
    ) -> Result<Vec<ListTableResult>, DbCoreError>;

    /// Lists all views in the current database, optionally filtered by `filter`.
    async fn list_views(
        &self,
        _filter: Option<&SchemaFilter>,
    ) -> Result<Vec<ListViewResult>, DbCoreError> {
        Ok(Vec::new())
    }

    /// Lists all stored functions and procedures in the current database.
    async fn list_routines(
        &self,
        _filter: Option<&SchemaFilter>,
    ) -> Result<Vec<ListRoutineResult>, DbCoreError> {
        Ok(Vec::new())
    }

    /// Lists all columns and their SQL data types for `table`.
    async fn list_table_columns(
        &self,
        table: &str,
        schema: Option<&str>,
    ) -> Result<Vec<ListTableColumnsResult>, DbCoreError>;

    /// Lists all trigger names attached to `table`.
    async fn list_table_triggers(
        &self,
        _table: &str,
        _schema: Option<&str>,
    ) -> Result<Vec<String>, DbCoreError> {
        Ok(Vec::new())
    }

    /// Lists all index names defined on `table`.
    async fn list_table_indexes(
        &self,
        _table: &str,
        _schema: Option<&str>,
    ) -> Result<Vec<String>, DbCoreError> {
        Ok(Vec::new())
    }

    /// Lists all foreign-key referenced parent table names for `table`.
    async fn get_table_references(
        &self,
        _table: &str,
        _schema: Option<&str>,
    ) -> Result<Vec<String>, DbCoreError> {
        Ok(Vec::new())
    }

    /// Lists primary and foreign key constraints for `table`.
    async fn get_table_keys(
        &self,
        _table: &str,
        _schema: Option<&str>,
    ) -> Result<Vec<TableKeysResult>, DbCoreError> {
        Ok(Vec::new())
    }

    /// Creates a [`CancellableQuery`] handle for `query_text`, which has no parameters.
    fn query(&self, query_text: &str) -> Result<Arc<dyn CancellableQuery>, DbCoreError> {
        self.query_with_params(query_text, &QueryParams::new())
    }

    /// Creates a [`CancellableQuery`] handle that binds `params` to `query_text`'s placeholders.
    ///
    /// Required for every adapter: parameters must go through the driver's prepared-statement
    /// API (see [`crate::core::params::prepare_statement`]), never into the SQL text.
    fn query_with_params(
        &self,
        query_text: &str,
        params: &QueryParams,
    ) -> Result<Arc<dyn CancellableQuery>, DbCoreError>;

    /// Executes `query_text` (supporting multiple semicolon-separated statements) and returns results.
    async fn execute_query(&self, query_text: &str) -> Result<Vec<QueryRowResult>, DbCoreError> {
        self.execute_query_with_params(query_text, &QueryParams::new())
            .await
    }

    /// Executes `query_text`, binding `params` through the driver's prepared-statement API.
    ///
    /// Required for every adapter, like [`query_with_params`](Self::query_with_params).
    async fn execute_query_with_params(
        &self,
        query_text: &str,
        params: &QueryParams,
    ) -> Result<Vec<QueryRowResult>, DbCoreError>;

    /// Builds a dialect-specific `SELECT TOP / LIMIT` query for `table`.
    async fn get_query_select_top(
        &self,
        table: &str,
        limit: u32,
        schema: Option<&str>,
    ) -> Result<String, DbCoreError>;

    /// Generates the `CREATE TABLE` DDL script for `table`.
    async fn get_table_create_script(
        &self,
        _table: &str,
        _schema: Option<&str>,
    ) -> Result<Vec<String>, DbCoreError> {
        Ok(Vec::new())
    }

    /// Generates the `CREATE VIEW` DDL script for `view`.
    async fn get_view_create_script(
        &self,
        _view: &str,
        _schema: Option<&str>,
    ) -> Result<Vec<String>, DbCoreError> {
        Ok(Vec::new())
    }

    /// Generates the `CREATE FUNCTION / PROCEDURE` DDL script for `routine`.
    async fn get_routine_create_script(
        &self,
        _routine: &str,
        _routine_type: &str,
        _schema: Option<&str>,
    ) -> Result<Vec<String>, DbCoreError> {
        Ok(Vec::new())
    }

    /// Truncates or deletes all rows from all user tables in the current database/schema.
    async fn truncate_all_tables(&self, _schema: Option<&str>) -> Result<(), DbCoreError> {
        Ok(())
    }
}

/// Instantiates a boxed [`DatabaseAdapter`] for `adapter_key`.
pub fn adapter_factory(
    adapter_key: &str,
    server: &Server,
    database: Option<&str>,
) -> Result<Box<dyn DatabaseAdapter>, DbCoreError> {
    let adapters = get_adapters();
    if !adapters.iter().any(|a| a.key == adapter_key) {
        return Err(DbCoreError::UnknownAdapter(adapter_key.to_string()));
    }

    match adapter_key {
        "sqlite" => Ok(Box::new(SqliteAdapter::new(server, database))),
        _ => Ok(Box::new(
            DialectAdapter::new(adapter_key, server, database)
                .with_emulation_allowed(emulation_enabled()),
        )),
    }
}
