//! AES-256-CBC password encryption and decryption for saved server credentials.
//!
//! The key is always the 256-bit master key from a [`KeyProvider`](crate::core::keys::KeyProvider);
//! there is no default key and no key derived from a string. Workstream 4 replaces this cipher
//! with versioned, authenticated encryption bound to each record.

use crate::common::types::EncryptedPassword;
use crate::core::keys::MasterKey;
use aes::cipher::{block_padding::Pkcs7, BlockModeDecrypt, BlockModeEncrypt, KeyIvInit};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use rand::Rng;
use thiserror::Error;

type Aes256CbcEnc = cbc::Encryptor<aes::Aes256>;
type Aes256CbcDec = cbc::Decryptor<aes::Aes256>;

/// Cryptographic error variants returned during password encryption or decryption.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum CryptoError {
    /// Attempted to encrypt an empty plaintext string.
    #[error("Missing plain text")]
    MissingPlainText,
    /// Encrypted payload is missing its IV or ciphertext.
    #[error("Invalid encrypted valued")]
    InvalidEncryptedValue,
    /// Base64 decoding, IV length, or PKCS#7 unpadding failed.
    #[error("The encrypted value is not a valid format")]
    InvalidEncryptedFormat,
    /// Legacy non-IV cipher format is deprecated and rejected.
    #[error("This method is no longer implemented. You will need to delete any existing ecrypted values and re-encrypt them using the new encrypt method")]
    UnsafeDecryptDeprecated,
}

/// Encrypts `plain_text` using AES-256-CBC with a random 16-byte IV and PKCS#7 padding.
pub fn encrypt(plain_text: &str, key: &MasterKey) -> Result<EncryptedPassword, CryptoError> {
    if plain_text.is_empty() {
        return Err(CryptoError::MissingPlainText);
    }
    let mut iv = [0u8; 16];
    rand::rng().fill_bytes(&mut iv);

    let pt_bytes = plain_text.as_bytes();
    let mut buf = vec![0u8; pt_bytes.len() + 16];
    buf[..pt_bytes.len()].copy_from_slice(pt_bytes);

    let ct = Aes256CbcEnc::new(key.as_bytes().into(), &iv.into())
        .encrypt_padded::<Pkcs7>(&mut buf, pt_bytes.len())
        .map_err(|_| CryptoError::InvalidEncryptedFormat)?;

    Ok(EncryptedPassword {
        iv_text: BASE64.encode(iv),
        encrypted_text: BASE64.encode(ct),
    })
}

/// Decrypts an [`EncryptedPassword`] using AES-256-CBC and the master `key`.
pub fn decrypt(encrypted: &EncryptedPassword, key: &MasterKey) -> Result<String, CryptoError> {
    if encrypted.iv_text.is_empty() || encrypted.encrypted_text.is_empty() {
        return Err(CryptoError::InvalidEncryptedValue);
    }

    let iv_vec = BASE64
        .decode(&encrypted.iv_text)
        .map_err(|_| CryptoError::InvalidEncryptedFormat)?;
    if iv_vec.len() != 16 {
        return Err(CryptoError::InvalidEncryptedFormat);
    }
    let mut iv = [0u8; 16];
    iv.copy_from_slice(&iv_vec);

    let mut ct_vec = BASE64
        .decode(&encrypted.encrypted_text)
        .map_err(|_| CryptoError::InvalidEncryptedFormat)?;

    let pt = Aes256CbcDec::new(key.as_bytes().into(), &iv.into())
        .decrypt_padded::<Pkcs7>(&mut ct_vec)
        .map_err(|_| CryptoError::InvalidEncryptedFormat)?;

    String::from_utf8(pt.to_vec()).map_err(|_| CryptoError::InvalidEncryptedFormat)
}

/// Rejects legacy un-salted cipher strings and instructs the caller to re-encrypt with [`encrypt`].
pub fn unsafe_decrypt(_encrypted_text: &str, _key: &MasterKey) -> Result<String, CryptoError> {
    Err(CryptoError::UnsafeDecryptDeprecated)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(b: u8) -> MasterKey {
        MasterKey::from_bytes([b; 32])
    }

    #[test]
    fn test_encrypt_decrypt_roundtrip_with_master_key() {
        let enc = encrypt("my-secret-db-password-123!", &key(1)).unwrap();
        assert_eq!(
            decrypt(&enc, &key(1)).unwrap(),
            "my-secret-db-password-123!"
        );
    }

    #[test]
    fn test_errors() {
        assert_eq!(encrypt("", &key(1)), Err(CryptoError::MissingPlainText));
        let empty = EncryptedPassword {
            iv_text: "".into(),
            encrypted_text: "".into(),
        };
        assert_eq!(
            decrypt(&empty, &key(1)),
            Err(CryptoError::InvalidEncryptedValue)
        );
        assert_eq!(
            unsafe_decrypt("fa1d88ee82bd4439", &key(1)),
            Err(CryptoError::UnsafeDecryptDeprecated)
        );
    }
}
