//! Integration tests for all 7 database adapters' metadata introspection and common utilities.

use rust_sql::adapters::{create_server, ADAPTERS, CLIENTS};
use rust_sql::common::types::{FilterClause, FilterList, SchemaFilter, Server, SslConfig};
use rust_sql::common::utils::{append_semi_colon, escape_regexp_string, titlize, version_compare};

/// Verifies that the static [`ADAPTERS`] and [`CLIENTS`] registries are identical and contain all 7 dialects.
#[test]
fn test_adapters_equal_clients() {
    assert_eq!(*ADAPTERS, *CLIENTS);
    assert_eq!(CLIENTS.len(), 7);
}

/// Verifies string formatting, regex escaping, and semantic version comparison utilities.
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

/// Verifies version discovery and schema/table/view/routine/column/trigger/index/key introspection across all 7 dialects.
#[tokio::test]
async fn test_all_7_adapters_metadata_spec() {
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

        let ver = db_conn.get_version().expect("get_version");
        let expected_ver_name = match adapter_key {
            "postgresql" => "PostgreSQL",
            "redshift" => "Redshift",
            "mysql" => "MySQL",
            "mariadb" => "MariaDB",
            "sqlite" => "SQLite",
            "sqlserver" => "SQL Server",
            "cassandra" => "Cassandra",
            _ => unreachable!(),
        };
        assert!(ver.name.contains(expected_ver_name));
        assert!(!ver.string.is_empty());

        let dbs = db_conn.list_databases(None).await.expect("list_databases");
        if adapter_key == "sqlite" {
            assert_eq!(dbs[0], ":memory:");
        } else {
            assert!(dbs.contains(&"rust_sql".to_string()));
        }

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

        if adapter_key != "cassandra" {
            let views = db_conn
                .list_views(schema_filter.as_ref())
                .await
                .expect("list_views");
            assert_eq!(views.len(), 1);
            assert_eq!(views[0].name, "email_view");
        }

        let routines = db_conn
            .list_routines(schema_filter.as_ref())
            .await
            .expect("list_routines");
        match adapter_key {
            "postgresql" => assert_eq!(routines.len(), 2),
            "redshift" | "mysql" | "mariadb" | "sqlserver" => assert_eq!(routines.len(), 1),
            "sqlite" | "cassandra" => assert!(routines.is_empty()),
            _ => unreachable!(),
        }

        let cols = db_conn
            .list_table_columns("users", None)
            .await
            .expect("list_table_columns");
        assert_eq!(cols.len(), 6);

        let triggers = db_conn
            .list_table_triggers("users", None)
            .await
            .expect("list_table_triggers");
        if matches!(adapter_key, "cassandra" | "redshift") {
            assert!(triggers.is_empty());
        } else {
            assert_eq!(triggers, vec!["dummy_trigger".to_string()]);
        }

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

        db_conn.disconnect().await;
    }
}
