//! Multi-dialect database adapter.
//!
//! PostgreSQL and Redshift connect live through `tokio-postgres`. MySQL, MariaDB, SQL Server,
//! and Cassandra have no live driver yet: they refuse to connect unless the user opts in to the
//! in-memory emulator, and an emulated session reports [`ConnectionMode::Emulated`].

pub mod executor;
pub mod live_pg;
pub mod live_pg_decode;
pub mod live_pg_metadata;
pub mod live_pg_params;
pub mod live_pg_query;
pub mod metadata;
pub mod query_dispatch;
pub mod scripts;
pub mod seed;
pub mod sql_templates;

pub use executor::{execute_dialect_query_internal, DialectCancellableQuery};
pub use live_pg::{connect_live_postgres, LivePostgres};
pub use live_pg_metadata::*;
pub use live_pg_params::execute_live_query_with_params;
pub use live_pg_query::{execute_live_query, PgCancellableQuery};
pub use metadata::*;
pub use scripts::*;
pub use seed::{
    default_version_for_adapter, list_databases_for_emulator, seed_default_dialect_schema,
    DialectAdapter,
};
pub use sql_templates::{map_legacy_cassandra_data_type, mysql_sql, pg_sql, sqlserver_sql};

use crate::adapters::{
    build_query_select_top, wrap_identifier_for_adapter, CancellableQuery, DatabaseAdapter,
};
use crate::common::types::{
    AdapterVersion, ConnectionMode, DatabaseFilter, ListRoutineResult, ListTableColumnsResult,
    ListTableResult, ListViewResult, QueryRowResult, SchemaFilter, TableKeysResult,
};
use crate::core::errors::DbCoreError;
use crate::core::params::QueryParams;
use async_trait::async_trait;
use scripts::ScriptKind;
use seed::Backend;
use std::sync::Arc;

#[async_trait]
impl DatabaseAdapter for DialectAdapter {
    async fn connect(&mut self) -> Result<(), DbCoreError> {
        self.connect_internal().await
    }

    async fn disconnect(&mut self) -> Result<(), DbCoreError> {
        self.pg_client = None;
        self.emulated_conn = None;
        Ok(())
    }

    fn get_version(&self) -> AdapterVersion {
        self.version.clone()
    }

    fn connection_mode(&self) -> ConnectionMode {
        if self.emulated_conn.is_some() {
            ConnectionMode::Emulated
        } else {
            ConnectionMode::Live
        }
    }

    async fn set_read_only(&self, read_only: bool) -> Result<(), DbCoreError> {
        self.apply_read_only(read_only).await
    }

    fn wrap_identifier(&self, value: &str) -> String {
        wrap_identifier_for_adapter(&self.adapter_key, value)
    }

    async fn list_databases(
        &self,
        filter: Option<&DatabaseFilter>,
    ) -> Result<Vec<String>, DbCoreError> {
        if let Backend::Live(client) = self.backend()? {
            return live_list_databases(client, filter).await;
        }
        list_databases_for_emulator(&self.extra_databases, filter)
    }

    async fn list_schemas(
        &self,
        filter: Option<&SchemaFilter>,
    ) -> Result<Vec<String>, DbCoreError> {
        match self.backend()? {
            Backend::Live(c) => live_list_schemas(c, filter).await,
            Backend::Emulated(_) => {
                list_schemas_for_adapter(&self.adapter_key, &self.default_schema, filter)
            }
        }
    }

    async fn list_tables(
        &self,
        filter: Option<&SchemaFilter>,
    ) -> Result<Vec<ListTableResult>, DbCoreError> {
        match self.backend()? {
            Backend::Live(c) => live_list_tables(c, filter).await,
            Backend::Emulated(e) => {
                list_tables_for_adapter(&self.adapter_key, &self.default_schema, e, filter)
            }
        }
    }

    async fn list_views(
        &self,
        filter: Option<&SchemaFilter>,
    ) -> Result<Vec<ListViewResult>, DbCoreError> {
        match self.backend()? {
            Backend::Live(c) => live_list_views(c, filter).await,
            Backend::Emulated(e) => {
                list_views_for_adapter(&self.adapter_key, &self.default_schema, e, filter)
            }
        }
    }

    async fn list_routines(
        &self,
        filter: Option<&SchemaFilter>,
    ) -> Result<Vec<ListRoutineResult>, DbCoreError> {
        match self.backend()? {
            Backend::Live(c) => live_list_routines(c, filter).await,
            Backend::Emulated(_) => {
                list_routines_for_adapter(&self.adapter_key, &self.default_schema, filter)
            }
        }
    }

    async fn list_table_columns(
        &self,
        table: &str,
        schema: Option<&str>,
    ) -> Result<Vec<ListTableColumnsResult>, DbCoreError> {
        match self.backend()? {
            Backend::Live(c) => live_list_columns(c, self.schema_or_default(schema), table).await,
            Backend::Emulated(e) => list_columns_for_adapter(&self.adapter_key, e, table),
        }
    }

    async fn list_table_triggers(
        &self,
        table: &str,
        schema: Option<&str>,
    ) -> Result<Vec<String>, DbCoreError> {
        match self.backend()? {
            Backend::Live(c) => live_list_triggers(c, self.schema_or_default(schema), table).await,
            Backend::Emulated(_) => list_triggers_for_adapter(&self.adapter_key, table),
        }
    }

    async fn list_table_indexes(
        &self,
        table: &str,
        schema: Option<&str>,
    ) -> Result<Vec<String>, DbCoreError> {
        match self.backend()? {
            Backend::Live(c) => live_list_indexes(c, self.schema_or_default(schema), table).await,
            Backend::Emulated(_) => list_indexes_for_adapter(&self.adapter_key, table),
        }
    }

    async fn get_table_references(
        &self,
        table: &str,
        schema: Option<&str>,
    ) -> Result<Vec<String>, DbCoreError> {
        match self.backend()? {
            Backend::Live(c) => {
                live_table_references(c, self.schema_or_default(schema), table).await
            }
            Backend::Emulated(_) => get_references_for_adapter(&self.adapter_key, table),
        }
    }

    async fn get_table_keys(
        &self,
        table: &str,
        schema: Option<&str>,
    ) -> Result<Vec<TableKeysResult>, DbCoreError> {
        match self.backend()? {
            Backend::Live(c) => live_table_keys(c, self.schema_or_default(schema), table).await,
            Backend::Emulated(_) => get_keys_for_adapter(&self.adapter_key, table),
        }
    }

    fn query_with_params(
        &self,
        query_text: &str,
        params: &QueryParams,
    ) -> Result<Arc<dyn CancellableQuery>, DbCoreError> {
        self.cancellable_query(query_text, params)
    }

    async fn execute_query_with_params(
        &self,
        query_text: &str,
        params: &QueryParams,
    ) -> Result<Vec<QueryRowResult>, DbCoreError> {
        self.run_query(query_text, params).await
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
        self.create_script(ScriptKind::Table, table)
    }

    async fn get_view_create_script(
        &self,
        view: &str,
        _schema: Option<&str>,
    ) -> Result<Vec<String>, DbCoreError> {
        self.create_script(ScriptKind::View, view)
    }

    async fn get_routine_create_script(
        &self,
        routine: &str,
        _routine_type: &str,
        _schema: Option<&str>,
    ) -> Result<Vec<String>, DbCoreError> {
        self.create_script(ScriptKind::Routine, routine)
    }

    async fn truncate_all_tables(&self, _schema: Option<&str>) -> Result<(), DbCoreError> {
        let Backend::Emulated(emulated) = self.backend()? else {
            return Err(self.live_unsupported("Truncating all tables"));
        };
        let tables = self.list_tables(None).await?;
        let conn = emulated
            .lock()
            .map_err(|e| DbCoreError::Message(e.to_string()))?;
        for t in tables {
            let _ = conn.execute(&format!("DELETE FROM {};", t.name), []);
        }
        Ok(())
    }
}
