//! Tests for the encrypted SSH host-key pin store: nothing readable on disk, tamper and key
//! mismatches refused, a mandatory keychain, and legacy plaintext pins discarded.

use super::*;
use crate::common::types::HostKeyChallengeKind;
use crate::core::keys::{InMemoryKeyProvider, UnavailableKeyProvider};
use crate::core::servers::secrets::FIELD_PASSWORD;

fn challenge(host: &str, fp: &str) -> HostKeyChallenge {
    HostKeyChallenge {
        kind: HostKeyChallengeKind::Unknown,
        host: host.into(),
        port: 22,
        key_algorithm: "ssh-ed25519".into(),
        fingerprint_sha256: fp.into(),
    }
}

/// Runs `body` with a fresh config file in a temp dir, holding the global config lock.
fn with_temp_config(body: impl FnOnce(&std::path::Path)) {
    let _cfg = config::TEST_CONFIG_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("rust-sql.json");
    config::set_config_path_override(Some(path.clone()));
    body(&path);
    config::set_config_path_override(None);
}

fn raw(path: &std::path::Path) -> String {
    std::fs::read_to_string(path).unwrap()
}

#[test]
fn approved_pins_are_encrypted_on_disk_and_round_trip() {
    with_temp_config(|path| {
        let keys = InMemoryKeyProvider::new_for_tests();
        approve_host_key(&challenge("bastion.example", "SHA256:aaaa"), &keys).unwrap();
        approve_host_key(&challenge("other.example", "SHA256:bbbb"), &keys).unwrap();

        let text = raw(path);
        for leaked in ["bastion.example", "other.example", "SHA256:", "ssh-ed25519"] {
            assert!(
                !text.contains(leaked),
                "{leaked} stored in plaintext: {text}"
            );
        }
        assert!(text.contains("\"sshHostKeys\": {"), "{text}");
        assert!(text.contains("\"ciphertext\": \"v2:"), "{text}");

        let pins = load_pins(&keys).unwrap();
        let fps: Vec<_> = pins.iter().map(|p| p.fingerprint_sha256.as_str()).collect();
        assert_eq!(fps, ["SHA256:aaaa", "SHA256:bbbb"]);
    });
}

#[test]
fn a_different_master_key_cannot_read_the_pins() {
    with_temp_config(|_| {
        let keys = InMemoryKeyProvider::with_key_for_tests([1u8; 32]);
        approve_host_key(&challenge("b", "SHA256:aaaa"), &keys).unwrap();
        let other = InMemoryKeyProvider::with_key_for_tests([2u8; 32]);
        assert!(matches!(load_pins(&other), Err(TunnelError::PinStore(_))));
        // Nor can it add a pin on top: that would mean overwriting pins it cannot read.
        assert!(approve_host_key(&challenge("c", "SHA256:cccc"), &other).is_err());
        assert_eq!(load_pins(&keys).unwrap().len(), 1);
    });
}

#[test]
fn an_unavailable_keychain_stores_nothing() {
    with_temp_config(|path| {
        let err =
            approve_host_key(&challenge("b", "SHA256:aaaa"), &UnavailableKeyProvider).unwrap_err();
        assert!(err.to_string().contains("unavailable"), "{err}");
        assert!(!raw(path).contains("sshHostKeys"));
        // With nothing stored there is nothing to decrypt, so the next step is the prompt.
        assert_eq!(load_pins(&UnavailableKeyProvider).unwrap(), []);
    });
}

#[test]
fn reading_existing_pins_requires_the_keychain() {
    with_temp_config(|_| {
        let keys = InMemoryKeyProvider::new_for_tests();
        approve_host_key(&challenge("b", "SHA256:aaaa"), &keys).unwrap();
        assert!(matches!(
            load_pins(&UnavailableKeyProvider),
            Err(TunnelError::PinStore(_))
        ));
        assert!(delete_host_key("b", 22, &UnavailableKeyProvider).is_err());
    });
}

#[test]
fn tampered_ciphertext_is_refused() {
    let keys = InMemoryKeyProvider::new_for_tests();
    let pins = vec![SshHostKeyPin {
        host: "b".into(),
        port: 22,
        key_algorithm: "ssh-ed25519".into(),
        fingerprint_sha256: "SHA256:aaaa".into(),
        approved_at: OffsetDateTime::now_utc(),
    }];
    let Some(StoredHostKeyPins::Encrypted(enc)) = encrypt_pins(&pins, &keys).unwrap() else {
        panic!("expected ciphertext");
    };
    let mut chars: Vec<char> = enc.ciphertext.chars().collect();
    let i = chars.len() / 2;
    chars[i] = if chars[i] == 'A' { 'B' } else { 'A' };
    let tampered = StoredHostKeyPins::Encrypted(EncryptedPassword {
        ciphertext: chars.into_iter().collect(),
    });
    assert!(decrypt_pins(Some(&tampered), &keys).is_err());
    let good = StoredHostKeyPins::Encrypted(enc);
    assert_eq!(decrypt_pins(Some(&good), &keys).unwrap(), pins);
}

#[test]
fn a_credential_ciphertext_cannot_stand_in_for_the_pin_list() {
    // Same key, same record and field names, but encrypted as a saved password: the purpose's
    // own subkey and associated data make it undecryptable as pins.
    let keys = InMemoryKeyProvider::new_for_tests();
    let key = keys.master_key().unwrap();
    let forged = crypto::encrypt("[]", &key, PIN_STORE_RECORD, PIN_STORE_FIELD).unwrap();
    let stored = StoredHostKeyPins::Encrypted(forged);
    assert!(decrypt_pins(Some(&stored), &keys).is_err());
    let forged = crypto::encrypt("[]", &key, PIN_STORE_RECORD, FIELD_PASSWORD).unwrap();
    assert!(decrypt_pins(Some(&StoredHostKeyPins::Encrypted(forged)), &keys).is_err());
}

#[test]
fn deleting_the_last_pin_removes_the_value() {
    with_temp_config(|path| {
        let keys = InMemoryKeyProvider::new_for_tests();
        approve_host_key(&challenge("Bastion", "SHA256:aaaa"), &keys).unwrap();
        assert!(!delete_host_key("bastion", 2222, &keys).unwrap());
        assert!(delete_host_key("bastion", 22, &keys).unwrap());
        assert!(!raw(path).contains("sshHostKeys"));
        assert_eq!(load_pins(&keys).unwrap(), []);
    });
}

const LEGACY: &str = r#"{"servers":[],"sshHostKeys":[{"host":"old.example","port":22,
    "keyAlgorithm":"ssh-ed25519","fingerprintSha256":"SHA256:old",
    "approvedAt":"2026-11-01T00:00:00Z"}]}"#;

#[test]
fn legacy_plaintext_pins_are_refused_and_never_written_back() {
    with_temp_config(|path| {
        std::fs::write(path, LEGACY).unwrap();
        let keys = InMemoryKeyProvider::new_for_tests();
        let err = load_pins(&keys).unwrap_err().to_string();
        assert!(err.contains("no migration"), "{err}");

        // Any save of the config drops the plaintext list.
        let file = config::get_sync().unwrap();
        config::save_sync(&file).unwrap();
        assert!(!raw(path).contains("old.example"));
    });
}

#[test]
fn startup_discards_legacy_pins() {
    with_temp_config(|path| {
        std::fs::write(path, LEGACY).unwrap();
        let keys = InMemoryKeyProvider::new_for_tests();
        config::prepare_sync(&keys).unwrap();
        assert!(!raw(path).contains("old.example"));
        assert_eq!(load_pins(&keys).unwrap(), []);
    });
}

#[test]
fn approval_replaces_legacy_pins_instead_of_keeping_them() {
    with_temp_config(|path| {
        std::fs::write(path, LEGACY).unwrap();
        let keys = InMemoryKeyProvider::new_for_tests();
        approve_host_key(&challenge("new.example", "SHA256:new"), &keys).unwrap();
        let text = raw(path);
        assert!(
            !text.contains("old.example") && !text.contains("new.example"),
            "{text}"
        );
        let pins = load_pins(&keys).unwrap();
        assert_eq!(pins.len(), 1);
        assert_eq!(pins[0].host, "new.example");
    });
}
