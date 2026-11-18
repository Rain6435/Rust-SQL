//! Query history privacy through the GUI/TUI store: by default history keeps the query with its
//! placeholders and never the parameter values, and a connection marked "exclude from history"
//! records nothing, neither in the history log nor in the tab's own history.

mod support;

use rust_sql::common::types::{Server, SslConfig};
use rust_sql::core::params::{ParamInput, ParamKind};
use rust_sql::state::AppStore;

const EMAIL: &str = "alice@example.com";
const SQL: &str = "SELECT :email AS email";

fn sqlite(exclude: bool) -> Server {
    Server {
        name: "history".into(),
        client: "sqlite".into(),
        database: Some(":memory:".into()),
        ssl: Some(SslConfig::Bool(false)),
        exclude_from_history: exclude.then_some(true),
        ..Default::default()
    }
}

async fn run_with_email(exclude: bool) -> AppStore {
    let t = support::TestApp::new();
    let mut store = AppStore::new();
    store.keys = rust_sql::core::keys::SharedKeyProvider(t.keys.clone());
    store.load_config().expect("load_config");
    let saved = store.save_server(sqlite(exclude), None).expect("save");
    store
        .connect(&saved.id, None, false, None)
        .await
        .expect("connect");
    store.query_params.insert(
        "email".into(),
        ParamInput {
            kind: ParamKind::Text,
            text: EMAIL.into(),
        },
    );
    let res = store.execute_query_if_needed(SQL, false).await.unwrap();
    assert_eq!(
        res[0].rows[0]["email"], EMAIL,
        "the query itself still runs"
    );
    store
}

#[tokio::test]
async fn history_keeps_placeholders_but_not_parameter_values_by_default() {
    let _g = support::lock().await;
    let store = run_with_email(false).await;
    let entry = &store.bookmark_manager.history[0];
    assert_eq!(entry.sql, SQL);
    assert_eq!(entry.display_sql, None);
    let stored = serde_json::to_string(&store.bookmark_manager.history).unwrap();
    assert!(!stored.contains(EMAIL), "{stored}");
    assert_eq!(store.current_query().unwrap().query_history, vec![SQL]);
}

#[tokio::test]
async fn excluded_connection_leaves_no_history() {
    let _g = support::lock().await;
    let store = run_with_email(true).await;
    assert!(store.bookmark_manager.history.is_empty());
    assert!(store.current_query().unwrap().query_history.is_empty());
}
