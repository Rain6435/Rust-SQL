//! Query result table rendering, sorting, filtering, column statistics trigger, and export buttons.

use crate::common::types::{ConnectionMode, EMULATED_BANNER};
use crate::common::utils::value_to_string;
use crate::core::export::write_clipboard_text;
use crate::core::result_ops::{
    compute_column_stats, filter_result_refs, sort_result_refs, SortDirection,
};
use crate::gui::icons::{icon_subtle_button, paint_icon_ui, IconKind};
use crate::gui::theme::{badge_pill, card_frame, subtle_button, AppTheme};
use crate::gui::RustSqlGuiApp;
use crate::state::QueryTab;
use eframe::egui::{self, Color32, CornerRadius, Margin, RichText, Stroke};
use egui_extras::{Column, TableBuilder};
use serde_json::Value;

/// Draws the red "EMULATED" warning shown above every result set of an emulated session.
pub(crate) fn render_emulated_banner(ui: &mut egui::Ui) {
    egui::Frame::new()
        .fill(Color32::from_rgb(185, 28, 28))
        .corner_radius(CornerRadius::same(4))
        .inner_margin(Margin::symmetric(8, 3))
        .show(ui, |ui| {
            ui.label(
                RichText::new(EMULATED_BANNER)
                    .small()
                    .strong()
                    .color(Color32::WHITE),
            );
        });
}

impl RustSqlGuiApp {
    /// Renders execution status, errors, or result sets for `query_tab`.
    pub(crate) fn render_query_results(&mut self, ui: &mut egui::Ui, query_tab: &QueryTab) {
        let theme: AppTheme = self.current_theme();

        if self.store.connections.mode == Some(ConnectionMode::Emulated) {
            render_emulated_banner(ui);
            ui.add_space(4.0);
        }
        if query_tab.is_executing {
            ui.centered_and_justified(|ui| {
                let timeout = self.store.statement_timeout_label().unwrap_or_default();
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(
                        RichText::new(format!("Executing query... {timeout}"))
                            .size(15.0)
                            .color(theme.accent_primary),
                    );
                });
            });
            return;
        }

        if let Some(ref err) = query_tab.error {
            egui::Frame::new()
                .fill(theme.semantic_err.gamma_multiply(0.14))
                .stroke(Stroke::new(1.0, theme.semantic_err.gamma_multiply(0.75)))
                .corner_radius(CornerRadius::same(6))
                .inner_margin(Margin::same(12))
                .show(ui, |ui| {
                    ui.label(
                        RichText::new("Query Error")
                            .strong()
                            .color(theme.semantic_err),
                    );
                    ui.add_space(4.0);
                    ui.monospace(RichText::new(err).color(theme.text_primary));
                });
            return;
        }

        let Some(ref results) = query_tab.results else {
            ui.add_space(16.0);
            ui.centered_and_justified(|ui| {
                card_frame(&theme).show(ui, |ui| {
                    ui.vertical_centered(|ui| {
                        ui.label(
                            RichText::new("Ready to Query")
                                .strong()
                                .size(15.0)
                                .color(theme.text_primary),
                        );
                        ui.add_space(4.0);
                        ui.label(
                            RichText::new(
                                "Press Ctrl+Enter to run  ·  Ctrl+Space for autocomplete  ·  Click a table in the explorer to preview rows",
                            )
                            .small()
                            .color(theme.text_muted),
                        );
                        ui.add_space(8.0);
                        ui.horizontal(|ui| {
                            if icon_subtle_button(ui, &theme, IconKind::Bookmark, "Open Bookmarks")
                                .clicked()
                            {
                                self.show_bookmarks_modal = true;
                            }
                            if icon_subtle_button(
                                ui,
                                &theme,
                                IconKind::FolderOpen,
                                "Open .sql File",
                            )
                            .clicked()
                            {
                                self.show_open_query_modal = true;
                            }
                        });
                    });
                });
            });
            return;
        };

        let csv_delim = self
            .store
            .config
            .data
            .as_ref()
            .map(|c| c.csv_delimiter.clone())
            .unwrap_or_else(|| ",".to_string());

        egui::ScrollArea::vertical().show(ui, |ui| {
            for (idx, res) in results.iter().enumerate() {
                if results.len() > 1 {
                    ui.label(
                        RichText::new(format!("Result {}", idx + 1))
                            .strong()
                            .size(14.5)
                            .color(theme.text_primary),
                    );
                }

                let secs = query_tab.execution_time.unwrap_or(0) as f64 / 1000.0;
                let is_select_or_explain = matches!(
                    res.command.as_str(),
                    "SELECT" | "EXPLAIN" | "PRAGMA" | "SHOW" | "UNKNOWN"
                );

                if !is_select_or_explain {
                    egui::Frame::new()
                        .fill(theme.semantic_ok.gamma_multiply(0.12))
                        .stroke(Stroke::new(1.0, theme.semantic_ok.gamma_multiply(0.55)))
                        .corner_radius(CornerRadius::same(6))
                        .inner_margin(Margin::symmetric(12, 8))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                badge_pill(ui, "OK", theme.semantic_ok);
                                ui.label(
                                    RichText::new(format!(
                                        "Query executed successfully. Affected rows: {}. Took {:.3}s.",
                                        res.affected_rows.unwrap_or(0),
                                        secs
                                    ))
                                    .color(theme.text_primary),
                                );
                            });
                        });
                    continue;
                }

                if res.command == "EXPLAIN" {
                    let col = res
                        .fields
                        .first()
                        .map(|f| f.name.as_str())
                        .unwrap_or("QUERY PLAN");
                    let plan: Vec<String> = res
                        .rows
                        .iter()
                        .filter_map(|r| r.get(col).map(value_to_string))
                        .collect();
                    egui::Frame::new()
                        .fill(theme.bg_surface)
                        .stroke(Stroke::new(1.0, theme.border_hairline))
                        .corner_radius(CornerRadius::same(6))
                        .inner_margin(Margin::same(10))
                        .show(ui, |ui| {
                            ui.monospace(plan.join("\n"));
                        });
                    continue;
                }

                if res.fields.is_empty() && res.rows.is_empty() {
                    ui.label(
                        RichText::new(format!("Query executed successfully. Took {:.3}s.", secs))
                            .color(theme.semantic_ok),
                    );
                    continue;
                }

                let mut display_rows =
                    filter_result_refs(&res.rows, &self.result_filter_text, None);
                if let Some((ref s_col, dir)) = self.result_sort_col {
                    display_rows = sort_result_refs(display_rows, s_col, dir);
                }

                let row_count = res.row_count.unwrap_or(res.rows.len());
                let is_filtered = !self.result_filter_text.trim().is_empty();
                let count_badge = if is_filtered {
                    format!("{}/{} rows", display_rows.len(), row_count)
                } else {
                    format!("{row_count} rows")
                };

                egui::Frame::new()
                    .fill(theme.bg_surface)
                    .stroke(Stroke::new(1.0, theme.border_hairline))
                    .corner_radius(CornerRadius::same(6))
                    .inner_margin(Margin::symmetric(10, 6))
                    .show(ui, |ui| {
                        ui.horizontal_wrapped(|ui| {
                            badge_pill(ui, &count_badge, theme.accent_secondary);
                            if let Some(page) = query_tab.page.as_ref().filter(|_| idx == 0) {
                                ui.label(
                                    RichText::new(format!("page {}", page.page_size))
                                        .small()
                                        .color(theme.text_muted),
                                );
                                if page.next.is_some() {
                                    if ui
                                        .add(subtle_button(&theme, "Load more"))
                                        .clicked()
                                    {
                                        let rt = self.rt.clone();
                                        if let Err(e) = rt.block_on(self.store.fetch_more_rows()) {
                                            self.store.status = e;
                                        }
                                    }
                                } else {
                                    ui.label(
                                        RichText::new("all loaded")
                                            .small()
                                            .color(theme.text_muted),
                                    );
                                }
                            }
                            ui.separator();
                            ui.label(
                                RichText::new(format!("{:.3}s", secs))
                                    .monospace()
                                    .small()
                                    .color(theme.accent_primary),
                            );
                            if row_count > 0 {
                                ui.separator();
                                paint_icon_ui(ui, IconKind::Search, 12.0, theme.text_muted);
                                ui.add(
                                    egui::TextEdit::singleline(&mut self.result_filter_text)
                                        .hint_text("Filter rows...")
                                        .desired_width(130.0),
                                );
                                if is_filtered
                                    && ui.small_button("×").on_hover_text("Clear filter").clicked()
                                {
                                    self.result_filter_text.clear();
                                }
                                if ui
                                    .horizontal(|ui| {
                                        ui.spacing_mut().item_spacing.x = 3.0;
                                        paint_icon_ui(
                                            ui,
                                            IconKind::Chart,
                                            12.0,
                                            theme.accent_secondary,
                                        );
                                        ui.small_button("Stats")
                                    })
                                    .inner
                                    .clicked()
                                {
                                    let col_names: Vec<String> =
                                        res.fields.iter().map(|f| f.name.clone()).collect();
                                    self.stats_modal_data =
                                        Some(compute_column_stats(&res.rows, &col_names));
                                }
                                ui.separator();
                                paint_icon_ui(ui, IconKind::Download, 12.0, theme.text_muted);
                                ui.label(RichText::new("Save:").small().color(theme.text_muted));
                                for fmt in ["CSV", "JSON", "MARKDOWN", "SQL", "JSONL"] {
                                    if ui.small_button(fmt).clicked() {
                                        self.show_export_file_modal =
                                            Some((idx, fmt.to_string()));
                                    }
                                }
                                ui.separator();
                                paint_icon_ui(ui, IconKind::Copy, 12.0, theme.text_muted);
                                ui.label(RichText::new("Copy:").small().color(theme.text_muted));
                                for fmt in ["CSV", "JSON", "MARKDOWN", "SQL", "JSONL"] {
                                    if ui.small_button(fmt).clicked() {
                                        let _ = self.store.db.export_query_result_to_clipboard(
                                            &res.rows,
                                            fmt,
                                            Some(&csv_delim),
                                        );
                                    }
                                }
                            }
                        });
                    });

                ui.add_space(4.0);

                if display_rows.is_empty() {
                    ui.label(RichText::new("No matching rows").italics().color(theme.text_muted));
                    continue;
                }

                let cols: Vec<String> = res.fields.iter().map(|f| f.name.clone()).collect();
                egui::Frame::new()
                    .fill(theme.bg_surface)
                    .stroke(Stroke::new(1.0, theme.border_hairline))
                    .corner_radius(CornerRadius::same(6))
                    .inner_margin(Margin::same(6))
                    .show(ui, |ui| {
                        let mut builder = TableBuilder::new(ui)
                            .striped(true)
                            .resizable(true)
                            .cell_layout(egui::Layout::left_to_right(egui::Align::Center));

                        for _ in &cols {
                            builder =
                                builder.column(Column::initial(130.0).at_least(40.0).clip(true));
                        }

                        builder
                            .header(24.0, |mut header| {
                                for col_name in &cols {
                                    header.col(|ui| {
                                        let (sort_indicator, is_sorted) =
                                            match &self.result_sort_col {
                                                Some((c, SortDirection::Ascending))
                                                    if c == col_name =>
                                                {
                                                    (" ▲", true)
                                                }
                                                Some((c, SortDirection::Descending))
                                                    if c == col_name =>
                                                {
                                                    (" ▼", true)
                                                }
                                                _ => ("", false),
                                            };
                                        let col_color = if is_sorted {
                                            theme.accent_primary
                                        } else {
                                            theme.text_primary
                                        };
                                        if ui
                                            .add(
                                                egui::Button::new(
                                                    RichText::new(format!(
                                                        "{col_name}{sort_indicator}"
                                                    ))
                                                    .strong()
                                                    .color(col_color),
                                                )
                                                .frame(false),
                                            )
                                            .clicked()
                                        {
                                            let next_dir = match &self.result_sort_col {
                                                Some((c, SortDirection::Ascending))
                                                    if c == col_name =>
                                                {
                                                    SortDirection::Descending
                                                }
                                                _ => SortDirection::Ascending,
                                            };
                                            self.result_sort_col =
                                                Some((col_name.clone(), next_dir));
                                        }
                                    });
                                }
                            })
                            .body(|body| {
                                body.rows(22.0, display_rows.len(), |mut row| {
                                    let row_data = display_rows[row.index()];
                                    for col_name in &cols {
                                        row.col(|ui| {
                                            let val =
                                                row_data.get(col_name).unwrap_or(&Value::Null);
                                            if val.is_null() {
                                                ui.label(
                                                    RichText::new("NULL")
                                                        .monospace()
                                                        .small()
                                                        .color(theme.text_muted)
                                                        .italics(),
                                                );
                                            } else {
                                                let text = value_to_string(val);
                                                ui.label(
                                                    RichText::new(&text).color(theme.text_primary),
                                                )
                                                .context_menu(|ui| {
                                                    if ui.button("Copy Cell").clicked() {
                                                        write_clipboard_text(&text);
                                                        self.store.status =
                                                            "Cell value copied to clipboard"
                                                                .to_string();
                                                        ui.close();
                                                    }
                                                    if ui.button("Open Preview").clicked() {
                                                        self.preview_cell_value =
                                                            Some(val.clone());
                                                        ui.close();
                                                    }
                                                });
                                            }
                                        });
                                    }
                                });
                            });
                    });
            }
        });
    }
}
