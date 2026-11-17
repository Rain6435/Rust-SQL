//! Master-key access for credential encryption, behind the [`KeyProvider`] seam.
//!
//! Every front end (desktop GUI, TUI, CLI, and the web server) obtains the key that encrypts
//! saved passwords through a [`KeyProvider`]. No key is ever compiled into the binary, accepted
//! from a client, or substituted when the provider fails: a missing key is an explicit
//! [`KeyError`], and callers refuse to store or decrypt credentials.
//!
//! The production provider is [`OsKeychainProvider`]. Reading and creating the master key in
//! the OS keychain (macOS Keychain, Windows Credential Manager, Linux Secret Service) is
//! Workstream 4; until it lands, the provider reports the keychain as unavailable, so saved
//! passwords cannot be stored or decrypted. Connections without saved passwords still work.
//!
//! [`InMemoryKeyProvider`] exists only for tests and is only reachable through constructors
//! whose names end in `_for_tests`. No production code path constructs it.

use rand::Rng;
use std::fmt;
use std::sync::Arc;
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
    /// The key store cannot be reached, access was denied, or it is not implemented yet.
    #[error("Saved credentials are unavailable: {0}")]
    Unavailable(String),
}

/// Source of the master key used for all credential encryption and decryption.
pub trait KeyProvider: Send + Sync {
    /// Returns the master key, or an error explaining why it is unavailable.
    fn master_key(&self) -> Result<MasterKey, KeyError>;
}

/// Production provider backed by the operating system keychain.
#[derive(Debug, Default, Clone, Copy)]
pub struct OsKeychainProvider;

/// Message returned until the keychain-backed master key (Workstream 4) is implemented.
pub const KEYCHAIN_NOT_IMPLEMENTED: &str = "the OS keychain master key is not implemented yet \
     (Workstream 4), so saved passwords cannot be stored or decrypted. \
     Connections without a saved password still work";

impl KeyProvider for OsKeychainProvider {
    fn master_key(&self) -> Result<MasterKey, KeyError> {
        Err(KeyError::Unavailable(KEYCHAIN_NOT_IMPLEMENTED.to_string()))
    }
}

/// Returns the production key provider (the OS keychain).
pub fn system_key_provider() -> Arc<dyn KeyProvider> {
    Arc::new(OsKeychainProvider)
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
        let mut bytes = [0u8; MASTER_KEY_LEN];
        rand::rng().fill_bytes(&mut bytes);
        Self::with_key_for_tests(bytes)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn production_provider_fails_closed_instead_of_returning_a_default_key() {
        let err = system_key_provider().master_key().unwrap_err();
        assert!(err.to_string().contains("keychain"), "{err}");
    }

    #[test]
    fn master_key_debug_output_is_redacted() {
        let key = MasterKey::from_bytes([7u8; MASTER_KEY_LEN]);
        assert_eq!(format!("{key:?}"), "MasterKey(<redacted>)");
    }

    #[test]
    fn test_provider_returns_its_key() {
        let p = InMemoryKeyProvider::with_key_for_tests([9u8; MASTER_KEY_LEN]);
        assert_eq!(p.master_key().unwrap().as_bytes(), &[9u8; MASTER_KEY_LEN]);
    }
}
