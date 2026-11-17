//! Authorization: connection grants (403 and audited without one), per-user connection lists,
//! admin-only events, per-session isolation, and audit rows for queries and grant changes.

mod support;

use rust_sql::common::event::*;
use rust_sql::common::types::{Server, SslConfig};
use rust_sql::core::servers;
use rust_sql::web::WebIpcState;
use serde_json::json;
use support::TestApp;

fn save_sqlite(t: &TestApp, name: &str) -> String {
    let srv = Server {
        name: name.into(),
        client: "sqlite".into(),
        database: Some(":memory:".into()),
        ssl: Some(SslConfig::Bool(false)),
        ..Default::default()
    };
    servers::add_sync(&srv, &*t.keys).unwrap().data.unwrap().id
}

#[tokio::test]
async fn a_user_without_a_grant_gets_403_and_the_attempt_is_audited() {
    let _g = support::lock().await;
    let t = TestApp::new();
    let bob = t.user("bob", false);
    let id = save_sqlite(&t, "finance");
    let s = WebIpcState::new(4);

    let resp = t
        .ipc_as(&bob, &s, DB_CONNECT, vec![json!(id), json!(":memory:")])
        .await;
    assert_eq!(resp["ok"], false);
    assert!(
        resp["error"].as_str().unwrap().contains("Access denied"),
        "{resp}"
    );
    let denied = t.app.db.list_audit(5).unwrap();
    assert_eq!(denied[0].event_type, "access_denied");
    assert_eq!(denied[0].connection_id.as_deref(), Some(id.as_str()));
    assert_eq!(denied[0].user_id, Some(bob.id));

    let listed = t.ipc_as(&bob, &s, SERVERS_GET_ALL, vec![]).await;
    assert_eq!(listed["data"], json!([]));
    let cfg = t.ipc_as(&bob, &s, CONFIG_GET_FULL, vec![json!(true)]).await;
    assert_eq!(cfg["data"]["servers"], json!([]));
}

#[tokio::test]
async fn the_http_status_for_a_missing_grant_is_403() {
    let _g = support::lock().await;
    let t = TestApp::new();
    t.user("bob", false);
    let id = save_sqlite(&t, "finance");
    let r = t.router();
    let (cookie, csrf) = support::session(&r, "bob").await;
    let resp = support::ipc_http(&r, Some(&cookie), Some(&csrf), DB_CONNECT, vec![json!(id)]).await;
    assert_eq!(
        resp.status,
        axum::http::StatusCode::FORBIDDEN,
        "{}",
        resp.body
    );
}

#[tokio::test]
async fn granted_users_see_and_use_only_their_connections_and_queries_are_audited() {
    let _g = support::lock().await;
    let t = TestApp::new();
    let admin = t.user("root", true);
    let bob = t.user("bob", false);
    let mine = save_sqlite(&t, "mine");
    let _other = save_sqlite(&t, "other");
    let a = WebIpcState::new(4);
    let s = WebIpcState::new(4);

    let granted = t
        .ipc_as(&admin, &a, GRANTS_ADD, vec![json!(bob.id), json!(mine)])
        .await;
    assert_eq!(granted["data"]["changed"], true, "{granted}");
    let listed = t.ipc_as(&bob, &s, SERVERS_GET_ALL, vec![]).await;
    let names: Vec<_> = listed["data"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v["name"].clone())
        .collect();
    assert_eq!(names, [json!("mine")]);

    let conn = t
        .ipc_as(&bob, &s, DB_CONNECT, vec![json!(mine), json!(":memory:")])
        .await;
    assert_eq!(conn["data"]["mode"], "live", "{conn}");
    let q = "CREATE TABLE t (email TEXT); INSERT INTO t VALUES ('secret@example.com'); SELECT * FROM t;";
    let res = t
        .ipc_as(
            &bob,
            &s,
            DB_EXECUTE_QUERY,
            vec![json!(":memory:"), json!(q)],
        )
        .await;
    assert_eq!(res["ok"], true, "{res}");

    let audit = t.app.db.list_audit(20).unwrap();
    let queries: Vec<_> = audit
        .iter()
        .filter(|a| a.event_type == "query_executed")
        .collect();
    assert_eq!(queries.len(), 3);
    let select = queries
        .iter()
        .find(|a| a.statement_type.as_deref() == Some("SELECT"))
        .unwrap();
    assert_eq!(select.row_count, Some(1));
    assert_eq!(select.user_id, Some(bob.id));
    assert_eq!(select.connection_id.as_deref(), Some(mine.as_str()));
    let dump = format!("{audit:?}");
    assert!(
        !dump.contains("secret@example.com") && !dump.contains("CREATE TABLE"),
        "{dump}"
    );
    assert!(audit
        .iter()
        .any(|a| a.event_type == "grant_added" && a.user_id == Some(admin.id)));

    // Revoking the grant takes effect on the very next request of the open session.
    t.ipc_as(&admin, &a, GRANTS_REMOVE, vec![json!(bob.id), json!(mine)])
        .await;
    let res = t
        .ipc_as(
            &bob,
            &s,
            DB_EXECUTE_QUERY,
            vec![json!(":memory:"), json!("SELECT 1")],
        )
        .await;
    assert!(
        res["error"].as_str().unwrap().contains("Access denied"),
        "{res}"
    );
    assert!(t
        .app
        .db
        .list_audit(3)
        .unwrap()
        .iter()
        .any(|a| a.event_type == "grant_removed"));
}

#[tokio::test]
async fn sessions_do_not_share_open_connections() {
    let _g = support::lock().await;
    let t = TestApp::new();
    let admin = t.user("root", true);
    let id = save_sqlite(&t, "shared");
    let first = WebIpcState::new(4);
    let second = WebIpcState::new(4);
    t.ipc_as(
        &admin,
        &first,
        DB_CONNECT,
        vec![json!(id), json!(":memory:")],
    )
    .await;
    let other = t
        .ipc_as(&admin, &second, DB_LIST_TABLES, vec![json!(":memory:")])
        .await;
    assert!(
        other["error"].as_str().unwrap().contains("Not connected"),
        "{other}"
    );
}

#[tokio::test]
async fn admin_only_events_are_refused_for_regular_users_and_audited() {
    let _g = support::lock().await;
    let t = TestApp::new();
    let bob = t.user("bob", false);
    let s = WebIpcState::new(4);
    let id = save_sqlite(&t, "x");
    for (event, args) in [
        (USERS_LIST, vec![]),
        (
            USERS_CREATE,
            vec![json!("eve"), json!("a long enough password"), json!(true)],
        ),
        (USERS_SET_ADMIN, vec![json!(bob.id), json!(true)]),
        (GRANTS_ADD, vec![json!(bob.id), json!(id)]),
        (AUDIT_LIST, vec![]),
        (
            SERVERS_ADD,
            vec![json!({ "name": "n", "client": "sqlite" })],
        ),
        (SERVERS_REMOVE_BY_ID, vec![json!(id)]),
        (CONFIG_SAVE_SETTINS, vec![json!({})]),
        (CONFIG_PREPARE, vec![]),
        (DB_QUERY_OPEN, vec![json!("/etc/passwd")]),
        (
            DB_EXPORT_QUERY_RESULT_TO_FILE,
            vec![json!([]), json!("CSV")],
        ),
        (DB_SET_SELECT_LIMIT, vec![json!(5)]),
        (SSH_HOST_KEY_DELETE, vec![json!("bastion"), json!(22)]),
    ] {
        let resp = t.ipc_as(&bob, &s, event, args).await;
        assert!(
            resp["error"]
                .as_str()
                .unwrap_or("")
                .contains("Access denied"),
            "{event}: {resp}"
        );
    }
    let denied = t.app.db.list_audit(100).unwrap();
    assert_eq!(
        denied
            .iter()
            .filter(|a| a.event_type == "access_denied")
            .count(),
        13
    );
    assert_eq!(t.app.db.list_users().unwrap().len(), 1);
}
