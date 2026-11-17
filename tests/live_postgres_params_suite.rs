//! Live PostgreSQL prepared statements: a value containing `'`, `\`, and `--` round-trips
//! unchanged, numeric parameters stay numeric, results keep their types, and a comment
//! placeholder is not bound.
//!
//! Runs only when `RUST_SQL_TEST_PG_HOST` is set (see `live_postgres_suite.rs`).

use rust_sql::adapters::create_server;
use rust_sql::common::types::{Password, Server};
use rust_sql::core::params::{ParamValue, QueryParams};

const HOSTILE: &str = "O'Reilly \\' -- not a comment /* nor this */ '); DROP TABLE ws6_params; --";

fn params(pairs: &[(&str, ParamValue)]) -> QueryParams {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.clone()))
        .collect()
}

fn live_server() -> Option<(Server, String)> {
    let Ok(host) = std::env::var("RUST_SQL_TEST_PG_HOST") else {
        eprintln!("skipped: RUST_SQL_TEST_PG_HOST is not set");
        return None;
    };
    let env = |k: &str, d: &str| std::env::var(k).unwrap_or_else(|_| d.to_string());
    let database = env("RUST_SQL_TEST_PG_DB", "rust_sql");
    let srv = Server {
        name: "pg".into(),
        client: "postgresql".into(),
        host: Some(host),
        port: Some(env("RUST_SQL_TEST_PG_PORT", "5432").parse().expect("port")),
        database: Some(database.clone()),
        user: Some(env("RUST_SQL_TEST_PG_USER", "postgres")),
        password: Some(Password::plain(env(
            "RUST_SQL_TEST_PG_PASSWORD",
            "password",
        ))),
        ..Default::default()
    };
    Some((srv, database))
}

#[tokio::test]
async fn postgres_binds_typed_parameters_through_prepared_statements() {
    let Some((srv, database)) = live_server() else {
        return;
    };
    let mut session = create_server(srv).expect("create_server");
    let db = session.create_connection(Some(&database));
    db.connect().await.expect("live connect");
    db.execute_query(
        "DROP TABLE IF EXISTS ws6_params;
         CREATE TABLE ws6_params (id INT4, big INT8, small INT2, ratio FLOAT8, price NUMERIC(12,4),
                                  ok BOOL, note TEXT, born DATE, at TIMESTAMP, doc JSONB);",
    )
    .await
    .expect("setup");

    let p = params(&[
        ("id", ParamValue::Int(1)),
        ("big", ParamValue::Int(9_007_199_254_740_993)),
        ("small", ParamValue::Int(-7)),
        ("ratio", ParamValue::Float(0.25)),
        ("price", ParamValue::Float(12.5)),
        ("ok", ParamValue::Bool(true)),
        ("note", ParamValue::Text(HOSTILE.into())),
        ("born", ParamValue::Text("2024-02-29".into())),
        ("at", ParamValue::Text("2024-02-29 13:45:06.5".into())),
        ("doc", ParamValue::Text("{\"a\": [1, 2]}".into())),
    ]);
    let res = db
        .execute_query_with_params(
            "INSERT INTO ws6_params VALUES (:id, :big, :small, :ratio, :price, :ok, :note, :born, :at, :doc)",
            &p,
        )
        .await
        .expect("insert");
    assert_eq!(res[0].affected_rows, Some(1));

    let res = db
        .execute_query_with_params(
            "SELECT id, big, small, ratio, price, ok, note, born, at, doc, $1 + 1 AS next
             FROM ws6_params WHERE id = $1 -- AND note = :absent",
            &params(&[("$1", ParamValue::Int(1))]),
        )
        .await
        .expect("select");
    let row = &res[0].rows[0];
    assert_eq!(res[0].command, "SELECT");
    assert_eq!(row["note"], HOSTILE, "value must round-trip unchanged");
    assert_eq!(row["id"], 1);
    assert_eq!(row["big"], 9_007_199_254_740_993_i64);
    assert_eq!(row["small"], -7);
    assert_eq!(row["ratio"], 0.25);
    assert_eq!(row["price"], "12.5000");
    assert_eq!(row["ok"], true);
    assert_eq!(row["born"], "2024-02-29");
    assert_eq!(row["at"], "2024-02-29 13:45:06.5");
    assert_eq!(row["doc"]["a"][1], 2);
    assert_eq!(
        row["next"], 2,
        "a numeric parameter stays numeric on the server"
    );

    // Text that looks like a number is still text.
    let res = db
        .execute_query_with_params(
            "SELECT :v AS v",
            &params(&[("v", ParamValue::Text("007".into()))]),
        )
        .await
        .expect("text param");
    assert_eq!(res[0].rows[0]["v"], "007");

    // Out-of-range values and unsupported result types are explicit errors.
    let err = db
        .execute_query_with_params(
            "SELECT small FROM ws6_params WHERE small = :s",
            &params(&[("s", ParamValue::Int(70_000))]),
        )
        .await
        .expect_err("int2 overflow");
    assert!(err.to_string().contains("out of range"), "{err}");
    let err = db
        .execute_query_with_params(
            "SELECT '127.0.0.1'::inet AS addr WHERE :x",
            &params(&[("x", ParamValue::Bool(true))]),
        )
        .await
        .expect_err("inet result");
    assert!(err.to_string().contains("::text"), "{err}");

    // The cancellable path binds the same way, and the table is intact.
    let q = db
        .query_with_params(
            "UPDATE ws6_params SET note = :n WHERE id = :id",
            &params(&[("n", ParamValue::Null), ("id", ParamValue::Int(1))]),
        )
        .expect("handle");
    let res = q.execute().await.expect("update");
    assert_eq!(res[0].affected_rows, Some(1));
    let res = db
        .execute_query("SELECT count(*) AS c FROM ws6_params WHERE note IS NULL")
        .await
        .expect("count");
    assert_eq!(res[0].rows[0]["c"], "1");

    db.execute_query("DROP TABLE ws6_params")
        .await
        .expect("cleanup");
    db.disconnect().await;
}
