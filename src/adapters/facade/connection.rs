//! Single-database connection lifecycle, SSH tunnel attachment, and metadata/query delegation.

use crate::adapters::facade::get_select_limit;
use crate::adapters::{
    adapter_factory, wrap_identifier_for_adapter, CancellableQuery, DatabaseAdapter,
};
use crate::common::types::{
    AdapterVersion, DatabaseFilter, ListRoutineResult, ListTableColumnsResult, ListTableResult,
    ListViewResult, QueryRowResult, SchemaFilter, Server, TableKeysResult,
};
use crate::core::errors::DbCoreError;
use crate::core::tunnel::{connect_tunnel, TunnelHandle};
use std::sync::Arc;

/// Represents a single database connection along with its optional SSH tunnel.
pub struct DatabaseConnection {
    /// Server configuration for this connection.
    pub server_config: Server,
    /// Adapter key (`postgresql`, `mysql`, `sqlite`, etc.).
    pub adapter_key: String,
    /// Target database name or path.
    pub database: Option<String>,
    /// Whether a connection handshake is currently in progress.
    pub connecting: bool,
    /// Active low-level [`DatabaseAdapter`] instance when connected.
    pub connection: Option<Box<dyn DatabaseAdapter>>,
    /// Active SSH local port-forwarding tunnel when SSH is configured.
    pub ssh_tunnel: Option<Arc<TunnelHandle>>,
}

impl DatabaseConnection {
    /// Creates a new disconnected [`DatabaseConnection`].
    pub fn new(server_config: Server, adapter_key: String, database: Option<String>) -> Self {
        Self {
            server_config,
            adapter_key,
            database,
            connecting: false,
            connection: None,
            ssh_tunnel: None,
        }
    }

    /// Verifies that the connection is established and returns a reference to the underlying adapter.
    pub(crate) fn check_is_connected(&self) -> Result<&dyn DatabaseAdapter, DbCoreError> {
        if self.connecting || self.connection.is_none() {
            return Err(DbCoreError::NoConnectionAvailable);
        }
        Ok(self.connection.as_ref().unwrap().as_ref())
    }

    /// Establishes the optional SSH tunnel and connects the underlying database adapter.
    pub async fn connect(&mut self) -> Result<(), DbCoreError> {
        if self.connecting {
            return Err(DbCoreError::ConnectionInProgress);
        }
        self.connecting = true;

        if let Some(mut existing) = self.connection.take() {
            let _ = existing.disconnect().await;
        }

        if self.server_config.ssh.is_some() && self.ssh_tunnel.is_none() {
            match connect_tunnel(&self.server_config).await {
                Ok(tunnel) => {
                    self.server_config.local_host = Some(tunnel.local_host());
                    self.server_config.local_port = Some(tunnel.local_port());
                    self.ssh_tunnel = Some(Arc::new(tunnel));
                }
                Err(e) => {
                    self.connecting = false;
                    return Err(DbCoreError::Message(e));
                }
            }
        }

        let mut adapter = match adapter_factory(
            &self.adapter_key,
            &self.server_config,
            self.database.as_deref(),
        ) {
            Ok(a) => a,
            Err(e) => {
                self.connecting = false;
                return Err(e);
            }
        };

        if let Err(err) = adapter.connect().await {
            self.connecting = false;
            return Err(err);
        }

        self.connection = Some(adapter);
        self.connecting = false;
        Ok(())
    }

    /// Disconnects the database adapter and closes any active SSH tunnel.
    pub async fn disconnect(&mut self) {
        self.connecting = false;
        if let Some(mut conn) = self.connection.take() {
            let _ = conn.disconnect().await;
        }
        if let Some(tunnel) = self.ssh_tunnel.take() {
            tunnel.close();
        }
    }

    /// Returns the connected database server version metadata.
    pub fn get_version(&self) -> Result<AdapterVersion, DbCoreError> {
        Ok(self.check_is_connected()?.get_version())
    }

    /// Quotes an identifier using the connection's dialect rules.
    pub fn wrap(&self, value: &str) -> String {
        wrap_identifier_for_adapter(&self.adapter_key, value)
    }

    /// Lists databases available on the server.
    pub async fn list_databases(
        &self,
        filter: Option<&DatabaseFilter>,
    ) -> Result<Vec<String>, DbCoreError> {
        self.check_is_connected()?.list_databases(filter).await
    }

    /// Lists schemas in the connected database.
    pub async fn list_schemas(
        &self,
        filter: Option<&SchemaFilter>,
    ) -> Result<Vec<String>, DbCoreError> {
        self.check_is_connected()?.list_schemas(filter).await
    }

    /// Lists tables in the connected database.
    pub async fn list_tables(
        &self,
        filter: Option<&SchemaFilter>,
    ) -> Result<Vec<ListTableResult>, DbCoreError> {
        self.check_is_connected()?.list_tables(filter).await
    }

    /// Lists views in the connected database.
    pub async fn list_views(
        &self,
        filter: Option<&SchemaFilter>,
    ) -> Result<Vec<ListViewResult>, DbCoreError> {
        self.check_is_connected()?.list_views(filter).await
    }

    /// Lists functions and stored procedures in the connected database.
    pub async fn list_routines(
        &self,
        filter: Option<&SchemaFilter>,
    ) -> Result<Vec<ListRoutineResult>, DbCoreError> {
        self.check_is_connected()?.list_routines(filter).await
    }

    /// Lists columns and data types for `table`.
    pub async fn list_table_columns(
        &self,
        table: &str,
        schema: Option<&str>,
    ) -> Result<Vec<ListTableColumnsResult>, DbCoreError> {
        self.check_is_connected()?
            .list_table_columns(table, schema)
            .await
    }

    /// Lists triggers defined on `table`.
    pub async fn list_table_triggers(
        &self,
        table: &str,
        schema: Option<&str>,
    ) -> Result<Vec<String>, DbCoreError> {
        self.check_is_connected()?
            .list_table_triggers(table, schema)
            .await
    }

    /// Lists indexes defined on `table`.
    pub async fn list_table_indexes(
        &self,
        table: &str,
        schema: Option<&str>,
    ) -> Result<Vec<String>, DbCoreError> {
        self.check_is_connected()?
            .list_table_indexes(table, schema)
            .await
    }

    /// Lists tables referenced via foreign keys by `table`.
    pub async fn get_table_references(
        &self,
        table: &str,
        schema: Option<&str>,
    ) -> Result<Vec<String>, DbCoreError> {
        self.check_is_connected()?
            .get_table_references(table, schema)
            .await
    }

    /// Lists primary and foreign key constraints on `table`.
    pub async fn get_table_keys(
        &self,
        table: &str,
        schema: Option<&str>,
    ) -> Result<Vec<TableKeysResult>, DbCoreError> {
        self.check_is_connected()?
            .get_table_keys(table, schema)
            .await
    }

    /// Creates a cancellable query handle for `query_text`.
    pub fn query(&self, query_text: &str) -> Result<Arc<dyn CancellableQuery>, DbCoreError> {
        self.check_is_connected()?.query(query_text)
    }

    /// Executes `query_text` to completion.
    pub async fn execute_query(
        &self,
        query_text: &str,
    ) -> Result<Vec<QueryRowResult>, DbCoreError> {
        self.check_is_connected()?.execute_query(query_text).await
    }

    /// Builds a dialect-appropriate `SELECT TOP N` / `LIMIT N` query for `table`.
    pub async fn get_query_select_top(
        &self,
        table: &str,
        schema: Option<&str>,
        limit: Option<u32>,
    ) -> Result<String, DbCoreError> {
        let limit_val = limit.unwrap_or_else(get_select_limit);
        self.check_is_connected()?
            .get_query_select_top(table, limit_val, schema)
            .await
    }
}
