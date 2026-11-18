//! Web autocomplete (and import) need a grant for the open connection, re-checked on every
//! call; autocomplete offers tables and the columns of aliased tables.

mod support;

use rust_sql::common::event::*;
use rust_sql::web::WebIpcState;
use serde_json::json;
use support::editor::{connected, run_args, DB};
use support::TestApp;

#[tokio::test]
async fn revoking_the_grant_stops_import_and_autocomplete() {
    let _g = support::lock().await;
    let t = TestApp::new();
    let (bob, s, id) = connected(&t).await;
    t.app.db.remove_grant(bob.id, &id).unwrap();
    let r = t
        .ipc_as(
            &bob,
            &s,
            DB_IMPORT_RUN,
            run_args("id\n1\n", json!(["id"]), false),
        )
        .await;
    assert!(
        r["error"].as_str().unwrap().contains("Access denied"),
        "{r}"
    );
    let r = t
        .ipc_as(
            &bob,
            &s,
            DB_COMPLETE,
            vec![json!(DB), json!("SELECT * FROM i"), json!(15)],
        )
        .await;
    assert!(
        r["error"].as_str().unwrap().contains("Access denied"),
        "{r}"
    );
    let other = WebIpcState::new(4);
    let r = t
        .ipc_as(
            &bob,
            &other,
            DB_COMPLETE,
            vec![json!(DB), json!("x"), json!(1)],
        )
        .await;
    assert!(
        r["error"].as_str().unwrap().contains("Not connected"),
        "{r}"
    );
}

#[tokio::test]
async fn autocomplete_offers_tables_and_columns_of_aliased_tables() {
    let _g = support::lock().await;
    let t = TestApp::new();
    let (bob, s, _) = connected(&t).await;
    let call = |text: &str, cursor: usize, explicit: bool| {
        vec![json!(DB), json!(text), json!(cursor), json!(explicit)]
    };
    let r = t
        .ipc_as(&bob, &s, DB_COMPLETE, call("SELECT * FROM i", 15, false))
        .await;
    assert_eq!(
        r["data"]["items"],
        json!([{ "label": "imp", "kind": "table" }]),
        "{r}"
    );
    assert_eq!(r["data"]["start"], 14);

    // Cursor counts code points: the "é" before it is one character.
    let text = "SELECT x.em, 'é' FROM imp x";
    let r = t.ipc_as(&bob, &s, DB_COMPLETE, call(text, 11, false)).await;
    assert_eq!(
        r["data"]["items"],
        json!([{ "label": "email", "kind": "column" }]),
        "{r}"
    );

    // A table created later shows up after an explicit (Ctrl+Space) request.
    let sql = "CREATE TABLE invoices (id INTEGER)";
    t.ipc_as(&bob, &s, DB_EXECUTE_QUERY, vec![json!(DB), json!(sql)])
        .await;
    let r = t
        .ipc_as(&bob, &s, DB_COMPLETE, call("SELECT * FROM i", 15, true))
        .await;
    let labels: Vec<&str> = r["data"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["label"].as_str().unwrap())
        .collect();
    assert_eq!(labels, ["imp", "invoices"]);
}
