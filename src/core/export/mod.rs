//! Query result export to files, strings, and system clipboard, plus `.sql` file persistence.

pub mod formats;

pub use formats::{
    stringify_result_to_csv, stringify_result_to_json, stringify_result_to_jsonl,
    stringify_result_to_markdown, stringify_result_to_sql_insert,
};

use crate::common::types::{OpenedQueryInfo, SavedQueryInfo};
use serde_json::{Map, Value};
use std::path::Path;
use std::sync::{Mutex, OnceLock};

static CLIPBOARD_BUFFER: OnceLock<Mutex<String>> = OnceLock::new();

fn clipboard_store() -> &'static Mutex<String> {
    CLIPBOARD_BUFFER.get_or_init(|| Mutex::new(String::new()))
}

/// Serializes `rows` into the requested `export_type` (`CSV`, `JSON`, `JSONL`, `MARKDOWN`, or `SQL`).
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

/// Exports `rows` in `export_type` format to `filename` on disk, automatically appending the appropriate file extension.
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

/// Writes `text` to the in-memory clipboard buffer and any available system clipboard utility (`wl-copy`, `xclip`, `xsel`).
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

/// Reads the most recently written text from the in-memory clipboard buffer.
pub fn read_clipboard_text() -> String {
    clipboard_store()
        .lock()
        .map(|g| g.clone())
        .unwrap_or_default()
}

/// Formats `rows` as `export_type` and copies the result to the clipboard.
pub fn export_query_result_to_clipboard(
    rows: &[Map<String, Value>],
    export_type: &str,
    delimiter: Option<&str>,
) -> Result<String, String> {
    let value = export_query_result_to_string(rows, export_type, delimiter)?;
    write_clipboard_text(&value);
    Ok(value)
}

/// Saves a SQL `query` string to `filename` (ensuring a `.sql` extension) and returns [`SavedQueryInfo`].
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

/// Reads a `.sql` query file from `filename` and returns [`OpenedQueryInfo`].
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
