//! Multi-dialect database adapter supporting PostgreSQL, Redshift, MySQL, MariaDB,
//! Microsoft SQL Server, and Apache Cassandra.

pub mod executor;
pub mod metadata;
pub mod scripts;
pub mod seed;
pub mod sql_templates;

pub use executor::{execute_dialect_query_internal, DialectCancellableQuery};
pub use metadata::*;
pub use scripts::*;
pub use seed::{
    default_version_for_adapter, seed_default_dialect_schema, try_connect_live_postgres,
};
pub use sql_templates::{map_legacy_cassandra_data_type, mysql_sql, pg_sql, sqlserver_sql};

use crate::adapters::{
    build_query_select_top, wrap_identifier_for_adapter, CancellableQuery, DatabaseAdapter,
};
use crate::common::types::{
    AdapterVersion, DatabaseFilter, ListRoutineResult, ListTableColumnsResult, ListTableResult,
    ListViewResult, QueryRowResult, SchemaFilter, Server, TableKeysResult,
};
use crate::core::errors::DbCoreError;
use crate::core::filters::filter_database_name;
use async_trait::async_trait;
use rusqlite::Connection;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

/// Multi-dialect database adapter supporting live network connections (e.g., `tokio-postgres`)
/// as well as deterministic in-memory dialect execution for testing and offline development.
pub struct DialectAdapter {
    /// Adapter key (`postgresql`, `redshift`, `mysql`, `mariadb`, `sqlserver`, `cassandra`).
    pub adapter_key: String,
    /// Server connection configuration.
    pub server: Server,
    /// Target database or keyspace name.
    pub database: String,
    /// Default schema name (`"public"` for PostgreSQL/Redshift, `"dbo"` for SQL Server).
    pub default_schema: String,
    /// Discovered or default server version metadata.
    pub version: AdapterVersion,
    /// Optional live `tokio_postgres::Client` connection.
    pub pg_client: Option<Arc<tokio_postgres::Client>>,
    /// In-memory SQLite engine backing offline/emulated dialect queries.
    pub emulated_conn: Arc<Mutex<Connection>>,
    /// Tracked database list for the server session.
    pub extra_databases: Arc<Mutex<Vec<String>>>,
}

#[async_trait]
impl DatabaseAdapter for DialectAdapter {
    async fn connect(&mut self) -> Result<(), DbCoreError> {
        self.connect_internal().await
    }

    async fn disconnect(&mut self) -> Result<(), DbCoreError> {
        self.pg_client = None;
        Ok(())
    }

    fn get_version(&self) -> AdapterVersion {
        self.version.clone()
    }

    fn wrap_identifier(&self, value: &str) -> String {
        wrap_identifier_for_adapter(&self.adapter_key, value)
    }

    async fn list_databases(
        &self,
        filter: Option<&DatabaseFilter>,
    ) -> Result<Vec<String>, DbCoreError> {
        if let Some(ref client) = self.pg_client {
            if let Ok(rows) = client
                .query(&pg_sql::list_databases(filter), &[&false])
                .await
            {
                return Ok(rows.into_iter().filter_map(|r| r.try_get(0).ok()).collect());
            }
        }
        let dbs = self
            .extra_databases
            .lock()
            .map(|g| g.clone())
            .unwrap_or_else(|_| vec!["rust_sql".to_string()]);
        let mut out: Vec<String> = dbs
            .into_iter()
            .filter(|name| filter_database_name(name, filter))
            .collect();
        out.sort();
        out.dedup();
        Ok(out)
    }

    async fn list_schemas(
        &self,
        filter: Option<&SchemaFilter>,
    ) -> Result<Vec<String>, DbCoreError> {
        list_schemas_for_adapter(&self.adapter_key, &self.default_schema, filter)
    }

    async fn list_tables(
        &self,
        filter: Option<&SchemaFilter>,
    ) -> Result<Vec<ListTableResult>, DbCoreError> {
        list_tables_for_adapter(
            &self.adapter_key,
            &self.default_schema,
            &self.emulated_conn,
            filter,
        )
    }

    async fn list_views(
        &self,
        filter: Option<&SchemaFilter>,
    ) -> Result<Vec<ListViewResult>, DbCoreError> {
        list_views_for_adapter(
            &self.adapter_key,
            &self.default_schema,
            &self.emulated_conn,
            filter,
        )
    }

    async fn list_routines(
        &self,
        filter: Option<&SchemaFilter>,
    ) -> Result<Vec<ListRoutineResult>, DbCoreError> {
        list_routines_for_adapter(&self.adapter_key, &self.default_schema, filter)
    }

    async fn list_table_columns(
        &self,
        table: &str,
        _schema: Option<&str>,
    ) -> Result<Vec<ListTableColumnsResult>, DbCoreError> {
        list_columns_for_adapter(&self.adapter_key, &self.emulated_conn, table)
    }

    async fn list_table_triggers(
        &self,
        table: &str,
        _schema: Option<&str>,
    ) -> Result<Vec<String>, DbCoreError> {
        list_triggers_for_adapter(&self.adapter_key, table)
    }

    async fn list_table_indexes(
        &self,
        table: &str,
        _schema: Option<&str>,
    ) -> Result<Vec<String>, DbCoreError> {
        list_indexes_for_adapter(&self.adapter_key, table)
    }

    async fn get_table_references(
        &self,
        table: &str,
        _schema: Option<&str>,
    ) -> Result<Vec<String>, DbCoreError> {
        get_references_for_adapter(&self.adapter_key, table)
    }

    async fn get_table_keys(
        &self,
        table: &str,
        _schema: Option<&str>,
    ) -> Result<Vec<TableKeysResult>, DbCoreError> {
        get_keys_for_adapter(&self.adapter_key, table)
    }

    fn query(&self, query_text: &str) -> Result<Arc<dyn CancellableQuery>, DbCoreError> {
        if self.adapter_key == "cassandra" {
            return Err(DbCoreError::CassandraCancelUnsupported);
        }
        Ok(Arc::new(DialectCancellableQuery {
            adapter_key: self.adapter_key.clone(),
            emulated_conn: self.emulated_conn.clone(),
            extra_databases: self.extra_databases.clone(),
            query_text: query_text.to_string(),
            canceled: Arc::new(AtomicBool::new(false)),
        }))
    }

    async fn execute_query(&self, query_text: &str) -> Result<Vec<QueryRowResult>, DbCoreError> {
        execute_dialect_query_internal(
            &self.adapter_key,
            &self.emulated_conn,
            &self.extra_databases,
            query_text,
        )
    }

    async fn get_query_select_top(
        &self,
        table: &str,
        limit: u32,
        schema: Option<&str>,
    ) -> Result<String, DbCoreError> {
        Ok(build_query_select_top(
            &self.adapter_key,
            table,
            limit,
            schema.or(Some(&self.default_schema)),
        ))
    }

    async fn get_table_create_script(
        &self,
        table: &str,
        _schema: Option<&str>,
    ) -> Result<Vec<String>, DbCoreError> {
        get_table_create_script_for_adapter(&self.adapter_key, table)
    }

    async fn get_view_create_script(
        &self,
        view: &str,
        _schema: Option<&str>,
    ) -> Result<Vec<String>, DbCoreError> {
        get_view_create_script_for_adapter(&self.adapter_key, view)
    }

    async fn get_routine_create_script(
        &self,
        routine: &str,
        _routine_type: &str,
        _schema: Option<&str>,
    ) -> Result<Vec<String>, DbCoreError> {
        get_routine_create_script_for_adapter(&self.adapter_key, routine)
    }

    async fn truncate_all_tables(&self, _schema: Option<&str>) -> Result<(), DbCoreError> {
        let tables = self.list_tables(None).await?;
        let conn = self
            .emulated_conn
            .lock()
            .map_err(|e| DbCoreError::Message(e.to_string()))?;
        for t in tables {
            let _ = conn.execute(&format!("DELETE FROM {};", t.name), []);
        }
        Ok(())
    }
}
