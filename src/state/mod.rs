//! Application state store (`AppStore`) combining all state slices, database session facade,
//! query bookmarks/audit history, read-only guardrail mode, and runtime query parameters.

pub mod config_servers;
pub mod connections;
pub mod metadata;
pub mod metadata_table;
pub mod queries;
pub mod scripts;
pub mod slices_meta;
pub mod slices_query;

pub use slices_meta::{
    ColumnItem, ColumnState, ConfigState, ConnectionState, DatabaseState, IndexState, NamedItem,
    RoutineItem, RoutineState, SchemaState, ServerState, TableState, TriggerState, ViewState,
};
pub use slices_query::{
    DbObjectType, ObjectScripts, QueryState, QueryTab, ScriptActionType, ScriptState,
};

use crate::adapters::DatabaseFacade;
use crate::core::bookmarks::BookmarkManager;
use crate::core::config::DEFAULT_CRYPTO_SECRET;
use std::collections::HashMap;

/// Complete application store combining all state slices and the [`DatabaseFacade`].
#[derive(Default)]
pub struct AppStore {
    /// Application settings state slice.
    pub config: ConfigState,
    /// Saved server configurations state slice.
    pub servers: ServerState,
    /// Active server/database connection state slice.
    pub connections: ConnectionState,
    /// Discovered database names state slice.
    pub databases: DatabaseState,
    /// Discovered schemas per database state slice.
    pub schemas: SchemaState,
    /// Discovered tables per database state slice.
    pub tables: TableState,
    /// Discovered columns per table state slice.
    pub columns: ColumnState,
    /// Discovered triggers per table state slice.
    pub triggers: TriggerState,
    /// Discovered indexes per table state slice.
    pub indexes: IndexState,
    /// Discovered views per database state slice.
    pub views: ViewState,
    /// Discovered functions and procedures per database state slice.
    pub routines: RoutineState,
    /// Open SQL query tabs and execution state slice.
    pub queries: QueryState,
    /// Cached DDL/DML scripts per database object state slice.
    pub sqlscripts: ScriptState,
    /// Human-readable status bar message.
    pub status: String,
    /// Underlying database session and query execution facade.
    pub db: DatabaseFacade,
    /// Whether read-only safety guardrail mode is enabled on the session.
    pub read_only_mode: bool,
    /// Persistent SQL bookmarks and execution audit history manager.
    pub bookmark_manager: BookmarkManager,
    /// Runtime `:param` / `$param` / `{{param}}` parameter bindings for SQL execution.
    pub query_params: HashMap<String, String>,
}

impl AppStore {
    /// Creates a new default [`AppStore`].
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns the active encryption secret from the loaded configuration, or [`DEFAULT_CRYPTO_SECRET`].
    pub fn crypto_secret(&self) -> String {
        self.config
            .data
            .as_ref()
            .and_then(|c| c.crypto.as_ref())
            .map(|c| c.secret.clone())
            .unwrap_or_else(|| DEFAULT_CRYPTO_SECRET.to_string())
    }

    /// Returns an immutable reference to the currently selected [`QueryTab`], if any.
    pub fn current_query(&self) -> Option<&QueryTab> {
        let id = self.queries.current_query_id?;
        self.queries.queries_by_id.get(&id)
    }

    /// Returns a mutable reference to the currently selected [`QueryTab`], if any.
    pub fn current_query_mut(&mut self) -> Option<&mut QueryTab> {
        let id = self.queries.current_query_id?;
        self.queries.queries_by_id.get_mut(&id)
    }

    /// Returns the database name associated with the active query tab or the most recent connection.
    pub fn current_database(&self) -> Option<String> {
        self.current_query()
            .map(|q| q.database.clone())
            .or_else(|| self.connections.databases.last().cloned())
    }
}
