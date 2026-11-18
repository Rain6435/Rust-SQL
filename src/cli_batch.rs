//! Headless `--exec` and `--erd` batch mode for the `Rust-SQL` binary.
//!
//! When a connection is emulated, the EMULATED warning is printed to stderr before any output,
//! so piped stdout stays machine-readable while the terminal still shows the warning.

use rust_sql::adapters::{create_server, DatabaseConnection};
use rust_sql::common::types::{
    Config, ConnectionMode, ListTableColumnsResult, Server, TableKeysResult, EMULATED_BANNER,
};
use rust_sql::core::connection_string::parse_connection_uri;
use rust_sql::core::errors::DbCoreError;
use rust_sql::core::export::export_query_result_to_string;
use rust_sql::core::keys::system_key_provider;
use rust_sql::core::result_ops::generate_mermaid_erd;
use rust_sql::core::safety::{analyze_script, SqlDialect};
use rust_sql::core::servers;
use rust_sql::core::tunnel::approve_host_key;
use std::collections::HashMap;

/// Connects `db_conn`. An unknown or expiring SSH bastion key is trusted only when it matches
/// the fingerprint the user passed with `--trust-host-key`; a changed key always fails.
pub(crate) async fn connect_db(
    db_conn: &mut DatabaseConnection,
    trust: Option<&str>,
) -> Result<(), String> {
    let err = match db_conn.connect().await {
        Ok(()) => return Ok(()),
        Err(e) => e,
    };
    let DbCoreError::Tunnel(t) = &err else {
        return Err(err.to_string());
    };
    match (t.host_key_challenge(), trust) {
        (Some(ch), Some(fp)) if ch.fingerprint_sha256 == fp => {
            approve_host_key(ch, &*db_conn.keys.0).map_err(|e| e.to_string())?;
            db_conn.connect().await.map_err(|e| e.to_string())
        }
        (Some(ch), _) => Err(format!(
            "{t}\nIf your administrator confirms this fingerprint, re-run with \
             --trust-host-key {}",
            ch.fingerprint_sha256
        )),
        _ => Err(t.to_string()),
    }
}

/// Prints the EMULATED warning to stderr when `db_conn` is not a live connection.
fn warn_if_emulated(db_conn: &DatabaseConnection) {
    if matches!(db_conn.connection_mode(), Ok(ConnectionMode::Emulated)) {
        eprintln!("{EMULATED_BANNER}");
    }
}

/// Resolves the target [`Server`] for headless `--exec` or `--erd` mode from `--uri`, `--server`, or an in-memory SQLite fallback.
pub(crate) fn resolve_batch_server(
    conn_uri: Option<&str>,
    server_key: Option<&str>,
    config_data: &Config,
) -> Server {
    if let Some(uri) = conn_uri {
        parse_connection_uri(uri, None).unwrap_or_else(|| {
            eprintln!("Invalid connection URI: {uri}");
            std::process::exit(1);
        })
    } else if let Some(key) = server_key {
        let found = config_data
            .servers
            .iter()
            .find(|s| s.id == key || s.name == key)
            .cloned()
            .unwrap_or_else(|| {
                eprintln!("Server not found: {key}");
                std::process::exit(1);
            });
        servers::decrypt_secrets(&found, &*system_key_provider()).unwrap_or_else(|e| {
            eprintln!("Cannot use saved server {key}: {e}");
            std::process::exit(1);
        })
    } else {
        Server {
            name: "sqlite-batch".to_string(),
            client: "sqlite".to_string(),
            database: Some(":memory:".to_string()),
            ..Default::default()
        }
    }
}

/// Executes headless `--erd` or `--exec` batch commands and exits.
pub(crate) fn run_headless_batch(
    target_server: Server,
    exec_sql: Option<String>,
    want_erd: bool,
    read_only: bool,
    format_type: &str,
    csv_delimiter: &str,
    trust_host_key: Option<&str>,
) {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("tokio runtime");

    if want_erd {
        let erd_res: Result<String, String> = rt.block_on(async {
            let mut session = create_server(target_server.clone()).map_err(|e| e.to_string())?;
            let db_conn = session.create_connection(target_server.database.as_deref());
            connect_db(db_conn, trust_host_key).await?;
            warn_if_emulated(db_conn);
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
        let dialect = SqlDialect::from_adapter_key(&target_server.client);
        let safety = analyze_script(&sql, dialect, read_only);
        if safety.is_blocked() {
            eprintln!(
                "Blocked by Read-Only mode: {}",
                safety
                    .blocked_reason()
                    .unwrap_or_else(|| "Modification statement not allowed".to_string())
            );
            std::process::exit(1);
        }

        let res = rt.block_on(async {
            let mut session = create_server(target_server.clone()).map_err(|e| e.to_string())?;
            let db_conn = session.create_connection(target_server.database.as_deref());
            db_conn.read_only = read_only;
            connect_db(db_conn, trust_host_key).await?;
            warn_if_emulated(db_conn);
            db_conn.execute_query(&sql).await.map_err(|e| e.to_string())
        });

        match res {
            Ok(results) => {
                for r in results {
                    if !r.rows.is_empty() {
                        if let Ok(formatted) =
                            export_query_result_to_string(&r.rows, format_type, Some(csv_delimiter))
                        {
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
    }
}
