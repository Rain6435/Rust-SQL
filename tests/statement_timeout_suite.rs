//! Per-connection statement timeouts: a statement that runs past the connection's timeout is
//! stopped with a `StatementTimeout` error, on SQLite (interrupt watchdog) and on live
//! PostgreSQL (`statement_timeout`). Fast statements and connections without a timeout are
//! unaffected.

mod live_env;

use rust_sql::adapters::create_server;
use rust_sql::core::errors::DbCoreError;
use std::time::{Duration, Instant};

const SQLITE_SLOW: &str = "WITH RECURSIVE c(x) AS (SELECT 1 UNION ALL SELECT x + 1 FROM c) \
                           SELECT count(*) FROM c";

#[tokio::test]
async fn sqlite_statement_past_the_timeout_fails_with_a_timeout_error() {
    let dir = tempfile::tempdir().unwrap();
    let mut srv = live_env::sqlite_server(&dir);
    srv.statement_timeout_secs = Some(1);
    let mut session = create_server(srv.clone()).unwrap();
    let conn = session.create_connection(srv.database.as_deref());
    conn.connect().await.unwrap();

    let started = Instant::now();
    let err = conn.execute_query(SQLITE_SLOW).await.unwrap_err();
    assert!(matches!(err, DbCoreError::StatementTimeout(_)), "{err}");
    assert!(err.to_string().contains("1s statement timeout"), "{err}");
    assert!(started.elapsed() < Duration::from_secs(15));

    // The cancellable path (used by the GUI) enforces the same timeout.
    let q = conn.query(SQLITE_SLOW).unwrap();
    let err = q.execute().await.unwrap_err();
    assert!(matches!(err, DbCoreError::StatementTimeout(_)), "{err}");

    // The connection is still usable afterwards.
    let rows = conn.execute_query("SELECT 7 AS n").await.unwrap();
    assert_eq!(rows[0].rows[0]["n"], 7);
}

#[tokio::test]
async fn sqlite_without_a_timeout_runs_normally() {
    let dir = tempfile::tempdir().unwrap();
    let srv = live_env::sqlite_server(&dir);
    assert_eq!(srv.statement_timeout(), None);
    assert_eq!(srv.statement_timeout_label(), "Timeout: none");
    let mut session = create_server(srv.clone()).unwrap();
    let conn = session.create_connection(srv.database.as_deref());
    conn.connect().await.unwrap();
    let rows = conn
        .execute_query(
            "WITH RECURSIVE c(x) AS (SELECT 1 UNION ALL SELECT x + 1 FROM c WHERE x < 1000) \
             SELECT count(*) AS n FROM c",
        )
        .await
        .unwrap();
    assert_eq!(rows[0].rows[0]["n"], 1000);
}

#[tokio::test]
async fn postgres_statement_timeout_is_set_on_the_session_and_enforced() {
    let Some(mut srv) = live_env::live_pg_server() else {
        return;
    };
    srv.statement_timeout_secs = Some(1);
    let mut session = create_server(srv.clone()).unwrap();
    let conn = session.create_connection(srv.database.as_deref());
    conn.connect().await.expect("live connect");

    let shown = conn.execute_query("SHOW statement_timeout").await.unwrap();
    assert_eq!(shown[0].rows[0]["statement_timeout"], "1s");

    let err = conn.execute_query("SELECT pg_sleep(5)").await.unwrap_err();
    assert!(matches!(err, DbCoreError::StatementTimeout(_)), "{err}");
    let q = conn.query("SELECT pg_sleep(5)").unwrap();
    let err = q.execute().await.unwrap_err();
    assert!(matches!(err, DbCoreError::StatementTimeout(_)), "{err}");

    let ok = conn.execute_query("SELECT 1 AS one").await.unwrap();
    assert_eq!(ok[0].rows[0]["one"], "1");
}
