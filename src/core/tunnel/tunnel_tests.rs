//! End-to-end tunnel tests against the in-process bastion in [`super::test_server`].

use super::test_server::*;
use super::*;
use crate::common::types::{HostKeyChallengeKind, SshHostKeyPin};
use std::sync::atomic::Ordering;
use std::time::Duration;
use time::{Duration as TimeDuration, OffsetDateTime};

const PW: &str = "correct horse battery";

fn cfg(ssh_port: u16, dst_port: u16) -> TunnelConfig {
    TunnelConfig {
        username: "tester".into(),
        ssh_host: "127.0.0.1".into(),
        ssh_port: u32::from(ssh_port),
        dst_host: "127.0.0.1".into(),
        dst_port: u32::from(dst_port),
        password: Some(Zeroizing::new(PW.into())),
        passphrase: None,
        private_key_path: None,
        private_key_content: None,
        agent_sock: None,
    }
}

fn pin_for(port: u16, fp: String, age_days: i64) -> SshHostKeyPin {
    SshHostKeyPin {
        host: "127.0.0.1".into(),
        port: u32::from(port),
        key_algorithm: "ssh-ed25519".into(),
        fingerprint_sha256: fp,
        approved_at: OffsetDateTime::now_utc() - TimeDuration::days(age_days),
    }
}

async fn wait_until(mut f: impl FnMut() -> bool) -> bool {
    for _ in 0..100 {
        if f() {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    false
}

#[tokio::test]
async fn host_key_states_end_to_end() {
    let host_key = test_key(1);
    let fp = fingerprint(&host_key);
    let (ssh_port, _) = spawn_bastion(host_key, PW, None).await;
    let echo = spawn_echo().await;
    let c = cfg(ssh_port, echo);

    // Unknown: the error carries the real fingerprint so the UI can show it.
    let challenge = match connect_tunnel_with_pins(&c, vec![]).await {
        Err(TunnelError::HostKeyUnknown(ch)) => ch,
        other => panic!("expected HostKeyUnknown, got {other:?}"),
    };
    assert_eq!(challenge.fingerprint_sha256, fp);
    assert_eq!(challenge.kind, HostKeyChallengeKind::Unknown);

    // Approval then retry: matching pin connects.
    let mut pins = vec![];
    host_keys::apply_approval(&mut pins, &challenge, OffsetDateTime::now_utc()).unwrap();
    let tunnel = connect_tunnel_with_pins(&c, pins)
        .await
        .expect("trusted key connects");
    tunnel.close_and_wait().await;

    // Reconfirmation after 180 days.
    let old = vec![pin_for(ssh_port, fp.clone(), 181)];
    match connect_tunnel_with_pins(&c, old).await {
        Err(TunnelError::HostKeyReconfirmationRequired(ch)) => {
            assert_eq!(ch.fingerprint_sha256, fp)
        }
        other => panic!("expected reconfirmation, got {other:?}"),
    }

    // Changed: a different pinned key is a hard failure.
    let other = vec![pin_for(ssh_port, fingerprint(&test_key(2)), 1)];
    match connect_tunnel_with_pins(&c, other).await {
        Err(e @ TunnelError::HostKeyChanged { .. }) => assert!(e.host_key_challenge().is_none()),
        other => panic!("expected HostKeyChanged, got {other:?}"),
    }
}

#[tokio::test]
async fn forwards_over_direct_tcpip_and_survives_channel_errors() {
    let host_key = test_key(3);
    let fp = fingerprint(&host_key);
    let (ssh_port, state) = spawn_bastion(host_key, PW, None).await;
    let echo = spawn_echo().await;
    let tunnel = connect_tunnel_with_pins(&cfg(ssh_port, echo), vec![pin_for(ssh_port, fp, 0)])
        .await
        .unwrap();

    assert_eq!(round_trip(tunnel.local_port(), b"ping").await, b"ping");
    let expected = format!("127.0.0.1:{echo}");
    assert!(state.channel_requests.lock().unwrap().contains(&expected));

    // A refused channel closes only that connection; the listener keeps serving.
    state.reject_channels.store(true, Ordering::SeqCst);
    assert!(round_trip(tunnel.local_port(), b"x").await.is_empty());
    state.reject_channels.store(false, Ordering::SeqCst);
    assert_eq!(round_trip(tunnel.local_port(), b"again").await, b"again");

    // Closing stops the listener and tears down the SSH session.
    let port = tunnel.local_port();
    assert_eq!(state.live_sessions.load(Ordering::SeqCst), 1);
    tunnel.close_and_wait().await;
    assert!(tokio::net::TcpStream::connect(("127.0.0.1", port))
        .await
        .is_err());
    assert!(wait_until(|| state.live_sessions.load(Ordering::SeqCst) == 0).await);
}

#[tokio::test]
async fn drop_tears_down_session() {
    let host_key = test_key(4);
    let fp = fingerprint(&host_key);
    let (ssh_port, state) = spawn_bastion(host_key, PW, None).await;
    let tunnel = connect_tunnel_with_pins(&cfg(ssh_port, 9), vec![pin_for(ssh_port, fp, 0)])
        .await
        .unwrap();
    drop(tunnel);
    assert!(wait_until(|| state.live_sessions.load(Ordering::SeqCst) == 0).await);
}

#[tokio::test]
async fn wrong_password_is_an_auth_error_without_the_password() {
    let host_key = test_key(5);
    let fp = fingerprint(&host_key);
    let (ssh_port, _) = spawn_bastion(host_key, PW, None).await;
    let mut c = cfg(ssh_port, 9);
    c.password = Some(Zeroizing::new("Wr0ng-Pa55word!".into()));
    let err = connect_tunnel_with_pins(&c, vec![pin_for(ssh_port, fp, 0)])
        .await
        .unwrap_err();
    assert!(matches!(
        err,
        TunnelError::AuthFailed {
            method: SshAuthMethod::Password,
            ..
        }
    ));
    let text = err.to_string();
    assert!(
        text.contains("password") && !text.contains("Wr0ng-Pa55word!"),
        "{text}"
    );
}

#[tokio::test]
async fn private_key_auth_and_wrong_passphrase() {
    let host_key = test_key(6);
    let fp = fingerprint(&host_key);
    let client = test_key(7);
    let (ssh_port, _) = spawn_bastion(host_key, PW, Some(client.public_key().clone())).await;
    let echo = spawn_echo().await;
    let pins = vec![pin_for(ssh_port, fp, 0)];

    let mut c = cfg(ssh_port, echo);
    c.password = None;
    let pem = client
        .to_openssh(russh::keys::ssh_key::LineEnding::LF)
        .unwrap();
    c.private_key_content = Some(Zeroizing::new(pem.to_string()));
    let tunnel = connect_tunnel_with_pins(&c, pins.clone()).await.unwrap();
    assert_eq!(round_trip(tunnel.local_port(), b"key").await, b"key");
    tunnel.close_and_wait().await;

    let encrypted = client.encrypt(&mut rand::rng(), "right-phrase").unwrap();
    let pem = encrypted
        .to_openssh(russh::keys::ssh_key::LineEnding::LF)
        .unwrap();
    c.private_key_content = Some(Zeroizing::new(pem.to_string()));
    c.passphrase = Some(Zeroizing::new("Wr0ng-Phrase".into()));
    let err = connect_tunnel_with_pins(&c, pins.clone())
        .await
        .unwrap_err();
    assert!(matches!(
        err,
        TunnelError::AuthFailed {
            method: SshAuthMethod::PrivateKey,
            ..
        }
    ));
    assert!(!err.to_string().contains("Wr0ng-Phrase"));

    c.passphrase = Some(Zeroizing::new("right-phrase".into()));
    let tunnel = connect_tunnel_with_pins(&c, pins).await.unwrap();
    tunnel.close_and_wait().await;
}

#[cfg(unix)]
#[tokio::test]
async fn agent_auth_uses_agent_keys_only() {
    use russh::keys::agent::client::AgentClient;
    let host_key = test_key(8);
    let fp = fingerprint(&host_key);
    let client = test_key(9);
    let (ssh_port, _) = spawn_bastion(host_key, PW, Some(client.public_key().clone())).await;
    let dir = tempfile::tempdir().unwrap();
    let sock = dir.path().join("agent.sock");
    let listener = tokio::net::UnixListener::bind(&sock).unwrap();
    let incoming = futures::stream::unfold(listener, |l| async move {
        Some((l.accept().await.map(|(s, _)| s), l))
    });
    tokio::spawn(russh::keys::agent::server::serve(
        Box::pin(incoming),
        TestAgent,
    ));
    let mut agent = AgentClient::connect_uds(&sock).await.unwrap();
    let mut c = cfg(ssh_port, 9);
    c.password = None;
    c.agent_sock = Some(Zeroizing::new(sock.to_string_lossy().into_owned()));
    let pins = vec![pin_for(ssh_port, fp, 0)];

    // Empty agent: fails as an agent error and does not fall back to anything else.
    let err = connect_tunnel_with_pins(&c, pins.clone())
        .await
        .unwrap_err();
    assert!(
        matches!(
            err,
            TunnelError::AuthFailed {
                method: SshAuthMethod::Agent,
                ..
            }
        ),
        "{err}"
    );
    assert!(!err.to_string().contains("agent.sock"));

    agent.add_identity(&client, &[]).await.unwrap();
    let tunnel = connect_tunnel_with_pins(&c, pins).await.unwrap();
    tunnel.close_and_wait().await;
}

#[cfg(unix)]
#[derive(Clone)]
struct TestAgent;

#[cfg(unix)]
impl russh::keys::agent::server::Agent for TestAgent {}
