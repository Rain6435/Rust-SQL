//! Global keyboard shortcuts, query execution triggers, top menu/breadcrumb header, and status footer.

use crate::common::types::ConnectionMode;
use crate::core::sql_formatter::format_sql;
use crate::core::updater::check_for_updates;
use crate::gui::{ActiveRoute, RustSqlGuiApp};
use eframe::egui::{self, Color32, Key, KeyboardShortcut, Modifiers, RichText};

impl RustSqlGuiApp {
    /// Processes global keyboard shortcuts (`Ctrl+Q`, `Ctrl+T`, `Ctrl+W`, `Ctrl+Enter`/`Ctrl+R`, `Ctrl+I`, `Ctrl+S`).
    pub(crate) fn handle_global_shortcuts(&mut self, ctx: &egui::Context) {
        let ctrl_q = KeyboardShortcut::new(Modifiers::COMMAND, Key::Q);
        let ctrl_t = KeyboardShortcut::new(Modifiers::COMMAND, Key::T);
        let ctrl_w = KeyboardShortcut::new(Modifiers::COMMAND, Key::W);
        let ctrl_enter = KeyboardShortcut::new(Modifiers::COMMAND, Key::Enter);
        let ctrl_r = KeyboardShortcut::new(Modifiers::COMMAND, Key::R);
        let ctrl_i = KeyboardShortcut::new(Modifiers::COMMAND, Key::I);
        let ctrl_s = KeyboardShortcut::new(Modifiers::COMMAND, Key::S);

        ctx.input_mut(|i| {
            if i.consume_shortcut(&ctrl_q) {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            if matches!(self.route, ActiveRoute::QueryBrowser(_)) {
                if i.consume_shortcut(&ctrl_t) {
                    if let Some(db) = self.store.current_database() {
                        self.store.new_query(&db);
                    }
                }
                if i.consume_shortcut(&ctrl_w) {
                    if let Some(id) = self.store.queries.current_query_id {
                        self.store.remove_query(id);
                    }
                }
                if i.consume_shortcut(&ctrl_enter) || i.consume_shortcut(&ctrl_r) {
                    self.trigger_execute_current_query();
                }
                if i.consume_shortcut(&ctrl_i) {
                    self.format_current_query();
                }
                if i.consume_shortcut(&ctrl_s) {
                    self.show_save_query_modal = true;
                }
            }
        });
    }

    /// Runs safety analysis on the active query and either prompts for confirmation or executes it.
    pub(crate) fn trigger_execute_current_query(&mut self) {
        let Some(q) = self.store.current_query() else {
            return;
        };
        let sql = if !q.selected_query.trim().is_empty() {
            q.selected_query.clone()
        } else {
            q.query.clone()
        };
        let safety = self.store.check_query_safety(&sql);
        if !self.store.read_only_mode && safety.requires_confirmation {
            self.pending_safety_confirm = Some((sql, safety));
            return;
        }
        self.execute_sql_confirmed(&sql);
    }

    /// Executes `sql` in the active query tab and refreshes database/table metadata if needed.
    pub(crate) fn execute_sql_confirmed(&mut self, sql: &str) {
        let rt = self.rt.clone();
        let _ = rt.block_on(self.store.execute_query_if_needed(sql, false));
        if let Some(db) = self.store.current_database() {
            let _ = rt.block_on(self.store.fetch_databases_if_needed());
            let _ = rt.block_on(self.store.fetch_tables_if_needed(&db));
        }
    }

    /// Formats the SQL query in the currently active editor tab.
    pub(crate) fn format_current_query(&mut self) {
        let client = self
            .store
            .connections
            .server
            .as_ref()
            .map(|s| s.client.clone());
        if let Some(q) = self.store.current_query_mut() {
            q.query = format_sql(&q.query, client.as_deref());
        }
    }

    /// Connects to `server_id` (and optional `db_name`) and transitions to [`ActiveRoute::QueryBrowser`].
    pub(crate) fn connect_to_server(
        &mut self,
        server_id: &str,
        db_name: Option<&str>,
        passphrase: Option<String>,
    ) {
        let rt = self.rt.clone();
        if rt
            .block_on(self.store.connect(server_id, db_name, false, passphrase))
            .is_ok()
        {
            self.route = ActiveRoute::QueryBrowser(server_id.to_string());
            if let Some(db) = self.store.current_database() {
                self.expanded_databases.insert(db.clone());
                let _ = rt.block_on(self.store.fetch_databases_if_needed());
                let _ = rt.block_on(self.store.fetch_schemas_if_needed(&db));
                let _ = rt.block_on(self.store.fetch_tables_if_needed(&db));
                let _ = rt.block_on(self.store.fetch_views_if_needed(&db));
                let _ = rt.block_on(self.store.fetch_routines_if_needed(&db));
            }
        }
    }

    /// Renders the application menu bar and breadcrumb navigation header.
    pub(crate) fn render_header(&mut self, ui: &mut egui::Ui) {
        egui::MenuBar::new().ui(ui, |ui| {
            ui.menu_button("File", |ui| {
                if ui.button("New Tab (Ctrl+T)").clicked() {
                    if let Some(db) = self.store.current_database() {
                        self.store.new_query(&db);
                    }
                    ui.close();
                }
                if ui.button("Close Tab (Ctrl+W)").clicked() {
                    if let Some(id) = self.store.queries.current_query_id {
                        self.store.remove_query(id);
                    }
                    ui.close();
                }
                ui.separator();
                if ui.button("Save Query (Ctrl+S)").clicked() {
                    self.show_save_query_modal = true;
                    ui.close();
                }
                if ui.button("Open Query (Ctrl+O)").clicked() {
                    self.show_open_query_modal = true;
                    ui.close();
                }
                ui.separator();
                if ui.button("Quit (Ctrl+Q)").clicked() {
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                }
            });
            ui.menu_button("Query", |ui| {
                if ui.button("Execute (Ctrl+Enter / Ctrl+R)").clicked() {
                    self.trigger_execute_current_query();
                    ui.close();
                }
            });
            ui.menu_button("Edit", |ui| {
                if ui.button("Beautify Query (Ctrl+I)").clicked() {
                    self.format_current_query();
                    ui.close();
                }
            });
            ui.menu_button("View", |ui| {
                for (label, delta) in [
                    ("Zoom In (Ctrl+=)", Some(0.2)),
                    ("Zoom Out (Ctrl+-)", Some(-0.2)),
                    ("Reset Zoom (Ctrl+0)", None),
                ] {
                    if ui.button(label).clicked() {
                        self.settings_draft.zoom_factor = match delta {
                            Some(d) => (self.settings_draft.zoom_factor + d).clamp(0.4, 3.0),
                            None => 1.0,
                        };
                        let base = self.settings_draft.to_base_config();
                        let _ = self.store.save_config(&base);
                        ui.close();
                    }
                }
            });
            ui.menu_button("Find", |ui| {
                if ui.button("Search databases (Shift+Ctrl+9)").clicked() {
                    self.sidebar_collapsed = false;
                    ui.close();
                }
            });
            ui.menu_button("Help", |ui| {
                if ui.button("Check for Updates").clicked() {
                    let rt = self.rt.clone();
                    self.update_apply_message = None;
                    self.update_modal_state = Some(rt.block_on(check_for_updates()));
                    ui.close();
                }
                if ui.button("Keyboard Shortcuts").clicked() {
                    self.show_shortcuts_modal = true;
                    ui.close();
                }
                ui.hyperlink_to(
                    "Report Issue",
                    "https://github.com/Rain6435/Rust-SQL/issues",
                );
                ui.hyperlink_to("About Rust-SQL", "https://github.com/Rain6435/Rust-SQL");
            });
        });

        ui.horizontal(|ui| {
            ui.heading(RichText::new("⚡ Rust-SQL").strong());
            ui.separator();

            match self.route.clone() {
                ActiveRoute::ServerManagement => {
                    ui.label(RichText::new("🖥 servers").strong());
                }
                ActiveRoute::QueryBrowser(ref server_id) => {
                    let srv_name = self
                        .store
                        .connections
                        .server
                        .as_ref()
                        .map(|s| s.name.clone())
                        .unwrap_or_else(|| server_id.clone());
                    let db_name = self.store.current_database().unwrap_or_default();
                    ui.label(RichText::new(format!("🖥 {srv_name}")).strong());
                    ui.label("›");
                    ui.label(RichText::new(format!("🗄 {db_name}")).strong());

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("⏻ Close connection").clicked() {
                            let rt = self.rt.clone();
                            rt.block_on(self.store.disconnect());
                            self.route = ActiveRoute::ServerManagement;
                        }
                        if ui.button("🔌 Reconnect").clicked() {
                            let sid = server_id.clone();
                            let db = self.store.current_database().unwrap_or_default();
                            let rt = self.rt.clone();
                            let _ = rt.block_on(self.store.reconnect(&sid, &db));
                        }
                    });
                }
            }
        });
    }

    /// Renders the bottom status bar and quick links footer.
    pub(crate) fn render_footer(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            if self.store.connections.mode == Some(ConnectionMode::Emulated) {
                crate::gui::query_results::render_emulated_banner(ui);
            }
            if !self.store.status.is_empty() {
                ui.label(RichText::new(&self.store.status).italics());
            } else {
                ui.label(RichText::new("Ready").weak());
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.small_button("⌨ Shortcuts").clicked() {
                    self.show_shortcuts_modal = true;
                }
                ui.hyperlink_to("GitHub", "https://github.com/Rain6435/Rust-SQL");
                if let Some(ref cfg) = self.store.config.data {
                    if cfg.log.console || cfg.log.file {
                        ui.label(
                            RichText::new(format!("Log: {}", cfg.log.level))
                                .small()
                                .color(Color32::from_rgb(234, 179, 8)),
                        );
                    }
                }
            });
        });
    }
}
