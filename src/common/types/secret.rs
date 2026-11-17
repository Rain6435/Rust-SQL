//! Credential value types: [`SecretString`] for plaintext held in memory, [`EncryptedPassword`]
//! for the versioned ciphertext stored on disk, and [`Password`], which is one or the other.
//!
//! Plaintext lives in [`SecretString`], whose buffer is zeroed on drop and whose `Debug` output
//! is redacted. Ciphertext is a single `v2:` string produced by [`crate::core::crypto`]; any
//! other on-disk shape, including the removed pre-v2 `{ivText, encryptedText}` object, is
//! rejected when the configuration is read.

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;
use std::ops::Deref;
use zeroize::Zeroizing;

/// A plaintext secret whose memory is zeroed on drop and which never prints its value.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct SecretString(Zeroizing<String>);

impl SecretString {
    /// Wraps `value`.
    pub fn new(value: String) -> Self {
        Self(Zeroizing::new(value))
    }

    /// Returns the plaintext.
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

impl Deref for SecretString {
    type Target = str;
    fn deref(&self) -> &str {
        self.0.as_str()
    }
}

impl From<String> for SecretString {
    fn from(value: String) -> Self {
        Self::new(value)
    }
}

impl From<&str> for SecretString {
    fn from(value: &str) -> Self {
        Self::new(value.to_string())
    }
}

impl fmt::Debug for SecretString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.0.is_empty() {
            f.write_str("SecretString(\"\")")
        } else {
            f.write_str("SecretString(<redacted>)")
        }
    }
}

impl Serialize for SecretString {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for SecretString {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        String::deserialize(d).map(Self::new)
    }
}

/// An encrypted credential as stored in `rust-sql.json`: `{"ciphertext": "v2:<base64>"}`.
///
/// The base64 payload is a 24-byte XChaCha20 nonce followed by the Poly1305-authenticated
/// ciphertext. See [`crate::core::crypto`] for the format and the associated data it binds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct EncryptedPassword {
    /// Version-prefixed ciphertext, for example `v2:...`.
    pub ciphertext: String,
}

/// A credential that is either plaintext in memory or encrypted on disk.
///
/// JSON form: a string is plaintext (only ever sent by a client or typed into the file by
/// hand, and encrypted when the configuration is next prepared); an object with a single
/// `ciphertext` field is encrypted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(untagged)]
pub enum Password {
    /// Encrypted value stored in `rust-sql.json`.
    Encrypted(EncryptedPassword),
    /// Decrypted or unsaved plaintext.
    Plain(SecretString),
}

/// Error shown when a configuration still holds a password in the removed pre-v2 format.
pub const LEGACY_PASSWORD_FORMAT_ERROR: &str = "a saved password uses the removed pre-v2 \
     format ({ivText, encryptedText}); there is no migration. Remove that password from \
     rust-sql.json and enter it again";

impl<'de> Deserialize<'de> for Password {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        use serde::de::Error;
        let value = serde_json::Value::deserialize(d)?;
        match value {
            serde_json::Value::String(s) => Ok(Password::Plain(SecretString::new(s))),
            serde_json::Value::Object(ref obj)
                if obj.contains_key("ivText") || obj.contains_key("encryptedText") =>
            {
                Err(D::Error::custom(LEGACY_PASSWORD_FORMAT_ERROR))
            }
            serde_json::Value::Object(_) => serde_json::from_value(value)
                .map(Password::Encrypted)
                .map_err(|e| D::Error::custom(format!("invalid encrypted password: {e}"))),
            _ => Err(D::Error::custom(
                "a password must be a string or an encrypted {\"ciphertext\": ...} object",
            )),
        }
    }
}

impl From<&str> for Password {
    fn from(value: &str) -> Self {
        Password::plain(value)
    }
}

impl From<String> for Password {
    fn from(value: String) -> Self {
        Password::plain(value)
    }
}

impl Default for Password {
    fn default() -> Self {
        Password::Plain(SecretString::default())
    }
}

impl Password {
    /// Wraps a plaintext value.
    pub fn plain(value: impl Into<SecretString>) -> Self {
        Password::Plain(value.into())
    }

    /// Returns the plaintext string if the password is in `Plain` form.
    pub fn as_plain(&self) -> Option<&str> {
        match self {
            Password::Plain(s) => Some(s.as_str()),
            Password::Encrypted(_) => None,
        }
    }

    /// Returns `true` if the plaintext or encrypted payload is empty.
    pub fn is_empty(&self) -> bool {
        match self {
            Password::Plain(s) => s.is_empty(),
            Password::Encrypted(e) => e.ciphertext.is_empty(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secret_string_debug_is_redacted() {
        let pw = Password::plain("hunter2-secret");
        assert!(!format!("{pw:?}").contains("hunter2"), "{pw:?}");
    }

    #[test]
    fn json_round_trip_for_both_forms() {
        for pw in [
            Password::plain("päss 🔑"),
            Password::Encrypted(EncryptedPassword {
                ciphertext: "v2:AAAA".into(),
            }),
        ] {
            let json = serde_json::to_string(&pw).unwrap();
            assert_eq!(serde_json::from_str::<Password>(&json).unwrap(), pw);
        }
    }

    #[test]
    fn legacy_aes_object_is_rejected_with_an_explanation() {
        let err = serde_json::from_str::<Password>(r#"{"ivText":"a","encryptedText":"b"}"#)
            .unwrap_err()
            .to_string();
        assert!(err.contains("pre-v2"), "{err}");
    }

    #[test]
    fn unknown_object_shapes_are_rejected() {
        assert!(serde_json::from_str::<Password>(r#"{"ciphertext":"v2:x","extra":1}"#).is_err());
        assert!(serde_json::from_str::<Password>("42").is_err());
    }
}
