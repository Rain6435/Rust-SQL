//! Encryption and decryption helpers for database and SSH passwords on [`Server`] profiles.
//!
//! The master key comes from a [`KeyProvider`] and is requested only when a password actually
//! has to be encrypted or decrypted, so profiles without saved passwords never need the key.
//! When the key is unavailable, these functions return the provider's error; they never fall
//! back to a default key or return the ciphertext as if it were the password.

use crate::common::types::{Password, Server};
use crate::core::crypto;
use crate::core::keys::{KeyProvider, MasterKey};

/// Lazily fetches the master key at most once per call.
struct LazyKey<'a> {
    provider: &'a dyn KeyProvider,
    key: Option<MasterKey>,
}

impl<'a> LazyKey<'a> {
    fn new(provider: &'a dyn KeyProvider) -> Self {
        Self {
            provider,
            key: None,
        }
    }

    fn get(&mut self) -> Result<&MasterKey, String> {
        if self.key.is_none() {
            self.key = Some(self.provider.master_key().map_err(|e| e.to_string())?);
        }
        Ok(self.key.as_ref().expect("key was just set"))
    }
}

fn encrypt_plain(pw: &Option<Password>, key: &mut LazyKey<'_>) -> Result<Option<Password>, String> {
    match pw {
        Some(Password::Plain(plain)) if !plain.is_empty() => {
            let enc = crypto::encrypt(plain, key.get()?).map_err(|e| e.to_string())?;
            Ok(Some(Password::Encrypted(enc)))
        }
        other => Ok(other.clone()),
    }
}

fn decrypt_value(pw: &Option<Password>, key: &mut LazyKey<'_>) -> Result<Option<Password>, String> {
    match pw {
        Some(Password::Encrypted(enc)) => {
            let dec = crypto::decrypt(enc, key.get()?).map_err(|e| e.to_string())?;
            Ok(Some(Password::Plain(dec)))
        }
        Some(Password::Plain(s)) if !s.is_empty() => {
            let dec = crypto::unsafe_decrypt(s, key.get()?).map_err(|e| e.to_string())?;
            Ok(Some(Password::Plain(dec)))
        }
        other => Ok(other.clone()),
    }
}

/// Encrypts plaintext database and SSH passwords on `server` with the provider's master key.
///
/// `old_server` is accepted for signature compatibility; an unchanged legacy plaintext value
/// on an encrypted record is rejected by [`crypto::unsafe_decrypt`].
pub fn encrypt_secrets(
    server: &Server,
    keys: &dyn KeyProvider,
    old_server: Option<&Server>,
) -> Result<Server, String> {
    let mut key = LazyKey::new(keys);
    let mut updated = server.clone();

    if let (Some(old), Some(Password::Plain(old_plain))) =
        (old_server, old_server.and_then(|o| o.password.as_ref()))
    {
        if old.encrypted == Some(true) && server.password == old.password {
            let dec = crypto::unsafe_decrypt(old_plain, key.get()?).map_err(|e| e.to_string())?;
            updated.password = Some(Password::Plain(dec));
        }
    }

    updated.password = encrypt_plain(&updated.password, &mut key)?;
    if let Some(ref mut ssh) = updated.ssh {
        ssh.password = encrypt_plain(&ssh.password, &mut key)?;
    }
    updated.encrypted = Some(true);
    Ok(updated)
}

/// Decrypts encrypted database and SSH passwords on `server` into plaintext in memory.
pub fn decrypt_secrets(server: &Server, keys: &dyn KeyProvider) -> Result<Server, String> {
    let mut updated = server.clone();
    if server.encrypted != Some(true) {
        return Ok(updated);
    }
    let mut key = LazyKey::new(keys);
    updated.password = decrypt_value(&server.password, &mut key)?;
    if let Some(ref mut ssh) = updated.ssh {
        ssh.password = decrypt_value(&ssh.password, &mut key)?;
    }
    updated.encrypted = Some(false);
    Ok(updated)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::types::SshConfig;
    use crate::core::keys::{InMemoryKeyProvider, OsKeychainProvider};

    fn server(pw: &str, ssh_pw: Option<&str>) -> Server {
        Server {
            name: "pg".into(),
            client: "postgresql".into(),
            password: Some(Password::Plain(pw.into())),
            ssh: ssh_pw.map(|p| SshConfig {
                user: "u".into(),
                host: "bastion".into(),
                port: 22,
                password: Some(Password::Plain(p.into())),
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    #[test]
    fn round_trip_database_and_ssh_passwords() {
        let keys = InMemoryKeyProvider::new_for_tests();
        let enc = encrypt_secrets(&server("db-pw", Some("ssh-pw")), &keys, None).unwrap();
        assert!(matches!(enc.password, Some(Password::Encrypted(_))));
        let dec = decrypt_secrets(&enc, &keys).unwrap();
        assert_eq!(dec.password, Some(Password::Plain("db-pw".into())));
        assert_eq!(
            dec.ssh.unwrap().password,
            Some(Password::Plain("ssh-pw".into()))
        );
    }

    #[test]
    fn unavailable_keychain_refuses_to_store_or_decrypt_passwords() {
        let err = encrypt_secrets(&server("db-pw", None), &OsKeychainProvider, None).unwrap_err();
        assert!(err.contains("keychain"), "{err}");

        let keys = InMemoryKeyProvider::new_for_tests();
        let enc = encrypt_secrets(&server("db-pw", None), &keys, None).unwrap();
        let err = decrypt_secrets(&enc, &OsKeychainProvider).unwrap_err();
        assert!(err.contains("keychain"), "{err}");
    }

    #[test]
    fn profiles_without_passwords_never_need_the_key() {
        let enc = encrypt_secrets(&server("", None), &OsKeychainProvider, None).unwrap();
        assert_eq!(enc.encrypted, Some(true));
        let dec = decrypt_secrets(&enc, &OsKeychainProvider).unwrap();
        assert_eq!(dec.password, Some(Password::Plain(String::new())));
    }

    #[test]
    fn legacy_plaintext_on_encrypted_record_is_rejected() {
        let old = Server {
            encrypted: Some(true),
            password: Some(Password::Plain("fa1d88ee82bd4439".into())),
            ..Default::default()
        };
        let keys = InMemoryKeyProvider::new_for_tests();
        assert!(decrypt_secrets(&old, &keys).is_err());
    }
}
