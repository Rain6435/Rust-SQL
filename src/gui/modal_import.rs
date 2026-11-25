//! Data import modal: load a CSV/JSON file, preview it, map columns to a table, and import.
//!
//! The import runs through [`crate::adapters::DatabaseFacade::import_rows`]: prepared-statement
//! inserts in one transaction. Large imports need the confirmation checkbox, and Read-Only mode
//! disables importing.

use crate::common::types::ListTableResult;
use crate::core::import::{
    mapping_slots, mappings_from_slots, parse, ColumnMapping, ImportData, ImportRequest,
    PREVIEW_ROWS,
};
use crate::core::params::ParamValue;
use crate::gui::RustSqlGuiApp;
use eframe::egui::{self, Color32, RichText};
use egui_extras::{Column, TableBuilder};
use std::path::Path;

/// Editable state of the import modal.
#[derive(Debug, Clone, Default)]
pub struct ImportModalState {
    /// File path typed by the user.
    pub path: String,
    /// Parsed file, once loaded.
    pub data: Option<ImportData>,
    /// Target table.
    pub table: Option<ListTableResult>,
    /// Columns of the target table.
    pub target_columns: Vec<String>,
    /// Chosen target column for each source column (`None` = skip).
    pub mapping: Vec<Option<String>>,
    /// The user confirmed a large import.
    pub confirmed: bool,
    /// Last error or success message (`Ok` = success).
    pub message: Option<Result<String, String>>,
}

impl ImportModalState {
    fn remap(&mut self) {
        let Some(data) = &self.data else { return };
        self.mapping = mapping_slots(&data.columns, &self.target_columns);
    }

    fn mappings(&self) -> Vec<ColumnMapping> {
        mappings_from_slots(&self.mapping)
    }
}

fn cell_text(v: &ParamValue) -> RichText {
    match v {
        ParamValue::Null => RichText::new("NULL").italics().color(Color32::GRAY),
        ParamValue::Bool(b) => RichText::new(b.to_string()),
        ParamValue::Int(i) => RichText::new(i.to_string()),
        ParamValue::Float(f) => RichText::new(f.to_string()),
        ParamValue::Text(s) => RichText::new(s),
    }
}

impl RustSqlGuiApp {
    fn import_load_file(&mut self) {
        let delim = self
            .store
            .config
            .data
            .as_ref()
            .map(|c| c.csv_delimiter.clone());
        let delim = delim.and_then(|d| d.bytes().next()).unwrap_or(b',');
        let Some(st) = self.import_modal.as_mut() else {
            return;
        };
        match parse(Path::new(st.path.trim()), delim) {
            Ok(data) => {
                st.data = Some(data);
                st.confirmed = false;
                st.message = None;
                st.remap();
            }
            Err(e) => st.message = Some(Err(e)),
        }
    }

    fn import_select_table(&mut self, db: &str, table: ListTableResult) {
        let cols = self.rt.block_on(self.store.db.list_table_columns(
            db,
            &table.name,
            table.schema.as_deref(),
        ));
        let Some(st) = self.import_modal.as_mut() else {
            return;
        };
        match cols {
            Ok(cols) => st.target_columns = cols.into_iter().map(|c| c.column_name).collect(),
            Err(e) => st.message = Some(Err(e)),
        }
        st.table = Some(table);
        st.remap();
    }

    fn import_run(&mut self, db: &str) {
        let Some(st) = self.import_modal.as_ref() else {
            return;
        };
        let (Some(data), Some(table)) = (&st.data, &st.table) else {
            return;
        };
        let mappings = st.mappings();
        let req = ImportRequest {
            table: &table.name,
            schema: table.schema.as_deref(),
            mappings: &mappings,
            data,
            confirmed: st.confirmed,
        };
        let res = self.rt.block_on(self.store.db.import_rows(db, req));
        let msg = res.map(|r| format!("Imported {} rows into {}.", r.rows_inserted, table.name));
        if let Some(st) = self.import_modal.as_mut() {
            st.message = Some(msg);
        }
    }

    /// Renders the import modal when it is open.
    pub(crate) fn render_import_modal(&mut self, ctx: &egui::Context) {
        let Some(mut st) = self.import_modal.clone() else {
            return;
        };
        let theme = self.current_theme();
        let db = self.store.current_database().unwrap_or_default();
        let tables = self
            .store
            .tables
            .items_by_database
            .get(&db)
            .cloned()
            .unwrap_or_default();
        let read_only = self.store.read_only_mode;
        let (mut open, mut load, mut pick, mut run) = (true, false, None, false);
        egui::Window::new(format!("Import into {db}"))
            .open(&mut open)
            .default_width(720.0)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label("File (.csv, .tsv, .json, .jsonl):");
                    ui.text_edit_singleline(&mut st.path);
                    load = ui
                        .add(crate::gui::theme::subtle_button(&theme, "Load"))
                        .clicked();
                });
                ui.horizontal(|ui| {
                    ui.label("Target table:");
                    let current = st.table.as_ref().map(|t| t.name.clone()).unwrap_or_default();
                    egui::ComboBox::from_id_salt("import_table").selected_text(current).show_ui(
                        ui,
                        |ui| {
                            for t in &tables {
                                if ui.selectable_label(false, &t.name).clicked() {
                                    pick = Some(t.clone());
                                }
                            }
                        },
                    );
                });
                if let Some(data) = st.data.clone() {
                    ui.label(
                        RichText::new(format!(
                            "{} rows. Preview of the first {}; pick a target column for each source column:",
                            data.rows.len(),
                            data.preview(PREVIEW_ROWS).len()
                        ))
                        .small()
                        .color(theme.text_muted),
                    );
                    let mut table = TableBuilder::new(ui).striped(true).max_scroll_height(240.0);
                    for _ in &data.columns {
                        table = table.column(Column::initial(120.0).at_least(60.0).clip(true));
                    }
                    table
                        .header(44.0, |mut h| {
                            for (i, name) in data.columns.iter().enumerate() {
                                h.col(|ui| {
                                    ui.vertical(|ui| {
                                        ui.strong(name);
                                        let sel = st.mapping[i].clone().unwrap_or("(skip)".into());
                                        egui::ComboBox::from_id_salt(("import_map", i))
                                            .selected_text(sel)
                                            .show_ui(ui, |ui| {
                                                ui.selectable_value(&mut st.mapping[i], None, "(skip)");
                                                for t in &st.target_columns {
                                                    ui.selectable_value(&mut st.mapping[i], Some(t.clone()), t);
                                                }
                                            });
                                    });
                                });
                            }
                        })
                        .body(|body| {
                            let rows = data.preview(PREVIEW_ROWS);
                            body.rows(20.0, rows.len(), |mut row| {
                                for v in &rows[row.index()] {
                                    row.col(|ui| {
                                        ui.label(cell_text(v));
                                    });
                                }
                            });
                        });
                    if data.requires_confirmation() {
                        ui.checkbox(
                            &mut st.confirmed,
                            format!("I confirm importing all {} rows", data.rows.len()),
                        );
                    }
                    let ready = st.table.is_some()
                        && !st.mappings().is_empty()
                        && (!data.requires_confirmation() || st.confirmed)
                        && !read_only;
                    ui.horizontal(|ui| {
                        run = ui
                            .add_enabled(ready, crate::gui::theme::primary_button(&theme, "Import"))
                            .clicked();
                        if read_only {
                            ui.label(
                                RichText::new("Import is disabled in Read-Only mode.")
                                    .small()
                                    .color(theme.semantic_warn),
                            );
                        }
                    });
                }
                match &st.message {
                    Some(Ok(m)) => ui.label(RichText::new(m).color(theme.semantic_ok)),
                    Some(Err(e)) => ui.label(RichText::new(e).color(theme.semantic_err)),
                    None => ui.label(""),
                };
            });
        self.import_modal = open.then_some(st);
        if load {
            self.import_load_file();
        }
        if let Some(t) = pick {
            self.import_select_table(&db, t);
        }
        if run {
            self.import_run(&db);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remap_matches_by_name_and_skipped_columns_are_left_out() {
        let mut st = ImportModalState {
            data: Some(crate::core::import::parse_csv("Id,x,NAME\n1,2,3\n", b',').unwrap()),
            target_columns: vec!["id".into(), "name".into()],
            ..Default::default()
        };
        st.remap();
        assert_eq!(
            st.mapping,
            vec![Some("id".into()), None, Some("name".into())]
        );
        let m = st.mappings();
        assert_eq!(m.len(), 2);
        assert_eq!((m[1].source, m[1].target.as_str()), (2, "name"));
    }
}
