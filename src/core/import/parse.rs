//! Parsing CSV and JSON import files into [`ImportData`].
//!
//! CSV: the first record is the header; an empty field becomes `NULL` and every other field is
//! text (the database converts it to the column's type). JSON: an array of objects (or JSON
//! Lines, one object per line); column order is the order keys are first seen, a missing key is
//! `NULL`, and numbers, booleans, and null keep their JSON type. Nested arrays and objects are
//! stored as their JSON text.

use crate::core::import::ImportData;
use crate::core::params::ParamValue;
use serde_json::{Map, Value};
use std::path::Path;

/// Supported import file formats.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportFormat {
    /// Comma (or custom) separated values with a header row.
    Csv,
    /// A JSON array of objects, or JSON Lines.
    Json,
}

impl ImportFormat {
    /// Picks the format from a file extension (`.csv`/`.tsv`, `.json`/`.jsonl`/`.ndjson`).
    pub fn from_path(path: &Path) -> Result<Self, String> {
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        match ext.as_str() {
            "csv" | "tsv" | "txt" => Ok(Self::Csv),
            "json" | "jsonl" | "ndjson" => Ok(Self::Json),
            _ => Err(format!(
                "Cannot tell the import format of {}: use a .csv or .json file",
                path.display()
            )),
        }
    }
}

/// Reads and parses the file at `path`. `delimiter` applies to CSV (`.tsv` uses a tab).
pub fn parse(path: &Path, delimiter: u8) -> Result<ImportData, String> {
    ImportFormat::from_path(path)?;
    let text = std::fs::read_to_string(path)
        .map_err(|e| format!("Could not read {}: {e}", path.display()))?;
    parse_named(&path.to_string_lossy(), &text, delimiter)
}

/// Parses `text` read from a file called `name` (only its extension is used, as in
/// [`parse`]). The web UI uses this for uploaded files, which never touch the server's disk.
pub fn parse_named(name: &str, text: &str, delimiter: u8) -> Result<ImportData, String> {
    let path = Path::new(name);
    let is_tsv = path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("tsv"));
    match ImportFormat::from_path(path)? {
        ImportFormat::Csv => parse_csv(text, if is_tsv { b'\t' } else { delimiter }),
        ImportFormat::Json => parse_json(text),
    }
}

/// Parses CSV text with a header row.
pub fn parse_csv(text: &str, delimiter: u8) -> Result<ImportData, String> {
    let mut reader = csv::ReaderBuilder::new()
        .delimiter(delimiter)
        .has_headers(true)
        .from_reader(text.trim_start_matches('\u{feff}').as_bytes());
    let columns: Vec<String> = reader
        .headers()
        .map_err(|e| format!("Invalid CSV header: {e}"))?
        .iter()
        .map(|h| h.trim().to_string())
        .collect();
    if columns.is_empty() || columns.iter().all(String::is_empty) {
        return Err("The CSV file has no header row".to_string());
    }
    let mut rows = Vec::new();
    for (i, record) in reader.records().enumerate() {
        let record = record.map_err(|e| format!("CSV row {}: {e}", i + 2))?;
        if record.len() != columns.len() {
            return Err(format!(
                "CSV row {} has {} fields, but the header has {}",
                i + 2,
                record.len(),
                columns.len()
            ));
        }
        rows.push(
            record
                .iter()
                .map(|f| match f {
                    "" => ParamValue::Null,
                    s => ParamValue::Text(s.to_string()),
                })
                .collect(),
        );
    }
    Ok(ImportData { columns, rows })
}

fn json_value(v: &Value) -> ParamValue {
    match v {
        Value::Array(_) | Value::Object(_) => ParamValue::Text(v.to_string()),
        other => {
            ParamValue::from_json(other).unwrap_or_else(|_| ParamValue::Text(other.to_string()))
        }
    }
}

/// Parses a JSON array of objects, or JSON Lines (one object per line).
pub fn parse_json(text: &str) -> Result<ImportData, String> {
    let trimmed = text.trim_start_matches('\u{feff}').trim();
    let objects: Vec<Map<String, Value>> = if trimmed.starts_with('[') {
        let all: Vec<Value> =
            serde_json::from_str(trimmed).map_err(|e| format!("Invalid JSON: {e}"))?;
        all.into_iter()
            .enumerate()
            .map(|(i, v)| match v {
                Value::Object(m) => Ok(m),
                _ => Err(format!("JSON item {} is not an object", i + 1)),
            })
            .collect::<Result<_, _>>()?
    } else {
        trimmed
            .lines()
            .enumerate()
            .filter(|(_, l)| !l.trim().is_empty())
            .map(|(i, l)| match serde_json::from_str::<Value>(l) {
                Ok(Value::Object(m)) => Ok(m),
                Ok(_) => Err(format!("JSON line {} is not an object", i + 1)),
                Err(e) => Err(format!("JSON line {}: {e}", i + 1)),
            })
            .collect::<Result<_, _>>()?
    };
    let mut columns: Vec<String> = Vec::new();
    for obj in &objects {
        for key in obj.keys() {
            if !columns.contains(key) {
                columns.push(key.clone());
            }
        }
    }
    let rows = objects
        .iter()
        .map(|obj| {
            columns
                .iter()
                .map(|c| obj.get(c).map(json_value).unwrap_or(ParamValue::Null))
                .collect()
        })
        .collect();
    Ok(ImportData { columns, rows })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_named_picks_the_format_from_the_name() {
        let tsv = parse_named("up.TSV", "a\tb\n1\t2\n", b',').unwrap();
        assert_eq!(tsv.columns, vec!["a", "b"]);
        let json = parse_named("x.jsonl", "{\"a\": 1}\n", b',').unwrap();
        assert_eq!(json.rows, vec![vec![ParamValue::Int(1)]]);
        assert!(parse_named("x.exe", "", b',').is_err());
    }

    #[test]
    fn csv_keeps_text_and_maps_empty_fields_to_null() {
        let data = parse_csv("id,name,note\n1,\"O'Reilly, Tim\",\n2,x,y\n", b',').unwrap();
        assert_eq!(data.columns, ["id", "name", "note"]);
        assert_eq!(data.rows[0][1], ParamValue::Text("O'Reilly, Tim".into()));
        assert_eq!(data.rows[0][2], ParamValue::Null);
        assert_eq!(data.rows[1][0], ParamValue::Text("2".into()));
    }

    #[test]
    fn csv_rejects_ragged_rows() {
        let err = parse_csv("a,b\n1\n", b',').unwrap_err();
        assert!(err.contains("row 2"), "{err}");
    }

    #[test]
    fn json_array_and_lines_keep_types_and_fill_missing_keys() {
        let arr = r#"[{"id": 1, "ok": true, "tags": ["a"]}, {"id": 2.5, "name": null}]"#;
        let data = parse_json(arr).unwrap();
        assert_eq!(data.columns, ["id", "ok", "tags", "name"]);
        assert_eq!(data.rows[0][0], ParamValue::Int(1));
        assert_eq!(data.rows[0][1], ParamValue::Bool(true));
        assert_eq!(data.rows[0][2], ParamValue::Text("[\"a\"]".into()));
        assert_eq!(data.rows[1][0], ParamValue::Float(2.5));
        assert_eq!(data.rows[1][1], ParamValue::Null);

        let lines = parse_json("{\"a\": \"x\"}\n\n{\"a\": \"y\"}\n").unwrap();
        assert_eq!(lines.rows.len(), 2);
        assert!(parse_json("[1, 2]").is_err());
    }

    #[test]
    fn format_comes_from_the_extension() {
        assert_eq!(
            ImportFormat::from_path(Path::new("a.CSV")),
            Ok(ImportFormat::Csv)
        );
        assert_eq!(
            ImportFormat::from_path(Path::new("a.jsonl")),
            Ok(ImportFormat::Json)
        );
        assert!(ImportFormat::from_path(Path::new("a.xlsx")).is_err());
    }
}
