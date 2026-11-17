//! In-process SSH bastion and helpers used only by the tunnel unit tests.
//!
//! The bastion accepts password and public-key authentication for user `tester`, serves
//! `direct-tcpip` channels by dialing the requested target, and records every channel request
//! and live session so tests can prove traffic went through SSH and that sessions are torn down.

use russh::keys::ssh_key::private::Ed25519Keypair;
use russh::keys::{HashAlg, PrivateKey, PublicKey};
use russh::server::{self, Auth, ChannelOpenHandle, Msg, Session, VerifiedPublicKey};
use russh::{Channel, ChannelOpenFailure};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{copy_bidirectional, AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

/// Deterministic Ed25519 key for tests; different seeds give different keys.
pub(crate) fn test_key(seed: u8) -> PrivateKey {
    PrivateKey::from(Ed25519Keypair::from_seed(&[seed; 32]))
}

/// OpenSSH `SHA256:` fingerprint of `key`'s public half.
pub(crate) fn fingerprint(key: &PrivateKey) -> String {
    key.public_key().fingerprint(HashAlg::Sha256).to_string()
}

/// Shared, observable state of a running test bastion.
#[derive(Clone, Default)]
pub(crate) struct BastionState {
    pub channel_requests: Arc<Mutex<Vec<String>>>,
    pub live_sessions: Arc<AtomicUsize>,
    pub reject_channels: Arc<AtomicBool>,
}

#[derive(Clone)]
struct BastionHandler {
    password: String,
    client_key: Option<PublicKey>,
    state: BastionState,
}

impl server::Handler for BastionHandler {
    type Error = russh::Error;

    async fn auth_password(&mut self, user: &str, password: &str) -> Result<Auth, Self::Error> {
        Ok(if user == "tester" && password == self.password {
            Auth::Accept
        } else {
            Auth::reject()
        })
    }

    async fn auth_publickey(
        &mut self,
        user: &str,
        key: VerifiedPublicKey<'_>,
    ) -> Result<Auth, Self::Error> {
        let ok = user == "tester"
            && self
                .client_key
                .as_ref()
                .is_some_and(|k| k.key_data() == key.public_key.key_data());
        Ok(if ok { Auth::Accept } else { Auth::reject() })
    }

    async fn channel_open_direct_tcpip(
        &mut self,
        channel: Channel<Msg>,
        host_to_connect: &str,
        port_to_connect: u32,
        _originator_address: &str,
        _originator_port: u32,
        reply: ChannelOpenHandle,
        _session: &mut Session,
    ) -> Result<(), Self::Error> {
        let target = format!("{host_to_connect}:{port_to_connect}");
        self.state
            .channel_requests
            .lock()
            .unwrap()
            .push(target.clone());
        if self.state.reject_channels.load(Ordering::SeqCst) {
            reply
                .reject(ChannelOpenFailure::AdministrativelyProhibited)
                .await;
            return Ok(());
        }
        match TcpStream::connect(&target).await {
            Ok(mut tcp) => {
                reply.accept().await;
                tokio::spawn(async move {
                    let mut stream = channel.into_stream();
                    let _ = copy_bidirectional(&mut stream, &mut tcp).await;
                });
            }
            Err(_) => reply.reject(ChannelOpenFailure::ConnectFailed).await,
        }
        Ok(())
    }
}

/// Starts a bastion on `127.0.0.1:0` with `host_key`. Returns its port and observable state.
pub(crate) async fn spawn_bastion(
    host_key: PrivateKey,
    password: &str,
    client_key: Option<PublicKey>,
) -> (u16, BastionState) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let state = BastionState::default();
    let handler = BastionHandler {
        password: password.to_string(),
        client_key,
        state: state.clone(),
    };
    let config = Arc::new(server::Config {
        keys: vec![host_key],
        auth_rejection_time: Duration::from_millis(5),
        auth_rejection_time_initial: Some(Duration::ZERO),
        ..Default::default()
    });
    tokio::spawn(async move {
        while let Ok((sock, _)) = listener.accept().await {
            let (cfg, h) = (config.clone(), handler.clone());
            let live = h.state.live_sessions.clone();
            tokio::spawn(async move {
                live.fetch_add(1, Ordering::SeqCst);
                if let Ok(running) = server::run_stream(cfg, sock, h).await {
                    let _ = running.await;
                }
                live.fetch_sub(1, Ordering::SeqCst);
            });
        }
    });
    (port, state)
}

/// Starts a TCP echo server standing in for a database. Returns its port.
pub(crate) async fn spawn_echo() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        while let Ok((mut sock, _)) = listener.accept().await {
            tokio::spawn(async move {
                let mut buf = [0u8; 1024];
                while let Ok(n) = sock.read(&mut buf).await {
                    if n == 0 || sock.write_all(&buf[..n]).await.is_err() {
                        break;
                    }
                }
            });
        }
    });
    port
}

/// Sends `msg` to `127.0.0.1:port` and returns what comes back (empty if the peer closed).
pub(crate) async fn round_trip(port: u16, msg: &[u8]) -> Vec<u8> {
    let mut sock = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    sock.write_all(msg).await.unwrap();
    let mut buf = vec![0u8; msg.len()];
    match tokio::time::timeout(Duration::from_secs(5), sock.read_exact(&mut buf)).await {
        Ok(Ok(_)) => buf,
        _ => Vec::new(),
    }
}
