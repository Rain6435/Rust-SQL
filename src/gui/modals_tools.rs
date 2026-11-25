//! SQL Bookmarks & Execution History modal, Column Statistics modal, and Mermaid ERD modal.

use crate::core::export::write_clipboard_text;
use crate::gui::theme::{badge_pill, primary_button, subtle_button};
use crate::gui::RustSqlGuiApp;
use eframe::egui::{self, CornerRadius, Margin, RichText, Stroke};

impl RustSqlGuiApp {
    /// Renders the SQL Bookmarks & Execution History modal, Column Statistics modal, and Mermaid ERD modal.
    pub(crate) fn render_bookmarks_stats_erd_modals(&mut self, ctx: &egui::Context) {
        let theme = self.current_theme();

        if self.show_bookmarks_modal {
            egui::Window::new("SQL Bookmarks & Execution History")
                .collapsible(false)
                .resizable(true)
                .default_width(600.0)
                .show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        ui.label("Save query:");
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
                        if ui.add(primary_button(&theme, "+ Save")).clicked() {
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
                    ui.add(
                        egui::TextEdit::singleline(&mut self.bookmark_search_input)
                            .hint_text("Search bookmarks and history...")
                            .desired_width(ui.available_width()),
                    );

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
                                ui.monospace(RichText::new(&bm.sql).color(theme.text_muted));
                                if ui.small_button("Insert").clicked() {
                                    self.store.append_query(&bm.sql);
                                }
                                if ui
                                    .small_button(RichText::new("Delete").color(theme.semantic_err))
                                    .clicked()
                                {
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
                                badge_pill(ui, &h.database, theme.accent_secondary);
                                ui.label(
                                    RichText::new(format!(
                                        "{}ms · {} rows",
                                        h.duration_ms, h.row_count
                                    ))
                                    .small()
                                    .color(theme.text_muted),
                                );
                                ui.monospace(h.display_sql.as_deref().unwrap_or(&h.sql));
                                if ui.small_button("Replay").clicked() {
                                    self.store.update_query_if_needed(&h.sql, None, None);
                                }
                            });
                        }
                    });

                    ui.separator();
                    if ui.add(subtle_button(&theme, "Close")).clicked() {
                        self.show_bookmarks_modal = false;
                    }
                });
        }

        if let Some(ref stats) = self.stats_modal_data.clone() {
            egui::Window::new("Column Summary Statistics")
                .collapsible(false)
                .resizable(true)
                .default_width(600.0)
                .show(ctx, |ui| {
                    let rows = stats.first().map(|s| s.total_rows).unwrap_or(0);
                    let partial = self
                        .store
                        .current_query()
                        .and_then(|q| q.page.as_ref())
                        .is_some_and(|p| p.next.is_some());
                    ui.label(
                        RichText::new(if partial {
                            format!("Computed over the {rows} rows loaded so far, not the full result. Load more rows to include them.")
                        } else {
                            format!("Computed over all {rows} rows of the result.")
                        })
                        .small()
                        .color(theme.text_muted),
                    );
                    ui.add_space(4.0);
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
                    if ui.add(subtle_button(&theme, "Close")).clicked() {
                        self.stats_modal_data = None;
                    }
                });
        }

        if let Some(ref erd) = self.erd_modal_diagram.clone() {
            egui::Window::new("Schema ER Diagram (Mermaid)")
                .collapsible(false)
                .resizable(true)
                .default_width(540.0)
                .show(ctx, |ui| {
                    egui::Frame::new()
                        .fill(theme.bg_canvas)
                        .stroke(Stroke::new(1.0, theme.border_hairline))
                        .corner_radius(CornerRadius::same(6))
                        .inner_margin(Margin::same(8))
                        .show(ui, |ui| {
                            egui::ScrollArea::vertical()
                                .max_height(340.0)
                                .show(ui, |ui| {
                                    ui.monospace(erd);
                                });
                        });
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        if ui.add(primary_button(&theme, "Copy Mermaid")).clicked() {
                            write_clipboard_text(erd);
                        }
                        if ui.add(subtle_button(&theme, "Close")).clicked() {
                            self.erd_modal_diagram = None;
                        }
                    });
                });
        }
    }
}
