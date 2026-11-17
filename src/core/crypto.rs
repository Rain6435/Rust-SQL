//! Authenticated encryption of saved credentials (format `v2`).
//!
//! - **Cipher:** XChaCha20-Poly1305 with a fresh random 24-byte nonce for every value.
//! - **Key:** a subkey derived with HKDF-SHA256 from the 256-bit master key held in the OS
//!   keychain (see [`crate::core::keys`]), using the info string [`CREDENTIAL_KEY_INFO`]. No key
//!   is ever derived from a password or a constant.
//! - **Associated data:** the record (connection) ID and the field name, length-prefixed, so a
//!   ciphertext moved to another connection or another field fails to decrypt.
//! - **Encoding:** `v2:` followed by standard base64 of `nonce || ciphertext || tag`. Values
//!   without a recognized version prefix are rejected; there is no legacy format.

use crate::common::types::{EncryptedPassword, SecretString};
use crate::core::keys::MasterKey;
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{Key, XChaCha20Poly1305, XNonce};
use hkdf::Hkdf;
use rand::Rng;
use sha2::Sha256;
use thiserror::Error;
use zeroize::Zeroizing;

/// Version tag prefixed to every encrypted value.
pub const FORMAT_V2_PREFIX: &str = "v2:";

/// HKDF info string for the subkey that encrypts saved connection credentials.
pub const CREDENTIAL_KEY_INFO: &[u8] = b"rust-sql/v2/connection-credentials";

const AAD_DOMAIN: &[u8] = b"rust-sql/v2/credential";
const NONCE_LEN: usize = 24;
const TAG_LEN: usize = 16;

/// Errors from encrypting or decrypting a credential. None of them include secret material.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum CryptoError {
    /// The record ID used as associated data is empty, so the value could not be bound to it.
    #[error("Cannot encrypt or decrypt a credential without a connection ID")]
    MissingRecordId,
    /// The value has no recognized version prefix (only `v2:` is supported).
    #[error("Unsupported encrypted value format: only \"v2:\" values are accepted")]
    UnsupportedVersion,
    /// The base64 payload is malformed or too short to hold a nonce and tag.
    #[error("The encrypted value is malformed")]
    Malformed,
    /// Authentication failed: the value was altered, belongs to another connection or field,
    /// or was encrypted with a different master key.
    #[error(
        "Saved credential could not be decrypted: it was modified, belongs to a different \
         connection, or was encrypted with a different OS keychain master key"
    )]
    AuthenticationFailed,
    /// The decrypted bytes are not valid UTF-8.
    #[error("The decrypted credential is not valid UTF-8")]
    InvalidUtf8,
}

fn credential_cipher(key: &MasterKey) -> XChaCha20Poly1305 {
    let hk = Hkdf::<Sha256>::new(None, key.as_bytes());
    let mut sub = Zeroizing::new([0u8; 32]);
    hk.expand(CREDENTIAL_KEY_INFO, sub.as_mut())
        .expect("32 bytes is a valid HKDF-SHA256 output length");
    XChaCha20Poly1305::new(&Key::from(*sub))
}

/// Builds the associated data binding a value to `record_id` and `field`.
fn associated_data(record_id: &str, field: &str) -> Vec<u8> {
    let mut aad = Vec::with_capacity(AAD_DOMAIN.len() + record_id.len() + field.len() + 8);
    aad.extend_from_slice(AAD_DOMAIN);
    for part in [record_id, field] {
        aad.extend_from_slice(&(part.len() as u32).to_be_bytes());
        aad.extend_from_slice(part.as_bytes());
    }
    aad
}

/// Encrypts `plain_text` for field `field` of the connection `record_id`.
///
/// Empty plaintext is allowed and round-trips; callers decide whether to store it.
pub fn encrypt(
    plain_text: &str,
    key: &MasterKey,
    record_id: &str,
    field: &str,
) -> Result<EncryptedPassword, CryptoError> {
    if record_id.trim().is_empty() {
        return Err(CryptoError::MissingRecordId);
    }
    let mut nonce = [0u8; NONCE_LEN];
    rand::rng().fill_bytes(&mut nonce);
    let aad = associated_data(record_id, field);
    let ct = credential_cipher(key)
        .encrypt(
            &XNonce::from(nonce),
            Payload {
                msg: plain_text.as_bytes(),
                aad: &aad,
            },
        )
        .map_err(|_| CryptoError::Malformed)?;
    let mut blob = Vec::with_capacity(NONCE_LEN + ct.len());
    blob.extend_from_slice(&nonce);
    blob.extend_from_slice(&ct);
    Ok(EncryptedPassword {
        ciphertext: format!("{FORMAT_V2_PREFIX}{}", BASE64.encode(blob)),
    })
}

/// Decrypts `encrypted`, which must have been produced for the same `record_id` and `field`.
pub fn decrypt(
    encrypted: &EncryptedPassword,
    key: &MasterKey,
    record_id: &str,
    field: &str,
) -> Result<SecretString, CryptoError> {
    if record_id.trim().is_empty() {
        return Err(CryptoError::MissingRecordId);
    }
    let body = encrypted
        .ciphertext
        .strip_prefix(FORMAT_V2_PREFIX)
        .ok_or(CryptoError::UnsupportedVersion)?;
    let blob = BASE64.decode(body).map_err(|_| CryptoError::Malformed)?;
    if blob.len() < NONCE_LEN + TAG_LEN {
        return Err(CryptoError::Malformed);
    }
    let (nonce, ct) = blob.split_at(NONCE_LEN);
    let nonce: [u8; NONCE_LEN] = nonce.try_into().map_err(|_| CryptoError::Malformed)?;
    let aad = associated_data(record_id, field);
    let plain = Zeroizing::new(
        credential_cipher(key)
            .decrypt(&XNonce::from(nonce), Payload { msg: ct, aad: &aad })
            .map_err(|_| CryptoError::AuthenticationFailed)?,
    );
    let text = std::str::from_utf8(&plain).map_err(|_| CryptoError::InvalidUtf8)?;
    Ok(SecretString::from(text))
}

#[cfg(test)]
#[path = "crypto_tests.rs"]
mod tests;
