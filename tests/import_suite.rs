//! CSV and JSON import: rows go through prepared statements in one transaction, hostile values
//! round-trip unchanged, a bad row rolls back the whole import, and large imports need explicit
//! confirmation. Runs on SQLite always and on live PostgreSQL when `RUST_SQL_TEST_PG_HOST` is set.

mod live_env;

use rust_sql::adapters::{create_server, DatabaseConnection, ServerSession};
use rust_sql::common::types::Server;
use rust_sql::core::import::{
    auto_map, parse_csv, parse_json, ImportData, ImportRequest, LARGE_IMPORT_ROWS,
};
use rust_sql::core::params::ParamValue;

const HOSTILE: &str = "O'Reilly \\' -- not a comment; DROP TABLE imp; --";

async fn connect(srv: Server) -> ServerSession {
    let mut session = create_server(srv.clone()).unwrap();
    let conn = session.create_connection(srv.database.as_deref());
    conn.connect().await.expect("connect");
    session
}

fn conn(session: &ServerSession) -> &DatabaseConnection {
    session.databases.values().next().unwrap()
}

async fn count(c: &DatabaseConnection) -> String {
    let r = c
        .execute_query("SELECT count(*) AS n FROM imp")
        .await
        .unwrap();
    r[0].rows[0]["n"].to_string().trim_matches('"').to_string()
}

fn request<'a>(
    data: &'a ImportData,
    maps: &'a [rust_sql::core::import::ColumnMapping],
) -> ImportRequest<'a> {
    ImportRequest {
        table: "imp",
        schema: None,
        mappings: maps,
        data,
        confirmed: false,
    }
}

async fn check_import(srv: Server) {
    let session = connect(srv).await;
    let c = conn(&session);
    c.execute_query("DROP TABLE IF EXISTS imp").await.unwrap();
    c.execute_query("CREATE TABLE imp (id INTEGER NOT NULL, email TEXT, score REAL)")
        .await
        .unwrap();
    let targets = vec!["id".to_string(), "email".to_string(), "score".to_string()];

    // CSV: header names differ in case, an extra column is skipped, empty means NULL.
    let csv = format!(
        "ID,Email,ignored,Score\n1,\"{}\",x,2.5\n2,,y,\n",
        HOSTILE.replace('"', "\"\"")
    );
    let data = parse_csv(&csv, b',').unwrap();
    let maps = auto_map(&data.columns, &targets);
    assert_eq!(maps.len(), 3);
    let report = c
        .import_rows(request(&data, &maps))
        .await
        .expect("csv import");
    assert_eq!(report.rows_inserted, 2);
    let rows = c
        .execute_query("SELECT email FROM imp WHERE id = 1")
        .await
        .unwrap();
    assert_eq!(
        rows[0].rows[0]["email"], HOSTILE,
        "value round-trips unchanged"
    );
    let nulls = c
        .execute_query(
            "SELECT count(*) AS n FROM imp WHERE id = 2 AND email IS NULL AND score IS NULL",
        )
        .await
        .unwrap();
    assert_eq!(nulls[0].rows[0]["n"].to_string().trim_matches('"'), "1");

    // JSON keeps types; a bad row (NULL into NOT NULL) rolls back every row of that import.
    let json = r#"[{"id": 3, "email": "a@b.c"}, {"id": null, "email": "bad"}]"#;
    let data = parse_json(json).unwrap();
    let maps = auto_map(&data.columns, &targets);
    let err = c.import_rows(request(&data, &maps)).await.unwrap_err();
    assert!(err.to_string().contains("Row 2"), "{err}");
    assert!(err.to_string().contains("rolled back"), "{err}");
    assert_eq!(
        count(c).await,
        "2",
        "row 1 of the failed import must not remain"
    );

    // Mapping to a column the table does not have is refused before anything runs.
    let bad_map = vec![rust_sql::core::import::ColumnMapping {
        source: 0,
        target: "nope".into(),
    }];
    assert!(c.import_rows(request(&data, &bad_map)).await.is_err());

    // Large imports need explicit confirmation.
    let big = ImportData {
        columns: vec!["id".into()],
        rows: (0..LARGE_IMPORT_ROWS as i64)
            .map(|i| vec![ParamValue::Int(i + 100)])
            .collect(),
    };
    let maps = auto_map(&big.columns, &targets);
    let err = c.import_rows(request(&big, &maps)).await.unwrap_err();
    assert!(err.to_string().contains("confirmed explicitly"), "{err}");
    assert_eq!(count(c).await, "2");
    let mut ok = request(&big, &maps);
    ok.confirmed = true;
    let report = c.import_rows(ok).await.expect("confirmed import");
    assert_eq!(report.rows_inserted, LARGE_IMPORT_ROWS);
    c.execute_query("DROP TABLE imp").await.unwrap();
}

#[tokio::test]
async fn sqlite_import_round_trips_and_is_all_or_nothing() {
    let dir = tempfile::tempdir().unwrap();
    check_import(live_env::sqlite_server(&dir)).await;
}

#[tokio::test]
async fn postgres_import_round_trips_and_is_all_or_nothing() {
    let Some(srv) = live_env::live_pg_server() else {
        return;
    };
    check_import(srv).await;
}

#[test]
fn cli_import_needs_yes_for_large_files_and_imports_by_column_name() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("cli.db");
    rusqlite::Connection::open(&db)
        .unwrap()
        .execute_batch("CREATE TABLE people (id INTEGER, name TEXT)")
        .unwrap();
    let small = dir.path().join("p.csv");
    std::fs::write(&small, "ID,name,unused\n1,Ann,x\n2,\"Bo, Jr\",y\n").unwrap();
    // A relative sqlite: URI keeps the test portable (no drive letter in the URI on Windows).
    let run = |file: &std::path::Path, extra: &[&str]| {
        std::process::Command::new(env!("CARGO_BIN_EXE_Rust-SQL"))
            .args([
                "--uri",
                "sqlite:cli.db",
                "--import",
                file.to_str().unwrap(),
                "--table",
                "people",
            ])
            .args(extra)
            .current_dir(dir.path())
            .env("HOME", dir.path())
            .output()
            .unwrap()
    };
    let out = run(&small, &[]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(String::from_utf8_lossy(&out.stdout).contains("Imported 2 rows"));
    assert!(String::from_utf8_lossy(&out.stderr).contains("Skipping columns"));

    let big = dir.path().join("big.json");
    let rows: Vec<String> = (0..LARGE_IMPORT_ROWS)
        .map(|i| format!("{{\"id\": {i}}}"))
        .collect();
    std::fs::write(&big, rows.join("\n")).unwrap();
    let out = run(&big, &[]);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("--yes"));
    let out = run(&big, &["--yes"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );

    let n: i64 = rusqlite::Connection::open(&db)
        .unwrap()
        .query_row("SELECT count(*) FROM people", [], |r| r.get(0))
        .unwrap();
    assert_eq!(n, 2 + LARGE_IMPORT_ROWS as i64);
}
