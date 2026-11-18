//! Schema-aware autocompletion through the store: table names come from the metadata cache,
//! columns of referenced tables are loaded on demand, and both Settings toggles are honored.

mod live_env;
mod support;

use rust_sql::core::completion::CompletionKind;
use rust_sql::state::AppStore;

#[tokio::test]
async fn completions_use_cached_tables_and_load_columns_of_referenced_tables() {
    let _g = support::lock().await;
    let t = support::TestApp::new();
    let dir = tempfile::tempdir().unwrap();
    let mut store = AppStore::new();
    store.keys = rust_sql::core::keys::SharedKeyProvider(t.keys.clone());
    store.load_config().unwrap();
    let srv = live_env::sqlite_server(&dir);
    let db = srv.database.clone().unwrap();
    let saved = store.save_server(srv, None).unwrap();
    store.connect(&saved.id, None, false, None).await.unwrap();
    store
        .db
        .execute_query(
            &db,
            "CREATE TABLE customers (id INTEGER, email TEXT); CREATE TABLE carts (id INTEGER)",
        )
        .await
        .unwrap();
    store.tables.did_invalidate = true;
    store.fetch_tables_if_needed(&db).await.unwrap();

    let after_from = store.completions(&db, "SELECT * FROM c", 15, false);
    let tables: Vec<_> = after_from.items.iter().map(|i| i.label.as_str()).collect();
    assert_eq!(tables, ["carts", "customers"]);

    let text = "SELECT cu.em FROM customers cu";
    store.load_completion_columns(&db, text).await;
    let cols = store.completions(&db, text, "SELECT cu.em".len(), false);
    assert_eq!(cols.items.len(), 1);
    assert_eq!(cols.items[0].label, "email");
    assert_eq!(cols.items[0].kind, CompletionKind::Column);

    store.queries.enabled_live_auto_complete = false;
    assert!(store
        .completions(&db, "SELECT * FROM c", 15, false)
        .items
        .is_empty());
    assert!(
        !store
            .completions(&db, "SELECT * FROM c", 15, true)
            .items
            .is_empty(),
        "Ctrl+Space still works"
    );
    store.queries.enabled_auto_complete = false;
    assert!(store
        .completions(&db, "SELECT * FROM c", 15, true)
        .items
        .is_empty());
}
