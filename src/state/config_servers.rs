//! Configuration and server CRUD actions on [`AppStore`].

use crate::common::types::{BaseConfig, Server, ValidationErrors};
use crate::core::{config, servers};
use crate::state::{AppStore, ConnectionState};

impl AppStore {
    /// Loads and sanitizes the persistent configuration file into `self.config` and `self.servers`.
    pub fn load_config(&mut self) -> Result<(), String> {
        let path = config::path();
        match config::get_full_sync(true) {
            Ok(cfg) => {
                self.db.set_select_limit(cfg.limit_query_default_select_top);
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

    /// Marks the application settings editor as open.
    pub fn start_editing_config(&mut self) {
        self.config.is_saving = false;
        self.config.is_editing = true;
    }

    /// Closes the application settings editor and clears any transient error.
    pub fn finish_editing_config(&mut self) {
        self.config.is_saving = false;
        self.config.is_editing = false;
        self.config.is_loaded = true;
        self.config.error = None;
    }

    /// Saves `config_data` to disk and reloads the active configuration state.
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

    /// Opens the server editor modal for creating a new server (`id == None`) or editing an existing one.
    pub fn start_editing_server(&mut self, id: Option<&str>) -> Result<(), String> {
        self.connections = ConnectionState::default();
        let Some(server_id) = id.filter(|s| !s.is_empty()) else {
            self.servers.is_saving = false;
            self.servers.is_editing = true;
            self.servers.editing_server = None;
            self.servers.error = None;
            return Ok(());
        };

        let Some(srv) = self
            .servers
            .items
            .iter()
            .find(|s| s.id == server_id)
            .cloned()
        else {
            return Err("Server not found".to_string());
        };
        let decrypted = servers::decrypt_secrets(&srv, self.key_provider())?;
        self.servers.is_saving = false;
        self.servers.is_editing = true;
        self.servers.editing_server = Some(decrypted);
        self.servers.error = None;
        Ok(())
    }

    /// Closes the server editor modal and resets transient editing state.
    pub fn finish_editing_server(&mut self) {
        self.connections = ConnectionState::default();
        self.servers.is_saving = false;
        self.servers.is_editing = false;
        self.servers.editing_server = None;
        self.servers.error = None;
    }

    /// Validates, encrypts, and persists `server` (creating a new entry or updating `id`).
    pub fn save_server(
        &mut self,
        mut server: Server,
        id: Option<&str>,
    ) -> Result<Server, ValidationErrors> {
        self.servers.is_saving = true;
        if let Some(existing_id) = id {
            server.id = existing_id.to_string();
        }
        match servers::add_or_update_sync(&server, self.key_provider()) {
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

    /// Duplicates `server` under a unique name and persists the copy.
    ///
    /// Saved credentials are bound to the original's ID, so they are decrypted here and
    /// re-encrypted for the copy's new ID when it is saved.
    pub fn duplicate_server(&mut self, server: Server) -> Result<Server, ValidationErrors> {
        let mut server = match servers::decrypt_secrets(&server, self.key_provider()) {
            Ok(s) => s,
            Err(e) => {
                let mut map = ValidationErrors::new();
                map.insert("general".to_string(), e);
                self.servers.error = Some(map.clone());
                return Err(map);
            }
        };
        self.servers.is_saving = true;
        let all = servers::get_all_sync().unwrap_or_default();
        server.name = servers::get_unique_name(&server, &all);
        server.id = String::new();
        self.save_server(server, None)
    }

    /// Removes the server with `id` from persistent storage and refreshes `self.servers`.
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
}
