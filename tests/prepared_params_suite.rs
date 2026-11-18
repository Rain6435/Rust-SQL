//! Query parameters run through prepared statements: hostile values round-trip unchanged,
//! placeholders in comments are not bound, numbers stay numbers and text stays text, and the
//! GUI/TUI store and the web IPC keep parameter types from input to execution.
//!
//! The live PostgreSQL counterpart is `live_postgres_params_suite.rs`.

mod support;

use rust_sql::adapters::{create_server, set_emulation_enabled};
use rust_sql::common::event::{DB_CONNECT, DB_EXECUTE_QUERY};
use rust_sql::common::types::{Server, SslConfig};
use rust_sql::core::params::{ParamInput, ParamKind, ParamValue, QueryParams, DISPLAY_ONLY_LABEL};
use rust_sql::core::servers;
use rust_sql::state::AppStore;
use rust_sql::web::WebIpcState;
use serde_json::json;

const HOSTILE: &str = "O'Reilly \\' -- not a comment /* nor this */ '); DROP TABLE t; --";

fn params(pairs: &[(&str, ParamValue)]) -> QueryParams {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.clone()))
        .collect()
}

fn server(client: &str) -> Server {
    Server {
        name: client.into(),
        client: client.into(),
        host: Some("db.example.test".into()),
        port: Some(3306),
        database: Some(
            if client == "sqlite" {
                ":memory:"
            } else {
                "rust_sql"
            }
            .into(),
        ),
        ssl: Some(SslConfig::Bool(false)),
        ..Default::default()
    }
}

/// Runs the round-trip checks on one connected adapter.
async fn check_round_trip(client: &str) {
    let mut session = create_server(server(client)).expect("create_server");
    let db = session.create_connection(Some(if client == "sqlite" {
        ":memory:"
    } else {
        "rust_sql"
    }));
    db.connect().await.expect("connect");
    db.execute_query("CREATE TABLE pp (id INTEGER, note TEXT)")
        .await
        .expect("create");

    let p = params(&[
        ("id", ParamValue::Int(1)),
        ("note", ParamValue::Text(HOSTILE.into())),
    ]);
    let res = db
        .execute_query_with_params("INSERT INTO pp (id, note) VALUES (:id, :note)", &p)
        .await
        .expect("insert");
    assert_eq!(res[0].affected_rows, Some(1), "{client}");

    let rows = db
        .execute_query_with_params(
            "SELECT note FROM pp WHERE id = $1",
            &params(&[("$1", ParamValue::Int(1))]),
        )
        .await
        .expect("select");
    assert_eq!(rows[0].rows.len(), 1, "{client}: table must survive");
    assert_eq!(rows[0].rows[0]["note"], HOSTILE, "{client}");

    // A placeholder inside a comment is neither bound nor required.
    let p = params(&[("id", ParamValue::Int(1))]);
    let rows = db
        .execute_query_with_params(
            "SELECT id -- :missing\nFROM pp /* @missing $9 */ WHERE id = :id",
            &p,
        )
        .await
        .expect("comment placeholders are ignored");
    assert_eq!(rows[0].rows[0]["id"], 1, "{client}");

    // A placeholder with no value is an error naming it, not a NULL.
    let err = db
        .execute_query_with_params("SELECT :id, :nope", &p)
        .await
        .expect_err("missing value");
    assert!(err.to_string().contains(":nope"), "{client}: {err}");

    // Through the cancellable path as well.
    let q = db
        .query_with_params(
            "SELECT note FROM pp WHERE note = :n",
            &params(&[("n", ParamValue::Text(HOSTILE.into()))]),
        )
        .expect("handle");
    assert_eq!(q.execute().await.expect("run")[0].rows.len(), 1, "{client}");
}

#[tokio::test]
async fn sqlite_binds_parameters_through_prepared_statements() {
    check_round_trip("sqlite").await;
}

#[tokio::test]
async fn emulated_mysql_binds_backslash_quote_values_safely() {
    set_emulation_enabled(true);
    check_round_trip("mysql").await;
    check_round_trip("mariadb").await;
}

#[tokio::test]
async fn sqlite_parameters_keep_their_types() {
    let mut session = create_server(server("sqlite")).expect("create_server");
    let db = session.create_connection(Some(":memory:"));
    db.connect().await.expect("connect");
    let p = params(&[
        ("i", ParamValue::Int(42)),
        ("f", ParamValue::Float(1.5)),
        ("s", ParamValue::Text("42".into())),
        ("b", ParamValue::Bool(true)),
        ("n", ParamValue::Null),
    ]);
    let res = db
        .execute_query_with_params(
            "SELECT typeof(:i) AS i, typeof(:f) AS f, typeof(:s) AS s, :b AS b, typeof(:n) AS n, :i + 1 AS sum",
            &p,
        )
        .await
        .expect("select");
    let row = &res[0].rows[0];
    assert_eq!(row["i"], "integer");
    assert_eq!(row["f"], "real");
    assert_eq!(row["s"], "text", "numeric-looking text must stay text");
    assert_eq!(row["b"], 1);
    assert_eq!(row["n"], "null");
    assert_eq!(row["sum"], 43);
}

#[tokio::test]
async fn store_executes_typed_inputs_and_records_display_only_history() {
    let _g = support::lock().await;
    let t = support::TestApp::new();
    let mut store = AppStore::new();
    store.keys = rust_sql::core::keys::SharedKeyProvider(t.keys.clone());
    store.load_config().expect("load_config");
    let saved = store.save_server(server("sqlite"), None).expect("save");
    store
        .connect(&saved.id, None, false, None)
        .await
        .expect("connect");
    // Display-only bound SQL is kept only with the history opt-in.
    store
        .config
        .data
        .as_mut()
        .unwrap()
        .history_store_bound_values = true;

    let input = |kind, text: &str| ParamInput {
        kind,
        text: text.into(),
    };
    store
        .query_params
        .insert("id".into(), input(ParamKind::Number, "7"));
    store
        .query_params
        .insert("zip".into(), input(ParamKind::Text, "02134"));
    store
        .query_params
        .insert("stale".into(), input(ParamKind::Number, "not used"));
    let sql = "SELECT typeof(:id) AS id_type, :zip AS zip -- :zip";
    let res = store
        .execute_query_if_needed(sql, false)
        .await
        .expect("execute");
    assert_eq!(res[0].rows[0]["id_type"], "integer");
    assert_eq!(res[0].rows[0]["zip"], "02134");

    let entry = &store.bookmark_manager.history[0];
    assert_eq!(
        entry.sql, sql,
        "history keeps placeholders so Replay re-binds"
    );
    let shown = entry.display_sql.as_deref().expect("display sql");
    assert!(shown.starts_with(DISPLAY_ONLY_LABEL), "{shown}");
    assert!(
        shown.ends_with("SELECT typeof(7) AS id_type, '02134' AS zip -- :zip"),
        "{shown}"
    );

    store
        .query_params
        .insert("id".into(), input(ParamKind::Number, "seven"));
    let err = store
        .execute_query_if_needed(sql, false)
        .await
        .expect_err("bad number");
    assert!(err.contains("not a number"), "{err}");
}

#[tokio::test]
async fn web_ipc_keeps_json_parameter_types() {
    let _g = support::lock().await;
    let t = support::TestApp::new();
    let admin = t.user("root", true);
    let srv = server("sqlite");
    let id = servers::add_sync(&srv, &*t.keys).unwrap().data.unwrap().id;
    let s = WebIpcState::new(4);
    let conn = t
        .ipc_as(&admin, &s, DB_CONNECT, vec![json!(id), json!(":memory:")])
        .await;
    assert_eq!(conn["ok"], true, "{conn}");

    let sql = "SELECT typeof(:n) AS n, typeof(:s) AS s, :s AS v";
    let args = vec![json!(":memory:"), json!(sql), json!({"n": 5, "s": HOSTILE})];
    let res = t.ipc_as(&admin, &s, DB_EXECUTE_QUERY, args).await;
    assert_eq!(res["ok"], true, "{res}");
    let row = &res["data"][0]["rows"][0];
    assert_eq!(row["n"], "integer");
    assert_eq!(row["s"], "text");
    assert_eq!(row["v"], HOSTILE);

    let bad = vec![json!(":memory:"), json!(sql), json!({"n": [1]})];
    let res = t.ipc_as(&admin, &s, DB_EXECUTE_QUERY, bad).await;
    assert_eq!(res["ok"], false, "{res}");
    assert!(
        res["error"].as_str().unwrap_or("").contains("Parameter n"),
        "{res}"
    );
}
