use crate::common::types::{Password, Server, ServerResult};
use crate::core::config;
use crate::core::crypto;
use crate::core::validators::{validate, validate_unique_id};
use uuid::Uuid;

pub async fn get_all() -> Result<Vec<Server>, String> {
    get_all_sync()
}

pub fn get_all_sync() -> Result<Vec<Server>, String> {
    let cfg = config::get_sync()?;
    Ok(cfg.servers)
}

pub async fn add(server: &Server, crypto_secret: &str) -> Result<ServerResult, String> {
    add_sync(server, crypto_secret)
}

pub fn add_sync(server: &Server, crypto_secret: &str) -> Result<ServerResult, String> {
    let mut srv = match validate(server) {
        Ok(s) => s,
        Err(e) => {
            return Ok(ServerResult {
                data: None,
                validation_errors: Some(e.validation_errors),
            });
        }
    };

    let mut data = config::get_sync()?;
    let new_id = loop {
        let candidate = Uuid::new_v4().to_string();
        if validate_unique_id(&data.servers, Some(&candidate)).unwrap_or(false) {
            break candidate;
        }
    };

    srv = encrypt_secrets(&srv, crypto_secret, None)?;
    srv.id = new_id;
    data.servers.push(srv.clone());
    config::save_sync(&data)?;

    Ok(ServerResult {
        data: Some(srv),
        validation_errors: None,
    })
}

pub async fn update(server: &Server, crypto_secret: &str) -> Result<ServerResult, String> {
    update_sync(server, crypto_secret)
}

pub fn update_sync(server: &Server, crypto_secret: &str) -> Result<ServerResult, String> {
    let mut srv = match validate(server) {
        Ok(s) => s,
        Err(e) => {
            return Ok(ServerResult {
                data: None,
                validation_errors: Some(e.validation_errors),
            });
        }
    };

    let mut data = config::get_sync()?;
    let index = data.servers.iter().position(|item| item.id == srv.id);
    let old_server = index.map(|i| &data.servers[i]);
    srv = encrypt_secrets(&srv, crypto_secret, old_server)?;

    if let Some(i) = index {
        data.servers[i] = srv.clone();
    } else {
        data.servers.push(srv.clone());
    }

    config::save_sync(&data)?;

    Ok(ServerResult {
        data: Some(srv),
        validation_errors: None,
    })
}

pub async fn add_or_update(server: &Server, crypto_secret: &str) -> Result<ServerResult, String> {
    add_or_update_sync(server, crypto_secret)
}

pub fn add_or_update_sync(server: &Server, crypto_secret: &str) -> Result<ServerResult, String> {
    let has_id = !server.id.trim().is_empty();
    if has_id {
        update_sync(server, crypto_secret)
    } else {
        add_sync(server, crypto_secret)
    }
}

pub async fn remove_by_id(id: &str) -> Result<(), String> {
    remove_by_id_sync(id)
}

pub fn remove_by_id_sync(id: &str) -> Result<(), String> {
    let mut data = config::get_sync()?;
    data.servers.retain(|srv| srv.id != id);
    config::save_sync(&data)
}

/// Generates a unique duplicate name by appending `-1`, `-2`, ... (`getUniqueName` in `src/renderer/actions/servers.ts`)
pub fn get_unique_name(server: &Server, existing_servers: &[Server]) -> String {
    let base_name = server.name.clone();
    let exists = |candidate: &str| existing_servers.iter().any(|s| s.name == candidate);
    if !exists(&base_name) {
        return base_name;
    }
    let mut num = 1usize;
    loop {
        let candidate = format!("{base_name}-{num}");
        if !exists(&candidate) {
            return candidate;
        }
        num += 1;
    }
}

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

/// Decrypts server passwords (`decryptSecrects` in `src/browser/core/servers.ts`)
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
