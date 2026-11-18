//! TUI import dialog: load a CSV/JSON file into the highlighted table.
//!
//! `i` on a table opens it. Type the file path and press Enter to load and preview it; then
//! Up/Down pick a source column, Left/Right change its target column (or skip it), `y`
//! confirms a large import, Enter imports, `e` edits the path again, and Esc closes.
//!
//! The import runs through [`crate::adapters::DatabaseFacade::import_rows`] (the same code as
//! the desktop app and `--import`): prepared-statement inserts, all rows in one transaction.
//! Imports of [`crate::core::import::LARGE_IMPORT_ROWS`] rows or more need `y`, and Read-Only
//! mode refuses imports.

use crate::common::types::ListTableResult;
use crate::core::import::{
    mapping_slots, mappings_from_slots, parse, ColumnMapping, ImportData, ImportRequest,
};
use crate::tui::TuiApp;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::path::Path;
use tokio::runtime::Runtime;

/// State of the open import dialog.
#[derive(Debug, Clone)]
pub struct TuiImport {
    /// Target table.
    pub table: ListTableResult,
    /// File path being typed.
    pub path: String,
    /// Parsed file, once loaded (`None` while the path is being edited).
    pub data: Option<ImportData>,
    /// Columns of the target table.
    pub target_columns: Vec<String>,
    /// Target column for each source column (`None` = skip).
    pub mapping: Vec<Option<String>>,
    /// Highlighted source column.
    pub selected: usize,
    /// The user confirmed a large import with `y`.
    pub confirmed: bool,
    /// Last result (`Ok` = success message).
    pub message: Option<Result<String, String>>,
}

impl TuiImport {
    /// The column mappings that will be imported.
    pub fn mappings(&self) -> Vec<ColumnMapping> {
        mappings_from_slots(&self.mapping)
    }

    /// Moves the highlighted source column's target forward or back through
    /// `(skip), target_1, ..., target_n`.
    fn cycle_target(&mut self, forward: bool) {
        let Some(slot) = self.mapping.get_mut(self.selected) else {
            return;
        };
        let n = self.target_columns.len() + 1;
        let pos = slot
            .as_ref()
            .and_then(|t| self.target_columns.iter().position(|c| c == t))
            .map_or(0, |i| i + 1);
        let next = if forward {
            (pos + 1) % n
        } else {
            (pos + n - 1) % n
        };
        *slot = next.checked_sub(1).map(|i| self.target_columns[i].clone());
    }
}

impl TuiApp {
    /// Opens the import dialog for the highlighted table.
    pub(crate) fn open_import(&mut self, rt: &Runtime, database: &str) {
        let Some(table) = self
            .store
            .tables
            .items_by_database
            .get(database)
            .and_then(|v| v.get(self.selected_table_idx))
            .cloned()
        else {
            self.store.status = "Select a table to import into".into();
            return;
        };
        let cols = rt.block_on(self.store.db.list_table_columns(
            database,
            &table.name,
            table.schema.as_deref(),
        ));
        let (target_columns, message) = match cols {
            Ok(c) => (c.into_iter().map(|c| c.column_name).collect(), None),
            Err(e) => (Vec::new(), Some(Err(e))),
        };
        self.import = Some(TuiImport {
            table,
            path: String::new(),
            data: None,
            target_columns,
            mapping: Vec::new(),
            selected: 0,
            confirmed: false,
            message,
        });
    }

    fn import_load_file(&mut self) {
        let delim = self
            .store
            .config
            .data
            .as_ref()
            .map(|c| c.csv_delimiter.clone());
        let delim = delim.and_then(|d| d.bytes().next()).unwrap_or(b',');
        let Some(st) = self.import.as_mut() else {
            return;
        };
        match parse(Path::new(st.path.trim()), delim) {
            Ok(data) => {
                st.mapping = mapping_slots(&data.columns, &st.target_columns);
                st.data = Some(data);
                st.selected = 0;
                st.confirmed = false;
                st.message = None;
            }
            Err(e) => st.message = Some(Err(e)),
        }
    }

    fn import_run(&mut self, rt: &Runtime, database: &str) {
        let read_only = self.store.read_only_mode;
        let Some(st) = self.import.as_ref() else {
            return;
        };
        let Some(data) = st.data.as_ref() else {
            return;
        };
        let refusal = if read_only {
            Some("Import is disabled in Read-Only mode.".to_string())
        } else if data.requires_confirmation() && !st.confirmed {
            Some(format!(
                "{} rows is a large import. Press y to confirm importing all of them.",
                data.rows.len()
            ))
        } else {
            None
        };
        let res = match refusal {
            Some(msg) => Err(msg),
            None => {
                let mappings = st.mappings();
                let req = ImportRequest {
                    table: &st.table.name,
                    schema: st.table.schema.as_deref(),
                    mappings: &mappings,
                    data,
                    confirmed: st.confirmed,
                };
                rt.block_on(self.store.db.import_rows(database, req))
                    .map(|r| format!("Imported {} rows into {}.", r.rows_inserted, st.table.name))
            }
        };
        if let Some(st) = self.import.as_mut() {
            if res.is_ok() {
                // Back to the path so a second Enter cannot import the same rows twice.
                st.data = None;
            }
            st.message = Some(res);
        }
    }

    /// Handles keys while the import dialog is open.
    pub(crate) fn handle_import_key(&mut self, key: KeyEvent, rt: &Runtime, database: &str) {
        let Some(st) = self.import.as_mut() else {
            return;
        };
        if key.code == KeyCode::Esc {
            self.import = None;
            return;
        }
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        if st.data.is_none() {
            match key.code {
                KeyCode::Enter => self.import_load_file(),
                KeyCode::Backspace => {
                    st.path.pop();
                }
                KeyCode::Char(c) if !ctrl => st.path.push(c),
                _ => {}
            }
            return;
        }
        let columns = st.data.as_ref().map_or(0, |d| d.columns.len());
        match key.code {
            KeyCode::Up => st.selected = st.selected.saturating_sub(1),
            KeyCode::Down if st.selected + 1 < columns => st.selected += 1,
            KeyCode::Left => st.cycle_target(false),
            KeyCode::Right => st.cycle_target(true),
            KeyCode::Char('y') => st.confirmed = !st.confirmed,
            KeyCode::Char('e') => st.data = None,
            KeyCode::Enter => self.import_run(rt, database),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cycling_a_target_walks_skip_and_every_column() {
        let mut st = TuiImport {
            table: ListTableResult {
                schema: None,
                name: "t".into(),
            },
            path: String::new(),
            data: None,
            target_columns: vec!["id".into(), "name".into()],
            mapping: vec![Some("id".into())],
            selected: 0,
            confirmed: false,
            message: None,
        };
        st.cycle_target(true);
        assert_eq!(st.mapping[0].as_deref(), Some("name"));
        st.cycle_target(true);
        assert_eq!(st.mapping[0], None);
        assert!(st.mappings().is_empty());
        st.cycle_target(false);
        assert_eq!(st.mapping[0].as_deref(), Some("name"));
    }
}
