//! Helpers for the web import and autocomplete suites: a granted, connected session on an
//! in-memory SQLite connection with table `imp`.

use super::TestApp;
use rust_sql::common::event::*;
use rust_sql::common::types::{Server, SslConfig};
use rust_sql::core::appdb::User;
use rust_sql::core::servers;
use rust_sql::web::WebIpcState;
use serde_json::{json, Value};

/// The database name of the in-memory connection.
pub const DB: &str = ":memory:";

/// Saves the in-memory SQLite connection and returns its ID.
pub fn save_sqlite(t: &TestApp) -> String {
    let srv = Server {
        name: "imports".into(),
        client: "sqlite".into(),
        database: Some(DB.into()),
        ssl: Some(SslConfig::Bool(false)),
        ..Default::default()
    };
    servers::add_sync(&srv, &*t.keys).unwrap().data.unwrap().id
}

/// Bob, granted the connection, connected, with table `imp`.
pub async fn connected(t: &TestApp) -> (User, WebIpcState, String) {
    let bob = t.user("bob", false);
    let id = save_sqlite(t);
    t.app.db.add_grant(bob.id, &id, None).unwrap();
    let s = WebIpcState::new(4);
    let r = t
        .ipc_as(&bob, &s, DB_CONNECT, vec![json!(id), json!(DB)])
        .await;
    assert_eq!(r["ok"], true, "{r}");
    let sql = "CREATE TABLE imp (id INTEGER NOT NULL, email TEXT)";
    let r = t
        .ipc_as(&bob, &s, DB_EXECUTE_QUERY, vec![json!(DB), json!(sql)])
        .await;
    assert_eq!(r["ok"], true, "{r}");
    (bob, s, id)
}

/// Arguments for `DB_IMPORT_RUN` into `imp` from `up.csv`.
pub fn run_args(text: &str, mapping: Value, confirmed: bool) -> Vec<Value> {
    vec![
        json!(DB),
        json!("imp"),
        Value::Null,
        json!("up.csv"),
        json!(text),
        mapping,
        json!(confirmed),
    ]
}

/// Rows in `imp`.
pub async fn count(t: &TestApp, bob: &User, s: &WebIpcState) -> Value {
    let sql = "SELECT count(*) AS n FROM imp";
    let r = t
        .ipc_as(bob, s, DB_EXECUTE_QUERY, vec![json!(DB), json!(sql)])
        .await;
    r["data"][0]["rows"][0]["n"].clone()
}
