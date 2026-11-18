//! Session and connection facade for managing database connections, global SELECT limits,
//! and multi-database server sessions.

pub mod connection;
pub mod connection_scripts;
pub mod db_facade;
pub mod db_facade_queries;
pub mod import;

pub use connection::DatabaseConnection;
pub use db_facade::DatabaseFacade;

use crate::adapters::get_adapters;
use crate::common::types::Server;
use crate::core::errors::DbCoreError;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

/// Default row limit applied to generated `SELECT TOP` / `LIMIT` queries.
pub const DEFAULT_LIMIT: u32 = 1000;

static SELECT_LIMIT: OnceLock<Mutex<Option<u32>>> = OnceLock::new();

fn select_limit_store() -> &'static Mutex<Option<u32>> {
    SELECT_LIMIT.get_or_init(|| Mutex::new(None))
}

/// Sets a process-wide override for the default `SELECT` row limit.
pub fn set_select_limit(limit: u32) {
    if let Ok(mut guard) = select_limit_store().lock() {
        *guard = Some(limit);
    }
}

/// Clears any process-wide `SELECT` row limit override, restoring [`DEFAULT_LIMIT`].
pub fn clear_select_limit() {
    if let Ok(mut guard) = select_limit_store().lock() {
        *guard = None;
    }
}

/// Returns the active `SELECT` row limit (configured override or [`DEFAULT_LIMIT`]).
pub fn get_select_limit() -> u32 {
    select_limit_store()
        .lock()
        .ok()
        .and_then(|g| *g)
        .unwrap_or(DEFAULT_LIMIT)
}

/// Represents a [`Server`] session managing multiple per-database connections.
pub struct ServerSession {
    /// Normalized server configuration.
    pub config: Server,
    /// Resolved adapter key (`postgresql`, `mysql`, `sqlite`, etc.).
    pub adapter_key: String,
    /// Map of database name to active [`DatabaseConnection`].
    pub databases: HashMap<String, DatabaseConnection>,
}

/// Validates `server_config` and creates a new [`ServerSession`].
pub fn create_server(mut server_config: Server) -> Result<ServerSession, DbCoreError> {
    let adapter_key = server_config
        .adapter
        .clone()
        .filter(|s| !s.is_empty())
        .or_else(|| {
            if !server_config.client.is_empty() {
                Some(server_config.client.clone())
            } else {
                None
            }
        })
        .ok_or(DbCoreError::MissingServerConfig)?;

    let adapters = get_adapters();
    if !adapters.iter().any(|a| a.key == adapter_key) {
        return Err(DbCoreError::InvalidAdapter(adapter_key));
    }

    server_config.adapter = Some(adapter_key.clone());
    if server_config.host.is_none() && server_config.socket_path.is_some() {
        server_config.host = server_config.socket_path.clone();
    }

    Ok(ServerSession {
        config: server_config,
        adapter_key,
        databases: HashMap::new(),
    })
}

impl ServerSession {
    /// Normalizes an optional database name into a map key.
    pub(crate) fn db_key(db_name: Option<&str>) -> String {
        db_name
            .filter(|s| !s.is_empty())
            .unwrap_or("undefined")
            .to_string()
    }

    /// Returns an immutable reference to the connection for `db_name`, if one exists.
    pub fn db(&self, db_name: Option<&str>) -> Option<&DatabaseConnection> {
        self.databases.get(&Self::db_key(db_name))
    }

    /// Returns a mutable reference to the connection for `db_name`, if one exists.
    pub fn db_mut(&mut self, db_name: Option<&str>) -> Option<&mut DatabaseConnection> {
        self.databases.get_mut(&Self::db_key(db_name))
    }

    /// Gets or creates a [`DatabaseConnection`] slot for `db_name`.
    pub fn create_connection(&mut self, db_name: Option<&str>) -> &mut DatabaseConnection {
        let key = Self::db_key(db_name);
        let cfg = self.config.clone();
        let adapter_key = self.adapter_key.clone();
        let db_opt = db_name.map(|s| s.to_string());
        self.databases
            .entry(key)
            .or_insert_with(|| DatabaseConnection::new(cfg, adapter_key, db_opt))
    }

    /// Disconnects and removes the connection for `db_name`.
    pub async fn remove_database(&mut self, db_name: Option<&str>) {
        let key = Self::db_key(db_name);
        if let Some(mut db) = self.databases.remove(&key) {
            db.disconnect().await;
        }
    }

    /// Disconnects all databases in the session.
    pub async fn end(&mut self) {
        for (_, mut db) in self.databases.drain() {
            db.disconnect().await;
        }
    }
}
