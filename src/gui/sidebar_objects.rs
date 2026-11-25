//! Sidebar tree rendering for tables, columns, triggers, indexes, views, functions, and procedures.

use crate::gui::RustSqlGuiApp;
use crate::state::{DbObjectType, ScriptActionType};
use eframe::egui::{self, RichText};

impl RustSqlGuiApp {
    /// Renders the collapsible Tables section and per-table context menus inside the sidebar.
    pub(crate) fn render_database_tables(
        &mut self,
        ui: &mut egui::Ui,
        db_name: &str,
        filter_val: &str,
        disabled_features: &[String],
    ) {
        let tables = self
            .store
            .tables
            .items_by_database
            .get(db_name)
            .cloned()
            .unwrap_or_default();
        ui.collapsing(format!("Tables ({})", tables.len()), |ui| {
            for tbl in tables {
                if !filter_val.is_empty() && !tbl.name.to_lowercase().contains(filter_val) {
                    continue;
                }
                let key = (db_name.to_string(), tbl.name.clone());
                let tbl_expanded = self.expanded_tables.contains(&key);
                ui.horizontal(|ui| {
                    if ui
                        .small_button(if tbl_expanded { "▾" } else { "▸" })
                        .clicked()
                    {
                        if tbl_expanded {
                            self.expanded_tables.remove(&key);
                        } else {
                            self.expanded_tables.insert(key.clone());
                            let rt = self.rt.clone();
                            let s = tbl.schema.as_deref();
                            let _ = rt.block_on(
                                self.store
                                    .fetch_table_columns_if_needed(db_name, &tbl.name, s),
                            );
                            let _ = rt.block_on(
                                self.store
                                    .fetch_table_triggers_if_needed(db_name, &tbl.name, s),
                            );
                            let _ = rt.block_on(
                                self.store
                                    .fetch_table_indexes_if_needed(db_name, &tbl.name, s),
                            );
                        }
                    }
                    let item_resp = ui.link(&tbl.name);
                    if item_resp.clicked() {
                        let rt = self.rt.clone();
                        let _ = rt.block_on(self.store.execute_default_select_query_if_needed(
                            db_name,
                            &tbl.name,
                            tbl.schema.as_deref(),
                        ));
                    }
                    item_resp.context_menu(|ui| {
                        if ui.button("Select Rows (with limit)").clicked() {
                            let rt = self.rt.clone();
                            let _ = rt.block_on(self.store.execute_default_select_query_if_needed(
                                db_name,
                                &tbl.name,
                                tbl.schema.as_deref(),
                            ));
                            ui.close();
                        }
                        ui.separator();
                        if !disabled_features.iter().any(|f| f == "scriptCreateTable")
                            && ui.button("Create Statement").clicked()
                        {
                            let rt = self.rt.clone();
                            let _ = rt.block_on(self.store.get_sql_script_if_needed(
                                db_name,
                                &tbl.name,
                                ScriptActionType::Create,
                                DbObjectType::Table,
                                tbl.schema.as_deref(),
                            ));
                            ui.close();
                        }
                        for (label, act) in [
                            ("Select Statement", ScriptActionType::Select),
                            ("Insert Statement", ScriptActionType::Insert),
                            ("Update Statement", ScriptActionType::Update),
                            ("Delete Statement", ScriptActionType::Delete),
                        ] {
                            if ui.button(label).clicked() {
                                let rt = self.rt.clone();
                                let _ = rt.block_on(self.store.get_sql_script_if_needed(
                                    db_name,
                                    &tbl.name,
                                    act,
                                    DbObjectType::Table,
                                    tbl.schema.as_deref(),
                                ));
                                ui.close();
                            }
                        }
                    });
                });

                if tbl_expanded {
                    self.render_table_children(ui, db_name, &tbl.name);
                }
            }
        });
    }

    fn render_table_children(&self, ui: &mut egui::Ui, db_name: &str, tbl_name: &str) {
        let theme = self.current_theme();
        ui.indent(format!("tbl_meta_{db_name}_{tbl_name}"), |ui| {
            let cols = self
                .store
                .columns
                .columns_by_table
                .get(db_name)
                .and_then(|m| m.get(tbl_name))
                .cloned()
                .unwrap_or_default();
            ui.collapsing(format!("Columns ({})", cols.len()), |ui| {
                for c in cols {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(&c.name).color(theme.text_primary));
                        ui.label(
                            RichText::new(c.data_type.to_uppercase())
                                .monospace()
                                .small()
                                .color(theme.text_muted),
                        );
                    });
                }
            });
            let triggers = self
                .store
                .triggers
                .triggers_by_table
                .get(db_name)
                .and_then(|m| m.get(tbl_name))
                .cloned()
                .unwrap_or_default();
            ui.collapsing(format!("Triggers ({})", triggers.len()), |ui| {
                for tr in triggers {
                    ui.label(RichText::new(&tr.name).color(theme.text_primary));
                }
            });
            let indexes = self
                .store
                .indexes
                .indexes_by_table
                .get(db_name)
                .and_then(|m| m.get(tbl_name))
                .cloned()
                .unwrap_or_default();
            ui.collapsing(format!("Indexes ({})", indexes.len()), |ui| {
                for idx in indexes {
                    ui.label(RichText::new(&idx.name).color(theme.text_primary));
                }
            });
        });
    }

    /// Renders Views, Functions, and Procedures sections inside the database sidebar.
    pub(crate) fn render_database_views_and_routines(
        &mut self,
        ui: &mut egui::Ui,
        db_name: &str,
        filter_val: &str,
        disabled_features: &[String],
    ) {
        let views = self
            .store
            .views
            .views_by_database
            .get(db_name)
            .cloned()
            .unwrap_or_default();
        ui.collapsing(format!("Views ({})", views.len()), |ui| {
            for vw in views {
                if !filter_val.is_empty() && !vw.name.to_lowercase().contains(filter_val) {
                    continue;
                }
                let resp = ui.link(&vw.name);
                if resp.clicked() {
                    let rt = self.rt.clone();
                    let _ = rt.block_on(self.store.execute_default_select_query_if_needed(
                        db_name,
                        &vw.name,
                        vw.schema.as_deref(),
                    ));
                }
                resp.context_menu(|ui| {
                    if ui.button("Select Rows (with limit)").clicked() {
                        let rt = self.rt.clone();
                        let _ = rt.block_on(self.store.execute_default_select_query_if_needed(
                            db_name,
                            &vw.name,
                            vw.schema.as_deref(),
                        ));
                        ui.close();
                    }
                    if !disabled_features.iter().any(|f| f == "scriptCreateTable")
                        && ui.button("Create Statement").clicked()
                    {
                        let rt = self.rt.clone();
                        let _ = rt.block_on(self.store.get_sql_script_if_needed(
                            db_name,
                            &vw.name,
                            ScriptActionType::Create,
                            DbObjectType::View,
                            vw.schema.as_deref(),
                        ));
                        ui.close();
                    }
                });
            }
        });

        let funcs = self
            .store
            .routines
            .functions_by_database
            .get(db_name)
            .cloned()
            .unwrap_or_default();
        ui.collapsing(format!("Functions ({})", funcs.len()), |ui| {
            for fn_item in funcs {
                ui.label(&fn_item.name).context_menu(|ui| {
                    if ui.button("Create Statement").clicked() {
                        let rt = self.rt.clone();
                        let _ = rt.block_on(self.store.get_sql_script_if_needed(
                            db_name,
                            &fn_item.name,
                            ScriptActionType::Create,
                            DbObjectType::Function,
                            fn_item.schema.as_deref(),
                        ));
                        ui.close();
                    }
                });
            }
        });

        let procs = self
            .store
            .routines
            .procedures_by_database
            .get(db_name)
            .cloned()
            .unwrap_or_default();
        ui.collapsing(format!("Procedures ({})", procs.len()), |ui| {
            for pr_item in procs {
                ui.label(&pr_item.name).context_menu(|ui| {
                    if ui.button("Create Statement").clicked() {
                        let rt = self.rt.clone();
                        let _ = rt.block_on(self.store.get_sql_script_if_needed(
                            db_name,
                            &pr_item.name,
                            ScriptActionType::Create,
                            DbObjectType::Procedure,
                            pr_item.schema.as_deref(),
                        ));
                        ui.close();
                    }
                });
            }
        });
    }
}
