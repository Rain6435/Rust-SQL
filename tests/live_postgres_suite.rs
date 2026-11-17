//! Live PostgreSQL connection tests: failures are errors (never emulated data), errors name the
//! host, port, and database but never the password, and a real server returns real rows.
//!
//! The real-server test runs only when `RUST_SQL_TEST_PG_HOST` is set (see `docs/`), for
//! example against the `postgres` service in `docker-compose.yml`.

use rust_sql::adapters::{create_server, set_emulation_enabled};
use rust_sql::common::types::{ConnectionMode, Password, Server};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

const SECRET: &str = "hunter2-Sup3r-Secret";

fn pg_server(host: &str, port: u32, password: &str) -> Server {
    Server {
        name: "pg".into(),
        client: "postgresql".into(),
        host: Some(host.into()),
        port: Some(port),
        database: Some("appdb".into()),
        user: Some("app".into()),
        password: Some(Password::Plain(password.into())),
        ..Default::default()
    }
}

async fn read_message_body(sock: &mut tokio::net::TcpStream) -> Vec<u8> {
    let len = sock.read_i32().await.expect("len") as usize;
    let mut body = vec![0u8; len - 4];
    sock.read_exact(&mut body).await.expect("body");
    body
}

/// Minimal PostgreSQL wire-protocol server that rejects every password like a real server does.
async fn spawn_rejecting_pg_server() -> (u16, tokio::task::JoinHandle<Vec<u8>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let port = listener.local_addr().expect("addr").port();
    let handle = tokio::spawn(async move {
        let (mut sock, _) = listener.accept().await.expect("accept");
        let mut startup = read_message_body(&mut sock).await;
        if startup.starts_with(&80877103i32.to_be_bytes()) {
            sock.write_all(b"N").await.expect("ssl refuse");
            startup = read_message_body(&mut sock).await;
        }
        assert!(startup.starts_with(&196608i32.to_be_bytes()), "startup");
        // AuthenticationCleartextPassword
        sock.write_all(&[b'R', 0, 0, 0, 8, 0, 0, 0, 3])
            .await
            .expect("auth");
        assert_eq!(sock.read_u8().await.expect("tag"), b'p');
        let password = read_message_body(&mut sock).await;
        let mut fields = Vec::new();
        for (code, value) in [
            (b'S', "FATAL"),
            (b'V', "FATAL"),
            (b'C', "28P01"),
            (b'M', "password authentication failed for user \"app\""),
        ] {
            fields.push(code);
            fields.extend_from_slice(value.as_bytes());
            fields.push(0);
        }
        fields.push(0);
        let mut msg = vec![b'E'];
        msg.extend_from_slice(&((fields.len() + 4) as i32).to_be_bytes());
        msg.extend_from_slice(&fields);
        sock.write_all(&msg).await.expect("error response");
        password
    });
    (port, handle)
}

#[tokio::test]
async fn wrong_password_is_an_error_with_no_rows_even_with_emulation_enabled() {
    set_emulation_enabled(true);
    let (port, server_task) = spawn_rejecting_pg_server().await;
    let srv = pg_server("127.0.0.1", port as u32, SECRET);
    let mut session = create_server(srv).expect("create_server");
    let db_conn = session.create_connection(Some("appdb"));

    let err = db_conn
        .connect()
        .await
        .expect_err("wrong password must fail");
    let msg = err.to_string();
    assert!(msg.contains("password authentication failed"), "{msg}");
    assert!(msg.contains(&format!("127.0.0.1:{port}")), "{msg}");
    assert!(msg.contains("appdb"), "{msg}");
    assert!(!msg.contains(SECRET), "password leaked: {msg}");

    let sent = server_task.await.expect("mock server");
    assert!(String::from_utf8_lossy(&sent).contains(SECRET));

    assert!(db_conn.execute_query("SELECT * FROM users").await.is_err());
    assert!(db_conn.list_tables(None).await.is_err());
    assert!(db_conn.connection_mode().is_err());
}

#[tokio::test]
async fn unreachable_server_is_an_error_naming_host_port_and_database() {
    set_emulation_enabled(true);
    let port = {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        listener.local_addr().expect("addr").port()
    };
    for client in ["postgresql", "redshift"] {
        let mut srv = pg_server("127.0.0.1", port as u32, SECRET);
        srv.client = client.into();
        let mut session = create_server(srv).expect("create_server");
        let db_conn = session.create_connection(Some("appdb"));
        let msg = db_conn.connect().await.expect_err("must fail").to_string();
        assert!(msg.starts_with("Could not connect to "), "{msg}");
        assert!(msg.contains(&format!("127.0.0.1:{port}")), "{msg}");
        assert!(msg.contains("\"appdb\""), "{msg}");
        assert!(!msg.contains(SECRET), "password leaked: {msg}");
    }
}

#[tokio::test]
async fn ssl_enabled_connection_is_refused_instead_of_downgraded() {
    let mut srv = pg_server("127.0.0.1", 1, SECRET);
    srv.ssl = Some(rust_sql::common::types::SslConfig::Bool(true));
    let mut session = create_server(srv).expect("create_server");
    let db_conn = session.create_connection(Some("appdb"));
    let msg = db_conn.connect().await.expect_err("must fail").to_string();
    assert!(msg.contains("TLS is not implemented"), "{msg}");
}

/// Runs against a real server when `RUST_SQL_TEST_PG_HOST` is set.
#[tokio::test]
async fn real_postgres_returns_live_rows_and_metadata() {
    let Ok(host) = std::env::var("RUST_SQL_TEST_PG_HOST") else {
        eprintln!("skipped: RUST_SQL_TEST_PG_HOST is not set");
        return;
    };
    let env = |k: &str, d: &str| std::env::var(k).unwrap_or_else(|_| d.to_string());
    let port: u32 = env("RUST_SQL_TEST_PG_PORT", "5432").parse().expect("port");
    let mut srv = pg_server(&host, port, &env("RUST_SQL_TEST_PG_PASSWORD", "password"));
    srv.user = Some(env("RUST_SQL_TEST_PG_USER", "postgres"));
    let database = env("RUST_SQL_TEST_PG_DB", "rust_sql");
    srv.database = Some(database.clone());

    let mut bad = srv.clone();
    bad.password = Some(Password::Plain("definitely-wrong-password".into()));
    let mut bad_session = create_server(bad).expect("create_server");
    let bad_conn = bad_session.create_connection(Some(&database));
    let bad_msg = bad_conn
        .connect()
        .await
        .expect_err("wrong password")
        .to_string();
    assert!(!bad_msg.contains("definitely-wrong-password"), "{bad_msg}");

    let mut session = create_server(srv).expect("create_server");
    let db = session.create_connection(Some(&database));
    db.connect().await.expect("live connect");
    assert_eq!(db.connection_mode().expect("mode"), ConnectionMode::Live);
    assert!(db
        .get_version()
        .expect("version")
        .string
        .contains("PostgreSQL"));

    db.execute_query(
        "DROP TABLE IF EXISTS ws1_child; DROP TABLE IF EXISTS ws1_parent;
         CREATE TABLE ws1_parent (id INT PRIMARY KEY);
         CREATE TABLE ws1_child (id INT PRIMARY KEY, parent_id INT REFERENCES ws1_parent(id), note TEXT);
         INSERT INTO ws1_parent VALUES (1), (2);
         INSERT INTO ws1_child VALUES (10, 1, 'live row'), (11, 2, NULL);",
    )
    .await
    .expect("setup");

    let res = db
        .execute_query("SELECT id, note FROM ws1_child ORDER BY id; UPDATE ws1_parent SET id = id")
        .await
        .expect("select");
    assert_eq!(res.len(), 2);
    assert_eq!(res[0].command, "SELECT");
    assert_eq!(res[0].row_count, Some(2));
    assert_eq!(res[0].rows[0]["note"], "live row");
    assert!(res[0].rows[1]["note"].is_null());
    assert_eq!(res[1].command, "UPDATE");
    assert_eq!(res[1].affected_rows, Some(2));

    let tables = db.list_tables(None).await.expect("tables");
    assert!(tables.iter().any(|t| t.name == "ws1_child"));
    assert!(
        !tables.iter().any(|t| t.name == "users"),
        "emulator seed leaked"
    );
    let cols = db
        .list_table_columns("ws1_child", None)
        .await
        .expect("cols");
    assert_eq!(cols.len(), 3);
    let keys = db.get_table_keys("ws1_child", None).await.expect("keys");
    assert!(keys.iter().any(|k| k.key_type == "FOREIGN KEY"));
    assert!(db
        .list_databases(None)
        .await
        .expect("dbs")
        .contains(&database));

    let q = db.query("SELECT pg_sleep(30)").expect("query handle");
    let runner = {
        let q = q.clone();
        tokio::spawn(async move { q.execute().await })
    };
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    q.cancel().await.expect("cancel");
    let err = runner.await.expect("join").expect_err("canceled");
    assert_eq!(err.error_code(), Some("CANCELED_BY_USER"));

    db.execute_query("DROP TABLE ws1_child; DROP TABLE ws1_parent;")
        .await
        .expect("cleanup");
    db.disconnect().await;
}
