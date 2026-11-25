//! Query browser layout and database tree sidebar rendering.

use crate::gui::icons::{icon_only_button, paint_icon_ui, IconKind};
use crate::gui::theme::{badge_pill, subtle_button};
use crate::gui::RustSqlGuiApp;
use eframe::egui::{self, CornerRadius, Margin, RichText, Stroke};

impl RustSqlGuiApp {
    /// Renders the connected query browser view (collapsible left sidebar + central editor/results area).
    pub(crate) fn render_query_browser(
        &mut self,
        ctx: &egui::Context,
        ui: &mut egui::Ui,
        server_id: &str,
    ) {
        let theme = self.current_theme();

        if !self.sidebar_collapsed {
            let sidebar_frame = egui::Frame::new()
                .fill(theme.bg_surface)
                .stroke(Stroke::new(1.0, theme.border_hairline))
                .corner_radius(CornerRadius::same(6))
                .inner_margin(Margin::same(10));
            egui::Panel::left("db_sidebar")
                .frame(sidebar_frame)
                .resizable(true)
                .default_size(255.0)
                .size_range(200.0..=600.0)
                .show(ui, |ui| {
                    self.render_sidebar(ui, server_id);
                });
        }

        let main_frame = egui::Frame::new()
            .fill(theme.bg_canvas)
            .inner_margin(Margin::symmetric(8, 0));
        egui::CentralPanel::default()
            .frame(main_frame)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    let toggle_label = if self.sidebar_collapsed {
                        "▶ Explorer"
                    } else {
                        "◀"
                    };
                    if ui
                        .add(subtle_button(&theme, toggle_label))
                        .on_hover_text("Toggle database explorer")
                        .clicked()
                    {
                        self.sidebar_collapsed = !self.sidebar_collapsed;
                    }
                    self.render_query_tabs_bar(ui);
                });
                ui.add_space(4.0);
                self.render_active_query_panel(ctx, ui);
            });
    }

    /// Renders the left-hand database explorer tree and search filters.
    pub(crate) fn render_sidebar(&mut self, ui: &mut egui::Ui, server_id: &str) {
        let theme = self.current_theme();

        if let Some((srv_client, srv_name)) = self
            .store
            .connections
            .server
            .as_ref()
            .map(|s| (s.client.clone(), s.name.clone()))
        {
            ui.horizontal(|ui| {
                badge_pill(
                    ui,
                    &srv_client.to_uppercase(),
                    theme.dialect_color(&srv_client),
                );
                ui.label(RichText::new(&srv_name).strong().color(theme.text_primary));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if icon_only_button(
                        ui,
                        &theme,
                        IconKind::Refresh,
                        "Refresh database & object metadata",
                    )
                    .clicked()
                    {
                        let rt = self.rt.clone();
                        let _ = rt.block_on(self.store.fetch_databases_if_needed());
                        if let Some(db) = self.store.current_database() {
                            self.store.refresh_database(&db);
                            let _ = rt.block_on(self.store.fetch_schemas_if_needed(&db));
                            let _ = rt.block_on(self.store.fetch_tables_if_needed(&db));
                            let _ = rt.block_on(self.store.fetch_views_if_needed(&db));
                            let _ = rt.block_on(self.store.fetch_routines_if_needed(&db));
                        }
                    }
                });
            });
            ui.add_space(6.0);
        }

        ui.horizontal(|ui| {
            paint_icon_ui(ui, IconKind::Search, 13.0, theme.text_muted);
            let filter_w = if self.db_search_filter.is_empty() {
                ui.available_width()
            } else {
                (ui.available_width() - 24.0).max(60.0)
            };
            ui.add(
                egui::TextEdit::singleline(&mut self.db_search_filter)
                    .hint_text("Filter databases...")
                    .desired_width(filter_w),
            );
            if !self.db_search_filter.is_empty()
                && ui.small_button("×").on_hover_text("Clear").clicked()
            {
                self.db_search_filter.clear();
            }
        });
        ui.add_space(4.0);
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
                let arrow = if is_expanded { "▾" } else { "▸" };
                let header_label = format!("{arrow}  {db_name}");

                let text = RichText::new(header_label).strong().color(if is_current {
                    theme.accent_secondary
                } else {
                    theme.text_primary
                });
                let resp = ui
                    .horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 4.0;
                        paint_icon_ui(
                            ui,
                            IconKind::Database,
                            13.0,
                            if is_current {
                                theme.accent_secondary
                            } else {
                                theme.text_muted
                            },
                        );
                        ui.selectable_label(is_current, text)
                    })
                    .inner;
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
                        ui.close();
                    }
                    if ui.button("➕ Open Tab").clicked() {
                        self.store.new_query(&db_name);
                        ui.close();
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
