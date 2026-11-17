//! Binary entry point for `Rust-SQL` supporting Desktop GUI (`--gui`), Terminal UI (`--tui`),
//! Web UI (`--web`), Mermaid ERD generation (`--erd`), and headless batch SQL execution (`--exec`).

mod cli_batch;

use cli_batch::{resolve_batch_server, run_headless_batch};
use rust_sql::adapters::{set_emulation_enabled, set_select_limit};
use rust_sql::core::config::get_full_sync;
use rust_sql::core::updater::run_update_cli;
use rust_sql::gui::run_native_gui;
use rust_sql::tui::run_tui;
use rust_sql::web::run_web_server;

/// Prints CLI usage and flags to stdout.
fn print_help() {
    println!(
        "Rust-SQL v{}
A fast, lightweight SQL client with cross database and platform support.

Usage: Rust-SQL [OPTIONS]

Options:
  --gui                    Launch native Desktop GUI (default when DISPLAY/WAYLAND is set)
  --tui, --term            Launch Terminal UI
  --web                    Launch Web UI & IPC Server
  --port <PORT>            Port for Web UI server (default: 8080)
  -e, --exec <SQL>         Execute SQL query in headless batch mode and exit
  --uri <CONN_URI>         Connection URI for batch execution or ERD generation
  --server <NAME_OR_ID>    Saved server name or ID for batch execution
  --format <FMT>           Output format: json, jsonl, csv, markdown, sql (default: json)
  --read-only              Enforce Read-Only safety guardrail
  --emulate                Allow MySQL, MariaDB, SQL Server and Cassandra to run on an
                           in-memory emulator (no live driver yet). Results are NOT from
                           your server and are labeled EMULATED in every front end.
  --trust-host-key <FP>    Trust an unknown SSH bastion host key in batch mode, only if
                           its SHA256 fingerprint equals <FP> (from your administrator)
  --erd                    Generate and print Mermaid ER Diagram for the target database
  --check-update           Check GitHub Releases for a newer version of Rust-SQL
  --update                 Download, verify SHA-256, and self-update Rust-SQL
  --dev                    Enable development mode and console logging
  -v, --version            Print version and exit
  -h, --help               Print help information",
        env!("CARGO_PKG_VERSION")
    );
}

/// Main CLI dispatcher for `Rust-SQL`.
fn main() {
    let _ = env_logger::try_init();
    let args: Vec<String> = std::env::args().collect();

    if args.iter().any(|a| a == "--help" || a == "-h") {
        print_help();
        return;
    }

    let want_check_update = args.iter().any(|a| a == "--check-update");
    let want_update = args.iter().any(|a| a == "--update");
    if want_check_update || want_update {
        if let Err(e) = run_update_cli(want_update) {
            eprintln!("Update error: {e}");
            std::process::exit(1);
        }
        return;
    }

    let config_data = get_full_sync(false).unwrap_or_default();
    if config_data.print_version == Some(true) || args.iter().any(|a| a == "--version" || a == "-v")
    {
        println!("{} {}", config_data.name, config_data.version);
        return;
    }

    set_select_limit(config_data.limit_query_default_select_top);
    set_emulation_enabled(
        args.iter().any(|a| a == "--emulate") || config_data.emulate_unsupported_dialects,
    );

    let find_arg = |flag: &str, short: Option<&str>| -> Option<String> {
        for i in 0..args.len() {
            if args[i] == flag || short.map(|s| args[i] == s).unwrap_or(false) {
                return args.get(i + 1).cloned();
            }
        }
        None
    };

    let exec_sql = find_arg("--exec", Some("-e"));
    let conn_uri = find_arg("--uri", None);
    let server_key = find_arg("--server", None);
    let format_type = find_arg("--format", None).unwrap_or_else(|| "json".to_string());
    let read_only = args.iter().any(|a| a == "--read-only");
    let want_erd = args.iter().any(|a| a == "--erd");

    if exec_sql.is_some() || want_erd {
        let srv = resolve_batch_server(conn_uri.as_deref(), server_key.as_deref(), &config_data);
        run_headless_batch(
            srv,
            exec_sql,
            want_erd,
            read_only,
            &format_type,
            &config_data.csv_delimiter,
            find_arg("--trust-host-key", None).as_deref(),
        );
        return;
    }

    let want_tui = args.iter().any(|a| a == "--tui" || a == "--term");
    let want_web = args.iter().any(|a| a == "--web");
    let want_gui = args.iter().any(|a| a == "--gui");
    let port = find_arg("--port", None)
        .and_then(|s| s.parse::<u16>().ok())
        .unwrap_or(8080);

    if want_tui {
        if let Err(e) = run_tui() {
            eprintln!("TUI error: {e}");
            std::process::exit(1);
        }
        return;
    }

    if want_web {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("tokio runtime");
        if let Err(e) = rt.block_on(run_web_server(port)) {
            eprintln!("Web server error: {e}");
            std::process::exit(1);
        }
        return;
    }

    let has_display = ["DISPLAY", "WAYLAND_DISPLAY"]
        .iter()
        .any(|k| std::env::var(k).map(|s| !s.is_empty()).unwrap_or(false));

    if want_gui || has_display {
        if let Err(e) = run_native_gui() {
            eprintln!("Desktop GUI error: {e}");
            std::process::exit(1);
        }
    } else if let Err(e) = run_tui() {
        log::warn!("TUI fallback failed ({e}), starting Web server on port {port}");
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("tokio runtime");
        let _ = rt.block_on(run_web_server(port));
    }
}
