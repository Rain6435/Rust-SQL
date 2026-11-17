//! AES-256-CBC password encryption and decryption for saved server credentials.

use crate::common::types::EncryptedPassword;
use aes::cipher::{block_padding::Pkcs7, BlockDecryptMut, BlockEncryptMut, KeyIvInit};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use rand::RngCore;
use thiserror::Error;

type Aes256CbcEnc = cbc::Encryptor<aes::Aes256>;
type Aes256CbcDec = cbc::Decryptor<aes::Aes256>;

/// Cryptographic error variants returned during password encryption or decryption.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum CryptoError {
    /// Attempted to encrypt an empty plaintext string.
    #[error("Missing plain text")]
    MissingPlainText,
    /// Attempted to encrypt with an empty secret key.
    #[error("Missing encrypt secret")]
    MissingEncryptSecret,
    /// Encrypted payload is missing its IV or ciphertext.
    #[error("Invalid encrypted valued")]
    InvalidEncryptedValue,
    /// Attempted to decrypt with an empty secret key.
    #[error("Missing decrypt secret")]
    MissingDecryptSecret,
    /// Base64 decoding, IV length, or PKCS#7 unpadding failed.
    #[error("The encrypted value is not a valid format")]
    InvalidEncryptedFormat,
    /// Derived key length is invalid.
    #[error("The secret is not valid format")]
    InvalidSecretFormat,
    /// Legacy non-IV cipher format is deprecated and rejected.
    #[error("This method is no longer implemented. You will need to delete any existing ecrypted values and re-encrypt them using the new encrypt method")]
    UnsafeDecryptDeprecated,
}

/// Derives a 32-byte AES-256 key from `secret` by zero-padding or truncating to 32 bytes.
fn derive_key(secret: &str) -> [u8; 32] {
    let mut key = [0u8; 32];
    let bytes = secret.as_bytes();
    let copy_len = bytes.len().min(32);
    key[..copy_len].copy_from_slice(&bytes[..copy_len]);
    key
}

/// Encrypts `plain_text` using AES-256-CBC with a random 16-byte IV and PKCS#7 padding.
pub fn encrypt(plain_text: &str, secret: &str) -> Result<EncryptedPassword, CryptoError> {
    if plain_text.is_empty() {
        return Err(CryptoError::MissingPlainText);
    }
    if secret.is_empty() {
        return Err(CryptoError::MissingEncryptSecret);
    }

    let key = derive_key(secret);
    let mut iv = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut iv);

    let pt_bytes = plain_text.as_bytes();
    let mut buf = vec![0u8; pt_bytes.len() + 16];
    buf[..pt_bytes.len()].copy_from_slice(pt_bytes);

    let ct = Aes256CbcEnc::new(&key.into(), &iv.into())
        .encrypt_padded_mut::<Pkcs7>(&mut buf, pt_bytes.len())
        .map_err(|_| CryptoError::InvalidEncryptedFormat)?;

    Ok(EncryptedPassword {
        iv_text: BASE64.encode(iv),
        encrypted_text: BASE64.encode(ct),
    })
}

/// Decrypts an [`EncryptedPassword`] using AES-256-CBC and `secret`.
pub fn decrypt(encrypted: &EncryptedPassword, secret: &str) -> Result<String, CryptoError> {
    if encrypted.iv_text.is_empty() || encrypted.encrypted_text.is_empty() {
        return Err(CryptoError::InvalidEncryptedValue);
    }
    if secret.is_empty() {
        return Err(CryptoError::MissingDecryptSecret);
    }

    let iv_vec = BASE64
        .decode(&encrypted.iv_text)
        .map_err(|_| CryptoError::InvalidEncryptedFormat)?;
    if iv_vec.len() != 16 {
        return Err(CryptoError::InvalidEncryptedFormat);
    }
    let mut iv = [0u8; 16];
    iv.copy_from_slice(&iv_vec);

    let key = derive_key(secret);

    let mut ct_vec = BASE64
        .decode(&encrypted.encrypted_text)
        .map_err(|_| CryptoError::InvalidEncryptedFormat)?;

    let pt = Aes256CbcDec::new(&key.into(), &iv.into())
        .decrypt_padded_mut::<Pkcs7>(&mut ct_vec)
        .map_err(|_| CryptoError::InvalidEncryptedFormat)?;

    String::from_utf8(pt.to_vec()).map_err(|_| CryptoError::InvalidEncryptedFormat)
}

/// Rejects legacy un-salted cipher strings and instructs the caller to re-encrypt with [`encrypt`].
pub fn unsafe_decrypt(_encrypted_text: &str, _secret: &str) -> Result<String, CryptoError> {
    Err(CryptoError::UnsafeDecryptDeprecated)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_aes256_cbc_known_vector() {
        let secret = "CHK`Ya91Hs{me!^8ndwPPaPPxwQ}`";
        let encrypted = EncryptedPassword {
            iv_text: "wGf6X9T+QSygOHqtgQPlcA==".to_string(),
            encrypted_text: "0LySDs9WPAvwSS9Qv+W3/A==".to_string(),
        };
        let decrypted = decrypt(&encrypted, secret).expect("should decrypt known vector");
        assert_eq!(decrypted, "password");
    }

    #[test]
    fn test_encrypt_decrypt_roundtrip() {
        let secret = "j[F6Y6NoWT}+YG|4c|-<89:ByJ83-9Aj?O8>$Zk/[WFk_~gFbg7<wm+*V|A{xQZ,";
        let enc = encrypt("my-secret-db-password-123!", secret).unwrap();
        let dec = decrypt(&enc, secret).unwrap();
        assert_eq!(dec, "my-secret-db-password-123!");
    }

    #[test]
    fn test_errors() {
        assert_eq!(encrypt("", "secret"), Err(CryptoError::MissingPlainText));
        assert_eq!(encrypt("abc", ""), Err(CryptoError::MissingEncryptSecret));
        assert_eq!(
            decrypt(
                &EncryptedPassword {
                    iv_text: "".into(),
                    encrypted_text: "".into()
                },
                "secret"
            ),
            Err(CryptoError::InvalidEncryptedValue)
        );
        assert_eq!(
            unsafe_decrypt("fa1d88ee82bd4439", "secret"),
            Err(CryptoError::UnsafeDecryptDeprecated)
        );
    }
}
