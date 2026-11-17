//! Server management dashboard rendering (`ActiveRoute::ServerManagement`).

use crate::common::types::Server;
use crate::common::utils::escape_regexp_string;
use crate::gui::{RustSqlGuiApp, ServerFormDraft, SettingsFormDraft};
use eframe::egui::{self, Color32, RichText, Stroke};
use regex::RegexBuilder;

impl RustSqlGuiApp {
    /// Renders the server search bar, connection error banner, and server cards/list grid.
    pub(crate) fn render_server_management(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label("🔍");
            ui.add(
                egui::TextEdit::singleline(&mut self.server_filter)
                    .hint_text("Search...")
                    .desired_width(280.0),
            );
            if ui.button("➕ Add").clicked() {
                let _ = self.store.start_editing_server(None);
                self.server_draft = ServerFormDraft::from_server(None);
            }
            if ui.button("⚙ Settings").clicked() {
                self.store.start_editing_config();
                self.settings_draft = SettingsFormDraft::from_store(&self.store);
            }
        });

        ui.add_space(8.0);

        if let Some(ref err) = self.store.connections.error.clone() {
            egui::Frame::group(ui.style())
                .fill(Color32::from_rgba_unmultiplied(220, 38, 38, 35))
                .stroke(Stroke::new(1.0_f32, Color32::from_rgb(220, 38, 38)))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(format!("Connection Error: {err}"))
                                .color(Color32::LIGHT_RED),
                        );
                        if ui.small_button("✖").clicked() {
                            self.store.connections.error = None;
                        }
                    });
                });
            ui.add_space(8.0);
        }

        let pattern = escape_regexp_string(&self.server_filter);
        let regex = RegexBuilder::new(&pattern)
            .case_insensitive(true)
            .build()
            .ok();

        let filtered_servers: Vec<Server> = self
            .store
            .servers
            .items
            .iter()
            .filter(|srv| {
                if self.server_filter.trim().is_empty() {
                    true
                } else if let Some(ref re) = regex {
                    re.is_match(&srv.name)
                } else {
                    srv.name
                        .to_lowercase()
                        .contains(&self.server_filter.to_lowercase())
                }
            })
            .cloned()
            .collect();

        if filtered_servers.is_empty() {
            ui.centered_and_justified(|ui| {
                ui.label(RichText::new("No results").size(16.0).weak());
            });
            return;
        }

        let as_list = self
            .store
            .config
            .data
            .as_ref()
            .map(|c| c.connections_as_list)
            .unwrap_or(false);

        egui::ScrollArea::vertical().show(ui, |ui| {
            if as_list {
                for srv in filtered_servers {
                    self.render_server_card_or_row(ui, &srv, true);
                    ui.add_space(4.0);
                }
            } else {
                ui.horizontal_wrapped(|ui| {
                    for srv in filtered_servers {
                        ui.allocate_ui(egui::vec2(260.0, 130.0), |ui| {
                            self.render_server_card_or_row(ui, &srv, false);
                        });
                    }
                });
            }
        });
    }

    fn render_server_card_or_row(&mut self, ui: &mut egui::Ui, srv: &Server, is_row: bool) {
        let addr = if let Some(ref h) = srv.host {
            format!("{}:{}", h, srv.port.unwrap_or(0))
        } else {
            srv.socket_path.clone().unwrap_or_default()
        };
        let meta = if let Some(ref ssh) = srv.ssh {
            format!("{addr} via {}", ssh.host)
        } else {
            addr
        };

        egui::Frame::group(ui.style())
            .inner_margin(10.0)
            .show(ui, |ui| {
                if is_row {
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(format!("[{}]", srv.client.to_uppercase())).strong(),
                        );
                        ui.label(RichText::new(&srv.name).strong().size(15.0));
                        ui.label(RichText::new(&meta).weak());
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.button("Connect").clicked() {
                                self.connect_to_server(&srv.id, None, None);
                            }
                            if ui.button("✏ Edit").clicked() {
                                let _ = self.store.start_editing_server(Some(&srv.id));
                                let editing = self.store.servers.editing_server.clone();
                                self.server_draft = ServerFormDraft::from_server(editing.as_ref());
                            }
                        });
                    });
                } else {
                    ui.vertical(|ui| {
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new(format!("[{}]", srv.client.to_uppercase()))
                                    .color(Color32::from_rgb(56, 189, 248))
                                    .strong(),
                            );
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if ui.small_button("✏").on_hover_text("Edit server").clicked()
                                    {
                                        let _ = self.store.start_editing_server(Some(&srv.id));
                                        let editing = self.store.servers.editing_server.clone();
                                        self.server_draft =
                                            ServerFormDraft::from_server(editing.as_ref());
                                    }
                                },
                            );
                        });
                        ui.label(RichText::new(&srv.name).strong().size(16.0));
                        ui.label(RichText::new(&meta).small().weak());
                        ui.add_space(6.0);
                        if ui
                            .add_sized([ui.available_width(), 26.0], egui::Button::new("Connect"))
                            .clicked()
                        {
                            self.connect_to_server(&srv.id, None, None);
                        }
                    });
                }
            });
    }
}
