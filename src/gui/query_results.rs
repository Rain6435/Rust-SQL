//! Query result table rendering, sorting, filtering, column statistics trigger, and export buttons.

use crate::common::types::{ConnectionMode, EMULATED_BANNER};
use crate::common::utils::value_to_string;
use crate::core::result_ops::{
    compute_column_stats, filter_result_rows, sort_result_rows, SortDirection,
};
use crate::gui::RustSqlGuiApp;
use crate::state::QueryTab;
use eframe::egui::{self, Color32, RichText, Stroke};
use egui_extras::{Column, TableBuilder};
use serde_json::Value;

/// Draws the red "EMULATED" warning shown above every result set of an emulated session.
pub(crate) fn render_emulated_banner(ui: &mut egui::Ui) {
    egui::Frame::group(ui.style())
        .fill(Color32::from_rgb(185, 28, 28))
        .show(ui, |ui| {
            ui.label(
                RichText::new(EMULATED_BANNER)
                    .strong()
                    .color(Color32::WHITE),
            );
        });
}

impl RustSqlGuiApp {
    /// Renders execution status, errors, or result sets for `query_tab`.
    pub(crate) fn render_query_results(&mut self, ui: &mut egui::Ui, query_tab: &QueryTab) {
        if self.store.connections.mode == Some(ConnectionMode::Emulated) {
            render_emulated_banner(ui);
        }
        if query_tab.is_executing {
            ui.centered_and_justified(|ui| {
                let timeout = self.store.statement_timeout_label().unwrap_or_default();
                ui.label(RichText::new(format!("⏳ Executing query... {timeout}")).size(16.0));
            });
            return;
        }

        if let Some(ref err) = query_tab.error {
            egui::Frame::group(ui.style())
                .fill(Color32::from_rgba_unmultiplied(220, 38, 38, 35))
                .stroke(Stroke::new(1.0_f32, Color32::from_rgb(220, 38, 38)))
                .show(ui, |ui| {
                    ui.label(RichText::new("Error").strong().color(Color32::LIGHT_RED));
                    ui.monospace(err);
                });
            return;
        }

        let Some(ref results) = query_tab.results else {
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
                    ui.heading(format!("Query {}", idx + 1));
                }

                let secs = query_tab.execution_time.unwrap_or(0) as f64 / 1000.0;
                let is_select_or_explain = matches!(
                    res.command.as_str(),
                    "SELECT" | "EXPLAIN" | "PRAGMA" | "SHOW" | "UNKNOWN"
                );

                if !is_select_or_explain {
                    ui.label(
                        RichText::new(format!(
                            "Query executed successfully. Affected rows: {}. Took {:.3}s.",
                            res.affected_rows.unwrap_or(0),
                            secs
                        ))
                        .color(Color32::from_rgb(34, 197, 94)),
                    );
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
                    ui.monospace(plan.join("\n"));
                    continue;
                }

                if res.fields.is_empty() && res.rows.is_empty() {
                    ui.label(format!("Query executed successfully. Took {:.3}s.", secs));
                    continue;
                }

                let row_count = res.row_count.unwrap_or(res.rows.len());
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new(format!("Rows: {row_count}")).strong());
                    ui.separator();
                    ui.label(format!("{:.3}s", secs));
                    if row_count > 0 {
                        ui.separator();
                        ui.label("🔍 Filter:");
                        ui.add(
                            egui::TextEdit::singleline(&mut self.result_filter_text)
                                .hint_text("Filter rows...")
                                .desired_width(130.0),
                        );
                        if ui.small_button("📊 Stats").clicked() {
                            let col_names: Vec<String> =
                                res.fields.iter().map(|f| f.name.clone()).collect();
                            self.stats_modal_data =
                                Some(compute_column_stats(&res.rows, &col_names));
                        }
                        ui.separator();
                        ui.label("Save as:");
                        for fmt in ["CSV", "JSON", "MARKDOWN", "SQL", "JSONL"] {
                            if ui.small_button(fmt).clicked() {
                                self.show_export_file_modal = Some((idx, fmt.to_string()));
                            }
                        }
                        ui.separator();
                        ui.label("Copy as:");
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

                let mut display_rows =
                    filter_result_rows(&res.rows, &self.result_filter_text, None);
                if let Some((ref s_col, dir)) = self.result_sort_col {
                    display_rows = sort_result_rows(&display_rows, s_col, dir);
                }

                if display_rows.is_empty() {
                    ui.label(RichText::new("No results found").italics().weak());
                    continue;
                }

                let cols: Vec<String> = res.fields.iter().map(|f| f.name.clone()).collect();
                let mut builder = TableBuilder::new(ui)
                    .striped(true)
                    .resizable(true)
                    .cell_layout(egui::Layout::left_to_right(egui::Align::Center));

                for _ in &cols {
                    builder = builder.column(Column::initial(130.0).at_least(40.0).clip(true));
                }

                builder
                    .header(22.0, |mut header| {
                        for col_name in &cols {
                            header.col(|ui| {
                                let sort_indicator = match &self.result_sort_col {
                                    Some((c, SortDirection::Ascending)) if c == col_name => " ▲",
                                    Some((c, SortDirection::Descending)) if c == col_name => " ▼",
                                    _ => "",
                                };
                                if ui
                                    .button(
                                        RichText::new(format!("{col_name}{sort_indicator}"))
                                            .strong(),
                                    )
                                    .clicked()
                                {
                                    let next_dir = match &self.result_sort_col {
                                        Some((c, SortDirection::Ascending)) if c == col_name => {
                                            SortDirection::Descending
                                        }
                                        _ => SortDirection::Ascending,
                                    };
                                    self.result_sort_col = Some((col_name.clone(), next_dir));
                                }
                            });
                        }
                    })
                    .body(|body| {
                        body.rows(20.0, display_rows.len(), |mut row| {
                            let row_data = &display_rows[row.index()];
                            for col_name in &cols {
                                row.col(|ui| {
                                    let val = row_data.get(col_name).unwrap_or(&Value::Null);
                                    if val.is_null() {
                                        ui.label(
                                            RichText::new("NULL")
                                                .small()
                                                .color(Color32::GRAY)
                                                .italics(),
                                        );
                                    } else {
                                        let text = value_to_string(val);
                                        ui.label(&text).context_menu(|ui| {
                                            if ui.button("Open Preview").clicked() {
                                                self.preview_cell_value = Some(val.clone());
                                                ui.close();
                                            }
                                        });
                                    }
                                });
                            }
                        });
                    });
            }
        });
    }
}
