//! Encryption and decryption of the credentials saved on a [`Server`] profile.
//!
//! Three fields are secret: the database password (`password`), the SSH password
//! (`ssh.password`), and the SSH private-key passphrase (`ssh.passphrase`). Each is encrypted
//! with [`crypto::encrypt`] and bound to the profile's `id` and the field name, so it can only
//! be decrypted on the record it was saved on.
//!
//! The master key comes from a [`KeyProvider`] and is requested only when a secret is present,
//! so profiles without saved credentials never need the keychain. When the key is unavailable
//! these functions return the provider's error; they never fall back to a default key or pass
//! ciphertext off as a password.

use crate::common::types::{Password, Server};
use crate::core::crypto;
use crate::core::keys::{KeyProvider, MasterKey};

/// Field name bound into the database password's ciphertext.
pub const FIELD_PASSWORD: &str = "password";
/// Field name bound into the SSH password's ciphertext.
pub const FIELD_SSH_PASSWORD: &str = "ssh.password";
/// Field name bound into the SSH key passphrase's ciphertext.
pub const FIELD_SSH_PASSPHRASE: &str = "ssh.passphrase";

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
        self.key
            .as_ref()
            .ok_or_else(|| "master key unavailable".to_string())
    }
}

fn field_error(field: &str, e: crypto::CryptoError) -> String {
    format!("Saved credential \"{field}\": {e}")
}

fn seal(
    pw: &Option<Password>,
    key: &mut LazyKey<'_>,
    id: &str,
    field: &str,
) -> Result<Option<Password>, String> {
    match pw {
        Some(Password::Plain(plain)) if !plain.is_empty() => {
            let enc =
                crypto::encrypt(plain, key.get()?, id, field).map_err(|e| field_error(field, e))?;
            Ok(Some(Password::Encrypted(enc)))
        }
        Some(Password::Encrypted(enc)) => {
            // Keep it only if it really belongs to this record and field.
            crypto::decrypt(enc, key.get()?, id, field).map_err(|e| field_error(field, e))?;
            Ok(pw.clone())
        }
        other => Ok(other.clone()),
    }
}

fn open(
    pw: &Option<Password>,
    key: &mut LazyKey<'_>,
    id: &str,
    field: &str,
) -> Result<Option<Password>, String> {
    match pw {
        Some(Password::Encrypted(enc)) => {
            let plain =
                crypto::decrypt(enc, key.get()?, id, field).map_err(|e| field_error(field, e))?;
            Ok(Some(Password::Plain(plain)))
        }
        other => Ok(other.clone()),
    }
}

/// Encrypts every plaintext credential on `server`, bound to `server.id`.
///
/// `server.id` must already be assigned. Values that are already encrypted are kept only if
/// they decrypt for this record and field, so ciphertext copied from another profile is
/// refused instead of being saved in a form that can never be decrypted.
pub fn encrypt_secrets(server: &Server, keys: &dyn KeyProvider) -> Result<Server, String> {
    let mut key = LazyKey::new(keys);
    let mut updated = server.clone();
    let id = server.id.as_str();
    updated.password = seal(&server.password, &mut key, id, FIELD_PASSWORD)?;
    if let Some(ssh) = updated.ssh.as_mut() {
        ssh.password = seal(&ssh.password, &mut key, id, FIELD_SSH_PASSWORD)?;
        ssh.passphrase = seal(&ssh.passphrase, &mut key, id, FIELD_SSH_PASSPHRASE)?;
    }
    Ok(updated)
}

/// Decrypts every encrypted credential on `server` into plaintext held in memory.
pub fn decrypt_secrets(server: &Server, keys: &dyn KeyProvider) -> Result<Server, String> {
    let mut key = LazyKey::new(keys);
    let mut updated = server.clone();
    let id = server.id.as_str();
    updated.password = open(&server.password, &mut key, id, FIELD_PASSWORD)?;
    if let Some(ssh) = updated.ssh.as_mut() {
        ssh.password = open(&ssh.password, &mut key, id, FIELD_SSH_PASSWORD)?;
        ssh.passphrase = open(&ssh.passphrase, &mut key, id, FIELD_SSH_PASSPHRASE)?;
    }
    Ok(updated)
}

/// Returns `true` if any credential on `server` is still plaintext (and non-empty).
pub fn has_plaintext_secrets(server: &Server) -> bool {
    let plain = |pw: &Option<Password>| matches!(pw, Some(Password::Plain(p)) if !p.is_empty());
    plain(&server.password)
        || server
            .ssh
            .as_ref()
            .is_some_and(|ssh| plain(&ssh.password) || plain(&ssh.passphrase))
}

#[cfg(test)]
#[path = "secrets_tests.rs"]
mod tests;
