//! Data import from CSV and JSON files into an existing table.
//!
//! The flow is: [`parse()`] a file into an [`ImportData`], show a [`ImportData::preview`], map
//! source columns to the target table's columns ([`auto_map`], editable by the user), then
//! [`run_import`]. Every row is inserted through a prepared statement (values are bound, never
//! written into the SQL), all inside one transaction: a failing row rolls back the whole import.
//! Imports of [`LARGE_IMPORT_ROWS`] rows or more need explicit confirmation.

pub mod parse;
pub mod run;

pub use parse::{parse, parse_csv, parse_json, ImportFormat};
pub use run::{run_import, ImportReport, ImportRequest};

use crate::adapters::wrap_identifier_for_adapter;
use crate::core::params::{ParamValue, QueryParams};

/// Imports with at least this many rows require explicit confirmation.
pub const LARGE_IMPORT_ROWS: usize = 10_000;

/// Rows shown in an import preview.
pub const PREVIEW_ROWS: usize = 10;

/// Parsed file contents: a header and typed rows (one value per column).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ImportData {
    /// Source column names, in file order.
    pub columns: Vec<String>,
    /// Rows, each with exactly one value per column.
    pub rows: Vec<Vec<ParamValue>>,
}

impl ImportData {
    /// The first `n` rows, for the preview.
    pub fn preview(&self, n: usize) -> &[Vec<ParamValue>] {
        &self.rows[..self.rows.len().min(n)]
    }

    /// Whether importing every row needs the user's explicit confirmation.
    pub fn requires_confirmation(&self) -> bool {
        self.rows.len() >= LARGE_IMPORT_ROWS
    }
}

/// One mapped column: source column index into [`ImportData::columns`] -> target column.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColumnMapping {
    /// Index of the source column.
    pub source: usize,
    /// Name of the target table column.
    pub target: String,
}

/// Maps each source column to the target column with the same name (case-insensitive).
/// Source columns with no match are left unmapped (skipped).
pub fn auto_map(source: &[String], target: &[String]) -> Vec<ColumnMapping> {
    source
        .iter()
        .enumerate()
        .filter_map(|(i, name)| {
            target
                .iter()
                .find(|t| t.trim().eq_ignore_ascii_case(name.trim()))
                .map(|t| ColumnMapping {
                    source: i,
                    target: t.clone(),
                })
        })
        .collect()
}

/// Builds `INSERT INTO <table> (<cols>) VALUES (:c0, :c1, ...)` with quoted identifiers.
pub fn build_insert_sql(
    adapter_key: &str,
    table: &str,
    schema: Option<&str>,
    mappings: &[ColumnMapping],
) -> String {
    let wrap = |s: &str| wrap_identifier_for_adapter(adapter_key, s);
    let target = match schema.filter(|s| !s.is_empty()) {
        Some(s) => format!("{}.{}", wrap(s), wrap(table)),
        None => wrap(table),
    };
    let cols: Vec<String> = mappings.iter().map(|m| wrap(&m.target)).collect();
    let slots: Vec<String> = (0..mappings.len()).map(|i| format!(":c{i}")).collect();
    format!(
        "INSERT INTO {target} ({}) VALUES ({})",
        cols.join(", "),
        slots.join(", ")
    )
}

/// The parameter values for one row under `mappings` (`c0`, `c1`, ...).
pub fn row_params(row: &[ParamValue], mappings: &[ColumnMapping]) -> QueryParams {
    mappings
        .iter()
        .enumerate()
        .map(|(i, m)| {
            let v = row.get(m.source).cloned().unwrap_or(ParamValue::Null);
            (format!("c{i}"), v)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auto_map_matches_names_case_insensitively_and_skips_unknown() {
        let src = vec!["ID".to_string(), "Email".to_string(), "extra".to_string()];
        let tgt = vec!["id".to_string(), "email".to_string(), "name".to_string()];
        let m = auto_map(&src, &tgt);
        assert_eq!(
            m,
            vec![
                ColumnMapping {
                    source: 0,
                    target: "id".into()
                },
                ColumnMapping {
                    source: 1,
                    target: "email".into()
                },
            ]
        );
    }

    #[test]
    fn insert_sql_quotes_identifiers_and_uses_placeholders() {
        let m = vec![
            ColumnMapping {
                source: 1,
                target: "e\"mail".into(),
            },
            ColumnMapping {
                source: 0,
                target: "id".into(),
            },
        ];
        assert_eq!(
            build_insert_sql("postgresql", "users", Some("public"), &m),
            r#"INSERT INTO "public"."users" ("e""mail", "id") VALUES (:c0, :c1)"#
        );
        let row = vec![ParamValue::Int(7), ParamValue::Text("a@b.c".into())];
        let p = row_params(&row, &m);
        assert_eq!(p["c0"], ParamValue::Text("a@b.c".into()));
        assert_eq!(p["c1"], ParamValue::Int(7));
    }

    #[test]
    fn large_imports_require_confirmation() {
        let mut data = ImportData {
            columns: vec!["a".into()],
            rows: vec![vec![ParamValue::Null]; LARGE_IMPORT_ROWS - 1],
        };
        assert!(!data.requires_confirmation());
        data.rows.push(vec![ParamValue::Null]);
        assert!(data.requires_confirmation());
        assert_eq!(data.preview(PREVIEW_ROWS).len(), PREVIEW_ROWS);
    }
}
