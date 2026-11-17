//! Query browser layout and database tree sidebar rendering.

use crate::gui::RustSqlGuiApp;
use eframe::egui::{self, RichText};

impl RustSqlGuiApp {
    /// Renders the connected query browser view (collapsible left sidebar + central editor/results area).
    pub(crate) fn render_query_browser(
        &mut self,
        ctx: &egui::Context,
        ui: &mut egui::Ui,
        server_id: &str,
    ) {
        if !self.sidebar_collapsed {
            egui::SidePanel::left("db_sidebar")
                .resizable(true)
                .default_width(250.0)
                .width_range(200.0..=600.0)
                .show_inside(ui, |ui| {
                    self.render_sidebar(ui, server_id);
                });
        }

        egui::CentralPanel::default().show_inside(ui, |ui| {
            ui.horizontal(|ui| {
                if ui
                    .small_button(if self.sidebar_collapsed {
                        "▶ Sidebar"
                    } else {
                        "◀"
                    })
                    .clicked()
                {
                    self.sidebar_collapsed = !self.sidebar_collapsed;
                }
                self.render_query_tabs_bar(ui);
            });
            ui.separator();
            self.render_active_query_panel(ctx, ui);
        });
    }

    /// Renders the left-hand database explorer tree and search filters.
    pub(crate) fn render_sidebar(&mut self, ui: &mut egui::Ui, server_id: &str) {
        if let Some(ref srv) = self.store.connections.server {
            ui.horizontal(|ui| {
                ui.label(RichText::new(format!("[{}]", srv.client.to_uppercase())).strong());
                ui.label(RichText::new(&srv.name).strong());
            });
        }
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.label("🔍");
            ui.add(
                egui::TextEdit::singleline(&mut self.db_search_filter)
                    .hint_text("Search databases...")
                    .desired_width(ui.available_width()),
            );
        });
        ui.separator();

        let dbs: Vec<String> = self
            .store
            .databases
            .items
            .iter()
            .map(|d| d.name.clone())
            .filter(|name| {
                self.db_search_filter.trim().is_empty()
                    || name
                        .to_lowercase()
                        .contains(&self.db_search_filter.to_lowercase())
            })
            .collect();

        let current_db = self.store.current_database().unwrap_or_default();
        let disabled_features = self.store.connections.disabled_features.clone();

        egui::ScrollArea::vertical().show(ui, |ui| {
            for db_name in dbs {
                let is_current = db_name == current_db;
                let is_expanded = self.expanded_databases.contains(&db_name);
                let header_label = format!("{} 🗄 {}", if is_expanded { "▼" } else { "▶" }, db_name);

                let resp = ui.selectable_label(is_current, RichText::new(header_label).strong());
                if resp.clicked() {
                    if self.expanded_databases.contains(&db_name) {
                        self.expanded_databases.remove(&db_name);
                    } else {
                        self.expanded_databases.insert(db_name.clone());
                        let rt = self.rt.clone();
                        let _ =
                            rt.block_on(self.store.connect(server_id, Some(&db_name), false, None));
                        let _ = rt.block_on(self.store.fetch_schemas_if_needed(&db_name));
                        let _ = rt.block_on(self.store.fetch_tables_if_needed(&db_name));
                        let _ = rt.block_on(self.store.fetch_views_if_needed(&db_name));
                        let _ = rt.block_on(self.store.fetch_routines_if_needed(&db_name));
                    }
                }

                resp.context_menu(|ui| {
                    if ui.button("🔄 Refresh Database").clicked() {
                        self.store.refresh_database(&db_name);
                        let rt = self.rt.clone();
                        let _ = rt.block_on(self.store.fetch_schemas_if_needed(&db_name));
                        let _ = rt.block_on(self.store.fetch_tables_if_needed(&db_name));
                        let _ = rt.block_on(self.store.fetch_views_if_needed(&db_name));
                        let _ = rt.block_on(self.store.fetch_routines_if_needed(&db_name));
                        ui.close_menu();
                    }
                    if ui.button("➕ Open Tab").clicked() {
                        self.store.new_query(&db_name);
                        ui.close_menu();
                    }
                });

                if is_expanded {
                    ui.indent(format!("db_indent_{db_name}"), |ui| {
                        let filter_val = {
                            let obj_filter =
                                self.db_obj_filters.entry(db_name.clone()).or_default();
                            ui.horizontal(|ui| {
                                ui.label("🔎");
                                ui.add(
                                    egui::TextEdit::singleline(obj_filter)
                                        .hint_text("Search objects...")
                                        .desired_width(ui.available_width()),
                                );
                            });
                            obj_filter.to_lowercase()
                        };
                        self.render_database_tables(ui, &db_name, &filter_val, &disabled_features);
                        self.render_database_views_and_routines(
                            ui,
                            &db_name,
                            &filter_val,
                            &disabled_features,
                        );
                    });
                }
            }
        });
    }
}
