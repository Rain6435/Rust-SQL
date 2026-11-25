//! File Save/Open/Export modals, Destructive Query Safety warning modal, and Software Update modal.

use crate::core::updater::{perform_self_update, UpdateCheckResult};
use crate::gui::icons::{icon_primary_button, paint_icon_ui, IconKind};
use crate::gui::theme::{danger_button, modal_section, subtle_button};
use crate::gui::RustSqlGuiApp;
use eframe::egui::{self, CornerRadius, Margin, RichText, Stroke};

impl RustSqlGuiApp {
    /// Renders the server deletion confirmation dialog when `self.confirm_delete_server` is true.
    pub(crate) fn render_confirm_delete_modal(&mut self, ctx: &egui::Context) {
        if !self.confirm_delete_server {
            return;
        }
        let theme = self.current_theme();
        egui::Window::new(format!("Delete {}", self.server_draft.name))
            .collapsible(false)
            .resizable(false)
            .default_width(380.0)
            .show(ctx, |ui| {
                egui::Frame::new()
                    .fill(theme.semantic_err.gamma_multiply(0.12))
                    .stroke(Stroke::new(1.0, theme.semantic_err.gamma_multiply(0.65)))
                    .corner_radius(CornerRadius::same(6))
                    .inner_margin(Margin::same(10))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            paint_icon_ui(ui, IconKind::Warning, 15.0, theme.semantic_err);
                            ui.label(
                                RichText::new("Remove this saved server connection?")
                                    .strong()
                                    .color(theme.text_primary),
                            );
                        });
                    });
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.add(danger_button(&theme, "Yes, Remove")).clicked() {
                            let id = self.server_draft.id.clone();
                            let _ = self.store.remove_server(&id);
                            self.confirm_delete_server = false;
                        }
                        if ui.add(subtle_button(&theme, "Cancel")).clicked() {
                            self.confirm_delete_server = false;
                        }
                    });
                });
            });
    }

    /// Renders the Save Query, Open Query, Export Result, and Destructive Query Warning modals.
    pub(crate) fn render_file_and_safety_modals(&mut self, ctx: &egui::Context) {
        let theme = self.current_theme();

        if self.show_save_query_modal {
            egui::Window::new("Save Query (.sql)")
                .collapsible(false)
                .resizable(false)
                .default_width(420.0)
                .show(ctx, |ui| {
                    let mut submit = false;
                    modal_section(ui, &theme, IconKind::Save, "Destination File", |ui| {
                        let resp = ui.add(
                            egui::TextEdit::singleline(&mut self.file_dialog_path)
                                .hint_text("/path/to/query.sql")
                                .desired_width(ui.available_width()),
                        );
                        if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                            submit = true;
                        }
                    });
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if icon_primary_button(ui, &theme, IconKind::Save, "Save").clicked()
                                || submit
                            {
                                if let Some(q) = self.store.current_query().cloned() {
                                    if let Ok(saved) =
                                        self.store.db.save_query(&self.file_dialog_path, &q.query)
                                    {
                                        if let Some(qm) = self.store.current_query_mut() {
                                            qm.name = saved.name;
                                            qm.filename = Some(saved.filename);
                                        }
                                        self.store.status = "Query saved successfully".to_string();
                                    }
                                }
                                self.show_save_query_modal = false;
                            }
                            if ui.add(subtle_button(&theme, "Cancel")).clicked() {
                                self.show_save_query_modal = false;
                            }
                        });
                    });
                });
        }

        if self.show_open_query_modal {
            egui::Window::new("Open Query (.sql)")
                .collapsible(false)
                .resizable(false)
                .default_width(420.0)
                .show(ctx, |ui| {
                    let mut submit = false;
                    modal_section(ui, &theme, IconKind::FolderOpen, "Source File", |ui| {
                        let resp = ui.add(
                            egui::TextEdit::singleline(&mut self.file_dialog_path)
                                .hint_text("/path/to/query.sql")
                                .desired_width(ui.available_width()),
                        );
                        if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                            submit = true;
                        }
                    });
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if icon_primary_button(ui, &theme, IconKind::FolderOpen, "Open")
                                .clicked()
                                || submit
                            {
                                if let Ok(opened) = self.store.db.open_query(&self.file_dialog_path)
                                {
                                    if let Some(qm) = self.store.current_query_mut() {
                                        qm.name = opened.name;
                                        qm.query = opened.query;
                                        qm.filename = Some(opened.filename);
                                    }
                                }
                                self.show_open_query_modal = false;
                            }
                            if ui.add(subtle_button(&theme, "Cancel")).clicked() {
                                self.show_open_query_modal = false;
                            }
                        });
                    });
                });
        }

        if let Some((res_idx, ref exp_type)) = self.show_export_file_modal.clone() {
            egui::Window::new(format!("Export Result as {exp_type}"))
                .collapsible(false)
                .resizable(false)
                .default_width(420.0)
                .show(ctx, |ui| {
                    let mut submit = false;
                    modal_section(ui, &theme, IconKind::Download, "Export Path", |ui| {
                        let resp = ui.add(
                            egui::TextEdit::singleline(&mut self.file_dialog_path)
                                .hint_text("/path/to/export")
                                .desired_width(ui.available_width()),
                        );
                        if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                            submit = true;
                        }
                    });
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if icon_primary_button(ui, &theme, IconKind::Download, "Export")
                                .clicked()
                                || submit
                            {
                                let delim = self
                                    .store
                                    .config
                                    .data
                                    .as_ref()
                                    .map(|c| c.csv_delimiter.clone())
                                    .unwrap_or_else(|| ",".to_string());
                                if let Some(rows) = self
                                    .store
                                    .current_query()
                                    .and_then(|q| q.results.as_ref())
                                    .and_then(|r| r.get(res_idx))
                                    .map(|r| r.rows.clone())
                                {
                                    let _ = self.store.db.export_query_result_to_file(
                                        &rows,
                                        exp_type,
                                        Some(&delim),
                                        &self.file_dialog_path,
                                    );
                                    if let Some(qm) = self.store.current_query_mut() {
                                        qm.saved = Some(true);
                                    }
                                }
                                self.show_export_file_modal = None;
                            }
                            if ui.add(subtle_button(&theme, "Cancel")).clicked() {
                                self.show_export_file_modal = None;
                            }
                        });
                    });
                });
        }

        if let Some((sql, report)) = self.pending_safety_confirm.clone() {
            egui::Window::new("Destructive Query Warning")
                .collapsible(false)
                .resizable(false)
                .default_width(440.0)
                .show(ctx, |ui| {
                    egui::Frame::new()
                        .fill(theme.semantic_err.gamma_multiply(0.12))
                        .stroke(Stroke::new(1.0, theme.semantic_err.gamma_multiply(0.65)))
                        .corner_radius(CornerRadius::same(6))
                        .inner_margin(Margin::same(10))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                paint_icon_ui(ui, IconKind::Warning, 15.0, theme.semantic_err);
                                ui.label(
                                    RichText::new(
                                        "This query contains potentially destructive operations:",
                                    )
                                    .strong()
                                    .color(theme.semantic_err),
                                );
                            });
                            ui.add_space(4.0);
                            for f in &report.findings {
                                ui.label(format!("• {}", f.reason));
                            }
                        });
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.add(danger_button(&theme, "Execute Anyway")).clicked() {
                                self.pending_safety_confirm = None;
                                self.execute_sql_confirmed(&sql);
                            }
                            if ui.add(subtle_button(&theme, "Cancel")).clicked() {
                                self.pending_safety_confirm = None;
                            }
                        });
                    });
                });
        }

        if let Some(state) = self.update_modal_state.clone() {
            egui::Window::new("Software Update")
                .collapsible(false)
                .resizable(false)
                .default_width(400.0)
                .show(ctx, |ui| {
                    modal_section(
                        ui,
                        &theme,
                        IconKind::Refresh,
                        "Version Check",
                        |ui| match &state {
                            Ok(UpdateCheckResult::UpToDate { current }) => {
                                ui.horizontal(|ui| {
                                    paint_icon_ui(ui, IconKind::Check, 14.0, theme.semantic_ok);
                                    ui.colored_label(
                                        theme.semantic_ok,
                                        format!("Rust-SQL v{current} is already up to date."),
                                    );
                                });
                            }
                            Ok(UpdateCheckResult::UpdateAvailable(info)) => {
                                ui.label(
                                    RichText::new(format!(
                                        "New version available: {}",
                                        info.tag_name
                                    ))
                                    .strong(),
                                );
                                ui.hyperlink_to("Release Notes", &info.html_url);
                                ui.add_space(4.0);
                                if let Some(ref msg) = self.update_apply_message {
                                    match msg {
                                        Ok(ok) => ui.colored_label(theme.semantic_ok, ok),
                                        Err(err) => ui.colored_label(theme.semantic_err, err),
                                    };
                                } else if icon_primary_button(
                                    ui,
                                    &theme,
                                    IconKind::Download,
                                    "Download & Install Update",
                                )
                                .clicked()
                                {
                                    let rt = self.rt.clone();
                                    self.update_apply_message =
                                        Some(rt.block_on(perform_self_update(info)));
                                }
                            }
                            Err(err) => {
                                ui.colored_label(
                                    theme.semantic_err,
                                    format!("Update check failed: {err}"),
                                );
                            }
                        },
                    );
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.add(subtle_button(&theme, "Close")).clicked() {
                                self.update_modal_state = None;
                                self.update_apply_message = None;
                            }
                        });
                    });
                });
        }
    }
}
