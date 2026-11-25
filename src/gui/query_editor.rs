//! Multi-tab SQL query bar, code editor, inline parameter binding inputs, and action toolbar.

use crate::core::params::{extract_query_parameters, ParamKind};
use crate::gui::theme::{badge_pill, primary_button, subtle_button};
use crate::gui::RustSqlGuiApp;
use eframe::egui::{self, CornerRadius, FontId, Key, Margin, RichText, Stroke};

impl RustSqlGuiApp {
    /// Renders the horizontal tab bar for open [`crate::state::QueryTab`] instances.
    pub(crate) fn render_query_tabs_bar(&mut self, ui: &mut egui::Ui) {
        let theme = self.current_theme();
        let query_ids = self.store.queries.query_ids.clone();
        let current_id = self.store.queries.current_query_id;

        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(6.0, 4.0);
            for qid in query_ids {
                let Some(q_name) = self
                    .store
                    .queries
                    .queries_by_id
                    .get(&qid)
                    .map(|q| q.name.clone())
                else {
                    continue;
                };
                let is_active = Some(qid) == current_id;

                if self.renaming_tab_id == Some(qid) {
                    let resp = ui.add(
                        egui::TextEdit::singleline(&mut self.rename_tab_buffer)
                            .desired_width(140.0),
                    );
                    if resp.lost_focus() || ui.input(|i| i.key_pressed(Key::Enter)) {
                        self.store.select_query(qid);
                        let new_name = self.rename_tab_buffer.clone();
                        self.store.rename_query(&new_name);
                        self.renaming_tab_id = None;
                    }
                } else {
                    let tab_fill = if is_active {
                        theme.bg_elevated
                    } else {
                        theme.bg_surface
                    };
                    let tab_stroke = if is_active {
                        Stroke::new(1.2, theme.accent_primary)
                    } else {
                        Stroke::new(1.0, theme.border_hairline)
                    };
                    egui::Frame::new()
                        .fill(tab_fill)
                        .stroke(tab_stroke)
                        .corner_radius(CornerRadius::same(4))
                        .inner_margin(Margin::symmetric(8, 3))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 6.0;
                                let label_color = if is_active {
                                    theme.text_primary
                                } else {
                                    theme.text_muted
                                };
                                let btn = ui.add(
                                    egui::Button::new(RichText::new(&q_name).color(label_color))
                                        .frame(false),
                                );
                                if btn.clicked() {
                                    self.store.select_query(qid);
                                }
                                if btn.double_clicked() {
                                    self.renaming_tab_id = Some(qid);
                                    self.rename_tab_buffer = q_name;
                                }
                                if ui
                                    .add(
                                        egui::Button::new(
                                            RichText::new("×").small().color(theme.text_muted),
                                        )
                                        .frame(false),
                                    )
                                    .on_hover_text("Close tab (Ctrl+W)")
                                    .clicked()
                                {
                                    self.store.remove_query(qid);
                                }
                            });
                        });
                }
            }

            if ui
                .add(subtle_button(&theme, "+"))
                .on_hover_text("New Query Tab (Ctrl+T)")
                .clicked()
            {
                if let Some(db) = self.store.current_database() {
                    self.store.new_query(&db);
                }
            }
        });
    }

    /// Renders the SQL code editor, inline parameter binding bar, action toolbar, and query results.
    pub(crate) fn render_active_query_panel(&mut self, _ctx: &egui::Context, ui: &mut egui::Ui) {
        let theme = self.current_theme();
        let Some(qid) = self.store.queries.current_query_id else {
            return;
        };
        let Some(query_tab) = self.store.queries.queries_by_id.get(&qid).cloned() else {
            return;
        };

        let client_key = self
            .store
            .connections
            .server
            .as_ref()
            .map(|s| s.client.clone())
            .unwrap_or_default();
        let allow_cancel = !self
            .store
            .connections
            .disabled_features
            .iter()
            .any(|f| f == "cancelQuery");

        let mut sql_text = query_tab.query.clone();
        let editor_id = egui::Id::unique(("sql_editor", qid));
        let focused = ui.memory(|m| m.has_focus(editor_id));
        let has_items = self
            .completion
            .as_ref()
            .is_some_and(|c| !c.items.is_empty());
        let accept = has_items
            && focused
            && ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::Tab));
        let explicit =
            focused && ui.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, Key::Space));
        if has_items && ui.input(|i| i.key_pressed(Key::Escape)) {
            self.completion = None;
        }

        let editor_stroke = if focused {
            Stroke::new(1.5, theme.accent_primary)
        } else {
            Stroke::new(1.0, theme.border_hairline)
        };
        let mut cursor = None;
        egui::Frame::new()
            .fill(theme.bg_surface)
            .stroke(editor_stroke)
            .corner_radius(CornerRadius::same(6))
            .inner_margin(Margin::same(8))
            .show(ui, |ui| {
                let mut draw_editor = |ui: &mut egui::Ui| {
                    let output = egui::TextEdit::multiline(&mut sql_text)
                        .id(editor_id)
                        .frame(egui::Frame::NONE)
                        .code_editor()
                        .font(FontId::monospace(self.editor_font_size))
                        .desired_rows(8)
                        .desired_width(if self.wrap_contents {
                            f32::INFINITY
                        } else {
                            ui.available_width().max(600.0)
                        })
                        .hint_text(
                            "Write your SQL query here... (Ctrl+Space: suggestions, Tab: accept)",
                        )
                        .show(ui);
                    cursor = output.cursor_range.map(|r| r.primary.index.0);
                };
                if self.wrap_contents {
                    draw_editor(ui);
                } else {
                    egui::ScrollArea::horizontal()
                        .id_salt(("sql_editor_hscroll", qid))
                        .show(ui, draw_editor);
                }
            });

        let changed = sql_text != query_tab.query;
        if changed {
            self.store.update_query_if_needed(&sql_text, None, None);
        }
        if let (Some(cursor), true) = (cursor, changed || explicit) {
            self.refresh_completions(&query_tab.database, &sql_text, cursor, explicit);
        }
        let picked = self.render_completion_strip(ui, accept);
        if let Some(label) = picked {
            self.apply_completion(ui.ctx(), editor_id, &sql_text, &label);
        }

        let detected_params = extract_query_parameters(&query_tab.query);
        if !detected_params.is_empty() {
            egui::Frame::new()
                .fill(theme.bg_surface)
                .stroke(Stroke::new(1.0, theme.border_hairline))
                .corner_radius(CornerRadius::same(5))
                .inner_margin(Margin::symmetric(10, 6))
                .show(ui, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        badge_pill(ui, "PARAMS", theme.accent_secondary);
                        for p_name in detected_params {
                            ui.label(RichText::new(format!("{p_name} =")).monospace());
                            let entry = self.store.query_params.entry(p_name.clone()).or_default();
                            egui::ComboBox::from_id_salt(("param_kind", &p_name))
                                .width(80.0)
                                .selected_text(entry.kind.label())
                                .show_ui(ui, |ui| {
                                    for kind in ParamKind::ALL {
                                        ui.selectable_value(&mut entry.kind, kind, kind.label());
                                    }
                                });
                            match entry.kind {
                                ParamKind::Null => {}
                                ParamKind::Boolean => {
                                    let mut on = entry.text.eq_ignore_ascii_case("true");
                                    ui.checkbox(&mut on, "");
                                    entry.text = on.to_string();
                                }
                                ParamKind::Text | ParamKind::Number => {
                                    ui.add(
                                        egui::TextEdit::singleline(&mut entry.text)
                                            .desired_width(100.0),
                                    );
                                }
                            }
                        }
                    });
                });
        }

        ui.add_space(4.0);
        egui::Frame::new()
            .fill(theme.bg_surface)
            .stroke(Stroke::new(1.0, theme.border_hairline))
            .corner_radius(CornerRadius::same(6))
            .inner_margin(Margin::symmetric(10, 6))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.checkbox(&mut self.wrap_contents, "Wrap")
                        .on_hover_text("Wrap long SQL lines in editor");
                    if ui
                        .small_button("A-")
                        .on_hover_text("Decrease editor font size")
                        .clicked()
                    {
                        self.editor_font_size = (self.editor_font_size - 1.0).max(10.0);
                    }
                    if ui
                        .small_button("A+")
                        .on_hover_text("Increase editor font size")
                        .clicked()
                    {
                        self.editor_font_size = (self.editor_font_size + 1.0).min(24.0);
                    }
                    ui.checkbox(&mut self.store.read_only_mode, "Read-Only")
                        .on_hover_text("Block INSERT/UPDATE/DELETE/DDL statements");
                    if self.store.read_only_mode {
                        badge_pill(ui, "RO", theme.semantic_warn);
                    }
                    ui.separator();
                    if matches!(client_key.as_str(), "mysql" | "sqlserver")
                        && ui.add(subtle_button(&theme, "Notes")).clicked()
                    {
                        self.show_client_info_modal = true;
                    }
                    if ui
                        .add(subtle_button(&theme, "Format"))
                        .on_hover_text("Format SQL (Ctrl+I)")
                        .clicked()
                    {
                        self.format_current_query();
                    }
                    if ui.add(subtle_button(&theme, "Bookmarks")).clicked() {
                        self.show_bookmarks_modal = true;
                    }
                    if ui.add(subtle_button(&theme, "ERD")).clicked() {
                        if let Some(db) = self.store.current_database() {
                            let rt = self.rt.clone();
                            if let Ok(erd) = rt.block_on(self.store.generate_database_erd(&db)) {
                                self.erd_modal_diagram = Some(erd);
                            }
                        }
                    }
                    if ui
                        .add(subtle_button(&theme, "Import"))
                        .on_hover_text("Import CSV or JSON rows into a table")
                        .clicked()
                    {
                        self.import_modal = Some(Default::default());
                    }
                    if ui
                        .add(subtle_button(&theme, "Open"))
                        .on_hover_text("Open .sql file (Ctrl+O)")
                        .clicked()
                    {
                        self.show_open_query_modal = true;
                    }
                    if ui
                        .add(subtle_button(&theme, "Save"))
                        .on_hover_text("Save .sql file (Ctrl+S)")
                        .clicked()
                    {
                        self.show_save_query_modal = true;
                    }

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if query_tab.is_executing && allow_cancel {
                            if ui
                                .add_enabled(
                                    !query_tab.is_canceling,
                                    subtle_button(&theme, "Cancel"),
                                )
                                .clicked()
                            {
                                let rt = self.rt.clone();
                                let _ = rt.block_on(self.store.cancel_query(qid));
                            }
                        } else if ui.add(subtle_button(&theme, "Clear")).clicked() {
                            self.store.update_query_if_needed("", Some(""), None);
                        }

                        if ui
                            .add_enabled(
                                !query_tab.is_executing,
                                primary_button(&theme, "▶ Execute"),
                            )
                            .on_hover_text("Execute query or selection (Ctrl+Enter / Ctrl+R)")
                            .clicked()
                        {
                            self.trigger_execute_current_query();
                        }
                    });
                });
            });

        ui.add_space(6.0);
        self.render_query_results(ui, &query_tab);
    }
}
