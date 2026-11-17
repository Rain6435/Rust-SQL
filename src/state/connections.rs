//! Database connection lifecycle, reconnection, and connection testing actions on [`AppStore`].

use crate::adapters::{DatabaseFacade, CLIENTS};
use crate::common::types::{ConnectionMode, Server, EMULATED_BANNER};
use crate::core::servers;
use crate::state::{
    AppStore, ColumnState, ConnectionState, DatabaseState, IndexState, QueryState, RoutineState,
    SchemaState, ScriptState, TableState, TriggerState, ViewState,
};

impl AppStore {
    /// Marks the connection state slice as actively connecting.
    pub fn set_connecting(&mut self) {
        self.connections = ConnectionState {
            connecting: true,
            ..Default::default()
        };
    }

    fn reset_metadata_for_server_connection(&mut self) {
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

    /// Connects to `server_id` and `database_name`, optionally reconnecting or supplying an SSH passphrase.
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

        let mut decrypted_server = servers::decrypt_secrets(&server, self.key_provider())?;

        let db_client = CLIENTS.iter().find(|c| c.key == decrypted_server.client);
        let default_database = db_client.and_then(|c| c.default_database.clone());
        let target_db = database_name
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
            .or_else(|| decrypted_server.database.clone().filter(|s| !s.is_empty()))
            .or(default_database)
            .unwrap_or_else(|| "postgres".to_string());

        self.status = "Connecting to database...".to_string();
        if database_name.is_none() {
            self.reset_metadata_for_server_connection();
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
        self.connections.host_key_prompt = None;
        self.connections.host_key_retry_server_id = None;
        if let Err(err) = self
            .db
            .connect_checked(decrypted_server.clone(), Some(&target_db))
            .await
        {
            let err = self.record_host_key_challenge(&err, Some(server_id));
            self.connections.connected = false;
            self.connections.connecting = false;
            self.connections.waiting_private_key_passphrase = false;
            self.connections.error = Some(err.clone());
            self.connections.mode = None;
            self.status.clear();
            return Err(err);
        }

        self.connections.connected = true;
        self.connections.mode = self.db.connection_mode(&target_db).ok();
        self.connections.connecting = false;
        self.connections.waiting_private_key_passphrase = false;
        if !self.connections.databases.contains(&target_db) {
            self.connections.databases.push(target_db.clone());
        }
        self.status = match self.connections.mode {
            Some(ConnectionMode::Emulated) => EMULATED_BANNER.to_string(),
            _ => "Connection to database established".to_string(),
        };

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

    /// Disconnects the active server session and resets connection and query tab state.
    pub async fn disconnect(&mut self) {
        let _ = self.db.disconnect_server().await;
        self.connections = ConnectionState::default();
        self.queries = QueryState::default();
        self.status.clear();
    }

    /// Reconnects to `server_id` and `database_name`.
    pub async fn reconnect(&mut self, server_id: &str, database_name: &str) -> Result<(), String> {
        self.connect(server_id, Some(database_name), true, None)
            .await
    }

    /// Tests connectivity to `server` using an isolated temporary [`DatabaseFacade`].
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
        let mut res = test_facade
            .connect_checked(server, db.as_deref())
            .await
            .map_err(|e| self.record_host_key_challenge(&e, None));
        if res.is_ok() {
            let mode = test_facade.connection_mode(db.as_deref().unwrap_or("undefined"));
            if mode != Ok(ConnectionMode::Live) {
                res = Err(format!(
                    "Nothing was tested: this dialect has no live driver, so the connection \
                     would be emulated. {EMULATED_BANNER}"
                ));
            }
            let _ = test_facade.disconnect_server().await;
        }
        match res {
            Ok(()) => {
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
}
