//! Encryption and decryption helpers for database and SSH passwords on [`Server`] profiles.

use crate::common::types::{Password, Server};
use crate::core::crypto;

/// Encrypts plaintext database and SSH passwords on `server` using `crypto_secret`.
pub fn encrypt_secrets(
    server: &Server,
    crypto_secret: &str,
    old_server: Option<&Server>,
) -> Result<Server, String> {
    let mut updated = server.clone();

    if let Some(ref pw) = server.password {
        if let Some(old) = old_server {
            if old.encrypted == Some(true) {
                if let Some(Password::Plain(ref old_plain)) = old.password {
                    if server.password == old.password {
                        let dec = crypto::unsafe_decrypt(old_plain, crypto_secret)
                            .map_err(|e| e.to_string())?;
                        updated.password = Some(Password::Plain(dec));
                    }
                }
            }
        }

        if let Some(Password::Plain(ref plain)) = updated.password {
            if !plain.is_empty() {
                let enc = crypto::encrypt(plain, crypto_secret).map_err(|e| e.to_string())?;
                updated.password = Some(Password::Encrypted(enc));
            }
        } else if let Password::Encrypted(_) = pw {
            updated.password = Some(pw.clone());
        }
    }

    if let Some(ref mut ssh) = updated.ssh {
        if let Some(ref ssh_pw) = ssh.password {
            if let Password::Plain(ref plain) = ssh_pw {
                if !plain.is_empty() {
                    let enc = crypto::encrypt(plain, crypto_secret).map_err(|e| e.to_string())?;
                    ssh.password = Some(Password::Encrypted(enc));
                }
            }
        }
    }

    updated.encrypted = Some(true);
    Ok(updated)
}

/// Decrypts encrypted database and SSH passwords on `server` into plaintext in memory.
pub fn decrypt_secrets(server: &Server, crypto_secret: &str) -> Result<Server, String> {
    let mut updated = server.clone();
    if server.encrypted != Some(true) {
        return Ok(updated);
    }

    if let Some(ref pw) = server.password {
        match pw {
            Password::Plain(s) => {
                if !s.is_empty() {
                    let dec =
                        crypto::unsafe_decrypt(s, crypto_secret).map_err(|e| e.to_string())?;
                    updated.password = Some(Password::Plain(dec));
                }
            }
            Password::Encrypted(enc) => {
                let dec = crypto::decrypt(enc, crypto_secret).map_err(|e| e.to_string())?;
                updated.password = Some(Password::Plain(dec));
            }
        }
    }

    if let Some(ref mut ssh) = updated.ssh {
        if let Some(ref ssh_pw) = ssh.password {
            match ssh_pw {
                Password::Plain(s) => {
                    if !s.is_empty() {
                        let dec =
                            crypto::unsafe_decrypt(s, crypto_secret).map_err(|e| e.to_string())?;
                        ssh.password = Some(Password::Plain(dec));
                    }
                }
                Password::Encrypted(enc) => {
                    let dec = crypto::decrypt(enc, crypto_secret).map_err(|e| e.to_string())?;
                    ssh.password = Some(Password::Plain(dec));
                }
            }
        }
    }

    updated.encrypted = Some(false);
    Ok(updated)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::types::{EncryptedPassword, SshConfig, SslConfig};

    const SECRET: &str = "CHK`Ya91Hs{me!^8ndwPPaPPxwQ}`";

    #[test]
    fn test_decrypt_secrets() {
        let encrypted_server = Server {
            id: "".into(),
            name: "mysql-vm".into(),
            client: "mysql".into(),
            ssl: Some(SslConfig::Bool(false)),
            host: Some("10.10.10.10".into()),
            port: Some(3306),
            database: Some("mydb".into()),
            user: Some("usr".into()),
            encrypted: Some(true),
            password: Some(Password::Encrypted(EncryptedPassword {
                iv_text: "wGf6X9T+QSygOHqtgQPlcA==".into(),
                encrypted_text: "0LySDs9WPAvwSS9Qv+W3/A==".into(),
            })),
            ..Default::default()
        };

        let dec = decrypt_secrets(&encrypted_server, SECRET).unwrap();
        assert_eq!(dec.encrypted, Some(false));
        assert_eq!(dec.password, Some(Password::Plain("password".into())));
    }

    #[test]
    fn test_decrypt_secrets_ssh() {
        let encrypted_server = Server {
            id: "".into(),
            name: "mysql-vm".into(),
            client: "mysql".into(),
            ssl: Some(SslConfig::Bool(false)),
            host: Some("10.10.10.10".into()),
            port: Some(3306),
            database: Some("mydb".into()),
            user: Some("usr".into()),
            encrypted: Some(true),
            password: Some(Password::Plain("".into())),
            ssh: Some(SshConfig {
                user: "usr".into(),
                host: "localhost".into(),
                port: 22,
                password: Some(Password::Encrypted(EncryptedPassword {
                    iv_text: "wGf6X9T+QSygOHqtgQPlcA==".into(),
                    encrypted_text: "0LySDs9WPAvwSS9Qv+W3/A==".into(),
                })),
                ..Default::default()
            }),
            ..Default::default()
        };

        let dec = decrypt_secrets(&encrypted_server, SECRET).unwrap();
        assert_eq!(dec.encrypted, Some(false));
        assert_eq!(
            dec.ssh.unwrap().password,
            Some(Password::Plain("password".into()))
        );
    }

    #[test]
    fn test_decrypt_old_style_password_throws() {
        let old = Server {
            name: "mysql-vm".into(),
            client: "mysql".into(),
            encrypted: Some(true),
            password: Some(Password::Plain("fa1d88ee82bd4439".into())),
            ..Default::default()
        };
        assert!(decrypt_secrets(&old, SECRET).is_err());
    }
}
