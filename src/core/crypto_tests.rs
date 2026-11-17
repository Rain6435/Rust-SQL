//! Tests for `v2` credential encryption: round trips, tampering, record binding, and versions.

use super::*;

fn key(b: u8) -> MasterKey {
    MasterKey::from_bytes([b; 32])
}

fn round_trip(plain: &str) -> String {
    let enc = encrypt(plain, &key(1), "conn-1", "password").unwrap();
    assert!(enc.ciphertext.starts_with("v2:"), "{}", enc.ciphertext);
    decrypt(&enc, &key(1), "conn-1", "password")
        .unwrap()
        .to_string()
}

#[test]
fn round_trips_empty_unicode_and_large_values() {
    assert_eq!(round_trip(""), "");
    let unicode = "pässwörd 🔐 密码 \u{0}\n'\"\\";
    assert_eq!(round_trip(unicode), unicode);
    let large: String = "x9€".repeat(2_000);
    assert!(large.len() > 4096);
    assert_eq!(round_trip(&large), large);
}

#[test]
fn ciphertext_does_not_contain_the_plaintext_and_nonces_differ() {
    let a = encrypt("same-secret", &key(1), "c", "password").unwrap();
    let b = encrypt("same-secret", &key(1), "c", "password").unwrap();
    assert_ne!(a, b, "a fresh nonce must be used for every value");
    assert!(!a.ciphertext.contains("same-secret"));
}

#[test]
fn flipping_any_byte_of_the_payload_fails() {
    let enc = encrypt("tamper-me", &key(1), "c", "password").unwrap();
    let blob = BASE64.decode(&enc.ciphertext[3..]).unwrap();
    for i in 0..blob.len() {
        for bit in [0x01u8, 0x80] {
            let mut bad = blob.clone();
            bad[i] ^= bit;
            let tampered = EncryptedPassword {
                ciphertext: format!("v2:{}", BASE64.encode(&bad)),
            };
            assert_eq!(
                decrypt(&tampered, &key(1), "c", "password"),
                Err(CryptoError::AuthenticationFailed),
                "byte {i} bit {bit:#x}"
            );
        }
    }
}

#[test]
fn truncated_payload_is_rejected() {
    let enc = encrypt("x", &key(1), "c", "password").unwrap();
    let blob = BASE64.decode(&enc.ciphertext[3..]).unwrap();
    for len in [0, 10, NONCE_LEN + TAG_LEN - 1] {
        let short = EncryptedPassword {
            ciphertext: format!("v2:{}", BASE64.encode(&blob[..len])),
        };
        assert_eq!(
            decrypt(&short, &key(1), "c", "password"),
            Err(CryptoError::Malformed)
        );
    }
}

#[test]
fn moving_a_value_to_another_record_or_field_fails() {
    let enc = encrypt("db-pw", &key(1), "conn-a", "password").unwrap();
    assert_eq!(
        decrypt(&enc, &key(1), "conn-b", "password"),
        Err(CryptoError::AuthenticationFailed)
    );
    assert_eq!(
        decrypt(&enc, &key(1), "conn-a", "ssh.password"),
        Err(CryptoError::AuthenticationFailed)
    );
    // Length prefixes stop "ab"+"c" from colliding with "a"+"bc".
    let enc = encrypt("x", &key(1), "ab", "c").unwrap();
    assert_eq!(
        decrypt(&enc, &key(1), "a", "bc"),
        Err(CryptoError::AuthenticationFailed)
    );
}

#[test]
fn a_different_master_key_fails() {
    let enc = encrypt("db-pw", &key(1), "c", "password").unwrap();
    assert_eq!(
        decrypt(&enc, &key(2), "c", "password"),
        Err(CryptoError::AuthenticationFailed)
    );
}

#[test]
fn values_without_a_known_version_prefix_are_rejected() {
    let enc = encrypt("db-pw", &key(1), "c", "password").unwrap();
    let body = &enc.ciphertext[3..];
    for ciphertext in [
        body.to_string(),
        format!("v1:{body}"),
        format!("v3:{body}"),
        format!("V2:{body}"),
        String::new(),
    ] {
        let value = EncryptedPassword { ciphertext };
        assert_eq!(
            decrypt(&value, &key(1), "c", "password"),
            Err(CryptoError::UnsupportedVersion)
        );
    }
}

#[test]
fn an_empty_record_id_is_refused() {
    assert_eq!(
        encrypt("x", &key(1), " ", "password"),
        Err(CryptoError::MissingRecordId)
    );
    let enc = encrypt("x", &key(1), "c", "password").unwrap();
    assert_eq!(
        decrypt(&enc, &key(1), "", "password"),
        Err(CryptoError::MissingRecordId)
    );
}

#[test]
fn errors_never_contain_the_plaintext() {
    let enc = encrypt("very-secret-value", &key(1), "c", "password").unwrap();
    let err = decrypt(&enc, &key(2), "c", "password").unwrap_err();
    assert!(!err.to_string().contains("very-secret-value"));
}
