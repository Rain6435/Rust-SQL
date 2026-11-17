//! Store-level host-key prompt tests against the in-process SSH bastion.

use crate::common::types::{HostKeyChallengeKind, Password, Server, SshConfig};
use crate::core::config::{self, set_config_path_override};
use crate::core::tunnel::test_server::{fingerprint, spawn_bastion, test_key};
use crate::state::AppStore;

#[test]
fn approve_without_prompt_is_an_error() {
    let mut store = AppStore::new();
    assert!(store.approve_host_key().is_err());
}

#[test]
fn test_connection_prompts_then_approval_pins_the_key() {
    // Hold the config lock for the whole test: TuiApp::new in other tests writes the config file.
    let _cfg = config::TEST_CONFIG_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(prompt_then_approval());
}

async fn prompt_then_approval() {
    let host_key = test_key(42);
    let fp = fingerprint(&host_key);
    let (ssh_port, state) = spawn_bastion(host_key, "pw-for-store", None).await;
    let dir = tempfile::tempdir().unwrap();
    set_config_path_override(Some(dir.path().join("rust-sql.json")));

    let server = Server {
        name: "behind-bastion".into(),
        client: "postgresql".into(),
        host: Some("127.0.0.1".into()),
        port: Some(9),
        database: Some("postgres".into()),
        user: Some("app".into()),
        ssh: Some(SshConfig {
            host: "127.0.0.1".into(),
            port: u32::from(ssh_port),
            user: "tester".into(),
            password: Some(Password::plain("pw-for-store")),
            ..Default::default()
        }),
        ..Default::default()
    };
    let mut store = AppStore::new();

    // First attempt: unknown key becomes a prompt; nothing is pinned and nothing is forwarded.
    let err = store.test_connection(server.clone()).await.unwrap_err();
    assert!(err.contains(&fp), "{err}");
    let prompt = store.connections.host_key_prompt.clone().expect("prompt");
    assert_eq!(prompt.kind, HostKeyChallengeKind::Unknown);
    assert_eq!(prompt.fingerprint_sha256, fp);
    assert!(config::get_sync().unwrap().ssh_host_keys.is_empty());
    assert!(state.channel_requests.lock().unwrap().is_empty());

    // Approval pins the key; the retry gets through SSH and only fails at the fake database.
    assert_eq!(store.approve_host_key(), Ok(None));
    let pins = config::get_sync().unwrap().ssh_host_keys;
    assert_eq!(pins.len(), 1);
    assert_eq!(pins[0].fingerprint_sha256, fp);
    let err = store.test_connection(server).await.unwrap_err();
    assert!(store.connections.host_key_prompt.is_none());
    assert!(!err.contains("SSH host key"), "{err}");
    assert_eq!(
        state.channel_requests.lock().unwrap().as_slice(),
        ["127.0.0.1:9"]
    );

    // Deleting the pin is the only way back to a prompt.
    assert_eq!(
        store.delete_host_key("127.0.0.1", u32::from(ssh_port)),
        Ok(true)
    );
    assert!(config::get_sync().unwrap().ssh_host_keys.is_empty());

    // Both the approval and the deletion are in the audit log, attributed to the local user.
    let audit = crate::core::appdb::AppDb::open_default()
        .unwrap()
        .list_audit(10)
        .unwrap();
    let kinds: Vec<_> = audit.iter().map(|r| r.event_type.as_str()).collect();
    assert_eq!(kinds, ["host_key_deleted", "host_key_approved"]);
    assert!(audit[1].detail.as_deref().unwrap_or("").contains(&fp));
    assert!(audit.iter().all(|r| r.username.as_deref() == Some("local")));
    set_config_path_override(None);
}
