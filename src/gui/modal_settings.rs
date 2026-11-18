//! Settings modal, cell content preview modal, SSH passphrase prompt modal,
//! keyboard shortcuts modal, and database client info modal.

use crate::adapters::CLIENTS;
use crate::common::utils::value_to_string;
use crate::gui::RustSqlGuiApp;
use eframe::egui::{self, RichText};
use serde_json::Value;

impl RustSqlGuiApp {
    /// Renders the application Settings modal when `self.store.config.is_editing` is true.
    pub(crate) fn render_settings_modal(&mut self, ctx: &egui::Context) {
        if !self.store.config.is_editing {
            return;
        }
        egui::Window::new("Settings")
            .collapsible(false)
            .resizable(true)
            .default_width(520.0)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(format!(
                        "Zoom Factor ({}%):",
                        (self.settings_draft.zoom_factor * 100.0).round() as i32
                    ));
                    ui.add(
                        egui::Slider::new(&mut self.settings_draft.zoom_factor, 0.4..=3.0)
                            .step_by(0.2),
                    );
                });
                ui.horizontal(|ui| {
                    ui.label("Limit of Rows from Select Top Query:");
                    ui.text_edit_singleline(
                        &mut self.settings_draft.limit_query_default_select_top,
                    );
                });
                ui.separator();
                ui.horizontal(|ui| {
                    ui.checkbox(&mut self.settings_draft.enabled_dark_theme, "Dark Theme");
                    ui.checkbox(
                        &mut self.settings_draft.disabled_open_animation,
                        "Disable Intro",
                    );
                    ui.checkbox(
                        &mut self.settings_draft.connections_as_list,
                        "List Connections",
                    );
                });
                ui.horizontal(|ui| {
                    ui.label("Custom Font:");
                    ui.text_edit_singleline(&mut self.settings_draft.custom_font);
                });
                ui.separator();
                ui.horizontal(|ui| {
                    ui.checkbox(
                        &mut self.settings_draft.enabled_auto_complete,
                        "Auto Complete",
                    );
                    ui.checkbox(
                        &mut self.settings_draft.enabled_live_auto_complete,
                        "Live Auto Complete",
                    );
                });
                ui.horizontal(|ui| {
                    ui.checkbox(
                        &mut self.settings_draft.history_store_bound_values,
                        "Keep parameter values in history",
                    )
                    .on_hover_text("Off: history keeps only the query with its placeholders");
                    ui.label("Result page size:");
                    ui.add(
                        egui::TextEdit::singleline(&mut self.settings_draft.result_page_size)
                            .desired_width(60.0),
                    );
                });
                ui.separator();
                ui.horizontal(|ui| {
                    ui.label("Custom CSV Delimiter:");
                    ui.add_enabled(
                        !self.settings_draft.tab_delimited,
                        egui::TextEdit::singleline(&mut self.settings_draft.csv_delimiter)
                            .desired_width(60.0),
                    );
                    ui.checkbox(
                        &mut self.settings_draft.tab_delimited,
                        "Tab Delimited Values",
                    );
                });
                ui.separator();
                if ui.button("Trusted SSH host keys...").clicked() {
                    self.show_host_keys_modal = true;
                }
                ui.separator();
                ui.label(RichText::new("Logging").strong());
                ui.horizontal(|ui| {
                    ui.checkbox(&mut self.settings_draft.log_console, "Console");
                    ui.checkbox(&mut self.settings_draft.log_file, "File");
                    ui.label("Level:");
                    egui::ComboBox::from_id_salt("log_level_combo")
                        .selected_text(&self.settings_draft.log_level)
                        .show_ui(ui, |ui| {
                            for lvl in ["debug", "info", "warn", "error"] {
                                ui.selectable_value(
                                    &mut self.settings_draft.log_level,
                                    lvl.to_string(),
                                    lvl,
                                );
                            }
                        });
                });
                ui.horizontal(|ui| {
                    ui.label("Log Path:");
                    ui.text_edit_singleline(&mut self.settings_draft.log_path);
                });
                if let Some(ref p) = self.store.config.path {
                    ui.label(RichText::new(format!("Config Path: {p}")).small().weak());
                }
                ui.separator();
                ui.horizontal(|ui| {
                    if ui.button("Save").clicked() {
                        let base = self.settings_draft.to_base_config();
                        let _ = self.store.save_config(&base);
                    }
                    if ui.button("Cancel").clicked() {
                        self.store.finish_editing_config();
                    }
                });
            });
    }

    /// Renders the Cell Content Preview modal (`Plain Text` / `JSON`).
    pub(crate) fn render_preview_modal(&mut self, ctx: &egui::Context) {
        let Some(val) = self.preview_cell_value.clone() else {
            return;
        };
        egui::Window::new("Content Preview")
            .collapsible(false)
            .resizable(true)
            .default_width(500.0)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    if ui
                        .selectable_label(self.preview_mode == "plain", "Plain Text")
                        .clicked()
                    {
                        self.preview_mode = "plain".to_string();
                    }
                    if ui
                        .selectable_label(self.preview_mode == "json", "JSON")
                        .clicked()
                    {
                        self.preview_mode = "json".to_string();
                    }
                });
                ui.separator();
                let formatted = if self.preview_mode == "json" {
                    if let Value::String(ref s) = val {
                        if let Ok(parsed) = serde_json::from_str::<Value>(s) {
                            serde_json::to_string_pretty(&parsed).unwrap_or_else(|_| s.clone())
                        } else {
                            serde_json::to_string_pretty(&val)
                                .unwrap_or_else(|_| "Not valid format".to_string())
                        }
                    } else {
                        serde_json::to_string_pretty(&val)
                            .unwrap_or_else(|_| "Not valid format".to_string())
                    }
                } else {
                    value_to_string(&val)
                };
                egui::ScrollArea::vertical()
                    .max_height(350.0)
                    .show(ui, |ui| {
                        ui.monospace(formatted);
                    });
                ui.separator();
                if ui.button("Close").clicked() {
                    self.preview_cell_value = None;
                }
            });
    }

    /// Renders the SSH passphrase prompt, Keyboard Shortcuts modal, and Dialect Info modal.
    pub(crate) fn render_passphrase_and_info_modals(&mut self, ctx: &egui::Context) {
        if self.store.connections.waiting_private_key_passphrase {
            egui::Window::new("SSH Private Key Passphrase")
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
                    ui.label("Enter the private key passphrase:");
                    ui.add(
                        egui::TextEdit::singleline(&mut self.ssh_passphrase_input).password(true),
                    );
                    ui.horizontal(|ui| {
                        if ui.button("OK").clicked() {
                            if let Some(srv_id) =
                                self.store.connections.server.as_ref().map(|s| s.id.clone())
                            {
                                let pass = self.ssh_passphrase_input.clone();
                                self.ssh_passphrase_input.clear();
                                self.connect_to_server(&srv_id, None, Some(pass));
                            }
                        }
                        if ui.button("Cancel").clicked() {
                            let rt = self.rt.clone();
                            rt.block_on(self.store.disconnect());
                        }
                    });
                });
        }

        if self.show_shortcuts_modal {
            egui::Window::new("Keyboard Shortcuts")
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
                    ui.monospace("Ctrl+Enter / Ctrl+R  : Execute Query");
                    ui.monospace("Ctrl+T               : New Query Tab");
                    ui.monospace("Ctrl+W               : Close Query Tab");
                    ui.monospace("Ctrl+S               : Save Query");
                    ui.monospace("Ctrl+I               : Format SQL Query");
                    ui.monospace("Ctrl+Q               : Quit Application");
                    if ui.button("Close").clicked() {
                        self.show_shortcuts_modal = false;
                    }
                });
        }

        if self.show_client_info_modal {
            let client_key = self
                .store
                .connections
                .server
                .as_ref()
                .map(|s| s.client.clone())
                .unwrap_or_default();
            let client_name = CLIENTS
                .iter()
                .find(|c| c.key == client_key)
                .map(|c| c.name.as_str())
                .unwrap_or("Database");
            egui::Window::new(format!("{client_name} Query Information"))
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
                    let notes: &[&str] = match client_key.as_str() {
                        "mysql" => &[
                            "MySQL treats commented query as a non select query. So you may see \"affected rows\" for a commented query.",
                            "Usually executing a single query per tab will give better results.",
                        ],
                        "sqlserver" => &[
                            "MSSQL treats multiple non select queries as a single query result. So you affected rows will show the amount over all queries executed in the same tab.",
                            "Usually executing a single query per tab will give better results.",
                        ],
                        _ => &["Usually executing a single query per tab will give better results."],
                    };
                    for note in notes {
                        ui.label(format!("• {note}"));
                    }
                    if ui.button("Close").clicked() {
                        self.show_client_info_modal = false;
                    }
                });
        }
    }
}
