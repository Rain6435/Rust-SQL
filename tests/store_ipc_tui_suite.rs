//! Integration tests for server CRUD, secret encryption, `AppStore`, Web IPC, TUI navigation,
//! and the repository-wide modularization (`<= 275` lines per file) and documentation (`//!`) invariant.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use rust_sql::common::event::*;
use rust_sql::common::types::{Password, Server, SslConfig};
use rust_sql::core::appdb::AppDb;
use rust_sql::core::config::{self, set_config_path_override};
use rust_sql::core::keys::{InMemoryKeyProvider, SharedKeyProvider};
use rust_sql::core::servers;
use rust_sql::gui::{ServerFormDraft, SettingsFormDraft};
use rust_sql::state::{AppStore, DbObjectType, ScriptActionType};
use rust_sql::tui::{DatabaseFocus, TuiApp, TuiRoute};
use rust_sql::web::{dispatch_ipc_event, IpcCtx, IpcRequest, WebApp, WebIpcState, WebSettings};
use std::path::Path;
use std::sync::Arc;
use tempfile::TempDir;

/// Verifies server CRUD, secret encryption, `AppStore` actions, read-only guardrails, parameter binding, ERD generation, and Web IPC.
#[tokio::test]
async fn test_servers_crud_state_store_ipc_and_tui() {
    let tmp = TempDir::new().expect("tempdir");
    let cfg_path = tmp.path().join("rust-sql.json");
    set_config_path_override(Some(cfg_path.clone()));

    let keys = Arc::new(InMemoryKeyProvider::new_for_tests());
    config::prepare_sync(&*keys).expect("prepare_sync");

    let new_srv = Server {
        id: String::new(),
        name: "My New Mysql Server".into(),
        client: "mysql".into(),
        ssl: Some(SslConfig::Bool(false)),
        host: Some("10.10.10.15".into()),
        port: Some(3306),
        database: Some("authentication".into()),
        user: Some("root".into()),
        password: Some(Password::Plain("password".into())),
        ..Default::default()
    };

    let added = servers::add(&new_srv, &*keys).await.expect("add server");
    assert!(added.validation_errors.is_none());
    let created = added.data.expect("created server");
    assert!(!created.id.is_empty());
    assert_eq!(created.encrypted, Some(true));

    let dec = servers::decrypt_secrets(&created, &*keys).expect("decrypt");
    assert_eq!(dec.password, Some(Password::Plain("password".into())));

    let draft = ServerFormDraft::from_server(Some(&dec));
    let converted = draft.to_server();
    assert_eq!(converted.name, "My New Mysql Server");
    assert_eq!(converted.client, "mysql");

    let mut store = AppStore::new();
    store.keys = SharedKeyProvider(keys.clone());
    store.load_config().expect("load_config");
    assert_eq!(store.servers.items.len(), 1);

    let settings_draft = SettingsFormDraft::from_store(&store);
    assert_eq!(settings_draft.zoom_factor, 1.0);

    let sqlite_srv = Server {
        id: String::new(),
        name: "sqlite-test".into(),
        client: "sqlite".into(),
        database: Some(":memory:".into()),
        ..Default::default()
    };
    let saved_sqlite = store.save_server(sqlite_srv, None).expect("save sqlite");
    store
        .connect(&saved_sqlite.id, None, false, None)
        .await
        .expect("connect sqlite");
    assert!(store.connections.connected);
    assert_eq!(store.queries.query_ids.len(), 1);

    store
        .execute_query_if_needed(
            "CREATE TABLE document (info TEXT); INSERT INTO document VALUES ('Ipsum 0'), ('Ipsum 1');",
            false,
        )
        .await
        .expect("create & insert");

    store
        .fetch_tables_if_needed(":memory:")
        .await
        .expect("fetch tables");
    let tables = store.tables.items_by_database.get(":memory:").unwrap();
    assert_eq!(tables.len(), 1);
    assert_eq!(tables[0].name, "document");

    let select_script = store
        .get_sql_script_if_needed(
            ":memory:",
            "document",
            ScriptActionType::Select,
            DbObjectType::Table,
            None,
        )
        .await
        .expect("get_sql_script_if_needed");
    assert_eq!(select_script, "SELECT \"info\" FROM \"document\";");

    let default_res = store
        .execute_default_select_query_if_needed(":memory:", "document", None)
        .await
        .expect("default select");
    assert_eq!(default_res.len(), 1);
    assert_eq!(default_res[0].rows.len(), 2);

    store.read_only_mode = true;
    let blocked = store
        .execute_query_if_needed("DELETE FROM document;", false)
        .await;
    assert!(blocked.is_err());
    store.read_only_mode = false;

    store
        .query_params
        .insert("target_info".to_string(), "Ipsum 1".to_string());
    let param_res = store
        .execute_query_if_needed("SELECT * FROM document WHERE info = :target_info;", false)
        .await
        .expect("parameterized select");
    assert_eq!(param_res[0].rows.len(), 1);
    store.query_params.clear();

    assert!(!store.bookmark_manager.history.is_empty());
    let erd = store
        .generate_database_erd(":memory:")
        .await
        .expect("generate_database_erd");
    assert!(erd.contains("erDiagram"));
    assert!(erd.contains("document"));

    let app = WebApp::new(
        AppDb::open(&tmp.path().join("rust-sql.db")).expect("app db"),
        keys.clone(),
        WebSettings::loopback_for_tests(8080),
    );
    let admin = app
        .db
        .create_user("admin", "correct horse battery", true)
        .expect("admin");
    let ipc_state = WebIpcState::new(4);
    let ctx = IpcCtx {
        app: &app,
        session: &ipc_state,
        user: &admin,
        source_ip: None,
    };
    let clients_resp = dispatch_ipc_event(
        &ctx,
        IpcRequest {
            event: DB_GET_CLIENTS_SYNC.to_string(),
            args: vec![],
        },
    )
    .await;
    assert!(clients_resp.ok);

    let servers_resp = dispatch_ipc_event(
        &ctx,
        IpcRequest {
            event: SERVERS_GET_ALL.to_string(),
            args: vec![],
        },
    )
    .await;
    assert!(servers_resp.ok);
    assert_eq!(servers_resp.data.unwrap().as_array().unwrap().len(), 2);

    set_config_path_override(None);
}

/// Verifies keyboard navigation between [`TuiRoute::ServerList`] and [`TuiRoute::ServerSave`].
#[test]
fn test_tui_navigation() {
    let mut tui = TuiApp {
        store: AppStore::new(),
        route: TuiRoute::ServerList,
        selected_server_idx: 0,
        selected_table_idx: 0,
        selected_db_idx: 0,
        db_focus: DatabaseFocus::TableList,
        form_draft: ServerFormDraft::from_server(None),
        form_field_idx: 0,
        should_quit: false,
    };
    assert_eq!(tui.route, TuiRoute::ServerList);
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    tui.handle_key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE), &rt);
    assert_eq!(tui.route, TuiRoute::ServerSave(None));
    tui.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE), &rt);
    assert_eq!(tui.route, TuiRoute::ServerList);
    assert_eq!(tui.db_focus, DatabaseFocus::TableList);
}

fn check_dir_rs_files(dir: &Path) {
    for entry in std::fs::read_dir(dir).expect("read_dir") {
        let entry = entry.expect("dir entry");
        let path = entry.path();
        if path.is_dir() {
            check_dir_rs_files(&path);
        } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
            let content = std::fs::read_to_string(&path).expect("read rs file");
            let line_count = content.lines().count();
            assert!(
                line_count <= 275,
                "File {} has {} lines, exceeding the 275-line maximum!",
                path.display(),
                line_count
            );
            assert!(
                content.trim_start().starts_with("//!"),
                "File {} is missing module-level `//!` documentation!",
                path.display()
            );
        }
    }
}

/// Enforces that every `.rs` file in `src/` and `tests/` is `<= 275` lines and starts with `//!` module docs.
#[test]
fn test_codebase_modularization_and_documentation_invariant() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    check_dir_rs_files(&manifest_dir.join("src"));
    check_dir_rs_files(&manifest_dir.join("tests"));
}
