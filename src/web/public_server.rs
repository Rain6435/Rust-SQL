//! Client-facing view of a saved connection with every secret removed.
//!
//! Database passwords, SSH passwords, SSH private keys and passphrases, and SSL client keys are
//! replaced by `has*` booleans. Web responses only ever contain [`PublicServer`], never
//! [`Server`], so neither ciphertext nor plaintext leaves the server.

use crate::common::types::{Password, Server, SslConfig};
use serde::Serialize;

/// Which secrets a saved connection holds, without their values.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SecretFlags {
    /// A database password is saved.
    pub has_password: bool,
    /// An SSH password is saved.
    pub ssh_has_password: bool,
    /// An SSH private key is configured.
    pub ssh_has_private_key: bool,
    /// An SSH key passphrase is saved.
    pub ssh_has_passphrase: bool,
    /// An SSL client key is configured.
    pub ssl_has_key: bool,
}

/// A saved connection as sent to web clients.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PublicServer {
    /// The profile with all secret fields cleared.
    #[serde(flatten)]
    pub server: Server,
    /// What secrets exist.
    #[serde(flatten)]
    pub secrets: SecretFlags,
}

fn present(pw: &Option<Password>) -> bool {
    pw.as_ref().is_some_and(|p| !p.is_empty())
}

impl From<&Server> for PublicServer {
    fn from(src: &Server) -> Self {
        let mut server = src.clone();
        let mut secrets = SecretFlags {
            has_password: present(&server.password),
            ..Default::default()
        };
        server.password = None;
        server.encrypted = None;
        server.local_host = None;
        server.local_port = None;
        if let Some(ssh) = server.ssh.as_mut() {
            secrets.ssh_has_password = present(&ssh.password);
            secrets.ssh_has_private_key = ssh.private_key.as_ref().is_some_and(|k| !k.is_empty());
            secrets.ssh_has_passphrase = ssh.passphrase.as_ref().is_some_and(|p| !p.is_empty());
            ssh.password = None;
            ssh.private_key = None;
            ssh.passphrase = None;
        }
        if let Some(SslConfig::Details(d)) = server.ssl.as_mut() {
            secrets.ssl_has_key = d.key.as_ref().is_some_and(|k| !k.is_empty());
            d.key = None;
        }
        Self { server, secrets }
    }
}

/// Masks a list of servers.
pub fn mask_all<'a>(servers: impl IntoIterator<Item = &'a Server>) -> Vec<PublicServer> {
    servers.into_iter().map(PublicServer::from).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::types::{EncryptedPassword, SshConfig, SslDetails};

    #[test]
    fn every_secret_is_replaced_by_a_flag() {
        let srv = Server {
            id: "c1".into(),
            name: "prod".into(),
            password: Some(Password::Encrypted(EncryptedPassword {
                iv_text: "IVIVIV".into(),
                encrypted_text: "CIPHERTEXT".into(),
            })),
            encrypted: Some(true),
            ssh: Some(SshConfig {
                user: "u".into(),
                host: "bastion".into(),
                port: 22,
                password: Some(Password::Plain("ssh-plain-secret".into())),
                passphrase: Some("key-passphrase".into()),
                private_key: Some("/home/u/.ssh/id_ed25519".into()),
                ..Default::default()
            }),
            ssl: Some(SslConfig::Details(SslDetails {
                key: Some("-----BEGIN PRIVATE KEY-----".into()),
                ..Default::default()
            })),
            ..Default::default()
        };
        let json = serde_json::to_string(&PublicServer::from(&srv)).unwrap();
        for secret in [
            "CIPHERTEXT",
            "IVIVIV",
            "ssh-plain-secret",
            "key-passphrase",
            "id_ed25519",
            "PRIVATE KEY",
        ] {
            assert!(!json.contains(secret), "{secret} leaked: {json}");
        }
        for flag in [
            "\"hasPassword\":true",
            "\"sshHasPassword\":true",
            "\"sshHasPrivateKey\":true",
            "\"sshHasPassphrase\":true",
            "\"sslHasKey\":true",
        ] {
            assert!(json.contains(flag), "{flag} missing: {json}");
        }
        assert!(!json.contains("\"password\""));
    }
}
