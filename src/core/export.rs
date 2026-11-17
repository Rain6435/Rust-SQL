use crate::adapters::wrap_identifier_for_adapter;
use crate::common::types::{OpenedQueryInfo, SavedQueryInfo};
use crate::common::utils::value_to_string;
use crate::core::params::format_sql_literal;
use serde_json::{Map, Value};
use std::path::Path;
use std::sync::{Mutex, OnceLock};

static CLIPBOARD_BUFFER: OnceLock<Mutex<String>> = OnceLock::new();

fn clipboard_store() -> &'static Mutex<String> {
    CLIPBOARD_BUFFER.get_or_init(|| Mutex::new(String::new()))
}

/// Converts query result rows to CSV format with a header row (`stringifyResultToCSV` in `src/browser/core/db.ts`).
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

/// Converts query result rows to formatted JSON (`JSON.stringify(rows, null, 2)`).
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

pub fn export_query_result_to_string(
    rows: &[Map<String, Value>],
    export_type: &str,
    delimiter: Option<&str>,
) -> Result<String, String> {
    match export_type.to_uppercase().as_str() {
        "CSV" => {
            let delim = delimiter.filter(|d| !d.is_empty()).unwrap_or(",");
            stringify_result_to_csv(rows, delim)
        }
        "JSONL" | "NDJSON" => stringify_result_to_jsonl(rows),
        "MD" | "MARKDOWN" => stringify_result_to_markdown(rows),
        "SQL" => {
            stringify_result_to_sql_insert(rows, delimiter.unwrap_or("export_table"), "postgresql")
        }
        _ => stringify_result_to_json(rows),
    }
}

pub fn export_query_result_to_file_path(
    rows: &[Map<String, Value>],
    export_type: &str,
    delimiter: Option<&str>,
    filename: &str,
) -> Result<String, String> {
    let value = export_query_result_to_string(rows, export_type, delimiter)?;
    let ext_raw = match export_type.to_uppercase().as_str() {
        "MARKDOWN" => "md".to_string(),
        other => other.to_lowercase(),
    };
    let ext = format!(".{ext_raw}");
    let final_path = if filename.to_lowercase().ends_with(&ext) {
        filename.to_string()
    } else {
        format!("{filename}{ext}")
    };

    if let Some(parent) = Path::new(&final_path).parent() {
        if !parent.as_os_str().is_empty() {
            let _ = std::fs::create_dir_all(parent);
        }
    }
    std::fs::write(&final_path, value).map_err(|e| e.to_string())?;
    Ok(final_path)
}

pub fn write_clipboard_text(text: &str) {
    if let Ok(mut guard) = clipboard_store().lock() {
        *guard = text.to_string();
    }
    for (cmd, args) in [
        ("wl-copy", &[][..]),
        ("xclip", &["-selection", "clipboard"][..]),
        ("xsel", &["--clipboard", "--input"][..]),
    ] {
        if let Ok(mut child) = std::process::Command::new(cmd)
            .args(args)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
        {
            if let Some(mut stdin) = child.stdin.take() {
                use std::io::Write;
                let _ = stdin.write_all(text.as_bytes());
            }
            let _ = child.wait();
            break;
        }
    }
}

pub fn read_clipboard_text() -> String {
    clipboard_store()
        .lock()
        .map(|g| g.clone())
        .unwrap_or_default()
}

pub fn export_query_result_to_clipboard(
    rows: &[Map<String, Value>],
    export_type: &str,
    delimiter: Option<&str>,
) -> Result<String, String> {
    let value = export_query_result_to_string(rows, export_type, delimiter)?;
    write_clipboard_text(&value);
    Ok(value)
}

pub fn save_query_to_file(filename: &str, query: &str) -> Result<SavedQueryInfo, String> {
    let final_path = if Path::new(filename)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.eq_ignore_ascii_case("sql"))
        .unwrap_or(false)
    {
        filename.to_string()
    } else {
        format!("{filename}.sql")
    };

    if let Some(parent) = Path::new(&final_path).parent() {
        if !parent.as_os_str().is_empty() {
            let _ = std::fs::create_dir_all(parent);
        }
    }

    std::fs::write(&final_path, query).map_err(|e| e.to_string())?;
    let name = Path::new(&final_path)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("query")
        .to_string();

    Ok(SavedQueryInfo {
        name,
        filename: final_path,
    })
}

pub fn open_query_from_file(filename: &str) -> Result<OpenedQueryInfo, String> {
    let query = std::fs::read_to_string(filename).map_err(|e| e.to_string())?;
    let name = Path::new(filename)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("query")
        .to_string();

    Ok(OpenedQueryInfo {
        name,
        query,
        filename: filename.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stringify_csv_json_markdown_sql_jsonl() {
        let mut row1 = Map::new();
        row1.insert("id".to_string(), Value::from(1));
        row1.insert("name".to_string(), Value::from("Alice"));
        row1.insert("bio".to_string(), Value::Null);
        let rows = vec![row1];

        let csv_out = stringify_result_to_csv(&rows, ",").unwrap();
        assert_eq!(csv_out, "bio,id,name\nNULL,1,Alice\n");

        let tab_out = stringify_result_to_csv(&rows, "\t").unwrap();
        assert_eq!(tab_out, "bio\tid\tname\nNULL\t1\tAlice\n");

        let json_out = stringify_result_to_json(&rows).unwrap();
        assert!(json_out.contains("\"Alice\""));

        let jsonl_out = stringify_result_to_jsonl(&rows).unwrap();
        assert_eq!(jsonl_out.lines().count(), 1);

        let md_out = stringify_result_to_markdown(&rows).unwrap();
        assert!(md_out.contains("| bio | id | name |"));
        assert!(md_out.contains("| NULL | 1 | Alice |"));

        let sql_out = stringify_result_to_sql_insert(&rows, "users", "postgresql").unwrap();
        assert_eq!(
            sql_out,
            "INSERT INTO \"users\" (\"bio\", \"id\", \"name\") VALUES (NULL, 1, 'Alice');\n"
        );
    }
}
