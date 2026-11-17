//! Keys stay on the server and no response carries a saved password: `SERVERS_DECRYPT_SECRETS`
//! is gone, every IPC event is scanned for plaintext and ciphertext, `CONFIG_GET_FULL` reports
//! `hasPassword`, updates keep omitted passwords, and `DEFAULT_CRYPTO_SECRET` is gone from `src/`.

mod support;

use rust_sql::common::event::*;
use rust_sql::common::types::{Password, Server, SshConfig, SslConfig};
use rust_sql::core::{config, servers};
use rust_sql::web::WebIpcState;
use serde_json::{json, Value};
use std::path::Path;
use support::TestApp;

const DB_PW: &str = "Db-Pa55word-Plaintext";
const SSH_PW: &str = "Ssh-Pa55word-Plaintext";
const PASSPHRASE: &str = "Key-Passphrase-Plaintext";

fn seed(t: &TestApp) -> (String, String) {
    let sqlite = Server {
        name: "local".into(),
        client: "sqlite".into(),
        database: Some(":memory:".into()),
        password: Some(Password::Plain(DB_PW.into())),
        ssl: Some(SslConfig::Bool(false)),
        ..Default::default()
    };
    let tunneled = Server {
        name: "behind-bastion".into(),
        client: "postgresql".into(),
        host: Some("10.0.0.5".into()),
        port: Some(5432),
        user: Some("app".into()),
        password: Some(Password::Plain(DB_PW.into())),
        ssl: Some(SslConfig::Bool(false)),
        ssh: Some(SshConfig {
            user: "ops".into(),
            host: "bastion".into(),
            port: 22,
            password: Some(Password::Plain(SSH_PW.into())),
            passphrase: Some(PASSPHRASE.into()),
            private_key: Some("/keys/id_ed25519".into()),
            ..Default::default()
        }),
        ..Default::default()
    };
    let add = |s: &Server| servers::add_sync(s, &*t.keys).unwrap().data.unwrap().id;
    (add(&sqlite), add(&tunneled))
}

/// Every string an attacker must never see: plaintexts plus the stored ciphertexts.
/// (The private key *path* may appear in a connection error; listings mask it, see
/// `public_server` tests. Key material is never read into a response.)
fn forbidden_strings() -> Vec<String> {
    let mut out = vec![DB_PW.into(), SSH_PW.into(), PASSPHRASE.into()];
    for s in config::get_sync().unwrap().servers {
        let ssh = s.ssh.clone().unwrap_or_default();
        let pws = [s.password.clone(), ssh.password, ssh.passphrase];
        for p in pws.into_iter().flatten() {
            if let Password::Encrypted(e) = p {
                out.push(e.ciphertext);
            }
        }
    }
    out
}

fn all_event_names() -> Vec<String> {
    let src =
        std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("src/common/event.rs"))
            .unwrap();
    src.lines()
        .filter_map(|l| l.trim().strip_prefix("pub const "))
        .filter_map(|l| l.split('"').nth(1).map(str::to_string))
        .collect()
}

#[tokio::test]
async fn no_ipc_response_contains_a_password_or_ciphertext() {
    let _g = support::lock().await;
    let t = TestApp::new();
    let admin = t.user("root", true);
    let (sqlite_id, tunneled_id) = seed(&t);
    let forbidden = forbidden_strings();
    assert!(
        forbidden.len() >= 7,
        "seeded ciphertexts missing: {forbidden:?}"
    );
    let s = WebIpcState::new(4);
    t.ipc_as(
        &admin,
        &s,
        DB_CONNECT,
        vec![json!(sqlite_id), json!(":memory:")],
    )
    .await;

    let skip = [
        DB_EXPORT_QUERY_RESULT_TO_FILE,
        DB_QUERY_SAVE,
        SERVERS_REMOVE_BY_ID,
        USERS_DELETE,
    ];
    let events = all_event_names();
    assert!(events.len() > 60, "event list not parsed");
    for event in events.iter().filter(|e| !skip.contains(&e.as_str())) {
        for args in [
            vec![],
            vec![json!(true)],
            vec![json!(sqlite_id), json!(":memory:")],
            vec![json!(tunneled_id)],
            vec![json!(":memory:"), json!("SELECT 1")],
        ] {
            let resp = t.ipc_as(&admin, &s, event, args.clone()).await.to_string();
            for secret in &forbidden {
                assert!(
                    !resp.contains(secret.as_str()),
                    "{event} {args:?} leaked {secret}: {resp}"
                );
            }
        }
    }
}

#[tokio::test]
async fn decrypt_secrets_event_is_gone_and_config_reports_has_password() {
    let _g = support::lock().await;
    let t = TestApp::new();
    let admin = t.user("root", true);
    let (_, tunneled_id) = seed(&t);
    let s = WebIpcState::new(4);
    let gone = t
        .ipc_as(
            &admin,
            &s,
            "SERVERS_DECRYPT_SECRETS",
            vec![json!({}), json!("x")],
        )
        .await;
    assert!(
        gone["error"]
            .as_str()
            .unwrap()
            .contains("Unknown IPC event"),
        "{gone}"
    );

    let cfg = t
        .ipc_as(&admin, &s, CONFIG_GET_FULL, vec![json!(true)])
        .await;
    assert!(cfg["data"].get("crypto").is_none(), "{cfg}");
    let srv = cfg["data"]["servers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|v| v["id"] == tunneled_id)
        .unwrap();
    assert_eq!(srv["hasPassword"], true);
    assert_eq!(srv["sshHasPassword"], true);
    assert_eq!(srv["sshHasPrivateKey"], true);
    assert_eq!(srv["sshHasPassphrase"], true);
    assert!(srv.get("password").is_none() && srv["ssh"].get("password").is_none());
}

#[tokio::test]
async fn clients_cannot_send_keys_or_ciphertext_and_omitted_passwords_are_kept() {
    let _g = support::lock().await;
    let t = TestApp::new();
    let admin = t.user("root", true);
    let (_, id) = seed(&t);
    let s = WebIpcState::new(4);
    let new_srv = json!({ "name": "n", "client": "sqlite", "database": ":memory:", "ssl": false });
    let with_secret = t
        .ipc_as(&admin, &s, SERVERS_ADD, vec![new_srv, json!("client-key")])
        .await;
    assert!(
        with_secret["error"]
            .as_str()
            .unwrap()
            .contains("never accepted"),
        "{with_secret}"
    );
    let cipher = json!({ "name": "c", "client": "sqlite", "ssl": false,
        "password": { "ciphertext": "v2:AAAA" } });
    let refused = t.ipc_as(&admin, &s, SERVERS_ADD, vec![cipher]).await;
    assert!(
        refused["error"].as_str().unwrap().contains("plain text"),
        "{refused}"
    );
    let legacy = json!({ "name": "c", "client": "sqlite", "ssl": false,
        "password": { "ivText": "AAAA", "encryptedText": "BBBB" } });
    let refused = t.ipc_as(&admin, &s, SERVERS_ADD, vec![legacy]).await;
    assert!(
        refused["error"].as_str().unwrap().contains("pre-v2"),
        "{refused}"
    );

    let listed = t.ipc_as(&admin, &s, SERVERS_GET_ALL, vec![]).await;
    let mut edited: Value = listed["data"]
        .as_array()
        .unwrap()
        .iter()
        .find(|v| v["id"] == id)
        .unwrap()
        .clone();
    edited["name"] = json!("renamed");
    let saved = t.ipc_as(&admin, &s, SERVERS_UPDATE, vec![edited]).await;
    assert_eq!(saved["data"]["data"]["hasPassword"], true, "{saved}");
    let stored = servers::get_all_sync()
        .unwrap()
        .into_iter()
        .find(|s| s.id == id)
        .unwrap();
    let dec = servers::decrypt_secrets(&stored, &*t.keys).unwrap();
    assert_eq!(dec.name, "renamed");
    assert_eq!(dec.password, Some(Password::Plain(DB_PW.into())));
    assert_eq!(
        dec.ssh.unwrap().password,
        Some(Password::Plain(SSH_PW.into()))
    );
    let audit = t.app.db.list_audit(5).unwrap();
    assert!(audit
        .iter()
        .any(|a| a.event_type == "connection_updated"
            && a.connection_id.as_deref() == Some(id.as_str())));
}

fn scan(dir: &Path, needle: &str, hits: &mut Vec<String>) {
    for entry in std::fs::read_dir(dir).unwrap().flatten() {
        let p = entry.path();
        if p.is_dir() {
            scan(&p, needle, hits);
        } else if std::fs::read_to_string(&p).is_ok_and(|c| c.contains(needle)) {
            hits.push(p.display().to_string());
        }
    }
}

#[test]
fn default_crypto_secret_is_gone_from_src() {
    let mut hits = Vec::new();
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    for needle in [
        "DEFAULT_CRYPTO_SECRET",
        "j[F6Y6NoWT}",
        "SERVERS_DECRYPT_SECRETS",
    ] {
        scan(&src, needle, &mut hits);
    }
    assert!(hits.is_empty(), "{hits:?}");
}
