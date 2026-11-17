//! Typed SSH tunnel errors and secret redaction.
//!
//! Every message built from a foreign error passes through [`Redactor`], so passwords,
//! passphrases, private key material and the agent socket path never reach errors or logs.

use crate::common::types::HostKeyChallenge;
use std::fmt;
use thiserror::Error;

/// The SSH authentication method that was configured and attempted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SshAuthMethod {
    /// Keys held by the local SSH agent (`SSH_AUTH_SOCK`).
    Agent,
    /// A private key file, with an optional passphrase.
    PrivateKey,
    /// A password.
    Password,
}

impl fmt::Display for SshAuthMethod {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            SshAuthMethod::Agent => "SSH agent",
            SshAuthMethod::PrivateKey => "private key",
            SshAuthMethod::Password => "password",
        })
    }
}

/// Errors returned while building or running an SSH tunnel.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum TunnelError {
    /// The server profile has incomplete or contradictory SSH settings.
    #[error("Invalid SSH tunnel configuration: {0}")]
    InvalidConfig(String),
    /// The bastion could not be reached or the SSH handshake failed.
    #[error("Could not reach SSH bastion {host}:{port}: {reason}")]
    Connect {
        /// Bastion host.
        host: String,
        /// Bastion port.
        port: u32,
        /// Redacted cause.
        reason: String,
    },
    /// The bastion's host key has never been approved.
    #[error("SSH host key unknown. {}", .0.prompt_text())]
    HostKeyUnknown(HostKeyChallenge),
    /// The pin is older than the re-confirmation period and must be confirmed again.
    #[error("SSH host key needs re-confirmation. {}", .0.prompt_text())]
    HostKeyReconfirmationRequired(HostKeyChallenge),
    /// The bastion presented a different key than the pinned one. Never overridable in-app.
    #[error(
        "SSH HOST KEY CHANGED for {host}:{port}. Expected {expected}, but the server presented \
         {presented}. Refusing to connect: this can mean someone is intercepting the connection. \
         If your administrator confirms the key was rotated, delete the pin for this host in \
         Settings and connect again."
    )]
    HostKeyChanged {
        /// Bastion host.
        host: String,
        /// Bastion port.
        port: u32,
        /// Pinned `algorithm fingerprint`.
        expected: String,
        /// Presented `algorithm fingerprint`.
        presented: String,
    },
    /// The configured authentication method was rejected or could not be used.
    #[error("SSH authentication with the {method} failed for user \"{user}\" on {host}:{port}: {reason}")]
    AuthFailed {
        /// Method that failed. No other method is tried afterwards.
        method: SshAuthMethod,
        /// SSH user name.
        user: String,
        /// Bastion host.
        host: String,
        /// Bastion port.
        port: u32,
        /// Redacted cause.
        reason: String,
    },
    /// The host-key pin store could not be read or written.
    #[error("SSH host key store error: {0}")]
    PinStore(String),
}

impl TunnelError {
    /// Returns the approval challenge when the error asks the user to approve a host key.
    pub fn host_key_challenge(&self) -> Option<&HostKeyChallenge> {
        match self {
            TunnelError::HostKeyUnknown(c) | TunnelError::HostKeyReconfirmationRequired(c) => {
                Some(c)
            }
            _ => None,
        }
    }
}

/// Removes known secret values from text before it is placed in an error or log line.
#[derive(Default, Clone)]
pub struct Redactor {
    secrets: Vec<String>,
}

impl fmt::Debug for Redactor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Redactor({} secrets)", self.secrets.len())
    }
}

impl Drop for Redactor {
    fn drop(&mut self) {
        use zeroize::Zeroize;
        for s in &mut self.secrets {
            s.zeroize();
        }
    }
}

impl Redactor {
    /// Registers a secret. Values shorter than 3 characters are still redacted.
    pub fn add(&mut self, secret: &str) {
        if !secret.is_empty() {
            self.secrets.push(secret.to_string());
            // Also redact each non-trivial line of multi-line material such as PEM keys.
            for line in secret.lines().map(str::trim).filter(|l| l.len() >= 16) {
                self.secrets.push(line.to_string());
            }
        }
    }

    /// Returns `text` with every registered secret replaced by `[REDACTED]`.
    pub fn redact(&self, text: &str) -> String {
        let mut out = text.to_string();
        let mut sorted: Vec<&String> = self.secrets.iter().collect();
        sorted.sort_by_key(|s| std::cmp::Reverse(s.len()));
        for s in sorted {
            if out.contains(s.as_str()) {
                out = out.replace(s.as_str(), "[REDACTED]");
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redactor_removes_secrets_and_key_lines() {
        let mut r = Redactor::default();
        r.add("hunter2-password");
        r.add("-----BEGIN KEY-----\nAAAAB3NzaC1yc2EAAAADAQABAAABAQ\n-----END KEY-----");
        r.add("/tmp/ssh-XYZ/agent.123");
        let msg =
            "bad hunter2-password at /tmp/ssh-XYZ/agent.123 near AAAAB3NzaC1yc2EAAAADAQABAAABAQ";
        let out = r.redact(msg);
        assert!(!out.contains("hunter2"));
        assert!(!out.contains("agent.123"));
        assert!(!out.contains("AAAAB3Nza"));
        assert_eq!(out.matches("[REDACTED]").count(), 3);
    }

    #[test]
    fn auth_error_names_the_method() {
        let e = TunnelError::AuthFailed {
            method: SshAuthMethod::PrivateKey,
            user: "deploy".into(),
            host: "bastion".into(),
            port: 22,
            reason: "rejected".into(),
        };
        assert!(e.to_string().contains("private key"));
    }
}
