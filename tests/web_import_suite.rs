//! Web import: needs a grant for the open connection, goes through the shared all-or-nothing
//! import with the large-import confirmation, is audited, and the import endpoint keeps the
//! session and CSRF guards, accepts only import events, and takes bodies over 1 MiB.

mod support;

use rust_sql::common::event::*;
use rust_sql::core::import::LARGE_IMPORT_ROWS;
use serde_json::{json, Value};
use support::editor::{connected, count, run_args, save_sqlite, DB};
use support::TestApp;

const HOSTILE: &str = "O'Reilly -- not a comment; DROP TABLE imp; --";

#[tokio::test]
async fn preview_maps_columns_and_import_inserts_hostile_values_and_is_audited() {
    let _g = support::lock().await;
    let t = TestApp::new();
    let (bob, s, id) = connected(&t).await;
    let csv = format!("ID,extra,Email\n1,x,\"{HOSTILE}\"\n2,y,\n");
    let mut args = run_args(&csv, Value::Null, false);
    args.truncate(5);
    let p = t.ipc_as(&bob, &s, DB_IMPORT_PREVIEW, args).await;
    assert_eq!(p["ok"], true, "{p}");
    assert_eq!(p["data"]["rowCount"], 2);
    assert_eq!(p["data"]["mapping"], json!(["id", null, "email"]));
    assert_eq!(p["data"]["preview"][1], json!(["2", "y", null]));
    assert_eq!(p["data"]["requiresConfirmation"], false);

    let mapping = p["data"]["mapping"].clone();
    let r = t
        .ipc_as(&bob, &s, DB_IMPORT_RUN, run_args(&csv, mapping, false))
        .await;
    assert_eq!(r["data"]["rowsInserted"], 2, "{r}");
    let sql = "SELECT email FROM imp WHERE id = 1";
    let q = t
        .ipc_as(&bob, &s, DB_EXECUTE_QUERY, vec![json!(DB), json!(sql)])
        .await;
    assert_eq!(q["data"][0]["rows"][0]["email"], HOSTILE);

    let audit = t.app.db.list_audit(10).unwrap();
    let row = audit
        .iter()
        .find(|a| a.statement_type.as_deref() == Some("IMPORT"))
        .expect("import audited");
    assert_eq!(row.connection_id.as_deref(), Some(id.as_str()));
    assert_eq!(row.row_count, Some(2));
    assert_eq!(row.user_id, Some(bob.id));
}

#[tokio::test]
async fn a_bad_row_rolls_back_and_large_imports_need_confirmation() {
    let _g = support::lock().await;
    let t = TestApp::new();
    let (bob, s, _) = connected(&t).await;
    let bad = "id,email\n1,a\n,b\n";
    let r = t
        .ipc_as(
            &bob,
            &s,
            DB_IMPORT_RUN,
            run_args(bad, json!(["id", "email"]), false),
        )
        .await;
    assert_eq!(r["ok"], false);
    assert!(r["error"].as_str().unwrap().contains("Row 2"), "{r}");
    assert_eq!(count(&t, &bob, &s).await, json!(0));

    let mut big = String::from("id\n");
    for i in 0..LARGE_IMPORT_ROWS {
        big.push_str(&format!("{i}\n"));
    }
    let r = t
        .ipc_as(
            &bob,
            &s,
            DB_IMPORT_RUN,
            run_args(&big, json!(["id"]), false),
        )
        .await;
    assert!(r["error"].as_str().unwrap().contains("confirmed"), "{r}");
    assert_eq!(count(&t, &bob, &s).await, json!(0));
    let r = t
        .ipc_as(&bob, &s, DB_IMPORT_RUN, run_args(&big, json!(["id"]), true))
        .await;
    assert_eq!(r["data"]["rowsInserted"], LARGE_IMPORT_ROWS, "{r}");
}

#[tokio::test]
async fn the_import_endpoint_needs_csrf_accepts_only_imports_and_large_bodies() {
    let _g = support::lock().await;
    let t = TestApp::new();
    let bob = t.user("bob", false);
    let id = save_sqlite(&t);
    t.app.db.add_grant(bob.id, &id, None).unwrap();
    let r = t.router();
    let (cookie, csrf) = support::session(&r, "bob").await;
    let at = |path, csrf: Option<String>, event, args| {
        let (r, cookie) = (r.clone(), cookie.clone());
        async move { support::ipc_http_at(&r, path, Some(&cookie), csrf.as_deref(), event, args).await }
    };
    let c = Some(csrf.clone());
    assert_eq!(
        at(
            "/api/ipc",
            c.clone(),
            DB_CONNECT,
            vec![json!(id), json!(DB)]
        )
        .await
        .body["ok"],
        true
    );
    let sql = "CREATE TABLE imp (id INTEGER NOT NULL, email TEXT)";
    at(
        "/api/ipc",
        c.clone(),
        DB_EXECUTE_QUERY,
        vec![json!(DB), json!(sql)],
    )
    .await;

    let no_csrf = at(
        "/api/ipc/import",
        None,
        DB_IMPORT_RUN,
        run_args("id\n1\n", json!(["id"]), false),
    )
    .await;
    assert_eq!(
        no_csrf.status,
        axum::http::StatusCode::FORBIDDEN,
        "{}",
        no_csrf.body
    );
    let other = at(
        "/api/ipc/import",
        c.clone(),
        DB_EXECUTE_QUERY,
        vec![json!(DB), json!("DELETE FROM imp")],
    )
    .await;
    assert_eq!(
        other.status,
        axum::http::StatusCode::BAD_REQUEST,
        "{}",
        other.body
    );

    // About 2 MiB: over the general IPC limit, under the import limit.
    let mut big = String::from("id,email\n");
    let mut n = 0;
    while big.len() < 2 * 1024 * 1024 {
        big.push_str(&format!(
            "{n},user{n}@example.com padding padding padding\n"
        ));
        n += 1;
    }
    let args = run_args(&big, json!(["id", "email"]), true);
    let too_big = at("/api/ipc", c.clone(), DB_IMPORT_RUN, args.clone()).await;
    assert_eq!(too_big.status, axum::http::StatusCode::PAYLOAD_TOO_LARGE);
    let ok = at("/api/ipc/import", c, DB_IMPORT_RUN, args).await;
    assert_eq!(ok.body["data"]["rowsInserted"], n, "{}", ok.body);
}
