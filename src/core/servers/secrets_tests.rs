//! Tests for saved-credential encryption on server profiles.

use super::*;
use crate::common::types::SshConfig;
use crate::core::keys::{InMemoryKeyProvider, UnavailableKeyProvider};

fn server(id: &str, pw: &str, ssh_pw: Option<&str>) -> Server {
    Server {
        id: id.into(),
        name: "pg".into(),
        client: "postgresql".into(),
        password: Some(Password::plain(pw)),
        ssh: ssh_pw.map(|p| SshConfig {
            user: "u".into(),
            host: "bastion".into(),
            port: 22,
            password: Some(Password::plain(p)),
            passphrase: Some(Password::plain("key-phrase")),
            ..Default::default()
        }),
        ..Default::default()
    }
}

#[test]
fn round_trip_database_ssh_password_and_passphrase() {
    let keys = InMemoryKeyProvider::new_for_tests();
    let enc = encrypt_secrets(&server("c1", "db-pw", Some("ssh-pw")), &keys).unwrap();
    let json = serde_json::to_string(&enc).unwrap();
    for secret in ["db-pw", "ssh-pw", "key-phrase"] {
        assert!(!json.contains(secret), "{secret} stored in clear: {json}");
    }
    assert_eq!(json.matches("\"v2:").count(), 3, "{json}");
    assert!(!has_plaintext_secrets(&enc));

    let dec = decrypt_secrets(&enc, &keys).unwrap();
    assert_eq!(dec.password, Some(Password::plain("db-pw")));
    let ssh = dec.ssh.unwrap();
    assert_eq!(ssh.password, Some(Password::plain("ssh-pw")));
    assert_eq!(ssh.passphrase, Some(Password::plain("key-phrase")));
}

#[test]
fn unavailable_keychain_refuses_to_store_or_decrypt_passwords() {
    let err = encrypt_secrets(&server("c1", "db-pw", None), &UnavailableKeyProvider).unwrap_err();
    assert!(err.contains("keychain"), "{err}");

    let keys = InMemoryKeyProvider::new_for_tests();
    let enc = encrypt_secrets(&server("c1", "db-pw", None), &keys).unwrap();
    let err = decrypt_secrets(&enc, &UnavailableKeyProvider).unwrap_err();
    assert!(err.contains("keychain"), "{err}");
}

#[test]
fn profiles_without_passwords_never_need_the_key() {
    let enc = encrypt_secrets(&server("c1", "", None), &UnavailableKeyProvider).unwrap();
    let dec = decrypt_secrets(&enc, &UnavailableKeyProvider).unwrap();
    assert_eq!(dec.password, Some(Password::plain("")));
}

#[test]
fn ciphertext_moved_to_another_connection_fails_to_decrypt() {
    let keys = InMemoryKeyProvider::new_for_tests();
    let a = encrypt_secrets(&server("conn-a", "pw-a", None), &keys).unwrap();
    let mut b = encrypt_secrets(&server("conn-b", "pw-b", None), &keys).unwrap();
    b.password = a.password.clone();
    let err = decrypt_secrets(&b, &keys).unwrap_err();
    assert!(err.contains("different"), "{err}");
}

#[test]
fn ciphertext_moved_to_another_field_fails_to_decrypt() {
    let keys = InMemoryKeyProvider::new_for_tests();
    let mut enc = encrypt_secrets(&server("c1", "db-pw", Some("ssh-pw")), &keys).unwrap();
    let ssh = enc.ssh.as_mut().unwrap();
    std::mem::swap(&mut enc.password, &mut ssh.password);
    assert!(decrypt_secrets(&enc, &keys).is_err());
}

#[test]
fn saving_ciphertext_copied_from_another_record_is_refused() {
    let keys = InMemoryKeyProvider::new_for_tests();
    let mut copy = encrypt_secrets(&server("conn-a", "pw-a", None), &keys).unwrap();
    copy.id = "conn-b".into();
    assert!(encrypt_secrets(&copy, &keys).is_err());
}

#[test]
fn already_encrypted_values_for_the_same_record_are_kept() {
    let keys = InMemoryKeyProvider::new_for_tests();
    let enc = encrypt_secrets(&server("c1", "db-pw", None), &keys).unwrap();
    assert_eq!(encrypt_secrets(&enc, &keys).unwrap(), enc);
}

#[test]
fn a_profile_without_an_id_cannot_store_a_password() {
    let keys = InMemoryKeyProvider::new_for_tests();
    let err = encrypt_secrets(&server("", "db-pw", None), &keys).unwrap_err();
    assert!(err.contains("connection ID"), "{err}");
}
