//! SQL Bookmarks & Execution History modal, Column Statistics modal, and Mermaid ERD modal.

use crate::core::export::write_clipboard_text;
use crate::gui::RustSqlGuiApp;
use eframe::egui;

impl RustSqlGuiApp {
    /// Renders the SQL Bookmarks & Execution History modal, Column Statistics modal, and Mermaid ERD modal.
    pub(crate) fn render_bookmarks_stats_erd_modals(&mut self, ctx: &egui::Context) {
        if self.show_bookmarks_modal {
            egui::Window::new("🔖 SQL Bookmarks & Execution History")
                .collapsible(false)
                .resizable(true)
                .default_width(580.0)
                .show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        ui.label("Save Current Query as Bookmark:");
                        ui.add(
                            egui::TextEdit::singleline(&mut self.bookmark_title_input)
                                .hint_text("Title...")
                                .desired_width(140.0),
                        );
                        ui.add(
                            egui::TextEdit::singleline(&mut self.bookmark_tags_input)
                                .hint_text("Tags (comma-separated)...")
                                .desired_width(140.0),
                        );
                        if ui.button("➕ Save Bookmark").clicked() {
                            if let Some(sql) = self.store.current_query().map(|q| q.query.clone()) {
                                let tags: Vec<String> = self
                                    .bookmark_tags_input
                                    .split(',')
                                    .map(|s| s.trim().to_string())
                                    .filter(|s| !s.is_empty())
                                    .collect();
                                let client = self
                                    .store
                                    .connections
                                    .server
                                    .as_ref()
                                    .map(|s| s.client.clone());
                                self.store.bookmark_manager.add_bookmark(
                                    &self.bookmark_title_input,
                                    &sql,
                                    tags,
                                    client,
                                );
                                self.bookmark_title_input.clear();
                                self.bookmark_tags_input.clear();
                            }
                        }
                    });
                    ui.separator();
                    ui.horizontal(|ui| {
                        ui.label("🔍 Search:");
                        ui.text_edit_singleline(&mut self.bookmark_search_input);
                    });

                    ui.collapsing("Saved Bookmarks", |ui| {
                        let bms: Vec<_> = self
                            .store
                            .bookmark_manager
                            .search_bookmarks(&self.bookmark_search_input)
                            .into_iter()
                            .cloned()
                            .collect();
                        for bm in bms {
                            ui.horizontal(|ui| {
                                ui.strong(&bm.title);
                                ui.monospace(&bm.sql);
                                if ui.small_button("Insert").clicked() {
                                    self.store.append_query(&bm.sql);
                                }
                                if ui.small_button("🗑").clicked() {
                                    self.store.bookmark_manager.remove_bookmark(&bm.id);
                                }
                            });
                        }
                    });

                    ui.collapsing("Execution History Audit Log", |ui| {
                        let hist: Vec<_> = self
                            .store
                            .bookmark_manager
                            .search_history(&self.bookmark_search_input)
                            .into_iter()
                            .cloned()
                            .collect();
                        for h in hist {
                            ui.horizontal(|ui| {
                                ui.label(format!(
                                    "[{}] {}ms ({} rows)",
                                    h.database, h.duration_ms, h.row_count
                                ));
                                ui.monospace(h.display_sql.as_deref().unwrap_or(&h.sql));
                                if ui.small_button("Replay").clicked() {
                                    self.store.update_query_if_needed(&h.sql, None, None);
                                }
                            });
                        }
                    });

                    ui.separator();
                    if ui.button("Close").clicked() {
                        self.show_bookmarks_modal = false;
                    }
                });
        }

        if let Some(ref stats) = self.stats_modal_data.clone() {
            egui::Window::new("📊 Column Summary Statistics")
                .collapsible(false)
                .resizable(true)
                .default_width(600.0)
                .show(ctx, |ui| {
                    egui::Grid::new("col_stats_grid")
                        .striped(true)
                        .show(ui, |ui| {
                            for h in ["Column", "Total", "Nulls", "Distinct", "Min", "Max", "Mean"]
                            {
                                ui.strong(h);
                            }
                            ui.end_row();
                            for s in stats {
                                ui.label(&s.column_name);
                                ui.label(s.total_rows.to_string());
                                ui.label(s.null_count.to_string());
                                ui.label(s.distinct_count.to_string());
                                ui.label(s.min_value.as_deref().unwrap_or("-"));
                                ui.label(s.max_value.as_deref().unwrap_or("-"));
                                ui.label(
                                    s.mean
                                        .map(|m| format!("{m:.2}"))
                                        .unwrap_or_else(|| "-".to_string()),
                                );
                                ui.end_row();
                            }
                        });
                    ui.separator();
                    if ui.button("Close").clicked() {
                        self.stats_modal_data = None;
                    }
                });
        }

        if let Some(ref erd) = self.erd_modal_diagram.clone() {
            egui::Window::new("🗺 Schema ER Diagram (Mermaid)")
                .collapsible(false)
                .resizable(true)
                .default_width(540.0)
                .show(ctx, |ui| {
                    egui::ScrollArea::vertical()
                        .max_height(340.0)
                        .show(ui, |ui| {
                            ui.monospace(erd);
                        });
                    ui.separator();
                    ui.horizontal(|ui| {
                        if ui.button("📋 Copy Mermaid").clicked() {
                            write_clipboard_text(erd);
                        }
                        if ui.button("Close").clicked() {
                            self.erd_modal_diagram = None;
                        }
                    });
                });
        }
    }
}
