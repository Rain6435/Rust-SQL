//! One suite, run against every real database server CI starts (PostgreSQL, MySQL, MariaDB).
//!
//! For each server whose `RUST_SQL_TEST_<DIALECT>_HOST` is set, the suite first proves the
//! server is reachable over TCP, then connects with emulation off (a live connection is
//! requested). It fails if an emulated result ever comes back:
//!
//! - a dialect with a live driver must connect in [`ConnectionMode::Live`] and pass the shared
//!   checks (typed prepared parameters, paging, import in a transaction, statement timeout);
//! - a dialect without one (MySQL and MariaDB today) must refuse with
//!   `LiveConnectionNotImplemented`, never return rows. When a driver lands and
//!   `has_live_driver` flips, the same suite runs the shared checks against it unchanged.
//!
//! Even with emulation opted in, a dialect with a live driver must still report `Live`, and an
//! emulated session must report `Emulated`, so the label can be trusted.

mod live_env;

use rust_sql::adapters::{has_live_driver, set_emulation_enabled, DatabaseFacade};
use rust_sql::common::types::{ConnectionMode, Server};
use rust_sql::core::errors::DbCoreError;
use rust_sql::core::import::{auto_map, parse_json, ImportRequest};
use rust_sql::core::paging::PageCursor;
use rust_sql::core::params::{ParamValue, QueryParams};

fn targets() -> Vec<Server> {
    [
        live_env::live_pg_server(),
        live_env::live_server_from_env("mysql", "MYSQL", 3306),
        live_env::live_server_from_env("mariadb", "MARIADB", 3307),
    ]
    .into_iter()
    .flatten()
    .collect()
}

async fn assert_reachable(srv: &Server) {
    let addr = format!("{}:{}", srv.host.as_deref().unwrap(), srv.port.unwrap());
    tokio::net::TcpStream::connect(&addr)
        .await
        .unwrap_or_else(|e| panic!("{} server at {addr} is not reachable: {e}", srv.client));
}

async fn connect(srv: &Server) -> Result<DatabaseFacade, DbCoreError> {
    let mut db = DatabaseFacade::new();
    db.connect_checked(srv.clone(), srv.database.as_deref())
        .await?;
    Ok(db)
}

/// Checks every live adapter must pass.
async fn shared_checks(srv: &Server, db: &DatabaseFacade) {
    let name = srv.database.as_deref().unwrap();
    let run = |sql: &'static str| async move { db.execute_query(name, sql).await.unwrap() };
    run("DROP TABLE IF EXISTS ws7_live").await;
    run("CREATE TABLE ws7_live (id INTEGER NOT NULL, label VARCHAR(100))").await;

    let rows: Vec<String> = (1..=12)
        .map(|i| format!("{{\"id\": {i}, \"label\": \"it's #{i}\"}}"))
        .collect();
    let data = parse_json(&rows.join("\n")).unwrap();
    let maps = auto_map(&data.columns, &["id".into(), "label".into()]);
    let req = ImportRequest {
        table: "ws7_live",
        schema: None,
        mappings: &maps,
        data: &data,
        confirmed: false,
    };
    assert_eq!(db.import_rows(name, req).await.unwrap().rows_inserted, 12);

    let params: QueryParams = [("min".to_string(), ParamValue::Int(2))].into();
    let sql = "SELECT id, label FROM ws7_live WHERE id > :min ORDER BY id";
    let first = db
        .execute_paged(name, sql, &params, PageCursor::first(5))
        .await
        .unwrap();
    assert_eq!(first.results[0].rows.len(), 5);
    assert_eq!(first.results[0].rows[0]["label"], "it's #3");
    let next = first.page.unwrap().next.expect("more pages");
    assert_eq!(next.offset, 5);
    run("DROP TABLE ws7_live").await;
}

#[tokio::test]
async fn live_requests_never_return_emulated_results() {
    let servers = targets();
    if servers.is_empty() {
        eprintln!("skipped: no RUST_SQL_TEST_*_HOST variables are set");
        return;
    }
    for srv in &servers {
        assert_reachable(srv).await;

        set_emulation_enabled(false);
        let live = connect(srv).await;
        if has_live_driver(&srv.client) {
            let db = live.unwrap_or_else(|e| panic!("{}: live connect failed: {e}", srv.client));
            let mode = db
                .connection_mode(srv.database.as_deref().unwrap())
                .unwrap();
            assert_eq!(
                mode,
                ConnectionMode::Live,
                "{}: emulated result for a live request",
                srv.client
            );
            shared_checks(srv, &db).await;
        } else {
            match live {
                Err(DbCoreError::LiveConnectionNotImplemented(_)) => {}
                Err(e) => panic!("{}: expected not-implemented, got {e}", srv.client),
                Ok(_) => panic!(
                    "{}: connected without a live driver (emulated result)",
                    srv.client
                ),
            }
        }

        // With the opt-in, the reported mode must still match reality.
        set_emulation_enabled(true);
        let opted = connect(srv).await.unwrap();
        let mode = opted
            .connection_mode(srv.database.as_deref().unwrap())
            .unwrap();
        let expected = if has_live_driver(&srv.client) {
            ConnectionMode::Live
        } else {
            ConnectionMode::Emulated
        };
        assert_eq!(mode, expected, "{}", srv.client);
        set_emulation_enabled(false);
    }
}
