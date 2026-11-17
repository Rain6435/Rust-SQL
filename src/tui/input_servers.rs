//! Keyboard input handling for server list, server save form, server removal, and database switcher routes.

use crate::common::types::{Password, Server, SslConfig};
use crate::gui::ServerFormDraft;
use crate::tui::{TuiApp, TuiRoute};
use crossterm::event::{self, KeyCode, KeyModifiers};
use tokio::runtime::Runtime;

impl TuiApp {
    /// Processes a single terminal keyboard event and updates [`TuiApp`] state.
    pub fn handle_key(&mut self, key: event::KeyEvent, rt: &Runtime) {
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('q') {
            self.should_quit = true;
            return;
        }
        if self.handle_host_key_key(key, rt) {
            return;
        }

        match self.route.clone() {
            TuiRoute::ServerList => self.handle_server_list_key(key, rt),
            TuiRoute::ServerSave(edit_id) => self.handle_server_save_key(key, edit_id),
            TuiRoute::ServerRemove(id) => self.handle_server_remove_key(key, &id),
            TuiRoute::Database {
                server_id,
                database,
            } => self.handle_database_key(key, rt, server_id, database),
            TuiRoute::DatabaseList {
                server_id,
                database,
            } => self.handle_database_list_key(key, rt, server_id, database),
        }
    }

    fn handle_server_list_key(&mut self, key: event::KeyEvent, rt: &Runtime) {
        match key.code {
            KeyCode::Char('q') => self.should_quit = true,
            KeyCode::Up | KeyCode::Char('k') => {
                self.selected_server_idx = self.selected_server_idx.saturating_sub(1);
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if self.selected_server_idx + 1 < self.store.servers.items.len() {
                    self.selected_server_idx += 1;
                }
            }
            KeyCode::Char('a') => {
                self.form_draft = ServerFormDraft::from_server(None);
                self.form_field_idx = 0;
                self.route = TuiRoute::ServerSave(None);
            }
            KeyCode::Char('e') => {
                if let Some(srv) = self
                    .store
                    .servers
                    .items
                    .get(self.selected_server_idx)
                    .cloned()
                {
                    let _ = self.store.start_editing_server(Some(&srv.id));
                    self.form_draft =
                        ServerFormDraft::from_server(self.store.servers.editing_server.as_ref());
                    self.form_field_idx = 0;
                    self.route = TuiRoute::ServerSave(Some(srv.id));
                }
            }
            KeyCode::Char('d') | KeyCode::Delete => {
                if let Some(srv) = self.store.servers.items.get(self.selected_server_idx) {
                    self.route = TuiRoute::ServerRemove(srv.id.clone());
                }
            }
            KeyCode::Enter => {
                if let Some(srv) = self
                    .store
                    .servers
                    .items
                    .get(self.selected_server_idx)
                    .cloned()
                {
                    self.connect_and_open(&srv.id, rt);
                }
            }
            _ => {}
        }
    }

    fn active_field_mut(&mut self) -> &mut String {
        match self.form_field_idx {
            0 => &mut self.form_draft.name,
            1 => &mut self.form_draft.client,
            2 => &mut self.form_draft.host,
            3 => &mut self.form_draft.port,
            4 => &mut self.form_draft.user,
            5 => &mut self.form_draft.password,
            _ => &mut self.form_draft.database,
        }
    }

    fn handle_server_save_key(&mut self, key: event::KeyEvent, edit_id: Option<String>) {
        match key.code {
            KeyCode::Esc => {
                self.store.finish_editing_server();
                self.route = TuiRoute::ServerList;
            }
            KeyCode::Tab | KeyCode::Down => {
                self.form_field_idx = (self.form_field_idx + 1) % 7;
            }
            KeyCode::Up => {
                self.form_field_idx = (self.form_field_idx + 6) % 7;
            }
            KeyCode::Enter => {
                let opt = |s: &str| (!s.is_empty()).then(|| s.to_string());
                let srv = Server {
                    id: edit_id.clone().unwrap_or_default(),
                    name: self.form_draft.name.clone(),
                    client: self.form_draft.client.clone(),
                    host: opt(&self.form_draft.host),
                    port: self.form_draft.port.parse::<u32>().ok(),
                    user: opt(&self.form_draft.user),
                    password: opt(&self.form_draft.password).map(Password::Plain),
                    database: opt(&self.form_draft.database),
                    ssl: Some(SslConfig::Bool(false)),
                    ..Default::default()
                };
                if self.store.save_server(srv, edit_id.as_deref()).is_ok() {
                    self.route = TuiRoute::ServerList;
                }
            }
            KeyCode::Backspace => {
                self.active_field_mut().pop();
            }
            KeyCode::Char(c) => {
                self.active_field_mut().push(c);
            }
            _ => {}
        }
    }

    fn handle_server_remove_key(&mut self, key: event::KeyEvent, id: &str) {
        match key.code {
            KeyCode::Char('y') | KeyCode::Char('Y') => {
                let _ = self.store.remove_server(id);
                self.selected_server_idx = 0;
                self.route = TuiRoute::ServerList;
            }
            KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                self.route = TuiRoute::ServerList;
            }
            _ => {}
        }
    }

    fn handle_database_list_key(
        &mut self,
        key: event::KeyEvent,
        rt: &Runtime,
        server_id: String,
        database: String,
    ) {
        match key.code {
            KeyCode::Esc => {
                self.route = TuiRoute::Database {
                    server_id,
                    database,
                };
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.selected_db_idx = self.selected_db_idx.saturating_sub(1);
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if self.selected_db_idx + 1 < self.store.databases.items.len() {
                    self.selected_db_idx += 1;
                }
            }
            KeyCode::Enter => {
                if let Some(db_item) = self
                    .store
                    .databases
                    .items
                    .get(self.selected_db_idx)
                    .cloned()
                {
                    let _ = rt.block_on(self.store.connect(
                        &server_id,
                        Some(&db_item.name),
                        false,
                        None,
                    ));
                    let _ = rt.block_on(self.store.fetch_tables_if_needed(&db_item.name));
                    self.selected_table_idx = 0;
                    self.route = TuiRoute::Database {
                        server_id,
                        database: db_item.name,
                    };
                }
            }
            _ => {}
        }
    }
}
