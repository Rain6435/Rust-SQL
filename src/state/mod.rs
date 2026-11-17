use crate::adapters::{DatabaseFacade, CLIENTS};
use crate::common::types::{
    BaseConfig, Config, DbTable, DbView, ListTableColumnsResult, QueryRowResult, Server,
    TableKeysResult, ValidationErrors,
};
use crate::core::bookmarks::BookmarkManager;
use crate::core::config::{self, DEFAULT_CRYPTO_SECRET};
use crate::core::params::bind_query_parameters;
use crate::core::result_ops::generate_mermaid_erd;
use crate::core::safety::{analyze_query_safety, QuerySafetyReport};
use crate::core::servers;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::time::Instant;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ConfigState {
    pub is_saving: bool,
    pub is_editing: bool,
    pub path: Option<String>,
    pub data: Option<Config>,
    pub error: Option<String>,
    pub is_loaded: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ServerState {
    pub error: Option<ValidationErrors>,
    pub is_saving: bool,
    pub is_editing: bool,
    pub items: Vec<Server>,
    pub editing_server: Option<Server>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionState {
    pub error: Option<String>,
    pub connected: bool,
    pub connecting: bool,
    pub server: Option<Server>,
    pub disabled_features: Vec<String>,
    pub waiting_private_key_passphrase: bool,
    pub databases: Vec<String>,
    pub test_connected: bool,
    pub test_connecting: bool,
    pub test_server: Option<Server>,
    pub test_error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct NamedItem {
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DatabaseState {
    pub error: Option<String>,
    pub is_fetching: bool,
    pub did_invalidate: bool,
    pub items: Vec<NamedItem>,
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SchemaState {
    pub error: Option<String>,
    pub is_fetching: bool,
    pub did_invalidate: bool,
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TableState {
    pub error: Option<String>,
    pub is_fetching: bool,
    pub did_invalidate: bool,
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ColumnItem {
    pub name: String,
    pub data_type: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ColumnState {
    pub error: Option<String>,
    pub did_invalidate: bool,
    pub is_fetching: HashMap<String, HashMap<String, bool>>,
    pub columns_by_table: HashMap<String, HashMap<String, Vec<ColumnItem>>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct TriggerState {
    pub error: Option<String>,
    pub is_fetching: bool,
    pub did_invalidate: bool,
    pub triggers_by_table: HashMap<String, HashMap<String, Vec<NamedItem>>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct IndexState {
    pub error: Option<String>,
    pub is_fetching: bool,
    pub did_invalidate: bool,
    pub indexes_by_table: HashMap<String, HashMap<String, Vec<NamedItem>>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ViewState {
    pub error: Option<String>,
    pub is_fetching: bool,
    pub did_invalidate: bool,
    pub views_by_database: HashMap<String, Vec<DbView>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RoutineItem {
    pub schema: Option<String>,
    pub name: String,
    pub routine_definition: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct RoutineState {
    pub error: Option<String>,
    pub is_fetching: bool,
    pub did_invalidate: bool,
    pub functions_by_database: HashMap<String, Vec<RoutineItem>>,
    pub procedures_by_database: HashMap<String, Vec<RoutineItem>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QueryTab {
    pub id: u64,
    pub database: String,
    pub name: String,
    pub filename: Option<String>,
    pub is_executing: bool,
    pub is_default_select: bool,
    pub did_invalidate: bool,
    pub query: String,
    pub selected_query: String,
    pub query_history: Vec<String>,
    pub results: Option<Vec<QueryRowResult>>,
    pub error: Option<String>,
    pub copied: Option<bool>,
    pub saved: Option<bool>,
    pub is_canceling: bool,
    pub result_items_per_page: u32,
    pub execution_time: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QueryState {
    pub last_created_id: u64,
    pub current_query_id: Option<u64>,
    pub query_ids: Vec<u64>,
    pub queries_by_id: HashMap<u64, QueryTab>,
    pub result_items_per_page: u32,
    pub enabled_auto_complete: bool,
    pub enabled_live_auto_complete: bool,
}

impl Default for QueryState {
    fn default() -> Self {
        Self {
            last_created_id: 0,
            current_query_id: None,
            query_ids: Vec::new(),
            queries_by_id: HashMap::new(),
            result_items_per_page: 100,
            enabled_auto_complete: true,
            enabled_live_auto_complete: true,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ScriptActionType {
    #[serde(rename = "CREATE")]
    Create,
    #[serde(rename = "SELECT")]
    Select,
    #[serde(rename = "INSERT")]
    Insert,
    #[serde(rename = "UPDATE")]
    Update,
    #[serde(rename = "DELETE")]
    Delete,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DbObjectType {
    Table,
    View,
    Function,
    Procedure,
}

impl DbObjectType {
    pub fn as_str(&self) -> &'static str {
        match self {
            DbObjectType::Table => "Table",
            DbObjectType::View => "View",
            DbObjectType::Function => "Function",
            DbObjectType::Procedure => "Procedure",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ObjectScripts {
    pub object_type: DbObjectType,
    pub create: Option<String>,
    pub select: Option<String>,
    pub insert: Option<String>,
    pub update: Option<String>,
    pub delete: Option<String>,
}

impl ObjectScripts {
    pub fn new(object_type: DbObjectType) -> Self {
        Self {
            object_type,
            create: None,
            select: None,
            insert: None,
            update: None,
            delete: None,
        }
    }

    pub fn get(&self, action: ScriptActionType) -> Option<&str> {
        match action {
            ScriptActionType::Create => self.create.as_deref(),
            ScriptActionType::Select => self.select.as_deref(),
            ScriptActionType::Insert => self.insert.as_deref(),
            ScriptActionType::Update => self.update.as_deref(),
            ScriptActionType::Delete => self.delete.as_deref(),
        }
    }

    pub fn set(&mut self, action: ScriptActionType, script: String) {
        match action {
            ScriptActionType::Create => self.create = Some(script),
            ScriptActionType::Select => self.select = Some(script),
            ScriptActionType::Insert => self.insert = Some(script),
            ScriptActionType::Update => self.update = Some(script),
            ScriptActionType::Delete => self.delete = Some(script),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ScriptState {
    pub error: Option<String>,
    pub is_fetching: bool,
    pub did_invalidate: bool,
    pub scripts_by_object: HashMap<String, HashMap<String, ObjectScripts>>,
}

/// Complete application store combining all 14 Redux state slices and the `DatabaseFacade`.
#[derive(Default)]
pub struct AppStore {
    pub config: ConfigState,
    pub servers: ServerState,
    pub connections: ConnectionState,
    pub databases: DatabaseState,
    pub schemas: SchemaState,
    pub tables: TableState,
    pub columns: ColumnState,
    pub triggers: TriggerState,
    pub indexes: IndexState,
    pub views: ViewState,
    pub routines: RoutineState,
    pub queries: QueryState,
    pub sqlscripts: ScriptState,
    pub status: String,
    pub db: DatabaseFacade,
    pub read_only_mode: bool,
    pub bookmark_manager: BookmarkManager,
    pub query_params: HashMap<String, String>,
}

impl AppStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn crypto_secret(&self) -> String {
        self.config
            .data
            .as_ref()
            .and_then(|c| c.crypto.as_ref())
            .map(|c| c.secret.clone())
            .unwrap_or_else(|| DEFAULT_CRYPTO_SECRET.to_string())
    }

    pub fn current_query(&self) -> Option<&QueryTab> {
        let id = self.queries.current_query_id?;
        self.queries.queries_by_id.get(&id)
    }

    pub fn current_query_mut(&mut self) -> Option<&mut QueryTab> {
        let id = self.queries.current_query_id?;
        self.queries.queries_by_id.get_mut(&id)
    }

    pub fn current_database(&self) -> Option<String> {
        self.current_query()
            .map(|q| q.database.clone())
            .or_else(|| self.connections.databases.last().cloned())
    }

    // ========================================================================
    // 1. Config Actions
    // ========================================================================

    pub fn load_config(&mut self) -> Result<(), String> {
        let path = config::path();
        match config::get_full_sync(true) {
            Ok(cfg) => {
                if let Some(limit) = cfg.limit_query_default_select_top.checked_into() {
                    self.db.set_select_limit(limit);
                }
                self.servers.items = cfg.servers.clone();
                self.config.data = Some(cfg);
                self.config.path = Some(path);
                self.config.is_loaded = true;
                self.config.error = None;
                Ok(())
            }
            Err(err) => {
                self.config.error = Some(err.clone());
                self.config.is_loaded = true;
                Err(err)
            }
        }
    }

    pub fn start_editing_config(&mut self) {
        self.config.is_saving = false;
        self.config.is_editing = true;
    }

    pub fn finish_editing_config(&mut self) {
        self.config.is_saving = false;
        self.config.is_editing = false;
        self.config.is_loaded = true;
        self.config.error = None;
    }

    pub fn save_config(&mut self, config_data: &BaseConfig) -> Result<(), String> {
        self.config.is_saving = true;
        config::save_settings_sync(config_data)?;
        if let Some(limit) = config_data.limit_query_default_select_top {
            self.db.set_select_limit(limit);
        }
        let refreshed = config::get_full_sync(true)?;
        self.servers.items = refreshed.servers.clone();
        self.config.data = Some(refreshed);
        self.config.is_saving = false;
        self.config.is_editing = false;
        Ok(())
    }

    // ========================================================================
    // 2. Server Actions
    // ========================================================================

    pub fn start_editing_server(&mut self, id: Option<&str>) -> Result<(), String> {
        self.connections = ConnectionState::default();
        let Some(server_id) = id.filter(|s| !s.is_empty()) else {
            self.servers.is_saving = false;
            self.servers.is_editing = true;
            self.servers.editing_server = None;
            self.servers.error = None;
            return Ok(());
        };

        let secret = self.crypto_secret();
        let Some(srv) = self
            .servers
            .items
            .iter()
            .find(|s| s.id == server_id)
            .cloned()
        else {
            return Err("Server not found".to_string());
        };
        let decrypted = servers::decrypt_secrets(&srv, &secret)?;
        self.servers.is_saving = false;
        self.servers.is_editing = true;
        self.servers.editing_server = Some(decrypted);
        self.servers.error = None;
        Ok(())
    }

    pub fn finish_editing_server(&mut self) {
        self.connections = ConnectionState::default();
        self.servers.is_saving = false;
        self.servers.is_editing = false;
        self.servers.editing_server = None;
        self.servers.error = None;
    }

    pub fn save_server(
        &mut self,
        mut server: Server,
        id: Option<&str>,
    ) -> Result<Server, ValidationErrors> {
        self.servers.is_saving = true;
        if let Some(existing_id) = id {
            server.id = existing_id.to_string();
        }
        let secret = self.crypto_secret();
        match servers::add_or_update_sync(&server, &secret) {
            Ok(res) => {
                if let Some(errs) = res.validation_errors {
                    self.servers.error = Some(errs.clone());
                    self.servers.is_saving = false;
                    return Err(errs);
                }
                let saved = res.data.unwrap();
                let _ = self.load_config();
                self.servers.error = None;
                self.servers.is_saving = false;
                self.servers.is_editing = false;
                self.servers.editing_server = None;
                Ok(saved)
            }
            Err(e) => {
                let mut map = ValidationErrors::new();
                map.insert("general".to_string(), e);
                self.servers.error = Some(map.clone());
                self.servers.is_saving = false;
                Err(map)
            }
        }
    }

    pub fn duplicate_server(&mut self, mut server: Server) -> Result<Server, ValidationErrors> {
        self.servers.is_saving = true;
        let all = servers::get_all_sync().unwrap_or_default();
        server.name = servers::get_unique_name(&server, &all);
        server.id = String::new();
        self.save_server(server, None)
    }

    pub fn remove_server(&mut self, id: &str) -> Result<(), String> {
        self.servers.is_saving = true;
        servers::remove_by_id_sync(id)?;
        self.servers.items.retain(|s| s.id != id);
        self.servers.error = None;
        self.servers.is_saving = false;
        self.servers.is_editing = false;
        let _ = self.load_config();
        Ok(())
    }

    // ========================================================================
    // 3. Connection Actions
    // ========================================================================

    pub fn set_connecting(&mut self) {
        self.connections = ConnectionState {
            connecting: true,
            ..Default::default()
        };
    }

    pub async fn connect(
        &mut self,
        server_id: &str,
        database_name: Option<&str>,
        reconnecting: bool,
        ssh_passphrase: Option<String>,
    ) -> Result<(), String> {
        if reconnecting {
            self.set_connecting();
            let _ = self.db.disconnect_server().await;
        }

        let all_servers = servers::get_all_sync()?;
        let server = all_servers
            .into_iter()
            .find(|s| s.id == server_id)
            .ok_or_else(|| "Server configuration not found".to_string())?;

        let secret = self.crypto_secret();
        let mut decrypted_server = servers::decrypt_secrets(&server, &secret)?;

        let db_client = CLIENTS.iter().find(|c| c.key == decrypted_server.client);
        let default_database = db_client.and_then(|c| c.default_database.clone());
        let target_db = database_name
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
            .or_else(|| decrypted_server.database.clone().filter(|s| !s.is_empty()))
            .or(default_database)
            .unwrap_or_else(|| "postgres".to_string());

        let is_server_connection = database_name.is_none();
        self.status = "Connecting to database...".to_string();

        if is_server_connection {
            self.databases = DatabaseState::default();
            self.schemas = SchemaState::default();
            self.tables = TableState::default();
            self.columns = ColumnState {
                did_invalidate: true,
                ..Default::default()
            };
            self.triggers = TriggerState {
                did_invalidate: true,
                ..Default::default()
            };
            self.indexes = IndexState {
                did_invalidate: true,
                ..Default::default()
            };
            self.views = ViewState {
                did_invalidate: true,
                ..Default::default()
            };
            self.routines = RoutineState {
                did_invalidate: true,
                ..Default::default()
            };
            self.sqlscripts = ScriptState {
                did_invalidate: true,
                ..Default::default()
            };
        }

        self.connections.server = Some(decrypted_server.clone());
        self.connections.disabled_features = db_client
            .map(|c| c.disabled_features.clone())
            .unwrap_or_default();
        self.connections.connected = false;
        self.connections.connecting = true;
        self.connections.error = None;

        if !self.connections.connected {
            if let Some(ref mut ssh) = decrypted_server.ssh {
                if ssh.private_key_with_passphrase == Some(true) {
                    if ssh_passphrase.is_none() {
                        self.connections.waiting_private_key_passphrase = true;
                        self.connections.connecting = false;
                        return Ok(());
                    }
                    ssh.passphrase = ssh_passphrase;
                }
            }
        }

        let was_already_connected = !self.connections.databases.is_empty();
        if let Err(err) = self
            .db
            .connect(decrypted_server.clone(), Some(&target_db))
            .await
        {
            self.connections.connected = false;
            self.connections.connecting = false;
            self.connections.waiting_private_key_passphrase = false;
            self.connections.error = Some(err.clone());
            self.status.clear();
            return Err(err);
        }

        self.connections.connected = true;
        self.connections.connecting = false;
        self.connections.waiting_private_key_passphrase = false;
        if !self.connections.databases.contains(&target_db) {
            self.connections.databases.push(target_db.clone());
        }
        self.status = "Connection to database established".to_string();

        if let Some(ref cfg) = self.config.data {
            self.queries.enabled_auto_complete = cfg.enabled_auto_complete;
            self.queries.enabled_live_auto_complete = cfg.enabled_live_auto_complete;
        }

        let skip_new_tab = reconnecting || (was_already_connected && database_name.is_none());
        if !skip_new_tab {
            self.add_new_query(&target_db, None);
        }

        Ok(())
    }

    pub async fn disconnect(&mut self) {
        let _ = self.db.disconnect_server().await;
        self.connections = ConnectionState::default();
        self.queries = QueryState::default();
        self.status.clear();
    }

    pub async fn reconnect(&mut self, server_id: &str, database_name: &str) -> Result<(), String> {
        self.connect(server_id, Some(database_name), true, None)
            .await
    }

    pub async fn test_connection(&mut self, mut server: Server) -> Result<(), String> {
        let db_client = CLIENTS.iter().find(|c| c.key == server.client);
        if server
            .database
            .as_ref()
            .map(|s| s.is_empty())
            .unwrap_or(true)
        {
            server.database = db_client.and_then(|c| c.default_database.clone());
        }
        self.connections.test_connected = false;
        self.connections.test_connecting = true;
        self.connections.test_server = Some(server.clone());
        self.connections.test_error = None;

        let mut test_facade = DatabaseFacade::new();
        let db = server.database.clone();
        match test_facade.connect(server, db.as_deref()).await {
            Ok(()) => {
                let _ = test_facade.disconnect_server().await;
                self.connections.test_connected = true;
                self.connections.test_connecting = false;
                Ok(())
            }
            Err(err) => {
                self.connections.test_connected = false;
                self.connections.test_connecting = false;
                self.connections.test_error = Some(err.clone());
                Err(err)
            }
        }
    }

    // ========================================================================
    // 4. Metadata Fetching Actions
    // ========================================================================

    pub fn refresh_database(&mut self, _database: &str) {
        self.schemas.did_invalidate = true;
        self.tables.did_invalidate = true;
        self.columns.did_invalidate = true;
        self.triggers.did_invalidate = true;
        self.indexes.did_invalidate = true;
        self.views.did_invalidate = true;
        self.routines.did_invalidate = true;
        self.sqlscripts.did_invalidate = true;
    }

    pub async fn fetch_databases_if_needed(&mut self) -> Result<(), String> {
        if self.databases.is_fetching || !self.databases.did_invalidate {
            return Ok(());
        }
        let Some(db_name) = self.current_database() else {
            return Ok(());
        };
        let filter = self
            .connections
            .server
            .as_ref()
            .and_then(|s| s.filter.as_ref())
            .map(|f| f.to_database_filter());

        self.databases.is_fetching = true;
        self.databases.did_invalidate = false;
        self.databases.error = None;

        match self.db.list_databases(&db_name, filter.as_ref()).await {
            Ok(list) => {
                self.databases.is_fetching = false;
                self.databases.items = list.into_iter().map(|name| NamedItem { name }).collect();
                Ok(())
            }
            Err(err) => {
                self.databases.is_fetching = false;
                self.databases.did_invalidate = true;
                self.databases.error = Some(err.clone());
                Err(err)
            }
        }
    }

    pub async fn fetch_schemas_if_needed(&mut self, database: &str) -> Result<(), String> {
        let should_fetch = !self.schemas.is_fetching
            && (!self.schemas.items_by_database.contains_key(database)
                || self.schemas.did_invalidate);
        if !should_fetch {
            return Ok(());
        }
        self.schemas.is_fetching = true;
        self.schemas.did_invalidate = false;
        self.schemas.error = None;

        match self.db.list_schemas(database, None).await {
            Ok(list) => {
                self.schemas.is_fetching = false;
                self.schemas.items_by_database.insert(
                    database.to_string(),
                    list.into_iter().map(|name| NamedItem { name }).collect(),
                );
                Ok(())
            }
            Err(err) => {
                self.schemas.is_fetching = false;
                self.schemas.did_invalidate = true;
                self.schemas.error = Some(err.clone());
                Err(err)
            }
        }
    }

    pub async fn fetch_tables_if_needed(&mut self, database: &str) -> Result<(), String> {
        let should_fetch = !self.tables.is_fetching
            && (!self.tables.items_by_database.contains_key(database)
                || self.tables.did_invalidate);
        if !should_fetch {
            return Ok(());
        }
        let filter = self
            .connections
            .server
            .as_ref()
            .and_then(|s| s.filter.as_ref())
            .map(|f| f.to_schema_filter());

        self.tables.is_fetching = true;
        self.tables.did_invalidate = false;
        self.tables.error = None;
        self.status = "Loading list of tables...".to_string();

        match self.db.list_tables(database, filter.as_ref()).await {
            Ok(list) => {
                self.tables.is_fetching = false;
                self.tables
                    .items_by_database
                    .insert(database.to_string(), list);
                self.status.clear();
                Ok(())
            }
            Err(err) => {
                self.tables.is_fetching = false;
                self.tables.did_invalidate = true;
                self.tables.error = Some(err.clone());
                self.status.clear();
                Err(err)
            }
        }
    }

    pub async fn fetch_views_if_needed(&mut self, database: &str) -> Result<(), String> {
        let should_fetch = !self.views.is_fetching
            && (!self.views.views_by_database.contains_key(database) || self.views.did_invalidate);
        if !should_fetch {
            return Ok(());
        }
        let filter = self
            .connections
            .server
            .as_ref()
            .and_then(|s| s.filter.as_ref())
            .map(|f| f.to_schema_filter());

        self.views.is_fetching = true;
        self.views.did_invalidate = false;
        self.views.error = None;

        match self.db.list_views(database, filter.as_ref()).await {
            Ok(list) => {
                self.views.is_fetching = false;
                self.views
                    .views_by_database
                    .insert(database.to_string(), list);
                Ok(())
            }
            Err(err) => {
                self.views.is_fetching = false;
                self.views.did_invalidate = true;
                self.views.error = Some(err.clone());
                Err(err)
            }
        }
    }

    pub async fn fetch_routines_if_needed(&mut self, database: &str) -> Result<(), String> {
        let should_fetch = !self.routines.is_fetching
            && (!self.routines.functions_by_database.contains_key(database)
                || !self.routines.procedures_by_database.contains_key(database)
                || self.routines.did_invalidate);
        if !should_fetch {
            return Ok(());
        }
        let filter = self
            .connections
            .server
            .as_ref()
            .and_then(|s| s.filter.as_ref())
            .map(|f| f.to_schema_filter());

        self.routines.is_fetching = true;
        self.routines.did_invalidate = false;
        self.routines.error = None;

        match self.db.list_routines(database, filter.as_ref()).await {
            Ok(list) => {
                self.routines.is_fetching = false;
                let mut funcs = Vec::new();
                let mut procs = Vec::new();
                for r in list {
                    let item = RoutineItem {
                        schema: r.schema,
                        name: r.routine_name,
                        routine_definition: None,
                    };
                    if r.routine_type == "FUNCTION" {
                        funcs.push(item);
                    } else if r.routine_type == "PROCEDURE" {
                        procs.push(item);
                    }
                }
                self.routines
                    .functions_by_database
                    .insert(database.to_string(), funcs);
                self.routines
                    .procedures_by_database
                    .insert(database.to_string(), procs);
                Ok(())
            }
            Err(err) => {
                self.routines.is_fetching = false;
                self.routines.did_invalidate = true;
                self.routines.error = Some(err.clone());
                Err(err)
            }
        }
    }

    pub async fn fetch_table_columns_if_needed(
        &mut self,
        database: &str,
        table: &str,
        schema: Option<&str>,
    ) -> Result<(), String> {
        let is_fetching = self
            .columns
            .is_fetching
            .get(database)
            .and_then(|m| m.get(table))
            .copied()
            .unwrap_or(false);
        let has_cols = self
            .columns
            .columns_by_table
            .get(database)
            .and_then(|m| m.get(table))
            .is_some();
        if is_fetching || (has_cols && !self.columns.did_invalidate) {
            return Ok(());
        }

        self.columns
            .is_fetching
            .entry(database.to_string())
            .or_default()
            .insert(table.to_string(), true);
        self.columns.did_invalidate = false;
        self.columns.error = None;

        match self.db.list_table_columns(database, table, schema).await {
            Ok(cols) => {
                self.columns
                    .is_fetching
                    .entry(database.to_string())
                    .or_default()
                    .insert(table.to_string(), false);
                let mapped: Vec<ColumnItem> = cols
                    .into_iter()
                    .map(|c: ListTableColumnsResult| ColumnItem {
                        name: c.column_name,
                        data_type: c.data_type,
                    })
                    .collect();
                self.columns
                    .columns_by_table
                    .entry(database.to_string())
                    .or_default()
                    .insert(table.to_string(), mapped);
                Ok(())
            }
            Err(err) => {
                self.columns
                    .is_fetching
                    .entry(database.to_string())
                    .or_default()
                    .insert(table.to_string(), false);
                self.columns.did_invalidate = true;
                self.columns.error = Some(err.clone());
                Err(err)
            }
        }
    }

    pub async fn fetch_table_triggers_if_needed(
        &mut self,
        database: &str,
        table: &str,
        schema: Option<&str>,
    ) -> Result<(), String> {
        let has_triggers = self
            .triggers
            .triggers_by_table
            .get(database)
            .and_then(|m| m.get(table))
            .is_some();
        if self.triggers.is_fetching || (has_triggers && !self.triggers.did_invalidate) {
            return Ok(());
        }

        self.triggers.is_fetching = true;
        self.triggers.did_invalidate = false;
        self.triggers.error = None;

        match self.db.list_table_triggers(database, table, schema).await {
            Ok(list) => {
                self.triggers.is_fetching = false;
                self.triggers
                    .triggers_by_table
                    .entry(database.to_string())
                    .or_default()
                    .insert(
                        table.to_string(),
                        list.into_iter().map(|name| NamedItem { name }).collect(),
                    );
                Ok(())
            }
            Err(err) => {
                self.triggers.is_fetching = false;
                self.triggers.did_invalidate = true;
                self.triggers.error = Some(err.clone());
                Err(err)
            }
        }
    }

    pub async fn fetch_table_indexes_if_needed(
        &mut self,
        database: &str,
        table: &str,
        schema: Option<&str>,
    ) -> Result<(), String> {
        let has_indexes = self
            .indexes
            .indexes_by_table
            .get(database)
            .and_then(|m| m.get(table))
            .is_some();
        if self.indexes.is_fetching || (has_indexes && !self.indexes.did_invalidate) {
            return Ok(());
        }

        self.indexes.is_fetching = true;
        self.indexes.did_invalidate = false;
        self.indexes.error = None;

        match self.db.list_table_indexes(database, table, schema).await {
            Ok(list) => {
                self.indexes.is_fetching = false;
                self.indexes
                    .indexes_by_table
                    .entry(database.to_string())
                    .or_default()
                    .insert(
                        table.to_string(),
                        list.into_iter().map(|name| NamedItem { name }).collect(),
                    );
                Ok(())
            }
            Err(err) => {
                self.indexes.is_fetching = false;
                self.indexes.did_invalidate = true;
                self.indexes.error = Some(err.clone());
                Err(err)
            }
        }
    }

    // ========================================================================
    // 5. Query Tab & Execution Actions
    // ========================================================================

    pub fn add_new_query(&mut self, database: &str, table: Option<&str>) -> u64 {
        let new_id = self.queries.last_created_id + 1;
        let name = match table.filter(|t| !t.is_empty()) {
            Some(t) => format!("{database} / {t} #{new_id}"),
            None => format!("{database} #{new_id}"),
        };
        let tab = QueryTab {
            id: new_id,
            database: database.to_string(),
            name,
            filename: None,
            is_executing: false,
            is_default_select: false,
            did_invalidate: true,
            query: String::new(),
            selected_query: String::new(),
            query_history: Vec::new(),
            results: None,
            error: None,
            copied: None,
            saved: None,
            is_canceling: false,
            result_items_per_page: self.queries.result_items_per_page,
            execution_time: None,
        };
        self.queries.last_created_id = new_id;
        self.queries.current_query_id = Some(new_id);
        self.queries.query_ids.push(new_id);
        self.queries.queries_by_id.insert(new_id, tab);
        new_id
    }

    pub fn new_query(&mut self, database: &str) -> u64 {
        self.add_new_query(database, None)
    }

    pub fn select_query(&mut self, id: u64) {
        if self.queries.queries_by_id.contains_key(&id) {
            self.queries.current_query_id = Some(id);
        }
    }

    pub fn rename_query(&mut self, name: &str) {
        if name.is_empty() {
            return;
        }
        if let Some(q) = self.current_query_mut() {
            q.name = name.to_string();
        }
    }

    pub fn remove_query(&mut self, query_id: u64) {
        let Some(current_db) = self
            .queries
            .queries_by_id
            .get(&query_id)
            .map(|q| q.database.clone())
        else {
            return;
        };

        let index = self
            .queries
            .query_ids
            .iter()
            .position(|&id| id == query_id)
            .unwrap_or(0);

        self.queries.queries_by_id.remove(&query_id);
        self.queries.query_ids.retain(|&id| id != query_id);

        if self.queries.query_ids.is_empty() {
            self.add_new_query(&current_db, None);
        } else {
            let next_idx = if index >= 1 { index - 1 } else { 0 };
            self.queries.current_query_id = self.queries.query_ids.get(next_idx).copied();
        }
    }

    pub fn update_query_if_needed(
        &mut self,
        query: &str,
        selected_query: Option<&str>,
        table: Option<&str>,
    ) {
        let Some(current) = self.current_query_mut() else {
            return;
        };
        if current.is_executing {
            return;
        }
        let sel = selected_query.unwrap_or("");
        current.query = query.to_string();
        current.selected_query = sel.to_string();
        current.copied = Some(false);
        if let Some(t) = table.filter(|t| !t.is_empty()) {
            current.name = format!("{} / {} #{}", current.database, t, current.id);
        }
    }

    pub fn append_query(&mut self, sql: &str) {
        let Some(current) = self.current_query_mut() else {
            return;
        };
        if current.is_executing {
            return;
        }
        let new_query = if current.query.is_empty() {
            sql.to_string()
        } else {
            format!("{}\n{}", current.query, sql)
        };
        current.query = new_query;
        current.selected_query.clear();
        current.copied = Some(false);
    }

    pub fn check_query_safety(&self, query_text: &str) -> QuerySafetyReport {
        analyze_query_safety(query_text, self.read_only_mode)
    }

    pub async fn generate_database_erd(&mut self, database: &str) -> Result<String, String> {
        self.fetch_tables_if_needed(database).await?;
        let tables = self
            .tables
            .items_by_database
            .get(database)
            .cloned()
            .unwrap_or_default();

        let mut cols_map: HashMap<String, Vec<ListTableColumnsResult>> = HashMap::new();
        let mut keys_map: HashMap<String, Vec<TableKeysResult>> = HashMap::new();

        for tbl in tables {
            let cols = self
                .db
                .list_table_columns(database, &tbl.name, tbl.schema.as_deref())
                .await
                .unwrap_or_default();
            let keys = self
                .db
                .get_table_keys(database, &tbl.name, tbl.schema.as_deref())
                .await
                .unwrap_or_default();
            cols_map.insert(tbl.name.clone(), cols);
            keys_map.insert(tbl.name, keys);
        }

        Ok(generate_mermaid_erd(&cols_map, &keys_map))
    }

    pub async fn execute_query_if_needed(
        &mut self,
        query_text: &str,
        is_default_select: bool,
    ) -> Result<Vec<QueryRowResult>, String> {
        let Some(query_id) = self.queries.current_query_id else {
            return Err("No active query tab".to_string());
        };
        let (db_name, is_executing) = {
            let q = self.queries.queries_by_id.get(&query_id).unwrap();
            (q.database.clone(), q.is_executing)
        };
        if is_executing {
            return Ok(Vec::new());
        }

        // Enforce Read-Only safety guardrail if enabled
        let safety = analyze_query_safety(query_text, self.read_only_mode);
        if self.read_only_mode && !safety.is_read_only_safe {
            let reason = safety
                .findings
                .first()
                .map(|f| f.reason.clone())
                .unwrap_or_else(|| "Query blocked by Read-Only mode".to_string());
            if let Some(q) = self.queries.queries_by_id.get_mut(&query_id) {
                q.results = None;
                q.is_executing = false;
                q.error = Some(reason.clone());
            }
            return Err(reason);
        }

        // Bind any configured query parameters
        let bound_query = if self.query_params.is_empty() {
            query_text.to_string()
        } else {
            let param_vals: HashMap<String, Value> = self
                .query_params
                .iter()
                .map(|(k, v)| (k.clone(), Value::String(v.clone())))
                .collect();
            bind_query_parameters(query_text, &param_vals)
        };

        let start = Instant::now();
        if let Some(q) = self.queries.queries_by_id.get_mut(&query_id) {
            q.copied = Some(false);
            q.is_executing = true;
            q.is_default_select = is_default_select;
            q.did_invalidate = false;
            q.execution_time = None;
            q.query_history.push(bound_query.clone());
        }
        self.status = "Executing query...".to_string();

        let can_cancel = !self
            .connections
            .disabled_features
            .iter()
            .any(|f| f == "cancelQuery");

        let exec_res = if can_cancel {
            if let Err(e) = self
                .db
                .create_cancellable_query(&db_name, query_id, &bound_query)
            {
                Err(e)
            } else {
                self.db.execute_cancellable_query(query_id).await
            }
        } else {
            self.db.execute_query(&db_name, &bound_query).await
        };

        let elapsed_ms = start.elapsed().as_millis() as u64;
        self.status.clear();

        match exec_res {
            Ok(results) => {
                let total_rows: usize = results.iter().map(|r| r.rows.len()).sum();
                self.bookmark_manager.record_history(
                    &db_name,
                    &bound_query,
                    elapsed_ms,
                    total_rows,
                    None,
                );

                if let Some(q) = self.queries.queries_by_id.get_mut(&query_id) {
                    q.error = None;
                    q.is_executing = false;
                    q.results = Some(results.clone());
                    q.execution_time = Some(elapsed_ms);
                }

                // Auto-invalidate metadata slices when DDL commands execute (`reducers/databases.ts`, `schemas.ts`, `tables.ts`)
                if results
                    .iter()
                    .any(|r| matches!(r.command.as_str(), "CREATE_DATABASE" | "DROP_DATABASE"))
                {
                    self.databases.did_invalidate = true;
                }
                if results
                    .iter()
                    .any(|r| matches!(r.command.as_str(), "CREATE_SCHEMA" | "DROP_SCHEMA"))
                {
                    self.schemas.did_invalidate = true;
                }
                if results
                    .iter()
                    .any(|r| matches!(r.command.as_str(), "CREATE_TABLE" | "DROP_TABLE"))
                {
                    self.tables.did_invalidate = true;
                }

                Ok(results)
            }
            Err(err) => {
                self.bookmark_manager.record_history(
                    &db_name,
                    &bound_query,
                    elapsed_ms,
                    0,
                    Some(err.clone()),
                );
                if let Some(q) = self.queries.queries_by_id.get_mut(&query_id) {
                    q.results = None;
                    q.is_executing = false;
                    q.error = Some(err.clone());
                }
                Err(err)
            }
        }
    }

    pub async fn execute_default_select_query_if_needed(
        &mut self,
        database: &str,
        table: &str,
        schema: Option<&str>,
    ) -> Result<Vec<QueryRowResult>, String> {
        let query_default_select = self
            .db
            .get_query_select_top(database, table, schema, None)
            .await?;

        let need_new_query = match self.current_query() {
            Some(current) => {
                if current.is_executing {
                    return Ok(Vec::new());
                }
                current.database != database
                    || (current.query != query_default_select && !current.query.trim().is_empty())
            }
            None => true,
        };

        if need_new_query {
            self.add_new_query(database, Some(table));
        }

        self.update_query_if_needed(&query_default_select, Some(""), Some(table));
        self.execute_query_if_needed(&query_default_select, true)
            .await
    }

    pub async fn cancel_query(&mut self, query_id: u64) -> Result<(), String> {
        if let Some(q) = self.queries.queries_by_id.get_mut(&query_id) {
            q.is_canceling = true;
        }
        let res = self.db.cancel_cancellable_query(query_id).await;
        if let Some(q) = self.queries.queries_by_id.get_mut(&query_id) {
            q.is_canceling = false;
            if let Err(ref e) = res {
                q.is_executing = false;
                q.results = None;
                q.error = Some(e.clone());
            }
        }
        res
    }

    // ========================================================================
    // 6. SQL Script Actions (`actions/sqlscripts.ts`)
    // ========================================================================

    pub async fn get_sql_script_if_needed(
        &mut self,
        database: &str,
        item: &str,
        action_type: ScriptActionType,
        object_type: DbObjectType,
        schema: Option<&str>,
    ) -> Result<String, String> {
        if !self.sqlscripts.did_invalidate {
            if let Some(cached) = self
                .sqlscripts
                .scripts_by_object
                .get(database)
                .and_then(|m| m.get(item))
                .and_then(|s| s.get(action_type))
            {
                let script = cached.to_string();
                self.append_query(&script);
                return Ok(script);
            }
        }

        self.sqlscripts.is_fetching = true;
        self.sqlscripts.did_invalidate = false;
        self.sqlscripts.error = None;

        let script_res: Result<String, String> = match (action_type, object_type) {
            (ScriptActionType::Create, DbObjectType::Table) => self
                .db
                .get_table_create_script(database, item, schema)
                .await
                .map(|v| v.into_iter().next().unwrap_or_default()),
            (ScriptActionType::Create, DbObjectType::View) => self
                .db
                .get_view_create_script(database, item, schema)
                .await
                .map(|v| v.into_iter().next().unwrap_or_default()),
            (ScriptActionType::Create, DbObjectType::Function | DbObjectType::Procedure) => self
                .db
                .get_routine_create_script(database, item, object_type.as_str(), schema)
                .await
                .map(|v| v.into_iter().next().unwrap_or_default()),
            (ScriptActionType::Select, _) => {
                self.db
                    .get_table_select_script(database, item, schema)
                    .await
            }
            (ScriptActionType::Insert, _) => {
                self.db
                    .get_table_insert_script(database, item, schema)
                    .await
            }
            (ScriptActionType::Update, _) => {
                self.db
                    .get_table_update_script(database, item, schema)
                    .await
            }
            (ScriptActionType::Delete, _) => {
                self.db
                    .get_table_delete_script(database, item, schema)
                    .await
            }
        };

        match script_res {
            Ok(script) => {
                self.sqlscripts.is_fetching = false;
                let entry = self
                    .sqlscripts
                    .scripts_by_object
                    .entry(database.to_string())
                    .or_default()
                    .entry(item.to_string())
                    .or_insert_with(|| ObjectScripts::new(object_type));
                entry.set(action_type, script.clone());
                self.append_query(&script);
                Ok(script)
            }
            Err(err) => {
                self.sqlscripts.is_fetching = false;
                self.sqlscripts.did_invalidate = true;
                self.sqlscripts.error = Some(err.clone());
                Err(err)
            }
        }
    }
}

trait CheckedInto<T> {
    fn checked_into(self) -> Option<T>;
}

impl CheckedInto<u32> for u32 {
    fn checked_into(self) -> Option<u32> {
        Some(self)
    }
}
