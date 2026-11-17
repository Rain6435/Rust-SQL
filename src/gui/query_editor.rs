//! Multi-tab SQL query bar, code editor, inline parameter binding inputs, and action toolbar.

use crate::core::params::extract_query_parameters;
use crate::gui::RustSqlGuiApp;
use eframe::egui::{self, FontId, Key, RichText};

impl RustSqlGuiApp {
    /// Renders the horizontal tab bar for open [`crate::state::QueryTab`] instances.
    pub(crate) fn render_query_tabs_bar(&mut self, ui: &mut egui::Ui) {
        let query_ids = self.store.queries.query_ids.clone();
        let current_id = self.store.queries.current_query_id;

        ui.horizontal_wrapped(|ui| {
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
                    let btn = ui.selectable_label(is_active, &q_name);
                    if btn.clicked() {
                        self.store.select_query(qid);
                    }
                    if btn.double_clicked() {
                        self.renaming_tab_id = Some(qid);
                        self.rename_tab_buffer = q_name;
                    }
                    if ui.small_button("✖").clicked() {
                        self.store.remove_query(qid);
                    }
                }
                ui.separator();
            }

            if ui
                .button("➕")
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
        ui.add_sized(
            [ui.available_width(), 160.0],
            egui::TextEdit::multiline(&mut sql_text)
                .code_editor()
                .font(FontId::monospace(self.editor_font_size))
                .desired_rows(8)
                .hint_text("Write your SQL query here..."),
        );
        if sql_text != query_tab.query {
            self.store.update_query_if_needed(&sql_text, None, None);
        }

        let detected_params = extract_query_parameters(&query_tab.query);
        if !detected_params.is_empty() {
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new("🔧 Params:").strong());
                for p_name in detected_params {
                    ui.label(format!("{p_name} ="));
                    let entry = self.store.query_params.entry(p_name).or_default();
                    ui.add(egui::TextEdit::singleline(entry).desired_width(100.0));
                }
            });
        }

        ui.horizontal(|ui| {
            ui.checkbox(&mut self.wrap_contents, "Wrap Contents");
            ui.checkbox(&mut self.store.read_only_mode, "🔒 Read-Only");
            if matches!(client_key.as_str(), "mysql" | "sqlserver") && ui.button("ℹ Info").clicked()
            {
                self.show_client_info_modal = true;
            }
            if ui.button("✨ Format").clicked() {
                self.format_current_query();
            }
            if ui.button("🔖 Bookmarks / History").clicked() {
                self.show_bookmarks_modal = true;
            }
            if ui.button("🗺 Schema ERD").clicked() {
                if let Some(db) = self.store.current_database() {
                    let rt = self.rt.clone();
                    if let Ok(erd) = rt.block_on(self.store.generate_database_erd(&db)) {
                        self.erd_modal_diagram = Some(erd);
                    }
                }
            }
            if ui.button("📂 Open").clicked() {
                self.show_open_query_modal = true;
            }
            if ui.button("💾 Save").clicked() {
                self.show_save_query_modal = true;
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if query_tab.is_executing && allow_cancel {
                    if ui
                        .add_enabled(!query_tab.is_canceling, egui::Button::new("⏹ Cancel"))
                        .clicked()
                    {
                        let rt = self.rt.clone();
                        let _ = rt.block_on(self.store.cancel_query(qid));
                    }
                } else if ui.button("🗑 Discard").clicked() {
                    self.store.update_query_if_needed("", Some(""), None);
                }

                if ui
                    .add_enabled(
                        !query_tab.is_executing,
                        egui::Button::new(RichText::new("▶ Execute").strong()),
                    )
                    .clicked()
                {
                    self.trigger_execute_current_query();
                }
            });
        });

        ui.separator();
        self.render_query_results(ui, &query_tab);
    }
}
