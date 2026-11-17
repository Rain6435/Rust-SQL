//! Read-Only mode end to end: the analyzer blocks what it cannot prove read-only, and the
//! database session rejects writes even when the analyzer is bypassed.
//!
//! The live PostgreSQL test runs only when `RUST_SQL_TEST_PG_HOST` is set (see `docs/`).

use rust_sql::adapters::{create_server, set_emulation_enabled, DatabaseFacade};
use rust_sql::common::types::{Password, Server};
use rust_sql::state::AppStore;

fn sqlite_file_with_rows(dir: &tempfile::TempDir) -> String {
    let path = dir.path().join("ro.db");
    let conn = rusqlite::Connection::open(&path).expect("open");
    conn.execute_batch("CREATE TABLE t (id INTEGER); INSERT INTO t VALUES (1), (2);")
        .expect("seed");
    path.to_string_lossy().into_owned()
}

fn sqlite_server(path: &str) -> Server {
    Server {
        name: "ro".into(),
        client: "sqlite".into(),
        database: Some(path.into()),
        ..Default::default()
    }
}

#[tokio::test]
async fn store_blocks_unverifiable_statements_and_driver_rejects_writes() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = sqlite_file_with_rows(&dir);
    let mut store = AppStore::new();
    store
        .db
        .connect(sqlite_server(&path), Some(&path))
        .await
        .expect("connect");
    store.add_new_query(&path, None);
    store.read_only_mode = true;

    for sql in [
        "DELETE FROM t -- WHERE id = 1",
        "WITH x AS (SELECT 1) DELETE FROM t",
        "REPLACE INTO t VALUES (3)",
        "this is not sql",
        "PRAGMA query_only = 0",
    ] {
        let err = store
            .execute_query_if_needed(sql, false)
            .await
            .expect_err(sql);
        assert!(err.contains("Statement #1"), "{sql}: {err}");
    }

    let rows = store
        .execute_query_if_needed("SELECT id FROM t", false)
        .await
        .expect("select allowed");
    assert_eq!(rows[0].rows.len(), 2);

    // The session itself is read-only: going around the analyzer still cannot write.
    assert!(store.db.read_only);
    let err = store
        .db
        .execute_query(&path, "DELETE FROM t")
        .await
        .expect_err("driver must reject the write");
    assert!(err.to_lowercase().contains("readonly"), "{err}");

    store.read_only_mode = false;
    store
        .execute_query_if_needed("DELETE FROM t WHERE id = 1", false)
        .await
        .expect("writes allowed again");
}

#[tokio::test]
async fn facade_applies_read_only_to_later_connections() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = sqlite_file_with_rows(&dir);
    let mut facade = DatabaseFacade::new();
    facade
        .set_read_only(true)
        .await
        .expect("set before connect");
    facade
        .connect(sqlite_server(&path), Some(&path))
        .await
        .expect("connect");
    assert!(facade.execute_query(&path, "DROP TABLE t").await.is_err());
    facade.set_read_only(false).await.expect("turn off");
    facade
        .execute_query(&path, "DELETE FROM t")
        .await
        .expect("write after turning off");
}

#[tokio::test]
async fn emulated_sessions_are_read_only_at_the_engine() {
    set_emulation_enabled(true);
    let srv = Server {
        name: "my".into(),
        client: "mysql".into(),
        host: Some("localhost".into()),
        database: Some("rust_sql".into()),
        ..Default::default()
    };
    let mut session = create_server(srv).expect("create_server");
    let db = session.create_connection(Some("rust_sql"));
    db.read_only = true;
    db.connect().await.expect("emulated connect");
    assert!(db.execute_query("DELETE FROM users").await.is_err());
    db.set_read_only(false).await.expect("turn off");
    db.execute_query("DELETE FROM users").await.expect("write");
    set_emulation_enabled(false);
}

/// Runs against a real server when `RUST_SQL_TEST_PG_HOST` is set.
#[tokio::test]
async fn real_postgres_session_is_read_only() {
    let Ok(host) = std::env::var("RUST_SQL_TEST_PG_HOST") else {
        eprintln!("skipped: RUST_SQL_TEST_PG_HOST is not set");
        return;
    };
    let env = |k: &str, d: &str| std::env::var(k).unwrap_or_else(|_| d.to_string());
    let database = env("RUST_SQL_TEST_PG_DB", "rust_sql");
    let srv = Server {
        name: "pg".into(),
        client: "postgresql".into(),
        host: Some(host),
        port: Some(env("RUST_SQL_TEST_PG_PORT", "5432").parse().expect("port")),
        user: Some(env("RUST_SQL_TEST_PG_USER", "postgres")),
        password: Some(Password::plain(env(
            "RUST_SQL_TEST_PG_PASSWORD",
            "password",
        ))),
        database: Some(database.clone()),
        ..Default::default()
    };
    let mut session = create_server(srv).expect("create_server");
    let db = session.create_connection(Some(&database));
    db.connect().await.expect("connect");
    db.execute_query(
        "DROP TABLE IF EXISTS ws5_ro; CREATE TABLE ws5_ro (id INT); INSERT INTO ws5_ro VALUES (1);",
    )
    .await
    .expect("setup");

    db.set_read_only(true).await.expect("read-only session");
    let err = db
        .execute_query("DELETE FROM ws5_ro")
        .await
        .expect_err("server must reject the write")
        .to_string();
    assert!(
        err.contains("25006"),
        "expected read_only_sql_transaction: {err}"
    );
    let rows = db
        .execute_query("SELECT id FROM ws5_ro")
        .await
        .expect("read");
    assert_eq!(rows[0].rows.len(), 1);

    db.set_read_only(false).await.expect("read-write session");
    db.execute_query("DROP TABLE ws5_ro")
        .await
        .expect("cleanup");
    db.disconnect().await;
}
