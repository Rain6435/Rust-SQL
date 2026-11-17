//! Format-specific serializers for query result rows (`CSV`, `JSON`, `JSONL`, `Markdown`, and `SQL INSERT`).

use crate::adapters::wrap_identifier_for_adapter;
use crate::common::utils::value_to_string;
use crate::core::params::format_sql_literal;
use serde_json::{Map, Value};

/// Converts query result rows to CSV format with a header row using the given `delimiter`.
pub fn stringify_result_to_csv(
    rows: &[Map<String, Value>],
    delimiter: &str,
) -> Result<String, String> {
    if rows.is_empty() {
        return Ok(String::new());
    }

    let delim_byte = delimiter.as_bytes().first().copied().unwrap_or(b',');
    let mut wtr = csv::WriterBuilder::new()
        .delimiter(delim_byte)
        .from_writer(vec![]);

    let columns: Vec<String> = rows[0].keys().cloned().collect();
    wtr.write_record(&columns).map_err(|e| e.to_string())?;

    for row in rows {
        let record: Vec<String> = columns
            .iter()
            .map(|col| {
                row.get(col)
                    .map(value_to_string)
                    .unwrap_or_else(|| "NULL".to_string())
            })
            .collect();
        wtr.write_record(&record).map_err(|e| e.to_string())?;
    }

    let bytes = wtr.into_inner().map_err(|e| e.to_string())?;
    String::from_utf8(bytes).map_err(|e| e.to_string())
}

/// Converts query result rows to pretty-printed JSON.
pub fn stringify_result_to_json(rows: &[Map<String, Value>]) -> Result<String, String> {
    serde_json::to_string_pretty(rows).map_err(|e| e.to_string())
}

/// Converts query result rows to newline-delimited JSON (`JSONL` / `NDJSON`).
pub fn stringify_result_to_jsonl(rows: &[Map<String, Value>]) -> Result<String, String> {
    let mut out = String::new();
    for row in rows {
        let line = serde_json::to_string(row).map_err(|e| e.to_string())?;
        out.push_str(&line);
        out.push('\n');
    }
    Ok(out)
}

/// Converts query result rows to a GitHub-Flavored Markdown table.
pub fn stringify_result_to_markdown(rows: &[Map<String, Value>]) -> Result<String, String> {
    if rows.is_empty() {
        return Ok(String::new());
    }
    let columns: Vec<String> = rows[0].keys().cloned().collect();
    let mut out = String::new();
    out.push_str("| ");
    out.push_str(&columns.join(" | "));
    out.push_str(" |\n| ");
    out.push_str(
        &columns
            .iter()
            .map(|_| "---")
            .collect::<Vec<_>>()
            .join(" | "),
    );
    out.push_str(" |\n");

    for row in rows {
        let cells: Vec<String> = columns
            .iter()
            .map(|col| {
                row.get(col)
                    .map(value_to_string)
                    .unwrap_or_else(|| "NULL".to_string())
                    .replace('|', "\\|")
                    .replace('\n', " ")
            })
            .collect();
        out.push_str("| ");
        out.push_str(&cells.join(" | "));
        out.push_str(" |\n");
    }

    Ok(out)
}

/// Converts query result rows into SQL `INSERT INTO` statements with dialect-specific identifier quoting.
pub fn stringify_result_to_sql_insert(
    rows: &[Map<String, Value>],
    table_name: &str,
    adapter_key: &str,
) -> Result<String, String> {
    if rows.is_empty() {
        return Ok(String::new());
    }
    let tbl = if table_name.trim().is_empty() {
        "export_table"
    } else {
        table_name.trim()
    };
    let wrapped_tbl = wrap_identifier_for_adapter(adapter_key, tbl);
    let columns: Vec<String> = rows[0].keys().cloned().collect();
    let wrapped_cols = columns
        .iter()
        .map(|c| wrap_identifier_for_adapter(adapter_key, c))
        .collect::<Vec<_>>()
        .join(", ");

    let mut out = String::new();
    for row in rows {
        let vals = columns
            .iter()
            .map(|col| format_sql_literal(row.get(col).unwrap_or(&Value::Null)))
            .collect::<Vec<_>>()
            .join(", ");
        out.push_str(&format!(
            "INSERT INTO {wrapped_tbl} ({wrapped_cols}) VALUES ({vals});\n"
        ));
    }
    Ok(out)
}
