//! Integration tests for SQL DDL/DML script generation, `SELECT TOP` limits,
//! statement execution (`INSERT`, `SELECT`, `UPDATE`, `DELETE`), and query cancellation.

use rust_sql::adapters::{clear_select_limit, create_server, set_select_limit};
use rust_sql::common::types::{Server, SslConfig};

/// Verifies DDL/DML script generation, `get_query_select_top`, statement execution, and query cancellation across all 7 dialects.
#[tokio::test]
async fn test_all_7_adapters_scripts_and_queries_spec() {
    let adapters = [
        "sqlite",
        "postgresql",
        "redshift",
        "mysql",
        "mariadb",
        "sqlserver",
        "cassandra",
    ];

    for adapter_key in adapters {
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
                    CREATE TABLE users (id INTEGER NOT NULL, username VARCHAR(45) NULL, email VARCHAR(150) NULL, password VARCHAR(45) NULL, role_id INT, createdat DATETIME NULL, PRIMARY KEY (id));
                    ",
                )
                .await
                .expect("setup sqlite schema");
        }

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
