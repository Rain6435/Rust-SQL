//! [`OsKeychainProvider`]: the master key lives in the operating system keychain.
//!
//! - Service name [`KEYCHAIN_SERVICE`]; account name `master-key:<data dir>`, so two data
//!   directories (for example `RUST_SQL_HOME` installs) never share or overwrite a key.
//! - First run: a random 256-bit key is generated, written, and read back. A file lock in the
//!   data directory stops two processes from both creating a key and orphaning one of them.
//! - A missing Secret Service, a locked or denied keychain, or an entry of the wrong size is a
//!   [`KeyError::Unavailable`] that says why. An unusable entry is never replaced, because that
//!   would make every saved password undecryptable.
//! - Keychain calls run on a short-lived thread, so the platform client never blocks or nests
//!   inside the caller's async runtime. A successfully read key is cached for the process.

use super::{KeyError, KeyProvider, MasterKey, MASTER_KEY_LEN};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use zeroize::Zeroizing;

/// Keychain service name under which the master key is stored.
pub const KEYCHAIN_SERVICE: &str = "rust-sql";

/// Minimal secret store interface, so the get-or-create logic is testable without a keychain.
pub(crate) trait SecretBackend: Send + Sync {
    /// Reads the secret, returning `Ok(None)` when no entry exists.
    fn read(&self, service: &str, account: &str) -> Result<Option<Zeroizing<Vec<u8>>>, String>;
    /// Creates or replaces the secret.
    fn write(&self, service: &str, account: &str, secret: &[u8]) -> Result<(), String>;
}

/// The platform keychain via the `keyring` crate.
struct OsBackend;

/// Describes a keyring error, replacing "no default store" with the reason the platform store
/// failed to initialize (for example, no Secret Service on the session bus).
fn describe(e: keyring::Error) -> String {
    match (&e, keyring::Entry::store_status()) {
        (keyring::Error::NoDefaultStore, Err(init)) => format!("keychain not reachable: {init}"),
        _ => e.to_string(),
    }
}

impl SecretBackend for OsBackend {
    fn read(&self, service: &str, account: &str) -> Result<Option<Zeroizing<Vec<u8>>>, String> {
        let entry = keyring::Entry::new(service, account).map_err(describe)?;
        match entry.get_secret() {
            Ok(bytes) => Ok(Some(Zeroizing::new(bytes))),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(describe(e)),
        }
    }

    fn write(&self, service: &str, account: &str, secret: &[u8]) -> Result<(), String> {
        keyring::Entry::new(service, account)
            .and_then(|entry| entry.set_secret(secret))
            .map_err(describe)
    }
}

/// Returns the keychain account name for a data directory.
pub fn keychain_account_for(data_dir: &Path) -> String {
    format!("master-key:{}", data_dir.display())
}

fn platform_hint() -> &'static str {
    if cfg!(target_os = "macos") {
        "the macOS Keychain"
    } else if cfg!(target_os = "windows") {
        "the Windows Credential Manager"
    } else {
        "the Secret Service (for example GNOME Keyring or KWallet; Linux without a running \
         Secret Service is not supported)"
    }
}

fn unavailable(action: &str, detail: &str) -> KeyError {
    KeyError::Unavailable(format!(
        "could not {action} the master key in the OS keychain, {}: {detail}",
        platform_hint()
    ))
}

/// Reads the master key, creating it on first use. Never replaces an existing entry.
pub(crate) fn get_or_create(
    backend: &dyn SecretBackend,
    account: &str,
) -> Result<MasterKey, KeyError> {
    let parse = |bytes: &[u8]| -> Result<MasterKey, KeyError> {
        if bytes.len() != MASTER_KEY_LEN {
            return Err(KeyError::Unavailable(format!(
                "the OS keychain entry \"{KEYCHAIN_SERVICE}\" / \"{account}\" holds {} bytes \
                 instead of {MASTER_KEY_LEN}. It was not replaced, because saved passwords \
                 would become unreadable; delete it yourself to start over",
                bytes.len()
            )));
        }
        let mut key = MasterKey::from_bytes([0u8; MASTER_KEY_LEN]);
        key.0.copy_from_slice(bytes);
        Ok(key)
    };
    let existing = backend
        .read(KEYCHAIN_SERVICE, account)
        .map_err(|e| unavailable("read", &e))?;
    if let Some(bytes) = existing {
        return parse(&bytes);
    }
    let fresh = MasterKey::generate();
    backend
        .write(KEYCHAIN_SERVICE, account, fresh.as_bytes())
        .map_err(|e| unavailable("store", &e))?;
    // Read back what the keychain actually holds and use that.
    match backend.read(KEYCHAIN_SERVICE, account) {
        Ok(Some(bytes)) => parse(&bytes),
        Ok(None) => Err(unavailable(
            "verify",
            "the new entry could not be read back after it was written",
        )),
        Err(e) => Err(unavailable("verify", &e)),
    }
}

/// Production provider backed by the operating system keychain.
pub struct OsKeychainProvider {
    account: String,
    lock_path: Option<PathBuf>,
    backend: Box<dyn SecretBackend>,
    cache: Mutex<Option<MasterKey>>,
}

impl std::fmt::Debug for OsKeychainProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OsKeychainProvider")
            .field("account", &self.account)
            .finish_non_exhaustive()
    }
}

impl OsKeychainProvider {
    /// Provider for the key belonging to `data_dir` (the directory holding `rust-sql.json`).
    pub fn for_data_dir(data_dir: &Path) -> Self {
        Self {
            account: keychain_account_for(data_dir),
            lock_path: Some(data_dir.join(".rust-sql-keychain.lock")),
            backend: Box::new(OsBackend),
            cache: Mutex::new(None),
        }
    }

    /// Provider for the active configuration's data directory.
    pub fn for_current_data_dir() -> Self {
        let cfg = crate::core::config::get_config_path();
        let dir = cfg
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."));
        let dir = std::fs::canonicalize(&dir).unwrap_or(dir);
        Self::for_data_dir(&dir)
    }

    /// Provider over an arbitrary backend, for tests only.
    #[cfg(test)]
    pub(crate) fn with_backend_for_tests(account: &str, backend: Box<dyn SecretBackend>) -> Self {
        Self {
            account: account.to_string(),
            lock_path: None,
            backend,
            cache: Mutex::new(None),
        }
    }

    /// The keychain account name this provider uses.
    pub fn account(&self) -> &str {
        &self.account
    }

    fn load_locked(&self) -> Result<MasterKey, KeyError> {
        let _guard = match &self.lock_path {
            Some(path) => Some(lock_file(path)?),
            None => None,
        };
        std::thread::scope(|s| {
            s.spawn(|| get_or_create(self.backend.as_ref(), &self.account))
                .join()
                .unwrap_or_else(|_| Err(unavailable("read", "the keychain client panicked")))
        })
    }
}

fn lock_file(path: &Path) -> Result<std::fs::File, KeyError> {
    let fail = |e: std::io::Error| {
        KeyError::Unavailable(format!(
            "could not lock {} while creating the master key: {e}",
            path.display()
        ))
    };
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(fail)?;
    }
    let file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(path)
        .map_err(fail)?;
    file.lock().map_err(fail)?;
    Ok(file)
}

impl KeyProvider for OsKeychainProvider {
    fn master_key(&self) -> Result<MasterKey, KeyError> {
        let mut cache = self
            .cache
            .lock()
            .map_err(|_| KeyError::Unavailable("the key cache lock is poisoned".into()))?;
        if let Some(key) = cache.as_ref() {
            return Ok(key.clone());
        }
        let key = self.load_locked()?;
        *cache = Some(key.clone());
        Ok(key)
    }
}

#[cfg(test)]
#[path = "keychain_tests.rs"]
mod tests;
