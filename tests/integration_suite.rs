use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use rust_sql::adapters::{clear_select_limit, create_server, set_select_limit, ADAPTERS, CLIENTS};
use rust_sql::common::event::*;
use rust_sql::common::types::{
    FilterClause, FilterList, Password, SchemaFilter, Server, SslConfig,
};
use rust_sql::common::utils::{append_semi_colon, escape_regexp_string, titlize, version_compare};
use rust_sql::core::config::{self, set_config_path_override};
use rust_sql::core::servers;
use rust_sql::gui::{ServerFormDraft, SettingsFormDraft};
use rust_sql::state::{AppStore, DbObjectType, ScriptActionType};
use rust_sql::tui::{DatabaseFocus, TuiApp, TuiRoute};
use rust_sql::web::{dispatch_ipc_event, IpcRequest, WebIpcState};
use tempfile::TempDir;

#[test]
fn test_adapters_equal_clients() {
    assert_eq!(*ADAPTERS, *CLIENTS);
    assert_eq!(CLIENTS.len(), 7);
}

#[test]
fn test_string_and_regexp_utils() {
    assert_eq!(titlize("postgresql"), "Postgresql");
    assert_eq!(titlize("TABLE"), "Table");
    assert_eq!(
        escape_regexp_string("my-server (test) [1]"),
        r"my-server \(test\) \[1\]"
    );
    assert_eq!(version_compare("8.0.2", "8.0.1"), 1);
    assert_eq!(version_compare("8.0.2", "8.0.3"), -1);
    assert_eq!(version_compare("8.0.2", "8"), 0);
    assert_eq!(append_semi_colon("\ntest\n"), "test;");
}

#[tokio::test]
async fn test_all_7_adapters_db_spec() {
    let adapters = [
        ("sqlite", None),
        ("postgresql", Some("public")),
        ("redshift", Some("public")),
        ("mysql", None),
        ("mariadb", None),
        ("sqlserver", Some("dbo")),
        ("cassandra", None),
    ];

    for (adapter_key, db_schema) in adapters {
        let srv_cfg = Server {
            name: adapter_key.to_string(),
            client: adapter_key.to_string(),
            adapter: Some(adapter_key.to_string()),
            host: Some("localhost".to_string()),
            port: Some(5432),
            database: Some(if adapter_key == "sqlite" {
                ":memory:".to_string()
            } else {
                "rust_sql".to_string()
            }),
            ssl: Some(SslConfig::Bool(false)),
            ..Default::default()
        };

        let mut server_session = create_server(srv_cfg.clone()).expect("create_server");
        let db_conn = server_session.create_connection(srv_cfg.database.as_deref());
        db_conn.connect().await.expect("connect");

        // Setup schema for SQLite in-memory to match the standard test fixtures
        if adapter_key == "sqlite" {
            db_conn
                .execute_query(
                    "
                    CREATE TABLE roles (id INTEGER PRIMARY KEY AUTOINCREMENT, name VARCHAR(100));
                    CREATE TABLE users (\n  id INTEGER NOT NULL,\n  username VARCHAR(45) NULL,\n  email VARCHAR(150) NULL,\n  password VARCHAR(45) NULL,\n  role_id INT,\n  createdat DATETIME NULL,\n  PRIMARY KEY (id),\n  FOREIGN KEY (role_id) REFERENCES roles (id)\n);
                    CREATE INDEX users_id_index ON users (id);
                    CREATE TRIGGER dummy_trigger AFTER INSERT ON users BEGIN SELECT 1; END;
                    CREATE VIEW email_view AS\n  SELECT users.email, users.password\n  FROM users;
                    ",
                )
                .await
                .expect("setup sqlite schema");
        }

        // 1. Version check
        let ver = db_conn.get_version().expect("get_version");
        let expected_ver_name = match adapter_key {
            "postgresql" | "redshift" => "PostgreSQL",
            "mysql" => "MySQL",
            "mariadb" => "MariaDB",
            "sqlite" => "SQLite",
            "sqlserver" => "SQL Server",
            "cassandra" => "Cassandra",
            _ => unreachable!(),
        };
        assert!(
            ver.name.contains(expected_ver_name),
            "adapter {adapter_key} version name {} should contain {expected_ver_name}",
            ver.name
        );
        assert!(!ver.string.is_empty());

        // 2. listDatabases
        let dbs = db_conn.list_databases_compat().await;
        if adapter_key == "sqlite" {
            assert_eq!(dbs[0], ":memory:");
        } else {
            assert!(dbs.contains(&"rust_sql".to_string()));
        }

        // 3. listTables
        let schema_filter = db_schema.map(|s| SchemaFilter {
            schema: Some(FilterClause::Exact(s.to_string())),
        });
        let tables = db_conn
            .list_tables(schema_filter.as_ref())
            .await
            .expect("list_tables");
        assert_eq!(tables.len(), 2);
        assert_eq!(tables[0].name, "roles");
        assert_eq!(tables[1].name, "users");
        assert_eq!(tables[0].schema.as_deref(), db_schema);

        // 4. listViews
        if adapter_key != "cassandra" {
            let views = db_conn
                .list_views(schema_filter.as_ref())
                .await
                .expect("list_views");
            assert_eq!(views.len(), 1);
            assert_eq!(views[0].name, "email_view");
        }

        // 5. listRoutines
        let routines = db_conn
            .list_routines(schema_filter.as_ref())
            .await
            .expect("list_routines");
        match adapter_key {
            "postgresql" => {
                assert_eq!(routines.len(), 2);
                assert_eq!(routines[1].routine_type, "FUNCTION");
            }
            "redshift" => {
                assert_eq!(routines.len(), 1);
                assert_eq!(routines[0].routine_type, "FUNCTION");
            }
            "mysql" | "mariadb" => {
                assert_eq!(routines.len(), 1);
                assert_eq!(routines[0].routine_type, "PROCEDURE");
                assert!(routines[0].schema.is_none());
            }
            "sqlserver" => {
                assert_eq!(routines.len(), 1);
                assert_eq!(routines[0].routine_type, "PROCEDURE");
                assert_eq!(routines[0].schema.as_deref(), Some("dbo"));
            }
            "sqlite" | "cassandra" => {
                assert!(routines.is_empty());
            }
            _ => unreachable!(),
        }

        // 6. listTableColumns
        let cols = db_conn
            .list_table_columns("users", None)
            .await
            .expect("list_table_columns");
        assert_eq!(cols.len(), 6);

        // 7. listTableTriggers
        let triggers = db_conn
            .list_table_triggers("users", None)
            .await
            .expect("list_table_triggers");
        if matches!(adapter_key, "cassandra" | "redshift") {
            assert!(triggers.is_empty());
        } else {
            assert_eq!(triggers, vec!["dummy_trigger".to_string()]);
        }

        // 8. listTableIndexes
        let indexes = db_conn
            .list_table_indexes("users", db_schema)
            .await
            .expect("list_table_indexes");
        match adapter_key {
            "cassandra" => assert!(indexes.is_empty()),
            "sqlite" => assert_eq!(indexes, vec!["users_id_index"]),
            "postgresql" | "redshift" => assert_eq!(indexes, vec!["users_pkey"]),
            "mysql" | "mariadb" => assert_eq!(indexes, vec!["PRIMARY", "role_id"]),
            "sqlserver" => assert!(indexes[0].starts_with("PK__users__")),
            _ => unreachable!(),
        }

        // 9. listSchemas
        let schemas = db_conn
            .list_schemas(Some(&SchemaFilter {
                schema: Some(FilterClause::List(FilterList {
                    only: Some(vec![
                        db_schema.unwrap_or("public").to_string(),
                        "dummy_schema".to_string(),
                    ]),
                    ignore: None,
                })),
            }))
            .await
            .expect("list_schemas");
        if matches!(adapter_key, "postgresql" | "redshift") {
            assert_eq!(schemas.len(), 2);
        } else if adapter_key == "sqlserver" {
            assert!(schemas.contains(&"dummy_schema".to_string()));
        } else {
            assert!(schemas.is_empty());
        }

        // 10. getTableReferences & getTableKeys
        let refs = db_conn
            .get_table_references("users", None)
            .await
            .expect("get_table_references");
        if matches!(adapter_key, "cassandra" | "sqlite") {
            assert!(refs.is_empty());
        } else {
            assert_eq!(refs, vec!["roles".to_string()]);
        }

        let keys = db_conn
            .get_table_keys("users", None)
            .await
            .expect("get_table_keys");
        match adapter_key {
            "cassandra" => assert_eq!(keys.len(), 1),
            "sqlite" => assert_eq!(keys.len(), 0),
            _ => assert_eq!(keys.len(), 2),
        }

        // 11. SQL Scripts: Select, Insert, Update, Delete, Create Table, View, Routine
        let select_script = db_conn
            .get_table_select_script("users", None)
            .await
            .expect("select script");
        match adapter_key {
            "mysql" | "mariadb" => assert_eq!(
                select_script,
                "SELECT `id`, `username`, `email`, `password`, `role_id`, `createdat` FROM `users`;"
            ),
            "sqlserver" => assert_eq!(
                select_script,
                "SELECT [id], [username], [email], [password], [role_id], [createdat] FROM [users];"
            ),
            "postgresql" | "redshift" | "sqlite" => assert_eq!(
                select_script,
                "SELECT \"id\", \"username\", \"email\", \"password\", \"role_id\", \"createdat\" FROM \"users\";"
            ),
            "cassandra" => assert_eq!(
                select_script,
                "SELECT \"id\", \"createdat\", \"email\", \"password\", \"role_id\", \"username\" FROM \"users\";"
            ),
            _ => unreachable!(),
        }

        let insert_script = db_conn
            .get_table_insert_script("users", Some("public"))
            .await
            .expect("insert script");
        if adapter_key == "sqlserver" {
            assert_eq!(
                insert_script,
                "INSERT INTO [public].[users] ([id], [username], [email], [password], [role_id], [createdat])\n VALUES (?, ?, ?, ?, ?, ?);"
            );
        }

        let update_script = db_conn
            .get_table_update_script("users", Some("public"))
            .await
            .expect("update script");
        if adapter_key == "postgresql" {
            assert_eq!(
                update_script,
                "UPDATE \"public\".\"users\"\n SET \"id\"=?, \"username\"=?, \"email\"=?, \"password\"=?, \"role_id\"=?, \"createdat\"=?\n WHERE <condition>;"
            );
        }

        let delete_script = db_conn
            .get_table_delete_script("roles", None)
            .await
            .expect("delete script");
        if adapter_key == "mysql" {
            assert_eq!(delete_script, "DELETE FROM `roles` WHERE <condition>;");
        }

        // 12. getQuerySelectTop with default, config limit, and explicit parameter limit
        clear_select_limit();
        let top_default = db_conn
            .get_query_select_top("test_table", None, None)
            .await
            .unwrap();
        match adapter_key {
            "mysql" | "mariadb" => assert_eq!(top_default, "SELECT * FROM `test_table` LIMIT 1000"),
            "postgresql" | "redshift" => {
                assert_eq!(
                    top_default,
                    "SELECT * FROM \"public\".\"test_table\" LIMIT 1000"
                )
            }
            "sqlite" | "cassandra" => {
                assert_eq!(top_default, "SELECT * FROM \"test_table\" LIMIT 1000")
            }
            "sqlserver" => assert_eq!(top_default, "SELECT TOP 1000 * FROM [test_table]"),
            _ => unreachable!(),
        }

        set_select_limit(125);
        let top_cfg = db_conn
            .get_query_select_top("test_table", None, None)
            .await
            .unwrap();
        assert!(top_cfg.contains("125"));
        clear_select_limit();

        // 13. executeQuery: INSERT, SELECT, UPDATE, DELETE
        db_conn
            .execute_query("INSERT INTO roles (id, name) VALUES (1, 'developer')")
            .await
            .expect("insert role");
        db_conn
            .execute_query("INSERT INTO users (id, username, email, password, role_id, createdat) VALUES (1, 'maxcnunes', 'maxcnunes@gmail.com', '123456', 1, '2016-10-25')")
            .await
            .expect("insert user");

        let sel_res = db_conn
            .execute_query("SELECT * FROM users")
            .await
            .expect("select users");
        assert_eq!(sel_res.len(), 1);
        assert_eq!(sel_res[0].command, "SELECT");
        assert_eq!(sel_res[0].row_count, Some(1));
        assert_eq!(
            sel_res[0].rows[0].get("username").and_then(|v| v.as_str()),
            Some("maxcnunes")
        );

        let upd_res = db_conn
            .execute_query("UPDATE users SET username = 'max' WHERE id = 1")
            .await
            .expect("update user");
        assert_eq!(upd_res[0].command, "UPDATE");

        let del_res = db_conn
            .execute_query("DELETE FROM users WHERE id = 1")
            .await
            .expect("delete user");
        assert_eq!(del_res[0].command, "DELETE");

        // 14. Cancellable query check for server-backed adapters
        if adapter_key != "cassandra" && adapter_key != "sqlite" {
            let q_handle = db_conn.query("SELECT pg_sleep(10);").expect("create query");
            q_handle.cancel().await.expect("cancel query");
            let err = q_handle.execute().await.unwrap_err();
            assert_eq!(err.error_code(), Some("CANCELED_BY_USER"));
        }

        db_conn.truncate_all_tables(None).await.expect("truncate");
        db_conn.disconnect().await;
    }
}

trait ListDatabasesCompat {
    async fn list_databases_compat(&self) -> Vec<String>;
}

impl ListDatabasesCompat for rust_sql::adapters::DatabaseConnection {
    async fn list_databases_compat(&self) -> Vec<String> {
        self.list_databases(None).await.expect("list_databases")
    }
}

#[tokio::test]
async fn test_servers_crud_state_store_ipc_and_tui() {
    let tmp = TempDir::new().expect("tempdir");
    let cfg_path = tmp.path().join("rust-sql.json");
    set_config_path_override(Some(cfg_path.clone()));

    let secret = "CHK`Ya91Hs{me!^8ndwPPaPPxwQ}`";
    config::prepare_sync(secret).expect("prepare_sync");

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

    let added = servers::add(&new_srv, secret).await.expect("add server");
    assert!(added.validation_errors.is_none());
    let created = added.data.expect("created server");
    assert!(!created.id.is_empty());
    assert_eq!(created.encrypted, Some(true));

    // Verify encrypted password roundtrip
    let dec = servers::decrypt_secrets(&created, secret).expect("decrypt");
    assert_eq!(dec.password, Some(Password::Plain("password".into())));

    // Test GUI Draft conversion roundtrip
    let draft = ServerFormDraft::from_server(Some(&dec));
    let converted = draft.to_server();
    assert_eq!(converted.name, "My New Mysql Server");
    assert_eq!(converted.client, "mysql");

    // Test AppStore workflow
    let mut store = AppStore::new();
    store.load_config().expect("load_config");
    assert_eq!(store.servers.items.len(), 1);

    let settings_draft = SettingsFormDraft::from_store(&store);
    assert_eq!(settings_draft.zoom_factor, 1.0);

    // Add an SQLite server and connect through AppStore
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

    // Test SQL script generation through AppStore
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

    // Test Read-Only guardrail, Parameter binding, Bookmarks/History, and ERD generation in AppStore
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

    // Test Web IPC dispatcher
    let ipc_state = WebIpcState::default();
    let clients_resp = dispatch_ipc_event(
        &ipc_state,
        IpcRequest {
            event: DB_GET_CLIENTS_SYNC.to_string(),
            args: vec![],
        },
    )
    .await;
    assert!(clients_resp.ok);

    let servers_resp = dispatch_ipc_event(
        &ipc_state,
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
