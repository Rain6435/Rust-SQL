//! Server Add/Edit modal and Server Delete confirmation modal rendering.

use crate::adapters::CLIENTS;
use crate::core::connection_string::{build_connection_uri, parse_connection_uri};
use crate::gui::{RustSqlGuiApp, ServerFormDraft};
use eframe::egui::{self, Color32, RichText};
use std::collections::HashSet;

impl RustSqlGuiApp {
    /// Renders the Server Add/Edit configuration modal when `self.store.servers.is_editing` is true.
    pub(crate) fn render_server_modal(&mut self, ctx: &egui::Context) {
        if !self.store.servers.is_editing {
            return;
        }
        egui::Window::new("Server Information")
            .collapsible(false)
            .resizable(true)
            .default_width(620.0)
            .show(ctx, |ui| {
                if self.store.connections.test_connected {
                    ui.colored_label(Color32::GREEN, "✔ Successfully connected");
                } else if let Some(ref err) = self.store.connections.test_error {
                    ui.colored_label(Color32::LIGHT_RED, format!("✖ Connection Error: {err}"));
                }
                if let Some(ref v_errs) = self.store.servers.error {
                    for (field, msg) in v_errs {
                        ui.colored_label(Color32::LIGHT_RED, format!("• {field}: {msg}"));
                    }
                }

                let disabled: HashSet<String> = CLIENTS
                    .iter()
                    .find(|c| c.key == self.server_draft.client)
                    .map(|c| c.disabled_features.iter().cloned().collect())
                    .unwrap_or_default();

                egui::ScrollArea::vertical()
                    .max_height(420.0)
                    .show(ui, |ui| {
                        self.render_server_modal_fields(ui, &disabled);
                    });

                ui.separator();
                ui.horizontal(|ui| {
                    if ui.button("Test").clicked() {
                        let srv = self.server_draft.to_server();
                        let rt = self.rt.clone();
                        let _ = rt.block_on(self.store.test_connection(srv));
                    }
                    let is_new = self.server_draft.id.trim().is_empty();
                    if !is_new && ui.button("Duplicate").clicked() {
                        let srv = self.server_draft.to_server();
                        let _ = self.store.duplicate_server(srv);
                    }
                    if !is_new && ui.button("🗑 Remove").clicked() {
                        self.confirm_delete_server = true;
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("Save").clicked() {
                            let srv = self.server_draft.to_server();
                            let id = (!self.server_draft.id.trim().is_empty())
                                .then(|| self.server_draft.id.clone());
                            let _ = self.store.save_server(srv, id.as_deref());
                        }
                        if ui.button("Cancel").clicked() {
                            self.store.finish_editing_server();
                        }
                    });
                });
            });
    }

    fn render_server_modal_fields(&mut self, ui: &mut egui::Ui, disabled: &HashSet<String>) {
        ui.horizontal(|ui| {
            ui.label("Name:");
            ui.text_edit_singleline(&mut self.server_draft.name);
            ui.label("Database Type:");
            let prev_client = self.server_draft.client.clone();
            egui::ComboBox::from_id_salt("server_client_combo")
                .selected_text(&self.server_draft.client)
                .show_ui(ui, |ui| {
                    for c in CLIENTS.iter() {
                        ui.selectable_value(&mut self.server_draft.client, c.key.clone(), &c.name);
                    }
                });
            if self.server_draft.client != prev_client {
                if let Some(dp) = CLIENTS
                    .iter()
                    .find(|c| c.key == self.server_draft.client)
                    .and_then(|c| c.default_port)
                {
                    self.server_draft.port = dp.to_string();
                }
            }
            ui.add_enabled(
                !disabled.contains("server:ssl"),
                egui::Checkbox::new(&mut self.server_draft.ssl, "SSL"),
            );
        });

        ui.separator();
        ui.label(RichText::new("Server Address").strong());
        let no_sock = self.server_draft.socket_path.trim().is_empty();
        ui.horizontal(|ui| {
            ui.label("Host:");
            ui.add_enabled(
                !disabled.contains("server:host") && no_sock,
                egui::TextEdit::singleline(&mut self.server_draft.host).desired_width(180.0),
            );
            ui.label("Port:");
            ui.add_enabled(
                !disabled.contains("server:port") && no_sock,
                egui::TextEdit::singleline(&mut self.server_draft.port).desired_width(70.0),
            );
            ui.label("Domain:");
            ui.add_enabled(
                !disabled.contains("server:domain"),
                egui::TextEdit::singleline(&mut self.server_draft.domain).desired_width(110.0),
            );
        });
        ui.horizontal(|ui| {
            ui.label("Unix Socket Path:");
            ui.add_enabled(
                !disabled.contains("server:socketPath") && self.server_draft.host.trim().is_empty(),
                egui::TextEdit::singleline(&mut self.server_draft.socket_path).desired_width(300.0),
            );
        });

        ui.separator();
        ui.label(RichText::new("Credentials & Database").strong());
        ui.horizontal(|ui| {
            ui.label("User:");
            ui.add_enabled(
                !disabled.contains("server:user"),
                egui::TextEdit::singleline(&mut self.server_draft.user).desired_width(150.0),
            );
            ui.label("Password:");
            ui.add_enabled(
                !disabled.contains("server:password"),
                egui::TextEdit::singleline(&mut self.server_draft.password)
                    .password(!self.server_draft.show_plain_password)
                    .desired_width(150.0),
            );
            let eye_label = if self.server_draft.show_plain_password {
                "🙈"
            } else {
                "👁"
            };
            if ui.button(eye_label).clicked() {
                self.server_draft.show_plain_password = !self.server_draft.show_plain_password;
                self.server_draft.conn_uri = build_connection_uri(
                    &self.server_draft.to_server(),
                    self.server_draft.show_plain_password,
                );
            }
        });

        ui.horizontal(|ui| {
            ui.label("Initial Database/Keyspace:");
            ui.text_edit_singleline(&mut self.server_draft.database);
            ui.label("Initial Schema:");
            ui.add_enabled(
                !disabled.contains("server:schema"),
                egui::TextEdit::singleline(&mut self.server_draft.schema).desired_width(120.0),
            );
        });

        ui.horizontal(|ui| {
            ui.label("Connection URI:");
            let resp = ui.add_enabled(
                self.server_draft.show_plain_password,
                egui::TextEdit::singleline(&mut self.server_draft.conn_uri).desired_width(360.0),
            );
            if resp.changed() {
                if let Some(parsed) = parse_connection_uri(
                    &self.server_draft.conn_uri,
                    Some(&self.server_draft.to_server()),
                ) {
                    let show_pw = self.server_draft.show_plain_password;
                    self.server_draft = ServerFormDraft::from_server(Some(&parsed));
                    self.server_draft.show_plain_password = show_pw;
                }
            }
        });

        if !disabled.contains("server:ssh") {
            ui.separator();
            ui.checkbox(&mut self.server_draft.ssh_enabled, "SSH Tunnel");
            if self.server_draft.ssh_enabled {
                ui.horizontal(|ui| {
                    ui.label("SSH Host:");
                    ui.text_edit_singleline(&mut self.server_draft.ssh_host);
                    ui.label("SSH Port:");
                    ui.add(
                        egui::TextEdit::singleline(&mut self.server_draft.ssh_port)
                            .desired_width(60.0),
                    );
                    ui.checkbox(&mut self.server_draft.ssh_use_agent, "Use ssh agent");
                });
                ui.horizontal(|ui| {
                    ui.label("SSH User:");
                    ui.text_edit_singleline(&mut self.server_draft.ssh_user);
                    ui.label("SSH Password:");
                    ui.add_enabled(
                        !self.server_draft.ssh_use_agent
                            && self.server_draft.ssh_private_key.trim().is_empty(),
                        egui::TextEdit::singleline(&mut self.server_draft.ssh_password)
                            .password(true),
                    );
                });
                ui.horizontal(|ui| {
                    ui.label("Private Key:");
                    ui.add_enabled(
                        !self.server_draft.ssh_use_agent
                            && self.server_draft.ssh_password.trim().is_empty(),
                        egui::TextEdit::singleline(&mut self.server_draft.ssh_private_key),
                    );
                    ui.checkbox(
                        &mut self.server_draft.ssh_private_key_with_passphrase,
                        "Passphrase",
                    );
                });
            }
        }

        ui.separator();
        ui.checkbox(&mut self.server_draft.filter_enabled, "Filter");
        if self.server_draft.filter_enabled {
            for (l1, f1, l2, f2) in [
                (
                    "Database Only:",
                    &mut self.server_draft.filter_db_only,
                    "Database Ignore:",
                    &mut self.server_draft.filter_db_ignore,
                ),
                (
                    "Schema Only:",
                    &mut self.server_draft.filter_schema_only,
                    "Schema Ignore:",
                    &mut self.server_draft.filter_schema_ignore,
                ),
            ] {
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.label(l1);
                        ui.text_edit_multiline(f1);
                    });
                    ui.vertical(|ui| {
                        ui.label(l2);
                        ui.text_edit_multiline(f2);
                    });
                });
            }
        }
    }
}
