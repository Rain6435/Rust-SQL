//! Emulation is opt-in only: without it, dialects that have no live driver refuse to connect;
//! with it, every connection reports `ConnectionMode::Emulated` and the EMULATED banner.
//!
//! The opt-in is process-wide, so the whole sequence runs in one test to avoid races.

use rust_sql::adapters::{create_server, emulation_enabled, set_emulation_enabled};
use rust_sql::common::event::{DB_CONNECT, DB_GET_CONNECTION_MODE};
use rust_sql::common::types::{ConnectionMode, Server, EMULATED_BANNER};
use rust_sql::web::{dispatch_ipc_event, IpcRequest, WebIpcState};
use serde_json::json;
use std::path::Path;

fn server(client: &str) -> Server {
    Server {
        name: client.into(),
        client: client.into(),
        host: Some("db.example.test".into()),
        port: Some(3306),
        database: Some("rust_sql".into()),
        ..Default::default()
    }
}

async fn ipc(state: &WebIpcState, event: &str, args: Vec<serde_json::Value>) -> serde_json::Value {
    let resp = dispatch_ipc_event(
        state,
        IpcRequest {
            event: event.to_string(),
            args,
        },
    )
    .await;
    serde_json::to_value(resp).expect("serialize response")
}

#[tokio::test]
async fn emulation_requires_explicit_opt_in_and_is_always_labeled() {
    assert!(!emulation_enabled(), "emulation must be off by default");

    for client in ["mysql", "mariadb", "sqlserver", "cassandra"] {
        let mut session = create_server(server(client)).expect("create_server");
        let db_conn = session.create_connection(Some("rust_sql"));
        let msg = db_conn
            .connect()
            .await
            .expect_err("must refuse")
            .to_string();
        assert!(msg.contains("not implemented"), "{client}: {msg}");
        assert!(msg.contains("--emulate"), "{client}: {msg}");
        assert!(db_conn.execute_query("SELECT * FROM users").await.is_err());
    }

    let state = WebIpcState::default();
    let resp = ipc(
        &state,
        DB_CONNECT,
        vec![json!(server("mysql")), json!("rust_sql")],
    )
    .await;
    assert_eq!(resp["ok"], false, "{resp}");
    assert!(resp["error"]
        .as_str()
        .unwrap_or("")
        .contains("not implemented"));

    set_emulation_enabled(true);

    for client in ["mysql", "mariadb", "sqlserver", "cassandra"] {
        let mut session = create_server(server(client)).expect("create_server");
        let db_conn = session.create_connection(Some("rust_sql"));
        db_conn.connect().await.expect("emulated connect");
        assert_eq!(
            db_conn.connection_mode().expect("mode"),
            ConnectionMode::Emulated
        );
    }

    let resp = ipc(
        &state,
        DB_CONNECT,
        vec![json!(server("mysql")), json!("rust_sql")],
    )
    .await;
    assert_eq!(resp["ok"], true, "{resp}");
    assert_eq!(resp["data"]["mode"], "emulated");
    assert_eq!(resp["data"]["banner"], EMULATED_BANNER);
    let resp = ipc(&state, DB_GET_CONNECTION_MODE, vec![json!("rust_sql")]).await;
    assert_eq!(resp["data"]["mode"], "emulated");

    let sqlite = Server {
        name: "sqlite".into(),
        client: "sqlite".into(),
        database: Some(":memory:".into()),
        ..Default::default()
    };
    let live_state = WebIpcState::default();
    let resp = ipc(
        &live_state,
        DB_CONNECT,
        vec![json!(sqlite), json!(":memory:")],
    )
    .await;
    assert_eq!(resp["data"]["mode"], "live", "{resp}");
    assert!(resp["data"]["banner"].is_null());

    set_emulation_enabled(false);
}

#[test]
fn banner_text_matches_the_specification() {
    assert_eq!(
        EMULATED_BANNER,
        "EMULATED \u{2014} results are not from your server."
    );
}

fn assert_no_hostname_hook(dir: &Path) {
    for entry in std::fs::read_dir(dir).expect("read_dir") {
        let path = entry.expect("entry").path();
        if path.is_dir() {
            assert_no_hostname_hook(&path);
        } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
            let src = std::fs::read_to_string(&path).expect("read");
            for needle in [
                "starts_with(\"invalid.\")",
                "\"255.255.255.255\"",
                "pg_sleep\")",
            ] {
                assert!(
                    !src.contains(needle),
                    "{} contains {needle}",
                    path.display()
                );
            }
        }
    }
}

/// The old hostname-based failure hook and the latency heuristic must not exist in `src/`.
#[test]
fn production_code_has_no_test_hooks() {
    assert_no_hostname_hook(&Path::new(env!("CARGO_MANIFEST_DIR")).join("src"));
}
