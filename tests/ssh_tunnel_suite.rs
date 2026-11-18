//! SSH tunnel tests against a real OpenSSH bastion and a PostgreSQL server behind it.
//!
//! Runs only when `RUST_SQL_TEST_SSH_HOST` is set, for example against the `ssh-bastion` and
//! `postgres-private` services in `docker-compose.yml` (see `docs/development/ssh-tunnel-tests.md`).
//! The database host is resolved by the bastion, so a database reachable only from the bastion
//! is queried successfully. Covers host-key approval, pinned reconnect, changed-key refusal,
//! refusal when the encrypted pin list is tampered with or the master key differs, and a wrong
//! password that must not appear in the error.

use rust_sql::adapters::DatabaseFacade;
use rust_sql::common::types::{Password, Server, SshConfig};
use rust_sql::core::config::{self, set_config_path_override};
use rust_sql::core::errors::DbCoreError;
use rust_sql::core::keys::{InMemoryKeyProvider, KeyProvider, SharedKeyProvider};
use rust_sql::core::tunnel::host_keys::{decrypt_pins, encrypt_pins};
use rust_sql::core::tunnel::{approve_host_key, SshAuthMethod, TunnelError};
use std::sync::Arc;

fn env(key: &str, default: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| default.to_string())
}

fn tunneled_server(bastion: &str, ssh_password: &str) -> Server {
    Server {
        name: "pg-behind-bastion".into(),
        client: "postgresql".into(),
        host: Some(env("RUST_SQL_TEST_SSH_DB_HOST", "postgres-private")),
        port: Some(
            env("RUST_SQL_TEST_SSH_DB_PORT", "5432")
                .parse()
                .expect("db port"),
        ),
        database: Some(env("RUST_SQL_TEST_PG_DB", "rust_sql")),
        user: Some(env("RUST_SQL_TEST_PG_USER", "postgres")),
        password: Some(Password::plain(env(
            "RUST_SQL_TEST_PG_PASSWORD",
            "password",
        ))),
        ssh: Some(SshConfig {
            host: bastion.into(),
            port: env("RUST_SQL_TEST_SSH_PORT", "2222")
                .parse()
                .expect("ssh port"),
            user: env("RUST_SQL_TEST_SSH_USER", "tunnel"),
            password: Some(Password::Plain(ssh_password.into())),
            ..Default::default()
        }),
        ..Default::default()
    }
}

async fn connect(
    server: &Server,
    keys: &Arc<InMemoryKeyProvider>,
) -> Result<DatabaseFacade, DbCoreError> {
    let mut facade = DatabaseFacade::new();
    facade.keys = SharedKeyProvider(keys.clone());
    facade
        .connect_checked(server.clone(), server.database.as_deref())
        .await?;
    Ok(facade)
}

#[tokio::test]
async fn tunnel_through_real_openssh_bastion() {
    let Ok(bastion) = std::env::var("RUST_SQL_TEST_SSH_HOST") else {
        eprintln!("skipped: RUST_SQL_TEST_SSH_HOST is not set");
        return;
    };
    let dir = tempfile::tempdir().expect("tempdir");
    set_config_path_override(Some(dir.path().join("rust-sql.json")));
    let ssh_password = env("RUST_SQL_TEST_SSH_PASSWORD", "tunnel-password");
    let server = tunneled_server(&bastion, &ssh_password);
    let db = server.database.clone().unwrap();
    let keys = Arc::new(InMemoryKeyProvider::new_for_tests());

    // Unknown bastion key: nothing connects until the user approves the fingerprint.
    let challenge = match connect(&server, &keys).await {
        Err(DbCoreError::Tunnel(TunnelError::HostKeyUnknown(ch))) => ch,
        Err(e) => panic!("expected HostKeyUnknown, got {e}"),
        Ok(_) => panic!("connected without approving the host key"),
    };
    assert!(challenge.fingerprint_sha256.starts_with("SHA256:"));
    approve_host_key(&challenge, &*keys).expect("approve");
    let on_disk = std::fs::read_to_string(dir.path().join("rust-sql.json")).unwrap();
    assert!(
        !on_disk.contains(&challenge.fingerprint_sha256),
        "pin stored in plaintext"
    );

    // Pinned: the query reaches a database that only the bastion can resolve.
    let facade = connect(&server, &keys)
        .await
        .expect("connect through bastion");
    let rows = facade
        .execute_query(&db, "SELECT 'via-bastion' AS marker")
        .await
        .expect("query through tunnel");
    let text = serde_json::to_string(&rows).unwrap();
    assert!(text.contains("via-bastion"), "{text}");

    // Wrong SSH password: an authentication error that does not contain the password.
    let bad = tunneled_server(&bastion, "Wr0ng-Tunnel-Pa55");
    match connect(&bad, &keys).await {
        Err(DbCoreError::Tunnel(e @ TunnelError::AuthFailed { .. })) => {
            assert!(matches!(
                e,
                TunnelError::AuthFailed {
                    method: SshAuthMethod::Password,
                    ..
                }
            ));
            assert!(!e.to_string().contains("Wr0ng-Tunnel-Pa55"));
        }
        Err(e) => panic!("expected AuthFailed, got {e}"),
        Ok(_) => panic!("wrong password connected"),
    }

    // Changed key: tamper with the pin and expect a hard refusal.
    let mut file = config::get_sync().expect("config");
    let pristine = file.ssh_host_keys.clone();
    let mut pins = decrypt_pins(file.ssh_host_keys.as_ref(), &*keys).expect("decrypt");
    pins[0].fingerprint_sha256 = "SHA256:not-the-real-key".into();
    file.ssh_host_keys = encrypt_pins(&pins, &*keys).expect("encrypt");
    config::save_sync(&file).expect("save");
    match connect(&server, &keys).await {
        Err(DbCoreError::Tunnel(TunnelError::HostKeyChanged { .. })) => {}
        Err(e) => panic!("expected HostKeyChanged, got {e}"),
        Ok(_) => panic!("connected despite a changed host key"),
    }

    // A different master key cannot read the pins: the tunnel is refused, not left unverified.
    file.ssh_host_keys = pristine;
    config::save_sync(&file).expect("save");
    let other = Arc::new(InMemoryKeyProvider::new_for_tests());
    assert!(other.master_key().is_ok());
    match connect(&server, &other).await {
        Err(DbCoreError::Tunnel(TunnelError::PinStore(_))) => {}
        Err(e) => panic!("expected PinStore, got {e}"),
        Ok(_) => panic!("connected with pins it could not decrypt"),
    }
    set_config_path_override(None);
}
