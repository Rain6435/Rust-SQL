//! Master-key access for credential encryption, behind the [`KeyProvider`] seam.
//!
//! Every front end (desktop GUI, TUI, CLI, and the web server) obtains the key that encrypts
//! saved passwords through a [`KeyProvider`]. No key is ever compiled into the binary, accepted
//! from a client, or substituted when the provider fails: a missing key is an explicit
//! [`KeyError`], and callers refuse to store or decrypt credentials.
//!
//! The production provider is [`OsKeychainProvider`]: on first use it creates a random 256-bit
//! master key in the OS keychain (macOS Keychain, Windows Credential Manager, or the Linux
//! Secret Service) and reads it back on later runs. There is no fallback store.
//!
//! [`InMemoryKeyProvider`] exists only for tests and is only reachable through constructors
//! whose names end in `_for_tests`. No production code path constructs it.

mod keychain;

pub use keychain::{keychain_account_for, OsKeychainProvider, KEYCHAIN_SERVICE};

use rand::Rng;
use std::fmt;
use std::sync::{Arc, OnceLock};
use thiserror::Error;
use zeroize::Zeroizing;

/// Length of the master key in bytes (256 bits).
pub const MASTER_KEY_LEN: usize = 32;

/// The 256-bit master key. Its memory is zeroed on drop and it never prints its bytes.
#[derive(Clone)]
pub struct MasterKey(Zeroizing<[u8; MASTER_KEY_LEN]>);

impl MasterKey {
    /// Wraps raw key bytes.
    pub fn from_bytes(bytes: [u8; MASTER_KEY_LEN]) -> Self {
        Self(Zeroizing::new(bytes))
    }

    /// Generates a fresh key from the operating system CSPRNG.
    pub fn generate() -> Self {
        let mut key = Self::from_bytes([0u8; MASTER_KEY_LEN]);
        rand::rng().fill_bytes(key.0.as_mut());
        key
    }

    /// Returns the raw key bytes.
    pub fn as_bytes(&self) -> &[u8; MASTER_KEY_LEN] {
        &self.0
    }
}

impl fmt::Debug for MasterKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("MasterKey(<redacted>)")
    }
}

/// Why the master key could not be obtained.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum KeyError {
    /// The keychain cannot be reached, access was denied, or the stored entry is unusable.
    #[error("Saved credentials are unavailable: {0}")]
    Unavailable(String),
}

/// Source of the master key used for all credential encryption and decryption.
pub trait KeyProvider: Send + Sync {
    /// Returns the master key, or an error explaining why it is unavailable.
    fn master_key(&self) -> Result<MasterKey, KeyError>;
}

/// Returns the process-wide production key provider (the OS keychain).
///
/// The same instance is shared by every caller so the keychain is read at most once per
/// process after it succeeds.
pub fn system_key_provider() -> Arc<dyn KeyProvider> {
    static PROVIDER: OnceLock<Arc<OsKeychainProvider>> = OnceLock::new();
    PROVIDER
        .get_or_init(|| Arc::new(OsKeychainProvider::for_current_data_dir()))
        .clone()
}

/// Shared handle to a [`KeyProvider`] that defaults to [`system_key_provider`].
#[derive(Clone)]
pub struct SharedKeyProvider(pub Arc<dyn KeyProvider>);

impl Default for SharedKeyProvider {
    fn default() -> Self {
        Self(system_key_provider())
    }
}

impl fmt::Debug for SharedKeyProvider {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SharedKeyProvider")
    }
}

/// Test-only provider holding a key in memory. Never constructed by production code.
pub struct InMemoryKeyProvider {
    key: MasterKey,
}

impl InMemoryKeyProvider {
    /// Creates a provider with a fresh random key, for tests only.
    pub fn new_for_tests() -> Self {
        Self {
            key: MasterKey::generate(),
        }
    }

    /// Creates a provider with the given key bytes, for tests only.
    pub fn with_key_for_tests(bytes: [u8; MASTER_KEY_LEN]) -> Self {
        Self {
            key: MasterKey::from_bytes(bytes),
        }
    }
}

impl KeyProvider for InMemoryKeyProvider {
    fn master_key(&self) -> Result<MasterKey, KeyError> {
        Ok(self.key.clone())
    }
}

/// Test-only provider that always fails, standing in for an unavailable keychain.
#[cfg(test)]
pub(crate) struct UnavailableKeyProvider;

#[cfg(test)]
impl KeyProvider for UnavailableKeyProvider {
    fn master_key(&self) -> Result<MasterKey, KeyError> {
        Err(KeyError::Unavailable(
            "the OS keychain is unavailable (test)".into(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn master_key_debug_output_is_redacted() {
        let key = MasterKey::from_bytes([7u8; MASTER_KEY_LEN]);
        assert_eq!(format!("{key:?}"), "MasterKey(<redacted>)");
    }

    #[test]
    fn generated_keys_are_random() {
        assert_ne!(
            MasterKey::generate().as_bytes(),
            MasterKey::generate().as_bytes()
        );
    }

    #[test]
    fn test_provider_returns_its_key() {
        let p = InMemoryKeyProvider::with_key_for_tests([9u8; MASTER_KEY_LEN]);
        assert_eq!(p.master_key().unwrap().as_bytes(), &[9u8; MASTER_KEY_LEN]);
    }
}
