//! SSH local port-forwarding tunnels through a bastion, using the `russh` client.
//!
//! - [`config_tunnel`] validates a [`Server`] profile into a [`TunnelConfig`] with exactly one
//!   authentication method (agent, then private key, then password).
//! - [`connect_tunnel`] authenticates to the bastion, verifies its host key against the
//!   trust-on-first-use pins in [`host_keys`], and forwards each local connection over a
//!   `direct-tcpip` channel. It never dials the database directly.

pub mod error;
mod handle;
pub mod host_keys;
mod session;

pub use error::{Redactor, SshAuthMethod, TunnelError};
pub use handle::{connect_tunnel, connect_tunnel_with_pins, TunnelHandle};
pub use host_keys::{approve_host_key, delete_host_key, load_pins};

use crate::common::types::{Password, Server};
use crate::core::config::resolve_home_path_to_absolute;
use std::fmt;
use zeroize::Zeroizing;

/// Validated SSH port-forwarding parameters extracted from a [`Server`] profile.
///
/// Secrets are held in [`Zeroizing`] buffers and are never printed by `Debug`.
#[derive(Clone)]
pub struct TunnelConfig {
    /// SSH username.
    pub username: String,
    /// SSH bastion host.
    pub ssh_host: String,
    /// SSH bastion port.
    pub ssh_port: u32,
    /// Target database host, resolved by the bastion.
    pub dst_host: String,
    /// Target database port, as seen from the bastion.
    pub dst_port: u32,
    /// SSH password, when password authentication is the configured method.
    pub password: Option<Zeroizing<String>>,
    /// Private key passphrase, when the key is encrypted.
    pub passphrase: Option<Zeroizing<String>>,
    /// Resolved absolute path to the private key file.
    pub private_key_path: Option<String>,
    /// PEM/OpenSSH private key file contents.
    pub private_key_content: Option<Zeroizing<String>>,
    /// Path to the SSH agent socket when agent authentication is enabled.
    pub agent_sock: Option<Zeroizing<String>>,
}

impl fmt::Debug for TunnelConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let hide = |set: bool| if set { "[REDACTED]" } else { "None" };
        f.debug_struct("TunnelConfig")
            .field("username", &self.username)
            .field("ssh_host", &self.ssh_host)
            .field("ssh_port", &self.ssh_port)
            .field("dst_host", &self.dst_host)
            .field("dst_port", &self.dst_port)
            .field("auth_method", &self.auth_method())
            .field("password", &hide(self.password.is_some()))
            .field("passphrase", &hide(self.passphrase.is_some()))
            .field("private_key_path", &self.private_key_path)
            .field(
                "private_key_content",
                &hide(self.private_key_content.is_some()),
            )
            .field("agent_sock", &hide(self.agent_sock.is_some()))
            .finish()
    }
}

impl TunnelConfig {
    /// Returns the single configured authentication method, in priority order.
    pub fn auth_method(&self) -> Option<SshAuthMethod> {
        if self.agent_sock.is_some() {
            Some(SshAuthMethod::Agent)
        } else if self.private_key_content.is_some() {
            Some(SshAuthMethod::PrivateKey)
        } else if self.password.is_some() {
            Some(SshAuthMethod::Password)
        } else {
            None
        }
    }

    /// Builds a [`Redactor`] that hides every secret in this configuration.
    pub fn redactor(&self) -> Redactor {
        let mut r = Redactor::default();
        for s in [
            &self.password,
            &self.passphrase,
            &self.private_key_content,
            &self.agent_sock,
        ]
        .into_iter()
        .flatten()
        {
            r.add(s);
        }
        r
    }
}

fn non_empty(s: Option<&str>) -> Option<&str> {
    s.filter(|v| !v.is_empty())
}

#[cfg(windows)]
fn agent_socket() -> Option<String> {
    Some(
        non_empty(std::env::var("SSH_AUTH_SOCK").ok().as_deref())
            .unwrap_or(r"\\.\pipe\openssh-ssh-agent")
            .to_string(),
    )
}

#[cfg(not(windows))]
fn agent_socket() -> Option<String> {
    non_empty(std::env::var("SSH_AUTH_SOCK").ok().as_deref()).map(str::to_string)
}

/// Validates and builds the SSH [`TunnelConfig`] for `server_info`.
///
/// Exactly one authentication method is selected: the SSH agent when `use_agent` is true,
/// otherwise the private key when a path is set, otherwise the password.
pub fn config_tunnel(server_info: &Server) -> Result<TunnelConfig, TunnelError> {
    let invalid = |m: &str| TunnelError::InvalidConfig(m.to_string());
    let dst_host = non_empty(server_info.host.as_deref())
        .ok_or_else(|| invalid("Host and port not specified for tunnel"))?
        .to_string();
    let dst_port = server_info
        .port
        .filter(|&p| p > 0 && p <= 65535)
        .ok_or_else(|| invalid("Host and port not specified for tunnel"))?;
    let ssh = server_info
        .ssh
        .as_ref()
        .ok_or_else(|| invalid("SSH information not specified"))?;
    if ssh.host.trim().is_empty() || ssh.user.trim().is_empty() {
        return Err(invalid("SSH host and user are required"));
    }
    let ssh_port = if ssh.port == 0 { 22 } else { ssh.port };
    if ssh_port > 65535 {
        return Err(invalid("SSH port must be between 1 and 65535"));
    }

    let mut cfg = TunnelConfig {
        username: ssh.user.clone(),
        ssh_host: ssh.host.clone(),
        ssh_port,
        dst_host,
        dst_port,
        password: None,
        passphrase: None,
        private_key_path: None,
        private_key_content: None,
        agent_sock: None,
    };

    if ssh.use_agent == Some(true) {
        let sock = agent_socket().ok_or_else(|| {
            invalid("SSH agent authentication is enabled but SSH_AUTH_SOCK is not set")
        })?;
        cfg.agent_sock = Some(Zeroizing::new(sock));
    } else if let Some(pk_path) = non_empty(ssh.private_key.as_deref()) {
        let resolved = resolve_home_path_to_absolute(pk_path);
        let content = std::fs::read_to_string(&resolved).map_err(|e| {
            TunnelError::InvalidConfig(format!("Cannot read SSH private key {resolved}: {e}"))
        })?;
        cfg.private_key_path = Some(resolved);
        cfg.private_key_content = Some(Zeroizing::new(content));
        cfg.passphrase = non_empty(ssh.passphrase.as_deref()).map(|p| Zeroizing::new(p.into()));
    } else if let Some(pw) = ssh.password.as_ref().filter(|p| !p.is_empty()) {
        match pw {
            Password::Plain(plain) => cfg.password = Some(Zeroizing::new(plain.clone())),
            Password::Encrypted(_) => {
                return Err(invalid(
                    "SSH password is still encrypted; decrypt it before connecting",
                ))
            }
        }
    } else {
        return Err(invalid(
            "No SSH authentication method configured: enable the SSH agent, set a private key, \
             or set a password",
        ));
    }
    Ok(cfg)
}

#[cfg(test)]
#[path = "config_tests.rs"]
mod config_tests;

#[cfg(test)]
pub(crate) mod test_server;

#[cfg(test)]
#[path = "tunnel_tests.rs"]
mod tunnel_tests;
