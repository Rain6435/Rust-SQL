//! Result paging: a single SELECT is fetched one page at a time and more rows load on demand,
//! through the facade, the GUI/TUI store, and the web IPC. Scripts that cannot be paged run in
//! full. Runs on SQLite always and on live PostgreSQL when `RUST_SQL_TEST_PG_HOST` is set.

mod live_env;
mod support;

use rust_sql::adapters::DatabaseFacade;
use rust_sql::common::event::*;
use rust_sql::common::types::{Server, SslConfig};
use rust_sql::core::paging::PageCursor;
use rust_sql::core::params::{ParamValue, QueryParams};
use rust_sql::core::result_ops::compute_column_stats;
use rust_sql::core::servers;
use rust_sql::state::AppStore;
use rust_sql::web::WebIpcState;
use serde_json::json;

const SETUP: &str = "DROP TABLE IF EXISTS paged; CREATE TABLE paged (id INTEGER, label TEXT); \
     INSERT INTO paged (id, label) SELECT g, 'row ' || g FROM \
     (WITH RECURSIVE s(g) AS (SELECT 1 UNION ALL SELECT g + 1 FROM s WHERE g < 25) SELECT g FROM s) AS gen;";

fn id(v: &serde_json::Value) -> i64 {
    v.as_i64()
        .unwrap_or_else(|| v.as_str().unwrap().parse().unwrap())
}

async fn check_facade_paging(srv: Server) {
    let db_name = srv.database.clone().unwrap();
    let mut db = DatabaseFacade::new();
    db.connect(srv, Some(&db_name)).await.expect("connect");
    db.execute_query(&db_name, SETUP).await.expect("setup");

    let sql = "SELECT id, label FROM paged WHERE id > :min ORDER BY id";
    let params: QueryParams = [("min".to_string(), ParamValue::Int(2))].into();
    let mut cursor = Some(PageCursor::first(10));
    let mut seen = Vec::new();
    let mut pages = 0;
    while let Some(c) = cursor {
        let out = db.execute_paged(&db_name, sql, &params, c).await.unwrap();
        let page = out.page.expect("a single SELECT is pageable");
        assert_eq!(page.cursor, c);
        assert!(out.results[0].rows.len() <= 10);
        seen.extend(out.results[0].rows.iter().map(|r| id(&r["id"])));
        cursor = page.next;
        pages += 1;
    }
    assert_eq!(pages, 3);
    assert_eq!(
        seen,
        (3..=25).collect::<Vec<_>>(),
        "pages keep the query's order"
    );

    let script = db
        .execute_paged(
            &db_name,
            "SELECT 1 AS a; SELECT 2 AS b",
            &QueryParams::new(),
            PageCursor::first(1),
        )
        .await
        .unwrap();
    assert!(script.page.is_none());
    assert_eq!(script.results.len(), 2);
    db.execute_query(&db_name, "DROP TABLE paged")
        .await
        .unwrap();
}

#[tokio::test]
async fn sqlite_pages_a_select_in_order_until_the_last_page() {
    let dir = tempfile::tempdir().unwrap();
    check_facade_paging(live_env::sqlite_server(&dir)).await;
}

#[tokio::test]
async fn postgres_pages_a_select_in_order_until_the_last_page() {
    let Some(srv) = live_env::live_pg_server() else {
        return;
    };
    check_facade_paging(srv).await;
}

#[tokio::test]
async fn store_loads_the_first_page_then_more_on_demand_and_stats_cover_loaded_rows() {
    let _g = support::lock().await;
    let t = support::TestApp::new();
    let dir = tempfile::tempdir().unwrap();
    let mut store = AppStore::new();
    store.keys = rust_sql::core::keys::SharedKeyProvider(t.keys.clone());
    store.load_config().unwrap();
    let saved = store
        .save_server(live_env::sqlite_server(&dir), None)
        .unwrap();
    store.connect(&saved.id, None, false, None).await.unwrap();
    store.config.data.as_mut().unwrap().result_page_size = 10;
    store.execute_query_if_needed(SETUP, false).await.unwrap();

    store
        .execute_query_if_needed("SELECT * FROM paged ORDER BY id", false)
        .await
        .unwrap();
    let tab = store.current_query().unwrap();
    assert_eq!(tab.results.as_ref().unwrap()[0].rows.len(), 10);
    let page = tab.page.clone().expect("paged");
    assert_eq!(
        (page.page_size, page.next.map(|n| n.offset)),
        (10, Some(10))
    );
    let rows = &tab.results.as_ref().unwrap()[0].rows;
    let stats = compute_column_stats(rows, &["id".to_string()]);
    assert_eq!(stats[0].total_rows, 10, "stats run on the loaded page");

    assert_eq!(store.fetch_more_rows().await.unwrap(), 10);
    assert_eq!(store.fetch_more_rows().await.unwrap(), 5);
    assert_eq!(store.fetch_more_rows().await.unwrap(), 0);
    let tab = store.current_query().unwrap();
    let ids: Vec<i64> = tab.results.as_ref().unwrap()[0]
        .rows
        .iter()
        .map(|r| id(&r["id"]))
        .collect();
    assert_eq!(ids, (1..=25).collect::<Vec<_>>());
    assert_eq!(tab.page.as_ref().unwrap().next, None);
    assert_eq!(
        store.bookmark_manager.history[0].sql,
        "SELECT * FROM paged ORDER BY id"
    );
}

#[tokio::test]
async fn web_ipc_returns_pages_and_audits_each_fetch() {
    let _g = support::lock().await;
    let t = support::TestApp::new();
    let admin = t.user("root", true);
    let srv = Server {
        name: "mem".into(),
        client: "sqlite".into(),
        database: Some(":memory:".into()),
        ssl: Some(SslConfig::Bool(false)),
        ..Default::default()
    };
    let conn_id = servers::add_sync(&srv, &*t.keys).unwrap().data.unwrap().id;
    let s = WebIpcState::new(4);
    t.ipc_as(
        &admin,
        &s,
        DB_CONNECT,
        vec![json!(conn_id), json!(":memory:")],
    )
    .await;
    let setup = t
        .ipc_as(
            &admin,
            &s,
            DB_EXECUTE_QUERY,
            vec![json!(":memory:"), json!(SETUP)],
        )
        .await;
    assert_eq!(setup["ok"], true, "{setup}");

    let sql = json!("SELECT id FROM paged ORDER BY id");
    let first = t
        .ipc_as(
            &admin,
            &s,
            DB_EXECUTE_PAGED,
            vec![
                json!(":memory:"),
                sql.clone(),
                json!(null),
                json!({"pageSize": 20}),
            ],
        )
        .await;
    assert_eq!(
        first["data"]["results"][0]["rows"]
            .as_array()
            .unwrap()
            .len(),
        20,
        "{first}"
    );
    let next = first["data"]["page"]["next"].clone();
    assert_eq!(next, json!({"offset": 20, "pageSize": 20}));
    let second = t
        .ipc_as(
            &admin,
            &s,
            DB_EXECUTE_PAGED,
            vec![json!(":memory:"), sql, json!(null), next],
        )
        .await;
    assert_eq!(
        second["data"]["results"][0]["rows"]
            .as_array()
            .unwrap()
            .len(),
        5
    );
    assert_eq!(second["data"]["page"]["next"], json!(null));
    let audited = t.app.db.list_audit(10).unwrap();
    assert!(
        audited
            .iter()
            .filter(|a| a.event_type == "query_executed")
            .count()
            >= 3
    );
}
