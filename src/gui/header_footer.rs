//! Global keyboard shortcuts, query execution triggers, top menu/breadcrumb header, and status footer.

use crate::common::types::ConnectionMode;
use crate::core::sql_formatter::format_sql;
use crate::core::updater::check_for_updates;
use crate::gui::{ActiveRoute, RustSqlGuiApp};
use eframe::egui::{self, Key, KeyboardShortcut, Modifiers, RichText};

impl RustSqlGuiApp {
    /// Processes global keyboard shortcuts (`Ctrl+Q`, `Ctrl+T`, `Ctrl+W`, `Ctrl+Enter`/`Ctrl+R`, `Ctrl+I`, `Ctrl+S`, `Ctrl+O`, Zoom, `Esc`).
    pub(crate) fn handle_global_shortcuts(&mut self, ctx: &egui::Context) {
        let ctrl_q = KeyboardShortcut::new(Modifiers::COMMAND, Key::Q);
        let ctrl_t = KeyboardShortcut::new(Modifiers::COMMAND, Key::T);
        let ctrl_w = KeyboardShortcut::new(Modifiers::COMMAND, Key::W);
        let ctrl_enter = KeyboardShortcut::new(Modifiers::COMMAND, Key::Enter);
        let ctrl_r = KeyboardShortcut::new(Modifiers::COMMAND, Key::R);
        let ctrl_i = KeyboardShortcut::new(Modifiers::COMMAND, Key::I);
        let ctrl_s = KeyboardShortcut::new(Modifiers::COMMAND, Key::S);
        let ctrl_o = KeyboardShortcut::new(Modifiers::COMMAND, Key::O);
        let ctrl_plus = KeyboardShortcut::new(Modifiers::COMMAND, Key::Equals);
        let ctrl_minus = KeyboardShortcut::new(Modifiers::COMMAND, Key::Minus);
        let ctrl_zero = KeyboardShortcut::new(Modifiers::COMMAND, Key::Num0);

        let mut zoom_delta: Option<Option<f64>> = None;

        ctx.input_mut(|i| {
            if i.consume_shortcut(&ctrl_q) {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            if i.consume_shortcut(&ctrl_plus) {
                zoom_delta = Some(Some(0.2));
            } else if i.consume_shortcut(&ctrl_minus) {
                zoom_delta = Some(Some(-0.2));
            } else if i.consume_shortcut(&ctrl_zero) {
                zoom_delta = Some(None);
            }

            if i.key_pressed(Key::Escape) {
                if self.preview_cell_value.is_some() {
                    self.preview_cell_value = None;
                } else if self.pending_safety_confirm.is_some() {
                    self.pending_safety_confirm = None;
                } else if self.show_save_query_modal {
                    self.show_save_query_modal = false;
                } else if self.show_open_query_modal {
                    self.show_open_query_modal = false;
                } else if self.show_export_file_modal.is_some() {
                    self.show_export_file_modal = None;
                } else if self.show_bookmarks_modal {
                    self.show_bookmarks_modal = false;
                } else if self.stats_modal_data.is_some() {
                    self.stats_modal_data = None;
                } else if self.erd_modal_diagram.is_some() {
                    self.erd_modal_diagram = None;
                } else if self.show_shortcuts_modal {
                    self.show_shortcuts_modal = false;
                } else if self.show_client_info_modal {
                    self.show_client_info_modal = false;
                } else if self.show_host_keys_modal {
                    self.show_host_keys_modal = false;
                } else if self.update_modal_state.is_some() {
                    self.update_modal_state = None;
                    self.update_apply_message = None;
                }
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
                if i.consume_shortcut(&ctrl_o) {
                    self.show_open_query_modal = true;
                }
            }
        });

        if let Some(delta) = zoom_delta {
            self.settings_draft.zoom_factor = match delta {
                Some(d) => (self.settings_draft.zoom_factor + d).clamp(0.4, 3.0),
                None => 1.0,
            };
            let base = self.settings_draft.to_base_config();
            let _ = self.store.save_config(&base);
        }
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

        let theme = self.current_theme();

        ui.horizontal(|ui| {
            crate::gui::logo::render_header_logo(ui, &mut self.logo_texture);
            ui.label(
                RichText::new("Rust-SQL")
                    .size(16.0)
                    .strong()
                    .color(theme.text_primary),
            );
            ui.separator();

            match self.route.clone() {
                ActiveRoute::ServerManagement => {
                    crate::gui::theme::badge_pill(ui, "Servers", theme.accent_secondary);
                }
                ActiveRoute::QueryBrowser(ref server_id) => {
                    let srv_name = self
                        .store
                        .connections
                        .server
                        .as_ref()
                        .map(|s| s.name.clone())
                        .unwrap_or_else(|| server_id.clone());
                    let client_key = self
                        .store
                        .connections
                        .server
                        .as_ref()
                        .map(|s| s.client.clone())
                        .unwrap_or_default();
                    let db_name = self.store.current_database().unwrap_or_default();

                    if !client_key.is_empty() {
                        crate::gui::icons::engine_logo_ui(ui, &client_key, 15.0, &theme);
                        crate::gui::theme::badge_pill(
                            ui,
                            &client_key.to_uppercase(),
                            theme.dialect_color(&client_key),
                        );
                    }
                    crate::gui::icons::paint_icon_ui(
                        ui,
                        crate::gui::icons::IconKind::Server,
                        13.0,
                        theme.text_muted,
                    );
                    ui.label(RichText::new(srv_name).strong().color(theme.text_primary));
                    ui.label(RichText::new("›").color(theme.text_muted));
                    crate::gui::icons::paint_icon_ui(
                        ui,
                        crate::gui::icons::IconKind::Database,
                        13.0,
                        theme.accent_secondary,
                    );
                    ui.label(
                        RichText::new(db_name)
                            .strong()
                            .color(theme.accent_secondary),
                    );

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if crate::gui::icons::icon_subtle_button(
                            ui,
                            &theme,
                            crate::gui::icons::IconKind::Disconnect,
                            "Disconnect",
                        )
                        .clicked()
                        {
                            let rt = self.rt.clone();
                            rt.block_on(self.store.disconnect());
                            self.route = ActiveRoute::ServerManagement;
                        }
                        if crate::gui::icons::icon_subtle_button(
                            ui,
                            &theme,
                            crate::gui::icons::IconKind::Refresh,
                            "Reconnect",
                        )
                        .clicked()
                        {
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
        let theme = self.current_theme();
        ui.horizontal(|ui| {
            if self.store.connections.mode == Some(ConnectionMode::Emulated) {
                crate::gui::query_results::render_emulated_banner(ui);
            } else if matches!(self.route, ActiveRoute::QueryBrowser(_)) {
                crate::gui::theme::badge_pill(ui, "LIVE", theme.semantic_ok);
            }

            if !self.store.status.is_empty() {
                ui.label(RichText::new(&self.store.status).color(theme.text_primary));
            } else {
                ui.label(RichText::new("Ready").color(theme.text_muted));
            }

            if let Some(t) = self.store.statement_timeout_label() {
                ui.separator();
                ui.label(RichText::new(t).small().color(theme.text_muted))
                    .on_hover_text("Per-statement timeout for this connection");
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let (theme_icon, theme_label) = if theme.is_dark {
                    (crate::gui::icons::IconKind::Sun, "Light")
                } else {
                    (crate::gui::icons::IconKind::Moon, "Dark")
                };
                if ui
                    .horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 4.0;
                        crate::gui::icons::paint_icon_ui(ui, theme_icon, 12.0, theme.text_muted);
                        ui.small_button(theme_label)
                    })
                    .inner
                    .on_hover_text("Switch color theme")
                    .clicked()
                {
                    self.settings_draft.enabled_dark_theme = !theme.is_dark;
                    let base = self.settings_draft.to_base_config();
                    let _ = self.store.save_config(&base);
                }
                if ui
                    .horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 4.0;
                        crate::gui::icons::paint_icon_ui(
                            ui,
                            crate::gui::icons::IconKind::Keyboard,
                            12.0,
                            theme.text_muted,
                        );
                        ui.small_button("Shortcuts")
                    })
                    .inner
                    .clicked()
                {
                    self.show_shortcuts_modal = true;
                }
                ui.hyperlink_to("GitHub", "https://github.com/Rain6435/Rust-SQL");
                if let Some(ref cfg) = self.store.config.data {
                    if cfg.log.console || cfg.log.file {
                        crate::gui::theme::badge_pill(
                            ui,
                            &format!("Log: {}", cfg.log.level),
                            theme.semantic_warn,
                        );
                    }
                }
            });
        });
    }
}
