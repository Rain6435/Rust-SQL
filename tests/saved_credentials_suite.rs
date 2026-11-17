//! Saved credentials end to end: a password saved through the shared store is encrypted on disk
//! as `v2` ciphertext and is decrypted and sent to the database by the desktop/terminal store
//! and by the web server; duplicating re-binds it; an unavailable keychain refuses to store or
//! use it; and the removed legacy format and names are gone.

mod support;

use rust_sql::common::event::{DB_CONNECT, SERVERS_ADD};
use rust_sql::common::types::{Password, Server, SslConfig};
use rust_sql::core::keys::{KeyError, KeyProvider, MasterKey, SharedKeyProvider};
use rust_sql::core::{config, servers};
use rust_sql::state::AppStore;
use rust_sql::web::WebIpcState;
use serde_json::json;
use std::path::Path;
use std::sync::Arc;
use support::fake_pg::spawn_password_capturing_pg;
use support::TestApp;

const SECRET: &str = "s4ved-pässword-🔑";

struct NoKeychain;

impl KeyProvider for NoKeychain {
    fn master_key(&self) -> Result<MasterKey, KeyError> {
        Err(KeyError::Unavailable(
            "could not read the master key in the OS keychain: locked".into(),
        ))
    }
}

fn pg(port: u16) -> Server {
    Server {
        name: "pg".into(),
        client: "postgresql".into(),
        host: Some("127.0.0.1".into()),
        port: Some(u32::from(port)),
        database: Some("appdb".into()),
        user: Some("app".into()),
        password: Some(Password::plain(SECRET)),
        ssl: Some(SslConfig::Bool(false)),
        ..Default::default()
    }
}

fn store_with(t: &TestApp) -> AppStore {
    let mut store = AppStore::new();
    store.keys = SharedKeyProvider(t.keys.clone());
    store.load_config().expect("load_config");
    store
}

#[tokio::test]
async fn desktop_and_terminal_store_send_the_saved_password() {
    let _g = support::lock().await;
    let t = TestApp::new();
    let (port, captured) = spawn_password_capturing_pg().await;
    let mut store = store_with(&t);
    let saved = store.save_server(pg(port), None).expect("save");

    let on_disk = std::fs::read_to_string(config::path()).unwrap();
    assert!(!on_disk.contains(SECRET), "plaintext on disk: {on_disk}");
    assert!(on_disk.contains("\"ciphertext\": \"v2:"), "{on_disk}");

    let err = store.connect(&saved.id, None, false, None).await;
    assert!(err.is_err(), "the fake server rejects every login");
    assert_eq!(captured.await.unwrap(), SECRET);
}

#[tokio::test]
async fn web_server_sends_the_saved_password() {
    let _g = support::lock().await;
    let t = TestApp::new();
    let admin = t.user("root", true);
    let (port, captured) = spawn_password_capturing_pg().await;
    let s = WebIpcState::new(4);
    let mut body = serde_json::to_value(pg(port)).unwrap();
    body["password"] = json!(SECRET);
    let added = t.ipc_as(&admin, &s, SERVERS_ADD, vec![body]).await;
    let id = added["data"]["data"]["id"]
        .as_str()
        .expect("id")
        .to_string();
    assert!(!added.to_string().contains(SECRET));

    let resp = t.ipc_as(&admin, &s, DB_CONNECT, vec![json!(id)]).await;
    assert!(!resp.to_string().contains(SECRET), "{resp}");
    assert_eq!(captured.await.unwrap(), SECRET);
}

#[tokio::test]
async fn duplicating_a_connection_rebinds_its_credentials() {
    let _g = support::lock().await;
    let t = TestApp::new();
    let mut store = store_with(&t);
    let original = store.save_server(pg(5432), None).expect("save");
    let copy = store.duplicate_server(original.clone()).expect("duplicate");
    assert_ne!(copy.id, original.id);
    assert_ne!(copy.password, original.password);
    let dec = servers::decrypt_secrets(&copy, &*t.keys).expect("copy decrypts");
    assert_eq!(dec.password, Some(Password::plain(SECRET)));
}

#[tokio::test]
async fn unavailable_keychain_refuses_to_store_or_use_saved_passwords() {
    let _g = support::lock().await;
    let t = TestApp::new();
    let saved = store_with(&t).save_server(pg(5432), None).expect("save");

    let mut locked = AppStore::new();
    locked.keys = SharedKeyProvider(Arc::new(NoKeychain));
    let errs = locked.save_server(pg(5432), None).unwrap_err();
    assert!(errs["general"].contains("keychain"), "{errs:?}");

    let err = locked
        .connect(&saved.id, None, false, None)
        .await
        .unwrap_err();
    assert!(err.contains("keychain"), "{err}");
    assert!(!err.contains(SECRET));
}

#[tokio::test]
async fn hand_typed_plaintext_is_encrypted_when_the_config_is_prepared() {
    let _g = support::lock().await;
    let t = TestApp::new();
    let srv = Server {
        id: "hand-typed".into(),
        ..pg(5432)
    };
    let file = json!({ "servers": [srv] });
    std::fs::write(config::path(), file.to_string()).unwrap();
    config::prepare_sync(&*t.keys).expect("prepare");
    let on_disk = std::fs::read_to_string(config::path()).unwrap();
    assert!(!on_disk.contains(SECRET), "{on_disk}");
    let stored = &config::get_sync().unwrap().servers[0];
    let dec = servers::decrypt_secrets(stored, &*t.keys).unwrap();
    assert_eq!(dec.password, Some(Password::plain(SECRET)));
}

#[tokio::test]
async fn the_removed_legacy_format_fails_to_load_with_an_explanation() {
    let _g = support::lock().await;
    let _t = TestApp::new();
    let legacy = r#"{"servers":[{"id":"a","name":"old","client":"postgresql","encrypted":true,
        "password":{"ivText":"wGf6X9T+QSygOHqtgQPlcA==","encryptedText":"0LySDs9WPAvwSS9Qv+W3/A=="}}]}"#;
    std::fs::write(config::path(), legacy).unwrap();
    let err = config::get_sync().unwrap_err();
    assert!(
        err.contains("pre-v2") && err.contains("enter it again"),
        "{err}"
    );
}

fn files_containing(dir: &Path, needle: &str, hits: &mut Vec<String>) {
    for entry in std::fs::read_dir(dir).unwrap().flatten() {
        let p = entry.path();
        if p.is_dir() {
            files_containing(&p, needle, hits);
        } else if std::fs::read_to_string(&p).is_ok_and(|c| c.contains(needle)) {
            hits.push(p.display().to_string());
        }
    }
}

#[test]
fn legacy_crypto_names_are_gone_from_src() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut hits = Vec::new();
    for needle in ["DEFAULT_CRYPTO_SECRET", "unsafe_decrypt", "AES"] {
        files_containing(&src, needle, &mut hits);
    }
    assert!(hits.is_empty(), "{hits:?}");
}

#[test]
fn the_in_memory_key_provider_is_only_used_by_tests() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut hits = Vec::new();
    files_containing(&src, "InMemoryKeyProvider", &mut hits);
    let allowed = ["keys/mod.rs", "_tests.rs", "/tests.rs", "web/state.rs"];
    let unexpected: Vec<_> = hits
        .iter()
        .filter(|h| !allowed.iter().any(|a| h.replace('\\', "/").ends_with(a)))
        .collect();
    assert!(unexpected.is_empty(), "{unexpected:?}");
    let state = std::fs::read_to_string(src.join("web/state.rs")).unwrap();
    let at = state.find("InMemoryKeyProvider").unwrap();
    assert!(state[..at]
        .rfind("#[cfg(test)]")
        .is_some_and(|c| at - c < 400));
}
