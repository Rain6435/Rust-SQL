use rust_sql::adapters::{create_server, set_select_limit};
use rust_sql::common::types::{ListTableColumnsResult, Server, TableKeysResult};
use rust_sql::core::config::{get_full_sync, DEFAULT_CRYPTO_SECRET};
use rust_sql::core::connection_string::parse_connection_uri;
use rust_sql::core::export::export_query_result_to_string;
use rust_sql::core::result_ops::generate_mermaid_erd;
use rust_sql::core::safety::analyze_query_safety;
use rust_sql::core::servers;
use rust_sql::gui::run_native_gui;
use rust_sql::tui::run_tui;
use rust_sql::web::run_web_server;
use std::collections::HashMap;

fn main() {
    let _ = env_logger::try_init();
    let args: Vec<String> = std::env::args().collect();

    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Rust-SQL v{}", env!("CARGO_PKG_VERSION"));
        println!("A fast, lightweight SQL client with cross database and platform support.\n");
        println!("Usage: Rust-SQL [OPTIONS]\n");
        println!("Options:");
        println!("  --gui                    Launch native Desktop GUI (default when DISPLAY/WAYLAND is set)");
        println!("  --tui, --term            Launch Terminal UI");
        println!("  --web                    Launch Web UI & IPC Server");
        println!("  --port <PORT>            Port for Web UI server (default: 8080)");
        println!("  -e, --exec <SQL>         Execute SQL query in headless batch mode and exit");
        println!("  --uri <CONN_URI>         Connection URI for batch execution or ERD generation");
        println!("  --server <NAME_OR_ID>    Saved server name or ID for batch execution");
        println!("  --format <FMT>           Output format: json, jsonl, csv, markdown, sql (default: json)");
        println!("  --read-only              Enforce Read-Only safety guardrail");
        println!("  --erd                    Generate and print Mermaid ER Diagram for the target database");
        println!("  --dev                    Enable development mode and console logging");
        println!("  -v, --version            Print version and exit");
        println!("  -h, --help               Print help information");
        return;
    }

    let config_data = get_full_sync(false).unwrap_or_default();

    if config_data.print_version == Some(true) || args.iter().any(|a| a == "--version" || a == "-v")
    {
        println!("{} {}", config_data.name, config_data.version);
        return;
    }

    set_select_limit(config_data.limit_query_default_select_top);

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
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("tokio runtime");

        let target_server: Server = if let Some(ref uri) = conn_uri {
            parse_connection_uri(uri, None).unwrap_or_else(|| {
                eprintln!("Invalid connection URI: {uri}");
                std::process::exit(1);
            })
        } else if let Some(ref key) = server_key {
            let secret = config_data
                .crypto
                .as_ref()
                .map(|c| c.secret.as_str())
                .unwrap_or(DEFAULT_CRYPTO_SECRET);
            let found = config_data
                .servers
                .iter()
                .find(|s| s.id == *key || s.name == *key)
                .cloned()
                .unwrap_or_else(|| {
                    eprintln!("Server not found: {key}");
                    std::process::exit(1);
                });
            servers::decrypt_secrets(&found, secret).unwrap_or(found)
        } else {
            Server {
                name: "sqlite-batch".to_string(),
                client: "sqlite".to_string(),
                database: Some(":memory:".to_string()),
                ..Default::default()
            }
        };

        if want_erd {
            let erd_res: Result<String, String> = rt.block_on(async {
                let mut session =
                    create_server(target_server.clone()).map_err(|e| e.to_string())?;
                let db_conn = session.create_connection(target_server.database.as_deref());
                db_conn.connect().await.map_err(|e| e.to_string())?;
                let tables = db_conn.list_tables(None).await.map_err(|e| e.to_string())?;
                let mut cols_map: HashMap<String, Vec<ListTableColumnsResult>> = HashMap::new();
                let mut keys_map: HashMap<String, Vec<TableKeysResult>> = HashMap::new();
                for t in tables {
                    let cols = db_conn
                        .list_table_columns(&t.name, t.schema.as_deref())
                        .await
                        .unwrap_or_default();
                    let keys = db_conn
                        .get_table_keys(&t.name, t.schema.as_deref())
                        .await
                        .unwrap_or_default();
                    cols_map.insert(t.name.clone(), cols);
                    keys_map.insert(t.name, keys);
                }
                Ok(generate_mermaid_erd(&cols_map, &keys_map))
            });
            match erd_res {
                Ok(erd) => print!("{erd}"),
                Err(e) => {
                    eprintln!("ERD error: {e}");
                    std::process::exit(1);
                }
            }
            return;
        }

        if let Some(sql) = exec_sql {
            let safety = analyze_query_safety(&sql, read_only);
            if read_only && !safety.is_read_only_safe {
                eprintln!(
                    "Blocked by Read-Only mode: {}",
                    safety
                        .findings
                        .first()
                        .map(|f| f.reason.as_str())
                        .unwrap_or("Modification statement not allowed")
                );
                std::process::exit(1);
            }

            let res = rt.block_on(async {
                let mut session =
                    create_server(target_server.clone()).map_err(|e| e.to_string())?;
                let db_conn = session.create_connection(target_server.database.as_deref());
                db_conn.connect().await.map_err(|e| e.to_string())?;
                db_conn.execute_query(&sql).await.map_err(|e| e.to_string())
            });

            match res {
                Ok(results) => {
                    for r in results {
                        if !r.rows.is_empty() {
                            if let Ok(formatted) = export_query_result_to_string(
                                &r.rows,
                                &format_type,
                                Some(&config_data.csv_delimiter),
                            ) {
                                print!("{formatted}");
                            }
                        } else if format_type.eq_ignore_ascii_case("json") {
                            println!("{}", serde_json::to_string_pretty(&r).unwrap_or_default());
                        }
                    }
                }
                Err(e) => {
                    eprintln!("Query error: {e}");
                    std::process::exit(1);
                }
            }
            return;
        }
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

    let has_display = std::env::var("DISPLAY")
        .map(|s| !s.is_empty())
        .unwrap_or(false)
        || std::env::var("WAYLAND_DISPLAY")
            .map(|s| !s.is_empty())
            .unwrap_or(false);

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
