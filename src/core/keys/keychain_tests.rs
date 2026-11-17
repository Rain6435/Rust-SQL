//! Tests for keychain get-or-create logic using a mock secret backend.

use super::*;
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

#[derive(Default)]
struct MockBackend {
    entries: Mutex<HashMap<(String, String), Vec<u8>>>,
    fail_reads: bool,
    fail_writes: bool,
    drop_writes: bool,
    reads: AtomicUsize,
}

impl SecretBackend for Arc<MockBackend> {
    fn read(&self, service: &str, account: &str) -> Result<Option<Zeroizing<Vec<u8>>>, String> {
        self.reads.fetch_add(1, Ordering::SeqCst);
        if self.fail_reads {
            return Err("Couldn't access platform storage: locked".into());
        }
        let map = self.entries.lock().unwrap();
        Ok(map
            .get(&(service.to_string(), account.to_string()))
            .map(|v| Zeroizing::new(v.clone())))
    }

    fn write(&self, service: &str, account: &str, secret: &[u8]) -> Result<(), String> {
        if self.fail_writes {
            return Err("Platform failure: no Secret Service".into());
        }
        if !self.drop_writes {
            self.entries
                .lock()
                .unwrap()
                .insert((service.into(), account.into()), secret.to_vec());
        }
        Ok(())
    }
}

fn provider(backend: &Arc<MockBackend>) -> OsKeychainProvider {
    OsKeychainProvider::with_backend_for_tests("master-key:/data", Box::new(backend.clone()))
}

#[test]
fn first_run_creates_a_key_and_later_runs_reuse_it() {
    let backend = Arc::new(MockBackend::default());
    let first = provider(&backend).master_key().unwrap();
    let second = provider(&backend).master_key().unwrap();
    assert_eq!(first.as_bytes(), second.as_bytes());
    let stored = backend.entries.lock().unwrap();
    assert_eq!(
        stored.get(&(KEYCHAIN_SERVICE.into(), "master-key:/data".into())),
        Some(&first.as_bytes().to_vec())
    );
}

#[test]
fn a_successful_read_is_cached_for_the_process() {
    let backend = Arc::new(MockBackend::default());
    let p = provider(&backend);
    p.master_key().unwrap();
    let reads = backend.reads.load(Ordering::SeqCst);
    p.master_key().unwrap();
    assert_eq!(backend.reads.load(Ordering::SeqCst), reads);
}

#[test]
fn an_unreachable_keychain_is_an_error_not_a_fallback() {
    let backend = Arc::new(MockBackend {
        fail_reads: true,
        ..Default::default()
    });
    let err = provider(&backend).master_key().unwrap_err().to_string();
    assert!(
        err.contains("OS keychain") && err.contains("locked"),
        "{err}"
    );
    if cfg!(target_os = "linux") {
        assert!(err.contains("Secret Service"), "{err}");
    }
}

#[test]
fn a_failed_write_is_an_error_and_errors_are_not_cached() {
    let backend = Arc::new(MockBackend {
        fail_writes: true,
        ..Default::default()
    });
    let p = provider(&backend);
    assert!(p.master_key().is_err());
    assert!(p.master_key().is_err());
    assert!(backend.reads.load(Ordering::SeqCst) >= 2);
}

#[test]
fn a_write_that_does_not_persist_is_an_error() {
    let backend = Arc::new(MockBackend {
        drop_writes: true,
        ..Default::default()
    });
    let err = provider(&backend).master_key().unwrap_err().to_string();
    assert!(err.contains("read back"), "{err}");
}

#[test]
fn a_wrong_sized_entry_is_reported_and_never_replaced() {
    let backend = Arc::new(MockBackend::default());
    let key = (KEYCHAIN_SERVICE.to_string(), "master-key:/data".to_string());
    backend
        .entries
        .lock()
        .unwrap()
        .insert(key.clone(), vec![1, 2, 3]);
    let err = provider(&backend).master_key().unwrap_err().to_string();
    assert!(err.contains("3 bytes"), "{err}");
    assert_eq!(backend.entries.lock().unwrap()[&key], vec![1, 2, 3]);
}

#[test]
fn accounts_are_tied_to_the_data_directory() {
    let a = keychain_account_for(Path::new("/home/a/.config/Rust-SQL"));
    let b = keychain_account_for(Path::new("/srv/rust-sql"));
    assert_ne!(a, b);
    assert!(a.starts_with("master-key:"));
}

#[test]
fn the_lock_file_is_created_in_the_data_directory() {
    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().join("nested").join(".rust-sql-keychain.lock");
    let first = lock_file(&path).unwrap();
    drop(first);
    assert!(path.exists());
    lock_file(&path).unwrap();
}
