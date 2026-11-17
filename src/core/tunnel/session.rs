//! Opens an authenticated `russh` client session to the bastion, verifying its host key first.

use super::error::{Redactor, SshAuthMethod, TunnelError};
use super::host_keys::{evaluate, find_pin, status_to_result, PresentedHostKey};
use super::TunnelConfig;
use crate::common::types::SshHostKeyPin;
use russh::client::{self, AuthResult, Handle};
use russh::keys::agent::client::{AgentClient, AgentStream};
use russh::keys::agent::AgentIdentity;
use russh::keys::{
    decode_secret_key, Algorithm, HashAlg, PrivateKeyWithHashAlg, PublicKeyOrCertificate,
};
use std::borrow::Cow;
use std::sync::Arc;
use std::time::Duration;

/// Maximum time allowed for TCP connect plus SSH handshake and host-key check.
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(20);

/// Error type for the `russh` handler: either a protocol error or a host-key verdict.
#[derive(Debug)]
pub(crate) enum HandlerError {
    Ssh(russh::Error),
    HostKey(TunnelError),
}

impl From<russh::Error> for HandlerError {
    fn from(e: russh::Error) -> Self {
        HandlerError::Ssh(e)
    }
}

/// Client callbacks: only host-key verification is customized.
pub(crate) struct ClientHandler {
    host: String,
    port: u32,
    pins: Vec<SshHostKeyPin>,
}

impl client::Handler for ClientHandler {
    type Error = HandlerError;

    async fn check_server_key(
        &mut self,
        server_public_key: &PublicKeyOrCertificate,
    ) -> Result<bool, Self::Error> {
        let key_data = match server_public_key {
            PublicKeyOrCertificate::PublicKey { key, .. } => key.key_data().clone(),
            PublicKeyOrCertificate::Certificate(cert) => cert.public_key().clone(),
        };
        let presented = PresentedHostKey {
            host: self.host.clone(),
            port: self.port,
            key_algorithm: key_data.algorithm().as_str().to_string(),
            fingerprint_sha256: key_data.fingerprint(HashAlg::Sha256).to_string(),
        };
        let status = evaluate(&self.pins, &presented, time::OffsetDateTime::now_utc());
        status_to_result(status, &presented).map_err(HandlerError::HostKey)?;
        Ok(true)
    }
}

/// Restricts host-key negotiation to the pinned algorithm so a server offering several key
/// types cannot trigger a spurious "changed" verdict by negotiating a different one.
fn preferred_for_pin(pin: Option<&SshHostKeyPin>) -> russh::Preferred {
    let mut preferred = russh::Preferred::DEFAULT.clone();
    if let Some(pin) = pin {
        let keep: Vec<Algorithm> = preferred
            .key
            .iter()
            .filter(|a| match a {
                Algorithm::Rsa { .. } => pin.key_algorithm == "ssh-rsa",
                other => other.as_str() == pin.key_algorithm,
            })
            .cloned()
            .collect();
        if !keep.is_empty() {
            preferred.key = Cow::Owned(keep);
        }
    }
    preferred
}

/// Connects to the bastion, verifies its host key against `pins`, and authenticates.
pub(crate) async fn open_session(
    cfg: &TunnelConfig,
    pins: Vec<SshHostKeyPin>,
) -> Result<Handle<ClientHandler>, TunnelError> {
    let redactor = cfg.redactor();
    let connect_err = |reason: String| TunnelError::Connect {
        host: cfg.ssh_host.clone(),
        port: cfg.ssh_port,
        reason: redactor.redact(&reason),
    };
    let method = cfg.auth_method().ok_or_else(|| {
        TunnelError::InvalidConfig("No SSH authentication method configured".into())
    })?;

    let config = Arc::new(client::Config {
        preferred: preferred_for_pin(find_pin(&pins, &cfg.ssh_host, cfg.ssh_port)),
        inactivity_timeout: None,
        keepalive_interval: Some(Duration::from_secs(30)),
        ..Default::default()
    });
    let handler = ClientHandler {
        host: cfg.ssh_host.clone(),
        port: cfg.ssh_port,
        pins,
    };
    let port = u16::try_from(cfg.ssh_port).map_err(|_| connect_err("invalid port".into()))?;
    let addr = (cfg.ssh_host.as_str(), port);
    let mut handle =
        match tokio::time::timeout(HANDSHAKE_TIMEOUT, client::connect(config, addr, handler)).await
        {
            Err(_) => return Err(connect_err("timed out".into())),
            Ok(Err(HandlerError::HostKey(e))) => return Err(e),
            Ok(Err(HandlerError::Ssh(e))) => return Err(connect_err(e.to_string())),
            Ok(Ok(h)) => h,
        };

    let auth_err = |reason: String| TunnelError::AuthFailed {
        method,
        user: cfg.username.clone(),
        host: cfg.ssh_host.clone(),
        port: cfg.ssh_port,
        reason: redactor.redact(&reason),
    };
    let result = match method {
        SshAuthMethod::Password => {
            let pw = cfg
                .password
                .as_ref()
                .map(|p| p.as_str())
                .unwrap_or_default();
            handle
                .authenticate_password(cfg.username.clone(), pw)
                .await
                .map_err(|e| auth_err(e.to_string()))?
        }
        SshAuthMethod::PrivateKey => {
            let content = cfg
                .private_key_content
                .as_ref()
                .map(|c| c.as_str())
                .unwrap_or_default();
            let passphrase = cfg.passphrase.as_ref().map(|p| p.as_str());
            let key = decode_secret_key(content, passphrase).map_err(|_| {
                auth_err(
                    "the key could not be decoded (wrong passphrase or unsupported format)".into(),
                )
            })?;
            let hash = handle
                .best_supported_rsa_hash()
                .await
                .map_err(|e| auth_err(e.to_string()))?
                .flatten();
            handle
                .authenticate_publickey(
                    cfg.username.clone(),
                    PrivateKeyWithHashAlg::new(Arc::new(key), hash),
                )
                .await
                .map_err(|e| auth_err(e.to_string()))?
        }
        SshAuthMethod::Agent => {
            let sock = cfg
                .agent_sock
                .as_ref()
                .map(|s| s.as_str())
                .unwrap_or_default();
            let agent = connect_agent(sock)
                .await
                .map_err(|_| auth_err("could not connect to the SSH agent".into()))?;
            agent_auth(&mut handle, &cfg.username, agent, &redactor)
                .await
                .map_err(auth_err)?
        }
    };
    match result {
        AuthResult::Success => Ok(handle),
        AuthResult::Failure { .. } => Err(auth_err("the server rejected the credentials".into())),
    }
}

#[cfg(unix)]
async fn connect_agent(
    sock: &str,
) -> Result<AgentClient<tokio::net::UnixStream>, russh::keys::Error> {
    AgentClient::connect_uds(sock).await
}

#[cfg(windows)]
async fn connect_agent(
    sock: &str,
) -> Result<AgentClient<tokio::net::windows::named_pipe::NamedPipeClient>, russh::keys::Error> {
    AgentClient::connect_named_pipe(sock).await
}

/// Offers each key held by the agent until one is accepted. Fails if none is.
async fn agent_auth<S: AgentStream + Send + Unpin + 'static>(
    handle: &mut Handle<ClientHandler>,
    user: &str,
    mut agent: AgentClient<S>,
    redactor: &Redactor,
) -> Result<AuthResult, String> {
    let identities = agent
        .request_identities()
        .await
        .map_err(|e| redactor.redact(&format!("could not list agent keys: {e}")))?;
    let hash = handle
        .best_supported_rsa_hash()
        .await
        .map_err(|e| redactor.redact(&e.to_string()))?
        .flatten();
    let mut last = AuthResult::Failure {
        remaining_methods: russh::MethodSet::empty(),
        partial_success: false,
    };
    for identity in identities {
        let AgentIdentity::PublicKey { key, .. } = identity else {
            continue;
        };
        last = handle
            .authenticate_publickey_with(user, key, hash, &mut agent)
            .await
            .map_err(|e| redactor.redact(&format!("agent signing failed: {e}")))?;
        if matches!(last, AuthResult::Success) {
            break;
        }
    }
    Ok(last)
}
