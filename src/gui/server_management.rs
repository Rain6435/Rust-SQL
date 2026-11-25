//! Server management dashboard rendering (`ActiveRoute::ServerManagement`).

use crate::common::types::Server;
use crate::common::utils::escape_regexp_string;
use crate::gui::icons::{
    icon_only_button, icon_primary_button, icon_primary_button_wide, icon_subtle_button,
    paint_icon_ui, IconKind,
};
use crate::gui::theme::{badge_pill, card_frame};
use crate::gui::{RustSqlGuiApp, ServerFormDraft, SettingsFormDraft};
use eframe::egui::{self, CornerRadius, Margin, RichText, Stroke};
use regex::RegexBuilder;

impl RustSqlGuiApp {
    /// Renders the server search bar, connection error banner, and server cards/list grid.
    pub(crate) fn render_server_management(&mut self, ui: &mut egui::Ui) {
        let theme = self.current_theme();

        let as_list = self
            .store
            .config
            .data
            .as_ref()
            .map(|c| c.connections_as_list)
            .unwrap_or(false);

        let mut filter_enter_pressed = false;

        egui::Frame::new()
            .fill(theme.bg_surface)
            .stroke(Stroke::new(1.0, theme.border_hairline))
            .corner_radius(CornerRadius::same(6))
            .inner_margin(Margin::symmetric(12, 8))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    paint_icon_ui(ui, IconKind::Server, 15.0, theme.accent_primary);
                    ui.label(
                        RichText::new("Connections")
                            .strong()
                            .size(15.0)
                            .color(theme.text_primary),
                    );
                    ui.separator();
                    paint_icon_ui(ui, IconKind::Search, 13.0, theme.text_muted);
                    let search_resp = ui.add(
                        egui::TextEdit::singleline(&mut self.server_filter)
                            .hint_text("Filter servers by name... (Enter to connect)")
                            .desired_width(240.0),
                    );
                    if search_resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        filter_enter_pressed = true;
                    }
                    if !self.server_filter.is_empty()
                        && ui.small_button("×").on_hover_text("Clear filter").clicked()
                    {
                        self.server_filter.clear();
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if icon_primary_button(ui, &theme, IconKind::Plus, "Add Server").clicked() {
                            let _ = self.store.start_editing_server(None);
                            self.server_draft = ServerFormDraft::from_server(None);
                        }
                        if icon_subtle_button(ui, &theme, IconKind::Settings, "Settings").clicked()
                        {
                            self.store.start_editing_config();
                            self.settings_draft = SettingsFormDraft::from_store(&self.store);
                        }
                        let (view_icon, view_label) = if as_list {
                            (IconKind::Grid, "Grid")
                        } else {
                            (IconKind::List, "List")
                        };
                        if icon_subtle_button(ui, &theme, view_icon, view_label)
                            .on_hover_text("Toggle between Grid and List view")
                            .clicked()
                        {
                            self.settings_draft.connections_as_list = !as_list;
                            let base = self.settings_draft.to_base_config();
                            let _ = self.store.save_config(&base);
                        }
                    });
                });
            });

        ui.add_space(10.0);

        if let Some(ref err) = self.store.connections.error.clone() {
            egui::Frame::new()
                .fill(theme.semantic_err.gamma_multiply(0.14))
                .stroke(Stroke::new(1.0, theme.semantic_err.gamma_multiply(0.7)))
                .corner_radius(CornerRadius::same(6))
                .inner_margin(Margin::symmetric(12, 8))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        paint_icon_ui(ui, IconKind::Warning, 14.0, theme.semantic_err);
                        ui.label(
                            RichText::new(format!("Connection Error: {err}"))
                                .color(theme.semantic_err)
                                .strong(),
                        );
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.small_button("Dismiss").clicked() {
                                self.store.connections.error = None;
                            }
                        });
                    });
                });
            ui.add_space(10.0);
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

        if filter_enter_pressed && filtered_servers.len() == 1 {
            let sid = filtered_servers[0].id.clone();
            self.connect_to_server(&sid, None, None);
            return;
        }

        if filtered_servers.is_empty() {
            ui.centered_and_justified(|ui| {
                let msg = if self.store.servers.items.is_empty() {
                    "No saved connections yet. Click '+ Add Server' to create one."
                } else {
                    "No matching servers found."
                };
                ui.label(RichText::new(msg).size(15.0).color(theme.text_muted));
            });
            return;
        }

        egui::ScrollArea::vertical().show(ui, |ui| {
            if as_list {
                for srv in filtered_servers {
                    self.render_server_card_or_row(ui, &srv, true);
                    ui.add_space(6.0);
                }
            } else {
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = egui::vec2(12.0, 12.0);
                    for srv in filtered_servers {
                        ui.allocate_ui(egui::vec2(280.0, 146.0), |ui| {
                            self.render_server_card_or_row(ui, &srv, false);
                        });
                    }
                });
            }
        });
    }

    fn render_server_card_or_row(&mut self, ui: &mut egui::Ui, srv: &Server, is_row: bool) {
        let theme = self.current_theme();
        let dialect_color = theme.dialect_color(&srv.client);
        let addr = if let Some(ref h) = srv.host {
            format!("{}:{}", h, srv.port.unwrap_or(0))
        } else {
            srv.socket_path
                .clone()
                .or_else(|| srv.database.clone())
                .unwrap_or_else(|| "local".to_string())
        };
        let meta = if let Some(ref ssh) = srv.ssh {
            format!("{addr}  via {}", ssh.host)
        } else {
            addr
        };

        card_frame(&theme).show(ui, |ui| {
            if is_row {
                ui.horizontal(|ui| {
                    crate::gui::icons::engine_logo_ui(ui, &srv.client, 16.0, &theme);
                    badge_pill(ui, &srv.client.to_uppercase(), dialect_color);
                    ui.label(
                        RichText::new(&srv.name)
                            .strong()
                            .size(15.0)
                            .color(theme.text_primary),
                    );
                    ui.label(
                        RichText::new(&meta)
                            .monospace()
                            .small()
                            .color(theme.text_muted),
                    );
                    if srv.ssh.is_some() {
                        badge_pill(ui, "SSH", theme.accent_secondary);
                    }
                    if srv.ssl.is_some() {
                        badge_pill(ui, "SSL", theme.semantic_ok);
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if icon_primary_button(ui, &theme, IconKind::Play, "Connect").clicked() {
                            self.connect_to_server(&srv.id, None, None);
                        }
                        if icon_subtle_button(ui, &theme, IconKind::Edit, "Edit").clicked() {
                            let _ = self.store.start_editing_server(Some(&srv.id));
                            let editing = self.store.servers.editing_server.clone();
                            self.server_draft = ServerFormDraft::from_server(editing.as_ref());
                        }
                    });
                });
            } else {
                ui.vertical(|ui| {
                    ui.horizontal(|ui| {
                        crate::gui::icons::engine_logo_ui(ui, &srv.client, 16.0, &theme);
                        badge_pill(ui, &srv.client.to_uppercase(), dialect_color);
                        if srv.ssh.is_some() {
                            badge_pill(ui, "SSH", theme.accent_secondary);
                        }
                        if srv.ssl.is_some() {
                            badge_pill(ui, "SSL", theme.semantic_ok);
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if icon_only_button(
                                ui,
                                &theme,
                                IconKind::Edit,
                                "Edit server configuration",
                            )
                            .clicked()
                            {
                                let _ = self.store.start_editing_server(Some(&srv.id));
                                let editing = self.store.servers.editing_server.clone();
                                self.server_draft = ServerFormDraft::from_server(editing.as_ref());
                            }
                        });
                    });
                    ui.add_space(6.0);
                    ui.label(
                        RichText::new(&srv.name)
                            .strong()
                            .size(16.0)
                            .color(theme.text_primary),
                    );
                    ui.label(
                        RichText::new(&meta)
                            .monospace()
                            .small()
                            .color(theme.text_muted),
                    );
                    ui.add_space(10.0);
                    if icon_primary_button_wide(
                        ui,
                        &theme,
                        IconKind::Play,
                        "Connect",
                        ui.available_width(),
                        28.0,
                    )
                    .clicked()
                    {
                        self.connect_to_server(&srv.id, None, None);
                    }
                });
            }
        });
    }
}
