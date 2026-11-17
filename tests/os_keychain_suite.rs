//! The production keychain provider against the real OS keychain.
//!
//! These tests touch the real macOS Keychain, Windows Credential Manager, or Linux Secret
//! Service, so they run only when `RUST_SQL_TEST_OS_KEYCHAIN=1` is set. Each test uses a fresh
//! temporary data directory (and therefore its own keychain entry) and deletes the entry
//! afterwards. On Linux, run them inside a session with an unlocked Secret Service, for example
//! `dbus-run-session -- sh -c 'echo pw | gnome-keyring-daemon --unlock; cargo test ...'`.

use rust_sql::common::types::{Password, Server};
use rust_sql::core::keys::{KeyProvider, OsKeychainProvider, KEYCHAIN_SERVICE};
use rust_sql::core::servers::{decrypt_secrets, encrypt_secrets};
use tempfile::TempDir;

fn enabled() -> bool {
    let on = std::env::var("RUST_SQL_TEST_OS_KEYCHAIN").is_ok_and(|v| v == "1");
    if !on {
        eprintln!("skipped: set RUST_SQL_TEST_OS_KEYCHAIN=1 to test the real OS keychain");
    }
    on
}

/// Deletes the keychain entry for a provider when dropped.
struct Cleanup(String);

impl Drop for Cleanup {
    fn drop(&mut self) {
        if let Ok(entry) = keyring::Entry::new(KEYCHAIN_SERVICE, &self.0) {
            let _ = entry.delete_credential();
        }
    }
}

#[test]
fn first_run_creates_a_key_that_later_runs_read_back() {
    if !enabled() {
        return;
    }
    let dir = TempDir::new().unwrap();
    let first = OsKeychainProvider::for_data_dir(dir.path());
    let _cleanup = Cleanup(first.account().to_string());
    let created = first
        .master_key()
        .expect("create master key in the OS keychain");

    let later = OsKeychainProvider::for_data_dir(dir.path());
    let read = later.master_key().expect("read master key back");
    assert_eq!(created.as_bytes(), read.as_bytes());

    let other_dir = TempDir::new().unwrap();
    let other = OsKeychainProvider::for_data_dir(other_dir.path());
    let _cleanup_other = Cleanup(other.account().to_string());
    assert_ne!(
        other.master_key().unwrap().as_bytes(),
        created.as_bytes(),
        "each data directory has its own key"
    );
}

#[test]
fn saved_password_round_trips_through_a_new_process_view_of_the_keychain() {
    if !enabled() {
        return;
    }
    let dir = TempDir::new().unwrap();
    let writer = OsKeychainProvider::for_data_dir(dir.path());
    let _cleanup = Cleanup(writer.account().to_string());
    let srv = Server {
        id: "conn-1".into(),
        password: Some(Password::plain("real-keychain-pw")),
        ..Default::default()
    };
    let enc = encrypt_secrets(&srv, &writer).expect("encrypt");
    // A fresh provider has an empty cache, as a restarted application would.
    let reader = OsKeychainProvider::for_data_dir(dir.path());
    let dec = decrypt_secrets(&enc, &reader).expect("decrypt");
    assert_eq!(dec.password, Some(Password::plain("real-keychain-pw")));
}

#[test]
fn a_wrong_sized_entry_is_reported_and_left_alone() {
    if !enabled() {
        return;
    }
    let dir = TempDir::new().unwrap();
    let provider = OsKeychainProvider::for_data_dir(dir.path());
    let _cleanup = Cleanup(provider.account().to_string());
    let entry = keyring::Entry::new(KEYCHAIN_SERVICE, provider.account()).unwrap();
    entry.set_secret(b"short").unwrap();
    let err = provider.master_key().unwrap_err().to_string();
    assert!(err.contains("5 bytes"), "{err}");
    assert_eq!(entry.get_secret().unwrap(), b"short");
}
