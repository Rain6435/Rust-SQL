//! TUI import and autocomplete, driven by key presses against a live SQLite connection:
//! `i` opens the import dialog, the file is previewed and mapped, large imports need `y`,
//! Read-Only mode refuses imports, and the SQL editor completes tables and columns.

mod live_env;
mod support;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use rust_sql::core::import::LARGE_IMPORT_ROWS;
use rust_sql::tui::{DatabaseFocus, TuiApp, TuiRoute};
use tokio::runtime::Runtime;

fn key(app: &mut TuiApp, rt: &Runtime, code: KeyCode) {
    app.handle_key(KeyEvent::new(code, KeyModifiers::NONE), rt);
}

fn typed(app: &mut TuiApp, rt: &Runtime, text: &str) {
    for c in text.chars() {
        key(app, rt, KeyCode::Char(c));
    }
}

/// A TUI connected to a fresh SQLite file with table `imp`, routed to its workspace.
fn connected(rt: &Runtime, t: &support::TestApp, dir: &tempfile::TempDir) -> TuiApp {
    let mut app = TuiApp::new();
    app.store.keys = rust_sql::core::keys::SharedKeyProvider(t.keys.clone());
    app.store.load_config().unwrap();
    let srv = live_env::sqlite_server(dir);
    let db = srv.database.clone().unwrap();
    let saved = app.store.save_server(srv, None).unwrap();
    rt.block_on(async {
        app.store
            .connect(&saved.id, None, false, None)
            .await
            .unwrap();
        app.store
            .db
            .execute_query(&db, "CREATE TABLE imp (id INTEGER NOT NULL, email TEXT)")
            .await
            .unwrap();
        app.store.tables.did_invalidate = true;
        app.store.fetch_tables_if_needed(&db).await.unwrap();
    });
    app.store.add_new_query(&db, None);
    app.route = TuiRoute::Database {
        server_id: saved.id,
        database: db,
    };
    app
}

fn count(app: &TuiApp, rt: &Runtime) -> String {
    let TuiRoute::Database { database, .. } = &app.route else {
        panic!("not in the workspace")
    };
    let r = rt
        .block_on(
            app.store
                .db
                .execute_query(database, "SELECT count(*) AS n FROM imp"),
        )
        .unwrap();
    r[0].rows[0]["n"].to_string()
}

#[test]
fn import_dialog_previews_maps_confirms_and_imports() {
    let rt = Runtime::new().unwrap();
    let _g = rt.block_on(support::lock());
    let t = support::TestApp::new();
    let dir = tempfile::tempdir().unwrap();
    let mut app = connected(&rt, &t, &dir);

    let small = dir.path().join("small.csv");
    std::fs::write(&small, "ID,skip_me,Email\n1,x,a@b.c\n2,y,\n").unwrap();
    key(&mut app, &rt, KeyCode::Char('i'));
    assert_eq!(app.import.as_ref().unwrap().table.name, "imp");
    typed(&mut app, &rt, small.to_str().unwrap());
    key(&mut app, &rt, KeyCode::Enter);
    let st = app.import.as_ref().unwrap();
    assert_eq!(
        st.mapping,
        vec![Some("id".into()), None, Some("email".into())]
    );
    // Map the skipped column onto `email` instead (Right: skip -> id -> email), then undo.
    key(&mut app, &rt, KeyCode::Down);
    key(&mut app, &rt, KeyCode::Right);
    assert_eq!(
        app.import.as_ref().unwrap().mapping[1].as_deref(),
        Some("id")
    );
    key(&mut app, &rt, KeyCode::Left);
    key(&mut app, &rt, KeyCode::Enter);
    let msg = app.import.as_ref().unwrap().message.clone().unwrap();
    assert_eq!(msg, Ok("Imported 2 rows into imp.".to_string()));
    assert_eq!(count(&app, &rt), "2");
    assert!(
        app.import.as_ref().unwrap().data.is_none(),
        "a second Enter cannot re-import"
    );

    let big = dir.path().join("big.csv");
    let body: String = (0..LARGE_IMPORT_ROWS).map(|i| format!("{i}\n")).collect();
    std::fs::write(&big, format!("id\n{body}")).unwrap();
    for _ in 0..small.to_str().unwrap().len() {
        key(&mut app, &rt, KeyCode::Backspace);
    }
    typed(&mut app, &rt, big.to_str().unwrap());
    key(&mut app, &rt, KeyCode::Enter);
    key(&mut app, &rt, KeyCode::Enter);
    let refused = app.import.as_ref().unwrap().message.clone().unwrap();
    assert!(refused.unwrap_err().contains("Press y"));
    assert_eq!(count(&app, &rt), "2");

    app.store.read_only_mode = true;
    key(&mut app, &rt, KeyCode::Char('y'));
    key(&mut app, &rt, KeyCode::Enter);
    let ro = app.import.as_ref().unwrap().message.clone().unwrap();
    assert!(ro.unwrap_err().contains("Read-Only"));
    app.store.read_only_mode = false;
    key(&mut app, &rt, KeyCode::Enter);
    let done = app.import.as_ref().unwrap().message.clone().unwrap();
    assert_eq!(
        done,
        Ok(format!("Imported {LARGE_IMPORT_ROWS} rows into imp."))
    );
    key(&mut app, &rt, KeyCode::Esc);
    assert!(app.import.is_none());
}

#[test]
fn editor_completes_tables_and_columns_with_tab() {
    let rt = Runtime::new().unwrap();
    let _g = rt.block_on(support::lock());
    let t = support::TestApp::new();
    let dir = tempfile::tempdir().unwrap();
    let mut app = connected(&rt, &t, &dir);
    app.db_focus = DatabaseFocus::QueryArea;

    typed(&mut app, &rt, "SELECT * FROM im");
    let c = app.completion.as_ref().expect("live suggestions");
    assert_eq!(c.items[0].label, "imp");
    key(&mut app, &rt, KeyCode::Tab);
    assert_eq!(
        app.db_focus,
        DatabaseFocus::QueryArea,
        "Tab inserted, did not switch panes"
    );
    assert_eq!(
        app.store.current_query().unwrap().query,
        "SELECT * FROM imp"
    );

    typed(&mut app, &rt, " x WHERE x.");
    let c = app.completion.as_ref().expect("columns after alias");
    let labels: Vec<&str> = c.items.iter().map(|i| i.label.as_str()).collect();
    assert_eq!(labels, ["id", "email"]);
    key(&mut app, &rt, KeyCode::Down);
    key(&mut app, &rt, KeyCode::Tab);
    assert_eq!(
        app.store.current_query().unwrap().query,
        "SELECT * FROM imp x WHERE x.email"
    );

    // Esc hides suggestions; Ctrl+Space brings them back even with live completion off.
    app.store.queries.enabled_live_auto_complete = false;
    typed(&mut app, &rt, " = 1 AND x.");
    assert!(app.completion.is_none());
    app.handle_key(
        KeyEvent::new(KeyCode::Char(' '), KeyModifiers::CONTROL),
        &rt,
    );
    assert!(app.completion.is_some());
    key(&mut app, &rt, KeyCode::Esc);
    assert!(app.completion.is_none());
    assert_eq!(app.db_focus, DatabaseFocus::QueryArea);
    key(&mut app, &rt, KeyCode::Tab);
    assert_ne!(
        app.db_focus,
        DatabaseFocus::QueryArea,
        "Tab switches panes again"
    );
}
