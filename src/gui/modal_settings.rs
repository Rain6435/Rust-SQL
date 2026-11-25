//! Settings modal, cell content preview modal, SSH passphrase prompt modal,
//! keyboard shortcuts modal, and database client info modal.

use crate::adapters::CLIENTS;
use crate::common::utils::value_to_string;
use crate::core::export::write_clipboard_text;
use crate::gui::icons::{icon_primary_button, icon_subtle_button, IconKind};
use crate::gui::theme::{modal_section, subtle_button};
use crate::gui::RustSqlGuiApp;
use eframe::egui::{self, CornerRadius, Margin, RichText, Stroke};
use serde_json::Value;

impl RustSqlGuiApp {
    /// Renders the application Settings modal when `self.store.config.is_editing` is true.
    pub(crate) fn render_settings_modal(&mut self, ctx: &egui::Context) {
        if !self.store.config.is_editing {
            return;
        }
        let theme = self.current_theme();
        egui::Window::new("Settings")
            .collapsible(false)
            .resizable(true)
            .default_width(560.0)
            .show(ctx, |ui| {
                egui::ScrollArea::vertical()
                    .max_height(480.0)
                    .show(ui, |ui| {
                        modal_section(
                            ui,
                            &theme,
                            if theme.is_dark {
                                IconKind::Moon
                            } else {
                                IconKind::Sun
                            },
                            "Appearance & Layout",
                            |ui| {
                                ui.horizontal(|ui| {
                                    ui.label(format!(
                                        "Zoom ({}%):",
                                        (self.settings_draft.zoom_factor * 100.0).round() as i32
                                    ));
                                    ui.add(
                                        egui::Slider::new(
                                            &mut self.settings_draft.zoom_factor,
                                            0.4..=3.0,
                                        )
                                        .step_by(0.2),
                                    );
                                });
                                ui.horizontal(|ui| {
                                    ui.checkbox(
                                        &mut self.settings_draft.enabled_dark_theme,
                                        "Dark Theme",
                                    );
                                    ui.checkbox(
                                        &mut self.settings_draft.connections_as_list,
                                        "List Connections",
                                    );
                                    ui.checkbox(
                                        &mut self.settings_draft.disabled_open_animation,
                                        "Disable Intro",
                                    );
                                });
                                ui.horizontal(|ui| {
                                    ui.label("Custom Font:");
                                    ui.text_edit_singleline(&mut self.settings_draft.custom_font);
                                });
                            },
                        );

                        ui.add_space(6.0);

                        modal_section(
                            ui,
                            &theme,
                            IconKind::Format,
                            "Editor & Query Execution",
                            |ui| {
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
                                    ui.label("Select Top Limit:");
                                    ui.add(
                                        egui::TextEdit::singleline(
                                            &mut self.settings_draft.limit_query_default_select_top,
                                        )
                                        .desired_width(70.0),
                                    );
                                    ui.label("Result Page Size:");
                                    ui.add(
                                        egui::TextEdit::singleline(
                                            &mut self.settings_draft.result_page_size,
                                        )
                                        .desired_width(70.0),
                                    );
                                });
                                ui.checkbox(
                                    &mut self.settings_draft.history_store_bound_values,
                                    "Keep bound parameter values in query history",
                                )
                                .on_hover_text(
                                    "Off: history keeps only the query with its placeholders",
                                );
                            },
                        );

                        ui.add_space(6.0);

                        modal_section(ui, &theme, IconKind::Download, "CSV & Export", |ui| {
                            ui.horizontal(|ui| {
                                ui.label("CSV Delimiter:");
                                ui.add_enabled(
                                    !self.settings_draft.tab_delimited,
                                    egui::TextEdit::singleline(
                                        &mut self.settings_draft.csv_delimiter,
                                    )
                                    .desired_width(50.0),
                                );
                                ui.checkbox(
                                    &mut self.settings_draft.tab_delimited,
                                    "Tab Delimited (TSV)",
                                );
                            });
                        });

                        ui.add_space(6.0);

                        modal_section(ui, &theme, IconKind::Lock, "Security & Logging", |ui| {
                            if icon_subtle_button(
                                ui,
                                &theme,
                                IconKind::Lock,
                                "Trusted SSH Host Keys...",
                            )
                            .clicked()
                            {
                                self.show_host_keys_modal = true;
                            }
                            ui.add_space(4.0);
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
                                ui.label(
                                    RichText::new(format!("Config: {p}"))
                                        .small()
                                        .color(theme.text_muted),
                                );
                            }
                        });
                    });

                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if icon_primary_button(ui, &theme, IconKind::Save, "Save").clicked() {
                            let base = self.settings_draft.to_base_config();
                            let _ = self.store.save_config(&base);
                        }
                        if ui.add(subtle_button(&theme, "Cancel")).clicked() {
                            self.store.finish_editing_config();
                        }
                    });
                });
            });
    }

    /// Renders the Cell Content Preview modal (`Plain Text` / `JSON`).
    pub(crate) fn render_preview_modal(&mut self, ctx: &egui::Context) {
        let Some(val) = self.preview_cell_value.clone() else {
            return;
        };
        let theme = self.current_theme();
        egui::Window::new("Content Preview")
            .collapsible(false)
            .resizable(true)
            .default_width(520.0)
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
                ui.add_space(4.0);
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
                egui::Frame::new()
                    .fill(theme.bg_canvas)
                    .stroke(Stroke::new(1.0, theme.border_hairline))
                    .corner_radius(CornerRadius::same(6))
                    .inner_margin(Margin::same(10))
                    .show(ui, |ui| {
                        egui::ScrollArea::vertical()
                            .max_height(350.0)
                            .show(ui, |ui| {
                                ui.monospace(&formatted);
                            });
                    });
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    if icon_subtle_button(ui, &theme, IconKind::Copy, "Copy").clicked() {
                        write_clipboard_text(&formatted);
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.add(subtle_button(&theme, "Close")).clicked() {
                            self.preview_cell_value = None;
                        }
                    });
                });
            });
    }

    /// Renders the SSH passphrase prompt, Keyboard Shortcuts modal, and Dialect Info modal.
    pub(crate) fn render_passphrase_and_info_modals(&mut self, ctx: &egui::Context) {
        let theme = self.current_theme();

        if self.store.connections.waiting_private_key_passphrase {
            egui::Window::new("SSH Private Key Passphrase")
                .collapsible(false)
                .resizable(false)
                .default_width(380.0)
                .show(ctx, |ui| {
                    modal_section(ui, &theme, IconKind::Lock, "Unlock Private Key", |ui| {
                        ui.label("Enter the passphrase for your SSH private key:");
                        let resp = ui.add(
                            egui::TextEdit::singleline(&mut self.ssh_passphrase_input)
                                .password(true)
                                .desired_width(ui.available_width()),
                        );
                        let enter =
                            resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                        if enter {
                            if let Some(srv_id) =
                                self.store.connections.server.as_ref().map(|s| s.id.clone())
                            {
                                let pass = self.ssh_passphrase_input.clone();
                                self.ssh_passphrase_input.clear();
                                self.connect_to_server(&srv_id, None, Some(pass));
                            }
                        }
                    });
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if icon_primary_button(ui, &theme, IconKind::Check, "Unlock").clicked()
                            {
                                if let Some(srv_id) =
                                    self.store.connections.server.as_ref().map(|s| s.id.clone())
                                {
                                    let pass = self.ssh_passphrase_input.clone();
                                    self.ssh_passphrase_input.clear();
                                    self.connect_to_server(&srv_id, None, Some(pass));
                                }
                            }
                            if ui.add(subtle_button(&theme, "Cancel")).clicked() {
                                let rt = self.rt.clone();
                                rt.block_on(self.store.disconnect());
                            }
                        });
                    });
                });
        }

        if self.show_shortcuts_modal {
            egui::Window::new("Keyboard Shortcuts")
                .collapsible(false)
                .resizable(false)
                .default_width(420.0)
                .show(ctx, |ui| {
                    modal_section(ui, &theme, IconKind::Keyboard, "Keybindings", |ui| {
                        egui::Grid::new("shortcuts_grid")
                            .striped(true)
                            .spacing([20.0, 6.0])
                            .show(ui, |ui| {
                                for (keys, desc) in [
                                    ("Ctrl+Enter / Ctrl+R", "Execute Query"),
                                    ("Ctrl+T", "New Query Tab"),
                                    ("Ctrl+W", "Close Query Tab"),
                                    ("Ctrl+O", "Open Query"),
                                    ("Ctrl+S", "Save Query"),
                                    ("Ctrl+I", "Format SQL Query"),
                                    ("Ctrl+= / Ctrl+- / Ctrl+0", "Zoom In / Out / Reset"),
                                    ("Esc", "Close Active Modal"),
                                    ("Ctrl+Q", "Quit Application"),
                                ] {
                                    ui.monospace(
                                        RichText::new(keys).strong().color(theme.accent_secondary),
                                    );
                                    ui.label(desc);
                                    ui.end_row();
                                }
                            });
                    });
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.add(subtle_button(&theme, "Close")).clicked() {
                                self.show_shortcuts_modal = false;
                            }
                        });
                    });
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
            egui::Window::new(format!("{client_name} Query Notes"))
                .collapsible(false)
                .resizable(false)
                .default_width(440.0)
                .show(ctx, |ui| {
                    modal_section(
                        ui,
                        &theme,
                        IconKind::Warning,
                        "Dialect Behavior",
                        |ui| {
                            let notes: &[&str] = match client_key.as_str() {
                                "mysql" => &[
                                    "MySQL treats a commented query as a non-select query, so affected rows may be shown.",
                                    "Executing a single query per tab gives the clearest results.",
                                ],
                                "sqlserver" => &[
                                    "MSSQL treats multiple non-select queries as a single result, summing affected rows across the batch.",
                                    "Executing a single query per tab gives the clearest results.",
                                ],
                                _ => &["Executing a single query per tab gives the clearest results."],
                            };
                            for note in notes {
                                ui.label(format!("• {note}"));
                            }
                        },
                    );
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.add(subtle_button(&theme, "Close")).clicked() {
                                self.show_client_info_modal = false;
                            }
                        });
                    });
                });
        }
    }
}
