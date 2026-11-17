//! State slice structures for configuration, servers, connections, and database metadata
//! (databases, schemas, tables, columns, triggers, indexes, views, and routines).

use crate::common::types::{
    Config, ConnectionMode, DbTable, DbView, HostKeyChallenge, Server, ValidationErrors,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// State slice tracking application configuration loading, editing, and persistence.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ConfigState {
    /// Whether configuration is currently being written to disk.
    pub is_saving: bool,
    /// Whether the settings modal/editor is currently open.
    pub is_editing: bool,
    /// Resolved filesystem path of the configuration file.
    pub path: Option<String>,
    /// Loaded and sanitized configuration payload.
    pub data: Option<Config>,
    /// Most recent configuration load/save error message.
    pub error: Option<String>,
    /// Whether the configuration has been loaded at least once.
    pub is_loaded: bool,
}

/// State slice tracking saved server configurations and the server editor modal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ServerState {
    /// Field-level validation errors from the most recent save attempt.
    pub error: Option<ValidationErrors>,
    /// Whether a server entry is currently being saved.
    pub is_saving: bool,
    /// Whether the server add/edit modal is currently active.
    pub is_editing: bool,
    /// List of configured database servers.
    pub items: Vec<Server>,
    /// Decrypted server currently being edited in the modal.
    pub editing_server: Option<Server>,
}

/// State slice tracking active database connections and connection testing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionState {
    /// Connection error message, if the last connection attempt failed.
    pub error: Option<String>,
    /// Whether a server session is currently connected.
    pub connected: bool,
    /// Whether a connection handshake is currently in progress.
    pub connecting: bool,
    /// Active server configuration for the current session.
    pub server: Option<Server>,
    /// Adapter feature keys disabled for the current dialect.
    pub disabled_features: Vec<String>,
    /// Whether the connection is awaiting an interactive SSH private-key passphrase.
    pub waiting_private_key_passphrase: bool,
    /// List of connected database names in the current session.
    pub databases: Vec<String>,
    /// Whether the most recent "Test Connection" probe succeeded.
    pub test_connected: bool,
    /// Whether a "Test Connection" probe is currently running.
    pub test_connecting: bool,
    /// Server configuration used for the most recent test connection probe.
    pub test_server: Option<Server>,
    /// Error message from the most recent test connection probe, if any.
    pub test_error: Option<String>,
    /// Live or emulated mode of the connected session. Emulated sessions must show a banner.
    pub mode: Option<ConnectionMode>,
    /// SSH bastion host key awaiting the user's approval or re-confirmation.
    pub host_key_prompt: Option<HostKeyChallenge>,
    /// Server to reconnect once [`Self::host_key_prompt`] is approved (`None` for test probes).
    pub host_key_retry_server_id: Option<String>,
}

/// Simple wrapper around an object's name used in state slices.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct NamedItem {
    /// Identifier name of the database, schema, trigger, or index.
    pub name: String,
}

/// State slice tracking discovered databases on the connected server.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DatabaseState {
    /// Error from the last database list fetch.
    pub error: Option<String>,
    /// Whether databases are currently being fetched.
    pub is_fetching: bool,
    /// Whether cached database items are stale and need refreshing.
    pub did_invalidate: bool,
    /// Discovered database items.
    pub items: Vec<NamedItem>,
    /// UI filter text for narrowing displayed databases.
    pub filter_name: String,
}

impl Default for DatabaseState {
    fn default() -> Self {
        Self {
            error: None,
            is_fetching: false,
            did_invalidate: true,
            items: Vec::new(),
            filter_name: String::new(),
        }
    }
}

/// State slice tracking discovered schemas keyed by database name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SchemaState {
    /// Error from the last schema list fetch.
    pub error: Option<String>,
    /// Whether schemas are currently being fetched.
    pub is_fetching: bool,
    /// Whether cached schema items are stale.
    pub did_invalidate: bool,
    /// Schemas grouped by database name.
    pub items_by_database: HashMap<String, Vec<NamedItem>>,
}

impl Default for SchemaState {
    fn default() -> Self {
        Self {
            error: None,
            is_fetching: false,
            did_invalidate: true,
            items_by_database: HashMap::new(),
        }
    }
}

/// State slice tracking discovered tables keyed by database name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TableState {
    /// Error from the last table list fetch.
    pub error: Option<String>,
    /// Whether tables are currently being fetched.
    pub is_fetching: bool,
    /// Whether cached table items are stale.
    pub did_invalidate: bool,
    /// Tables grouped by database name.
    pub items_by_database: HashMap<String, Vec<DbTable>>,
}

impl Default for TableState {
    fn default() -> Self {
        Self {
            error: None,
            is_fetching: false,
            did_invalidate: true,
            items_by_database: HashMap::new(),
        }
    }
}

/// Column metadata item stored inside [`ColumnState`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ColumnItem {
    /// Column name.
    pub name: String,
    /// Dialect-specific SQL data type.
    pub data_type: String,
}

/// State slice tracking discovered table columns keyed by database and table name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ColumnState {
    /// Error from the last column fetch.
    pub error: Option<String>,
    /// Whether cached column metadata is stale.
    pub did_invalidate: bool,
    /// Per-database, per-table fetch in-progress flags.
    pub is_fetching: HashMap<String, HashMap<String, bool>>,
    /// Columns grouped by database name and table name.
    pub columns_by_table: HashMap<String, HashMap<String, Vec<ColumnItem>>>,
}

/// State slice tracking discovered table triggers keyed by database and table name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct TriggerState {
    /// Error from the last trigger fetch.
    pub error: Option<String>,
    /// Whether triggers are currently being fetched.
    pub is_fetching: bool,
    /// Whether cached trigger metadata is stale.
    pub did_invalidate: bool,
    /// Triggers grouped by database name and table name.
    pub triggers_by_table: HashMap<String, HashMap<String, Vec<NamedItem>>>,
}

/// State slice tracking discovered table indexes keyed by database and table name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct IndexState {
    /// Error from the last index fetch.
    pub error: Option<String>,
    /// Whether indexes are currently being fetched.
    pub is_fetching: bool,
    /// Whether cached index metadata is stale.
    pub did_invalidate: bool,
    /// Indexes grouped by database name and table name.
    pub indexes_by_table: HashMap<String, HashMap<String, Vec<NamedItem>>>,
}

/// State slice tracking discovered views keyed by database name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ViewState {
    /// Error from the last view fetch.
    pub error: Option<String>,
    /// Whether views are currently being fetched.
    pub is_fetching: bool,
    /// Whether cached view metadata is stale.
    pub did_invalidate: bool,
    /// Views grouped by database name.
    pub views_by_database: HashMap<String, Vec<DbView>>,
}

/// Stored function or procedure item inside [`RoutineState`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RoutineItem {
    /// Optional schema name.
    pub schema: Option<String>,
    /// Routine name.
    pub name: String,
    /// Optional routine DDL definition.
    pub routine_definition: Option<String>,
}

/// State slice tracking discovered functions and stored procedures keyed by database name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct RoutineState {
    /// Error from the last routine fetch.
    pub error: Option<String>,
    /// Whether routines are currently being fetched.
    pub is_fetching: bool,
    /// Whether cached routine metadata is stale.
    pub did_invalidate: bool,
    /// Functions grouped by database name.
    pub functions_by_database: HashMap<String, Vec<RoutineItem>>,
    /// Stored procedures grouped by database name.
    pub procedures_by_database: HashMap<String, Vec<RoutineItem>>,
}
