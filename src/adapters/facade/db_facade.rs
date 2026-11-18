//! High-level [`DatabaseFacade`] coordinating active server sessions and database metadata discovery.

use crate::adapters::facade::{create_server, DatabaseConnection, ServerSession};
use crate::adapters::{get_adapters, CancellableQuery};
use crate::common::types::{
    AdapterInfo, ConnectionMode, DatabaseFilter, ListRoutineResult, ListTableColumnsResult,
    ListTableResult, ListViewResult, SchemaFilter, Server, TableKeysResult,
};
use crate::core::errors::DbCoreError;
use crate::core::keys::SharedKeyProvider;
use std::collections::HashMap;
use std::sync::Arc;

/// High-level database facade managing active server sessions and cancellable queries.
#[derive(Default)]
pub struct DatabaseFacade {
    /// Active server session, if connected.
    pub server_session: Option<ServerSession>,
    /// Registry of active cancellable queries keyed by numeric query ID.
    pub cancellable_queries: HashMap<u64, Arc<dyn CancellableQuery>>,
    /// Read-Only mode for every connection in the session (see [`Self::set_read_only`]).
    pub read_only: bool,
    /// Master-key source passed to every connection (decrypts SSH host-key pins).
    pub keys: SharedKeyProvider,
}

impl DatabaseFacade {
    /// Creates a new disconnected [`DatabaseFacade`].
    pub fn new() -> Self {
        Self::default()
    }

    pub(crate) fn get_db(&self, database: &str) -> Result<&DatabaseConnection, String> {
        let session = self
            .server_session
            .as_ref()
            .ok_or_else(|| format!("No database found {database}"))?;
        session
            .db(Some(database))
            .or_else(|| session.db(None))
            .ok_or_else(|| format!("No database found {database}"))
    }

    /// Returns static metadata for all supported database adapters.
    pub fn get_clients_sync(&self) -> Vec<AdapterInfo> {
        get_adapters()
    }

    /// Connects to `server` and `database`, creating a [`ServerSession`] if needed.
    pub async fn connect(&mut self, server: Server, database: Option<&str>) -> Result<(), String> {
        self.connect_checked(server, database)
            .await
            .map_err(|e| e.to_string())
    }

    /// Like [`Self::connect`], but keeps the typed error so callers can react to SSH host-key
    /// challenges ([`DbCoreError::Tunnel`]).
    pub async fn connect_checked(
        &mut self,
        server: Server,
        database: Option<&str>,
    ) -> Result<(), DbCoreError> {
        if self.server_session.is_none() {
            self.server_session = Some(create_server(server)?);
        }
        let session = self.server_session.as_mut().unwrap();
        let db_conn = session.create_connection(database);
        db_conn.read_only = self.read_only;
        db_conn.keys = self.keys.clone();
        if let Err(err) = db_conn.connect().await {
            let db_key = ServerSession::db_key(database);
            if let Some(mut removed) = session.databases.remove(&db_key) {
                removed.disconnect().await;
            }
            if session.databases.is_empty() {
                session.end().await;
                self.server_session = None;
            }
            return Err(err);
        }
        Ok(())
    }

    /// Adapter key of the connected server (`postgresql`, `sqlite`, ...), if any.
    pub fn adapter_key(&self) -> Option<&str> {
        self.server_session.as_ref().map(|s| s.adapter_key.as_str())
    }

    /// Turns Read-Only mode on or off for every open connection and for later connections.
    /// The database enforces it in the session as well as the analyzer.
    pub async fn set_read_only(&mut self, read_only: bool) -> Result<(), String> {
        if self.read_only == read_only {
            return Ok(());
        }
        if let Some(session) = self.server_session.as_mut() {
            for conn in session.databases.values_mut() {
                conn.set_read_only(read_only)
                    .await
                    .map_err(|e| e.to_string())?;
            }
        }
        self.read_only = read_only;
        Ok(())
    }

    /// Returns whether `database`'s connection is live or emulated.
    pub fn connection_mode(&self, database: &str) -> Result<ConnectionMode, String> {
        self.get_db(database)?
            .connection_mode()
            .map_err(|e| e.to_string())
    }

    /// Disconnects a specific `database` from the active server session.
    pub async fn disconnect_database(&mut self, database: Option<&str>) -> Result<(), String> {
        if let Some(ref mut session) = self.server_session {
            session.remove_database(database).await;
            Ok(())
        } else {
            Err(format!("No database found {}", database.unwrap_or("")))
        }
    }

    /// Disconnects the entire server session and closes all database connections.
    pub async fn disconnect_server(&mut self) -> Result<(), String> {
        if let Some(mut session) = self.server_session.take() {
            session.end().await;
        }
        Ok(())
    }

    /// Lists databases available on `database`'s connection.
    pub async fn list_databases(
        &self,
        database: &str,
        filter: Option<&DatabaseFilter>,
    ) -> Result<Vec<String>, String> {
        self.get_db(database)?
            .list_databases(filter)
            .await
            .map_err(|e| e.to_string())
    }

    /// Lists schemas in `database`.
    pub async fn list_schemas(
        &self,
        database: &str,
        filter: Option<&SchemaFilter>,
    ) -> Result<Vec<String>, String> {
        self.get_db(database)?
            .list_schemas(filter)
            .await
            .map_err(|e| e.to_string())
    }

    /// Lists tables in `database`.
    pub async fn list_tables(
        &self,
        database: &str,
        filter: Option<&SchemaFilter>,
    ) -> Result<Vec<ListTableResult>, String> {
        self.get_db(database)?
            .list_tables(filter)
            .await
            .map_err(|e| e.to_string())
    }

    /// Lists views in `database`.
    pub async fn list_views(
        &self,
        database: &str,
        filter: Option<&SchemaFilter>,
    ) -> Result<Vec<ListViewResult>, String> {
        self.get_db(database)?
            .list_views(filter)
            .await
            .map_err(|e| e.to_string())
    }

    /// Lists routines in `database`.
    pub async fn list_routines(
        &self,
        database: &str,
        filter: Option<&SchemaFilter>,
    ) -> Result<Vec<ListRoutineResult>, String> {
        self.get_db(database)?
            .list_routines(filter)
            .await
            .map_err(|e| e.to_string())
    }

    /// Lists columns for `table` in `database`.
    pub async fn list_table_columns(
        &self,
        database: &str,
        table: &str,
        schema: Option<&str>,
    ) -> Result<Vec<ListTableColumnsResult>, String> {
        self.get_db(database)?
            .list_table_columns(table, schema)
            .await
            .map_err(|e| e.to_string())
    }

    /// Lists triggers for `table` in `database`.
    pub async fn list_table_triggers(
        &self,
        database: &str,
        table: &str,
        schema: Option<&str>,
    ) -> Result<Vec<String>, String> {
        self.get_db(database)?
            .list_table_triggers(table, schema)
            .await
            .map_err(|e| e.to_string())
    }

    /// Lists indexes for `table` in `database`.
    pub async fn list_table_indexes(
        &self,
        database: &str,
        table: &str,
        schema: Option<&str>,
    ) -> Result<Vec<String>, String> {
        self.get_db(database)?
            .list_table_indexes(table, schema)
            .await
            .map_err(|e| e.to_string())
    }

    /// Lists foreign-key referenced tables for `table` in `database`.
    pub async fn get_table_references(
        &self,
        database: &str,
        table: &str,
        schema: Option<&str>,
    ) -> Result<Vec<String>, String> {
        self.get_db(database)?
            .get_table_references(table, schema)
            .await
            .map_err(|e| e.to_string())
    }

    /// Lists primary and foreign key constraints for `table` in `database`.
    pub async fn get_table_keys(
        &self,
        database: &str,
        table: &str,
        schema: Option<&str>,
    ) -> Result<Vec<TableKeysResult>, String> {
        self.get_db(database)?
            .get_table_keys(table, schema)
            .await
            .map_err(|e| e.to_string())
    }
}
