//! File Save/Open/Export modals, Destructive Query Safety warning modal, and Software Update modal.

use crate::core::updater::{perform_self_update, UpdateCheckResult};
use crate::gui::RustSqlGuiApp;
use eframe::egui::{self, Color32};

impl RustSqlGuiApp {
    /// Renders the server deletion confirmation dialog when `self.confirm_delete_server` is true.
    pub(crate) fn render_confirm_delete_modal(&mut self, ctx: &egui::Context) {
        if !self.confirm_delete_server {
            return;
        }
        egui::Window::new(format!("Delete {}", self.server_draft.name))
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| {
                ui.label("Are you sure you want to remove this server connection?");
                ui.horizontal(|ui| {
                    if ui.button("Yes, Remove").clicked() {
                        let id = self.server_draft.id.clone();
                        let _ = self.store.remove_server(&id);
                        self.confirm_delete_server = false;
                    }
                    if ui.button("No").clicked() {
                        self.confirm_delete_server = false;
                    }
                });
            });
    }

    /// Renders the Save Query, Open Query, Export Result, and Destructive Query Warning modals.
    pub(crate) fn render_file_and_safety_modals(&mut self, ctx: &egui::Context) {
        if self.show_save_query_modal {
            egui::Window::new("Save Query (.sql)")
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
                    ui.label("File path:");
                    ui.text_edit_singleline(&mut self.file_dialog_path);
                    ui.horizontal(|ui| {
                        if ui.button("Save").clicked() {
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
                        if ui.button("Cancel").clicked() {
                            self.show_save_query_modal = false;
                        }
                    });
                });
        }

        if self.show_open_query_modal {
            egui::Window::new("Open Query (.sql)")
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
                    ui.label("File path:");
                    ui.text_edit_singleline(&mut self.file_dialog_path);
                    ui.horizontal(|ui| {
                        if ui.button("Open").clicked() {
                            if let Ok(opened) = self.store.db.open_query(&self.file_dialog_path) {
                                if let Some(qm) = self.store.current_query_mut() {
                                    qm.name = opened.name;
                                    qm.query = opened.query;
                                    qm.filename = Some(opened.filename);
                                }
                            }
                            self.show_open_query_modal = false;
                        }
                        if ui.button("Cancel").clicked() {
                            self.show_open_query_modal = false;
                        }
                    });
                });
        }

        if let Some((res_idx, ref exp_type)) = self.show_export_file_modal.clone() {
            egui::Window::new(format!("Export Result as {exp_type}"))
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
                    ui.label("File path:");
                    ui.text_edit_singleline(&mut self.file_dialog_path);
                    ui.horizontal(|ui| {
                        if ui.button("Export").clicked() {
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
                        if ui.button("Cancel").clicked() {
                            self.show_export_file_modal = None;
                        }
                    });
                });
        }

        if let Some((sql, report)) = self.pending_safety_confirm.clone() {
            egui::Window::new("⚠ Destructive Query Warning")
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
                    ui.colored_label(
                        Color32::LIGHT_RED,
                        "This query contains potentially destructive operations:",
                    );
                    for f in &report.findings {
                        ui.label(format!("• {}", f.reason));
                    }
                    ui.separator();
                    ui.horizontal(|ui| {
                        if ui.button("Execute Anyway").clicked() {
                            self.pending_safety_confirm = None;
                            self.execute_sql_confirmed(&sql);
                        }
                        if ui.button("Cancel").clicked() {
                            self.pending_safety_confirm = None;
                        }
                    });
                });
        }

        if let Some(state) = self.update_modal_state.clone() {
            egui::Window::new("Software Update")
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
                    match &state {
                        Ok(UpdateCheckResult::UpToDate { current }) => {
                            ui.colored_label(
                                Color32::GREEN,
                                format!("Rust-SQL v{current} is already up to date."),
                            );
                        }
                        Ok(UpdateCheckResult::UpdateAvailable(info)) => {
                            ui.label(format!("New version available: {}", info.tag_name));
                            ui.hyperlink_to("Release Notes", &info.html_url);
                            if let Some(ref msg) = self.update_apply_message {
                                match msg {
                                    Ok(ok) => ui.colored_label(Color32::GREEN, ok),
                                    Err(err) => ui.colored_label(Color32::LIGHT_RED, err),
                                };
                            } else if ui.button("Download & Install Update").clicked() {
                                let rt = self.rt.clone();
                                self.update_apply_message =
                                    Some(rt.block_on(perform_self_update(info)));
                            }
                        }
                        Err(err) => {
                            ui.colored_label(
                                Color32::LIGHT_RED,
                                format!("Update check failed: {err}"),
                            );
                        }
                    }
                    ui.separator();
                    if ui.button("Close").clicked() {
                        self.update_modal_state = None;
                        self.update_apply_message = None;
                    }
                });
        }
    }
}
