//! Server Add/Edit modal and Server Delete confirmation modal rendering.

use crate::adapters::CLIENTS;
use crate::core::connection_string::{build_connection_uri, parse_connection_uri};
use crate::gui::icons::{icon_primary_button, icon_subtle_button, paint_icon_ui, IconKind};
use crate::gui::theme::modal_section;
use crate::gui::{RustSqlGuiApp, ServerFormDraft};
use eframe::egui::{self, CornerRadius, Margin, RichText, Stroke};
use std::collections::HashSet;

impl RustSqlGuiApp {
    /// Renders the Server Add/Edit configuration modal when `self.store.servers.is_editing` is true.
    pub(crate) fn render_server_modal(&mut self, ctx: &egui::Context) {
        if !self.store.servers.is_editing {
            return;
        }
        let theme = self.current_theme();
        egui::Window::new("Server Configuration")
            .collapsible(false)
            .resizable(true)
            .default_width(640.0)
            .show(ctx, |ui| {
                if self.store.connections.test_connected {
                    egui::Frame::new()
                        .fill(theme.semantic_ok.gamma_multiply(0.14))
                        .stroke(Stroke::new(1.0, theme.semantic_ok.gamma_multiply(0.65)))
                        .corner_radius(CornerRadius::same(6))
                        .inner_margin(Margin::symmetric(10, 6))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                paint_icon_ui(ui, IconKind::Check, 13.0, theme.semantic_ok);
                                ui.label(
                                    RichText::new("Successfully connected")
                                        .strong()
                                        .color(theme.semantic_ok),
                                );
                            });
                        });
                    ui.add_space(6.0);
                } else if let Some(ref err) = self.store.connections.test_error {
                    egui::Frame::new()
                        .fill(theme.semantic_err.gamma_multiply(0.14))
                        .stroke(Stroke::new(1.0, theme.semantic_err.gamma_multiply(0.65)))
                        .corner_radius(CornerRadius::same(6))
                        .inner_margin(Margin::symmetric(10, 6))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                paint_icon_ui(ui, IconKind::Warning, 13.0, theme.semantic_err);
                                ui.label(
                                    RichText::new(format!("Connection Error: {err}"))
                                        .color(theme.semantic_err),
                                );
                            });
                        });
                    ui.add_space(6.0);
                }
                if let Some(ref v_errs) = self.store.servers.error {
                    egui::Frame::new()
                        .fill(theme.semantic_err.gamma_multiply(0.12))
                        .stroke(Stroke::new(1.0, theme.semantic_err.gamma_multiply(0.6)))
                        .corner_radius(CornerRadius::same(6))
                        .inner_margin(Margin::symmetric(10, 6))
                        .show(ui, |ui| {
                            for (field, msg) in v_errs {
                                ui.label(
                                    RichText::new(format!("• {field}: {msg}"))
                                        .small()
                                        .color(theme.semantic_err),
                                );
                            }
                        });
                    ui.add_space(6.0);
                }

                let disabled: HashSet<String> = CLIENTS
                    .iter()
                    .find(|c| c.key == self.server_draft.client)
                    .map(|c| c.disabled_features.iter().cloned().collect())
                    .unwrap_or_default();

                egui::ScrollArea::vertical()
                    .max_height(460.0)
                    .show(ui, |ui| {
                        self.render_server_modal_fields(ui, &disabled);
                    });

                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if icon_subtle_button(ui, &theme, IconKind::Play, "Test").clicked() {
                        let srv = self.server_draft.to_server();
                        let rt = self.rt.clone();
                        let _ = rt.block_on(self.store.test_connection(srv));
                    }
                    let is_new = self.server_draft.id.trim().is_empty();
                    if !is_new
                        && icon_subtle_button(ui, &theme, IconKind::Copy, "Duplicate").clicked()
                    {
                        let srv = self.server_draft.to_server();
                        let _ = self.store.duplicate_server(srv);
                    }
                    if !is_new
                        && icon_subtle_button(ui, &theme, IconKind::Trash, "Remove").clicked()
                    {
                        self.confirm_delete_server = true;
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if icon_primary_button(ui, &theme, IconKind::Save, "Save").clicked() {
                            let srv = self.server_draft.to_server();
                            let id = (!self.server_draft.id.trim().is_empty())
                                .then(|| self.server_draft.id.clone());
                            let _ = self.store.save_server(srv, id.as_deref());
                        }
                        if ui
                            .add(crate::gui::theme::subtle_button(&theme, "Cancel"))
                            .clicked()
                        {
                            self.store.finish_editing_server();
                        }
                    });
                });
            });
    }

    fn render_server_modal_fields(&mut self, ui: &mut egui::Ui, disabled: &HashSet<String>) {
        let theme = self.current_theme();

        modal_section(ui, &theme, IconKind::Server, "Identity", |ui| {
            ui.horizontal(|ui| {
                ui.label("Name:");
                ui.add(
                    egui::TextEdit::singleline(&mut self.server_draft.name)
                        .hint_text("e.g. Production Postgres")
                        .desired_width(210.0),
                );
                ui.label("Engine:");
                let prev_client = self.server_draft.client.clone();
                egui::ComboBox::from_id_salt("server_client_combo")
                    .selected_text(&self.server_draft.client)
                    .show_ui(ui, |ui| {
                        for c in CLIENTS.iter() {
                            ui.selectable_value(
                                &mut self.server_draft.client,
                                c.key.clone(),
                                &c.name,
                            );
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
        });

        ui.add_space(6.0);

        modal_section(ui, &theme, IconKind::Database, "Server Address", |ui| {
            let no_sock = self.server_draft.socket_path.trim().is_empty();
            ui.horizontal(|ui| {
                ui.label("Host:");
                ui.add_enabled(
                    !disabled.contains("server:host") && no_sock,
                    egui::TextEdit::singleline(&mut self.server_draft.host)
                        .hint_text("localhost")
                        .desired_width(190.0),
                );
                ui.label("Port:");
                ui.add_enabled(
                    !disabled.contains("server:port") && no_sock,
                    egui::TextEdit::singleline(&mut self.server_draft.port).desired_width(68.0),
                );
                ui.label("Domain:");
                ui.add_enabled(
                    !disabled.contains("server:domain"),
                    egui::TextEdit::singleline(&mut self.server_draft.domain).desired_width(110.0),
                );
            });
            ui.horizontal(|ui| {
                ui.label("Unix Socket:");
                ui.add_enabled(
                    !disabled.contains("server:socketPath")
                        && self.server_draft.host.trim().is_empty(),
                    egui::TextEdit::singleline(&mut self.server_draft.socket_path)
                        .desired_width(340.0),
                );
            });
        });

        ui.add_space(6.0);

        modal_section(ui, &theme, IconKind::Lock, "Credentials & Database", |ui| {
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
                    "Hide"
                } else {
                    "Show"
                };
                if ui
                    .add(crate::gui::theme::subtle_button(&theme, eye_label))
                    .clicked()
                {
                    self.server_draft.show_plain_password = !self.server_draft.show_plain_password;
                    self.server_draft.conn_uri = build_connection_uri(
                        &self.server_draft.to_server(),
                        self.server_draft.show_plain_password,
                    );
                }
            });

            ui.horizontal(|ui| {
                ui.label("Database / Keyspace:");
                ui.add(
                    egui::TextEdit::singleline(&mut self.server_draft.database)
                        .desired_width(150.0),
                );
                ui.label("Schema:");
                ui.add_enabled(
                    !disabled.contains("server:schema"),
                    egui::TextEdit::singleline(&mut self.server_draft.schema).desired_width(120.0),
                );
            });

            ui.horizontal(|ui| {
                ui.label("Statement timeout (s):")
                    .on_hover_text("Empty or 0 = no timeout");
                ui.add(
                    egui::TextEdit::singleline(&mut self.server_draft.statement_timeout_secs)
                        .desired_width(60.0),
                );
                ui.checkbox(
                    &mut self.server_draft.exclude_from_history,
                    "Exclude from query history",
                );
            });

            ui.horizontal(|ui| {
                ui.label("Connection URI:");
                let resp = ui.add_enabled(
                    self.server_draft.show_plain_password,
                    egui::TextEdit::singleline(&mut self.server_draft.conn_uri)
                        .desired_width(380.0),
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
        });

        if !disabled.contains("server:ssh") {
            ui.add_space(6.0);
            modal_section(ui, &theme, IconKind::Lock, "SSH Tunnel", |ui| {
                ui.checkbox(&mut self.server_draft.ssh_enabled, "Enable SSH Tunnel");
                if self.server_draft.ssh_enabled {
                    ui.horizontal(|ui| {
                        ui.label("SSH Host:");
                        ui.text_edit_singleline(&mut self.server_draft.ssh_host);
                        ui.label("Port:");
                        ui.add(
                            egui::TextEdit::singleline(&mut self.server_draft.ssh_port)
                                .desired_width(60.0),
                        );
                        ui.checkbox(&mut self.server_draft.ssh_use_agent, "Use ssh-agent");
                    });
                    ui.horizontal(|ui| {
                        ui.label("SSH User:");
                        ui.text_edit_singleline(&mut self.server_draft.ssh_user);
                        ui.label("Password:");
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
            });
        }

        ui.add_space(6.0);
        modal_section(ui, &theme, IconKind::Search, "Object Filters", |ui| {
            ui.checkbox(&mut self.server_draft.filter_enabled, "Enable Filters");
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
        });
    }
}
