use crate::adapters::{
    adapter_factory, get_adapters, wrap_identifier_for_adapter, CancellableQuery, DatabaseAdapter,
};
use crate::common::types::{
    AdapterInfo, AdapterVersion, DatabaseFilter, ListRoutineResult, ListTableColumnsResult,
    ListTableResult, ListViewResult, OpenedQueryInfo, QueryRowResult, SavedQueryInfo, SchemaFilter,
    Server, TableKeysResult,
};
use crate::core::errors::DbCoreError;
use crate::core::export::{
    export_query_result_to_clipboard, export_query_result_to_file_path, open_query_from_file,
    save_query_to_file,
};
use crate::core::tunnel::{connect_tunnel, TunnelHandle};
use serde_json::{Map, Value};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

pub const DEFAULT_LIMIT: u32 = 1000;

static SELECT_LIMIT: OnceLock<Mutex<Option<u32>>> = OnceLock::new();

fn select_limit_store() -> &'static Mutex<Option<u32>> {
    SELECT_LIMIT.get_or_init(|| Mutex::new(None))
}

pub fn set_select_limit(limit: u32) {
    if let Ok(mut guard) = select_limit_store().lock() {
        *guard = Some(limit);
    }
}

pub fn clear_select_limit() {
    if let Ok(mut guard) = select_limit_store().lock() {
        *guard = None;
    }
}

pub fn get_select_limit() -> u32 {
    select_limit_store()
        .lock()
        .ok()
        .and_then(|g| *g)
        .unwrap_or(DEFAULT_LIMIT)
}

/// Represents a single database connection (`Database` in `sqlectron-db-core/src/database.ts`).
pub struct DatabaseConnection {
    pub server_config: Server,
    pub adapter_key: String,
    pub database: Option<String>,
    pub connecting: bool,
    pub connection: Option<Box<dyn DatabaseAdapter>>,
    pub ssh_tunnel: Option<Arc<TunnelHandle>>,
}

impl DatabaseConnection {
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

    fn check_is_connected(&self) -> Result<&dyn DatabaseAdapter, DbCoreError> {
        if self.connecting || self.connection.is_none() {
            return Err(DbCoreError::NoConnectionAvailable);
        }
        Ok(self.connection.as_ref().unwrap().as_ref())
    }

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

    pub async fn disconnect(&mut self) {
        self.connecting = false;
        if let Some(mut conn) = self.connection.take() {
            let _ = conn.disconnect().await;
        }
        if let Some(tunnel) = self.ssh_tunnel.take() {
            tunnel.close();
        }
    }

    pub fn get_version(&self) -> Result<AdapterVersion, DbCoreError> {
        Ok(self.check_is_connected()?.get_version())
    }

    pub fn wrap(&self, value: &str) -> String {
        wrap_identifier_for_adapter(&self.adapter_key, value)
    }

    fn resolve_schema(&self, schema: Option<&str>) -> String {
        match schema.filter(|s| !s.is_empty()) {
            Some(s) => format!("{}.", self.wrap(s)),
            None => String::new(),
        }
    }

    pub async fn list_databases(
        &self,
        filter: Option<&DatabaseFilter>,
    ) -> Result<Vec<String>, DbCoreError> {
        self.check_is_connected()?.list_databases(filter).await
    }

    pub async fn list_schemas(
        &self,
        filter: Option<&SchemaFilter>,
    ) -> Result<Vec<String>, DbCoreError> {
        self.check_is_connected()?.list_schemas(filter).await
    }

    pub async fn list_tables(
        &self,
        filter: Option<&SchemaFilter>,
    ) -> Result<Vec<ListTableResult>, DbCoreError> {
        self.check_is_connected()?.list_tables(filter).await
    }

    pub async fn list_views(
        &self,
        filter: Option<&SchemaFilter>,
    ) -> Result<Vec<ListViewResult>, DbCoreError> {
        self.check_is_connected()?.list_views(filter).await
    }

    pub async fn list_routines(
        &self,
        filter: Option<&SchemaFilter>,
    ) -> Result<Vec<ListRoutineResult>, DbCoreError> {
        self.check_is_connected()?.list_routines(filter).await
    }

    pub async fn list_table_columns(
        &self,
        table: &str,
        schema: Option<&str>,
    ) -> Result<Vec<ListTableColumnsResult>, DbCoreError> {
        self.check_is_connected()?
            .list_table_columns(table, schema)
            .await
    }

    pub async fn list_table_triggers(
        &self,
        table: &str,
        schema: Option<&str>,
    ) -> Result<Vec<String>, DbCoreError> {
        self.check_is_connected()?
            .list_table_triggers(table, schema)
            .await
    }

    pub async fn list_table_indexes(
        &self,
        table: &str,
        schema: Option<&str>,
    ) -> Result<Vec<String>, DbCoreError> {
        self.check_is_connected()?
            .list_table_indexes(table, schema)
            .await
    }

    pub async fn get_table_references(
        &self,
        table: &str,
        schema: Option<&str>,
    ) -> Result<Vec<String>, DbCoreError> {
        self.check_is_connected()?
            .get_table_references(table, schema)
            .await
    }

    pub async fn get_table_keys(
        &self,
        table: &str,
        schema: Option<&str>,
    ) -> Result<Vec<TableKeysResult>, DbCoreError> {
        self.check_is_connected()?
            .get_table_keys(table, schema)
            .await
    }

    pub fn query(&self, query_text: &str) -> Result<Arc<dyn CancellableQuery>, DbCoreError> {
        self.check_is_connected()?.query(query_text)
    }

    pub async fn execute_query(
        &self,
        query_text: &str,
    ) -> Result<Vec<QueryRowResult>, DbCoreError> {
        self.check_is_connected()?.execute_query(query_text).await
    }

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

    pub async fn get_table_column_names(
        &self,
        table: &str,
        schema: Option<&str>,
    ) -> Result<Vec<String>, DbCoreError> {
        let cols = self.list_table_columns(table, schema).await?;
        Ok(cols.into_iter().map(|c| c.column_name).collect())
    }

    pub async fn get_table_create_script(
        &self,
        table: &str,
        schema: Option<&str>,
    ) -> Result<Vec<String>, DbCoreError> {
        self.check_is_connected()?
            .get_table_create_script(table, schema)
            .await
    }

    /// Matches `Database.getTableSelectScript` in `sqlectron-db-core/src/database.ts`:
    /// `SELECT <col1>, <col2> FROM [schema.]<table>;`
    pub async fn get_table_select_script(
        &self,
        table: &str,
        schema: Option<&str>,
    ) -> Result<String, DbCoreError> {
        let column_names = self.get_table_column_names(table, schema).await?;
        let wrapped_cols = column_names
            .iter()
            .map(|c| self.wrap(c))
            .collect::<Vec<_>>()
            .join(", ");
        let schema_selection = self.resolve_schema(schema);
        Ok(format!(
            "SELECT {} FROM {}{};",
            wrapped_cols,
            schema_selection,
            self.wrap(table)
        ))
    }

    /// Matches `Database.getTableInsertScript` in `sqlectron-db-core/src/database.ts`:
    /// `INSERT INTO [schema.]<table> (<col1>, <col2>)\n VALUES (?, ?);`
    pub async fn get_table_insert_script(
        &self,
        table: &str,
        schema: Option<&str>,
    ) -> Result<String, DbCoreError> {
        let column_names = self.get_table_column_names(table, schema).await?;
        let wrapped_cols = column_names
            .iter()
            .map(|c| self.wrap(c))
            .collect::<Vec<_>>()
            .join(", ");
        let placeholders = vec!["?"; column_names.len()].join(", ");
        let schema_selection = self.resolve_schema(schema);
        Ok(format!(
            "INSERT INTO {}{} ({})\n VALUES ({});",
            schema_selection,
            self.wrap(table),
            wrapped_cols,
            placeholders
        ))
    }

    /// Matches `Database.getTableUpdateScript` in `sqlectron-db-core/src/database.ts`:
    /// `UPDATE [schema.]<table>\n SET <col1>=?, <col2>=?\n WHERE <condition>;`
    pub async fn get_table_update_script(
        &self,
        table: &str,
        schema: Option<&str>,
    ) -> Result<String, DbCoreError> {
        let column_names = self.get_table_column_names(table, schema).await?;
        let set_cols = column_names
            .iter()
            .map(|c| format!("{}=?", self.wrap(c)))
            .collect::<Vec<_>>()
            .join(", ");
        let schema_selection = self.resolve_schema(schema);
        Ok(format!(
            "UPDATE {}{}\n SET {}\n WHERE <condition>;",
            schema_selection,
            self.wrap(table),
            set_cols
        ))
    }

    /// Matches `Database.getTableDeleteScript` in `sqlectron-db-core/src/database.ts`:
    /// `DELETE FROM [schema.]<table> WHERE <condition>;`
    pub async fn get_table_delete_script(
        &self,
        table: &str,
        schema: Option<&str>,
    ) -> Result<String, DbCoreError> {
        let schema_selection = self.resolve_schema(schema);
        Ok(format!(
            "DELETE FROM {}{} WHERE <condition>;",
            schema_selection,
            self.wrap(table)
        ))
    }

    pub async fn get_view_create_script(
        &self,
        view: &str,
        schema: Option<&str>,
    ) -> Result<Vec<String>, DbCoreError> {
        self.check_is_connected()?
            .get_view_create_script(view, schema)
            .await
    }

    pub async fn get_routine_create_script(
        &self,
        routine: &str,
        routine_type: &str,
        schema: Option<&str>,
    ) -> Result<Vec<String>, DbCoreError> {
        self.check_is_connected()?
            .get_routine_create_script(routine, routine_type, schema)
            .await
    }

    pub async fn truncate_all_tables(&self, schema: Option<&str>) -> Result<(), DbCoreError> {
        self.check_is_connected()?.truncate_all_tables(schema).await
    }
}

/// Represents a `Server` session in `sqlectron-db-core/src/server.ts`.
pub struct ServerSession {
    pub config: Server,
    pub adapter_key: String,
    pub databases: HashMap<String, DatabaseConnection>,
}

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
    fn db_key(db_name: Option<&str>) -> String {
        db_name
            .filter(|s| !s.is_empty())
            .unwrap_or("undefined")
            .to_string()
    }

    pub fn db(&self, db_name: Option<&str>) -> Option<&DatabaseConnection> {
        self.databases.get(&Self::db_key(db_name))
    }

    pub fn db_mut(&mut self, db_name: Option<&str>) -> Option<&mut DatabaseConnection> {
        self.databases.get_mut(&Self::db_key(db_name))
    }

    pub fn create_connection(&mut self, db_name: Option<&str>) -> &mut DatabaseConnection {
        let key = Self::db_key(db_name);
        let cfg = self.config.clone();
        let adapter_key = self.adapter_key.clone();
        let db_opt = db_name.map(|s| s.to_string());
        self.databases
            .entry(key)
            .or_insert_with(|| DatabaseConnection::new(cfg, adapter_key, db_opt))
    }

    pub async fn remove_database(&mut self, db_name: Option<&str>) {
        let key = Self::db_key(db_name);
        if let Some(mut db) = self.databases.remove(&key) {
            db.disconnect().await;
        }
    }

    pub async fn end(&mut self) {
        for (_, mut db) in self.databases.drain() {
            db.disconnect().await;
        }
    }
}

/// Complete Rust port of `DatabaseFacade` (`src/browser/core/db.ts`) implementing `SqlectronDB`.
#[derive(Default)]
pub struct DatabaseFacade {
    pub server_session: Option<ServerSession>,
    pub cancellable_queries: HashMap<u64, Arc<dyn CancellableQuery>>,
}

impl DatabaseFacade {
    pub fn new() -> Self {
        Self::default()
    }

    fn get_db(&self, database: &str) -> Result<&DatabaseConnection, String> {
        let session = self
            .server_session
            .as_ref()
            .ok_or_else(|| format!("No database found {database}"))?;
        session
            .db(Some(database))
            .or_else(|| session.db(None))
            .ok_or_else(|| format!("No database found {database}"))
    }

    pub fn get_clients_sync(&self) -> Vec<AdapterInfo> {
        get_adapters()
    }

    pub async fn connect(&mut self, server: Server, database: Option<&str>) -> Result<(), String> {
        if self.server_session.is_none() {
            self.server_session = Some(create_server(server).map_err(|e| e.to_string())?);
        }

        let session = self.server_session.as_mut().unwrap();
        let db_conn = session.create_connection(database);
        if let Err(err) = db_conn.connect().await {
            let db_key = ServerSession::db_key(database);
            if let Some(mut removed) = session.databases.remove(&db_key) {
                removed.disconnect().await;
            }
            if session.databases.is_empty() {
                session.end().await;
                self.server_session = None;
            }
            return Err(err.to_string());
        }

        Ok(())
    }

    pub async fn disconnect_database(&mut self, database: Option<&str>) -> Result<(), String> {
        if let Some(ref mut session) = self.server_session {
            session.remove_database(database).await;
            Ok(())
        } else {
            Err(format!("No database found {}", database.unwrap_or("")))
        }
    }

    pub async fn disconnect_server(&mut self) -> Result<(), String> {
        if let Some(mut session) = self.server_session.take() {
            session.end().await;
        }
        Ok(())
    }

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

    pub fn create_cancellable_query(
        &mut self,
        database: &str,
        query_id: u64,
        query_text: &str,
    ) -> Result<(), String> {
        let q = self
            .get_db(database)?
            .query(query_text)
            .map_err(|e| e.to_string())?;
        self.cancellable_queries.insert(query_id, q);
        Ok(())
    }

    pub async fn cancel_cancellable_query(&mut self, query_id: u64) -> Result<(), String> {
        if let Some(q) = self.cancellable_queries.remove(&query_id) {
            q.cancel().await.map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    pub async fn execute_cancellable_query(
        &mut self,
        query_id: u64,
    ) -> Result<Vec<QueryRowResult>, String> {
        let Some(q) = self.cancellable_queries.remove(&query_id) else {
            return Ok(Vec::new());
        };
        q.execute().await.map_err(|e| e.to_string())
    }

    pub async fn execute_query(
        &self,
        database: &str,
        query_text: &str,
    ) -> Result<Vec<QueryRowResult>, String> {
        self.get_db(database)?
            .execute_query(query_text)
            .await
            .map_err(|e| e.to_string())
    }

    pub async fn get_query_select_top(
        &self,
        database: &str,
        table: &str,
        schema: Option<&str>,
        limit: Option<u32>,
    ) -> Result<String, String> {
        self.get_db(database)?
            .get_query_select_top(table, schema, limit)
            .await
            .map_err(|e| e.to_string())
    }

    pub async fn get_table_create_script(
        &self,
        database: &str,
        table: &str,
        schema: Option<&str>,
    ) -> Result<Vec<String>, String> {
        self.get_db(database)?
            .get_table_create_script(table, schema)
            .await
            .map_err(|e| e.to_string())
    }

    pub async fn get_table_select_script(
        &self,
        database: &str,
        table: &str,
        schema: Option<&str>,
    ) -> Result<String, String> {
        self.get_db(database)?
            .get_table_select_script(table, schema)
            .await
            .map_err(|e| e.to_string())
    }

    pub async fn get_table_insert_script(
        &self,
        database: &str,
        table: &str,
        schema: Option<&str>,
    ) -> Result<String, String> {
        self.get_db(database)?
            .get_table_insert_script(table, schema)
            .await
            .map_err(|e| e.to_string())
    }

    pub async fn get_table_update_script(
        &self,
        database: &str,
        table: &str,
        schema: Option<&str>,
    ) -> Result<String, String> {
        self.get_db(database)?
            .get_table_update_script(table, schema)
            .await
            .map_err(|e| e.to_string())
    }

    pub async fn get_table_delete_script(
        &self,
        database: &str,
        table: &str,
        schema: Option<&str>,
    ) -> Result<String, String> {
        self.get_db(database)?
            .get_table_delete_script(table, schema)
            .await
            .map_err(|e| e.to_string())
    }

    pub async fn get_view_create_script(
        &self,
        database: &str,
        view: &str,
        schema: Option<&str>,
    ) -> Result<Vec<String>, String> {
        self.get_db(database)?
            .get_view_create_script(view, schema)
            .await
            .map_err(|e| e.to_string())
    }

    pub async fn get_routine_create_script(
        &self,
        database: &str,
        routine: &str,
        routine_type: &str,
        schema: Option<&str>,
    ) -> Result<Vec<String>, String> {
        self.get_db(database)?
            .get_routine_create_script(routine, routine_type, schema)
            .await
            .map_err(|e| e.to_string())
    }

    pub async fn truncate_all_tables(
        &self,
        database: &str,
        schema: Option<&str>,
    ) -> Result<(), String> {
        self.get_db(database)?
            .truncate_all_tables(schema)
            .await
            .map_err(|e| e.to_string())
    }

    pub async fn get_table_column_names(
        &self,
        database: &str,
        table: &str,
        schema: Option<&str>,
    ) -> Result<Vec<String>, String> {
        self.get_db(database)?
            .get_table_column_names(table, schema)
            .await
            .map_err(|e| e.to_string())
    }

    pub fn set_select_limit(&self, limit: u32) {
        set_select_limit(limit);
    }

    pub fn export_query_result_to_file(
        &self,
        rows: &[Map<String, Value>],
        export_type: &str,
        delimiter: Option<&str>,
        filename: &str,
    ) -> Result<String, String> {
        export_query_result_to_file_path(rows, export_type, delimiter, filename)
    }

    pub fn export_query_result_to_clipboard(
        &self,
        rows: &[Map<String, Value>],
        export_type: &str,
        delimiter: Option<&str>,
    ) -> Result<String, String> {
        export_query_result_to_clipboard(rows, export_type, delimiter)
    }

    pub fn save_query(&self, filename: &str, query: &str) -> Result<SavedQueryInfo, String> {
        save_query_to_file(filename, query)
    }

    pub fn open_query(&self, filename: &str) -> Result<OpenedQueryInfo, String> {
        open_query_from_file(filename)
    }
}
