//! Headless `--import <FILE> --table <TABLE>` mode for the `Rust-SQL` binary.
//!
//! Parses a CSV or JSON file, prints a preview and the column mapping to stderr, and inserts
//! every row through prepared statements in one transaction. Source columns map to target
//! columns by name (case-insensitive); unmatched source columns are skipped and listed.
//! Imports of 10,000 rows or more require `--yes`.

use crate::cli_batch::connect_db;
use rust_sql::adapters::create_server;
use rust_sql::common::types::Server;
use rust_sql::core::import::{auto_map, parse, ImportRequest, PREVIEW_ROWS};
use rust_sql::core::params::ParamValue;
use std::path::Path;

/// Options for one `--import` run.
pub(crate) struct ImportArgs<'a> {
    /// File to import (`.csv`, `.tsv`, `.json`, `.jsonl`).
    pub file: &'a str,
    /// Target table.
    pub table: &'a str,
    /// Target schema.
    pub schema: Option<&'a str>,
    /// `--yes`: confirms a large import.
    pub confirmed: bool,
    /// `--read-only` was given (imports are refused).
    pub read_only: bool,
    /// CSV delimiter from the config.
    pub csv_delimiter: &'a str,
    /// `--trust-host-key` fingerprint.
    pub trust_host_key: Option<&'a str>,
}

async fn import(server: Server, args: &ImportArgs<'_>) -> Result<usize, String> {
    if args.read_only {
        return Err("Import is a write; it cannot run with --read-only".into());
    }
    let delim = args
        .csv_delimiter
        .as_bytes()
        .first()
        .copied()
        .unwrap_or(b',');
    let data = parse(Path::new(args.file), delim)?;
    eprintln!(
        "Read {} rows with columns: {}",
        data.rows.len(),
        data.columns.join(", ")
    );
    for row in data.preview(PREVIEW_ROWS.min(5)) {
        eprintln!(
            "  {}",
            row.iter()
                .map(ParamValue::display_text)
                .collect::<Vec<_>>()
                .join(" | ")
        );
    }

    let mut session = create_server(server.clone()).map_err(|e| e.to_string())?;
    let conn = session.create_connection(server.database.as_deref());
    connect_db(conn, args.trust_host_key).await?;
    let targets: Vec<String> = conn
        .list_table_columns(args.table, args.schema)
        .await
        .map_err(|e| e.to_string())?
        .into_iter()
        .map(|c| c.column_name)
        .collect();
    let mappings = auto_map(&data.columns, &targets);
    for m in &mappings {
        eprintln!("  {} -> {}", data.columns[m.source], m.target);
    }
    let skipped: Vec<&str> = (0..data.columns.len())
        .filter(|i| !mappings.iter().any(|m| m.source == *i))
        .map(|i| data.columns[i].as_str())
        .collect();
    if !skipped.is_empty() {
        eprintln!(
            "Skipping columns with no match in {}: {}",
            args.table,
            skipped.join(", ")
        );
    }
    if data.requires_confirmation() && !args.confirmed {
        return Err(format!(
            "{} rows is a large import. Re-run with --yes to confirm.",
            data.rows.len()
        ));
    }
    let report = conn
        .import_rows(ImportRequest {
            table: args.table,
            schema: args.schema,
            mappings: &mappings,
            data: &data,
            confirmed: args.confirmed,
        })
        .await
        .map_err(|e| e.to_string())?;
    Ok(report.rows_inserted)
}

/// Runs `--import` against `server` and exits non-zero on failure.
pub(crate) fn run_import_cli(server: Server, args: ImportArgs<'_>) {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("tokio runtime");
    match rt.block_on(import(server, &args)) {
        Ok(n) => println!("Imported {n} rows into {}", args.table),
        Err(e) => {
            eprintln!("Import error: {e}");
            std::process::exit(1);
        }
    }
}
