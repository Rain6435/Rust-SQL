//! Unit tests for accounts: password policy, Argon2id hash format, login checks, last-admin guard.

use crate::core::appdb::test_db;

#[test]
fn passwords_shorter_than_twelve_characters_are_rejected() {
    let (_d, db) = test_db();
    let err = db.create_user("alice", "elevenchars", false).unwrap_err();
    assert!(err.to_string().contains("at least 12"), "{err}");
    assert!(db.create_user("alice", "twelve-chars", false).is_ok());
}

#[test]
fn hashes_are_argon2id_phc_strings_with_salt_and_params() {
    let (_d, db) = test_db();
    db.create_user("alice", "correct horse battery", false)
        .unwrap();
    let hash: String = db
        .conn()
        .unwrap()
        .query_row("SELECT password_hash FROM users", [], |r| r.get(0))
        .unwrap();
    assert!(hash.starts_with("$argon2id$v=19$m="), "{hash}");
    assert_eq!(hash.split('$').count(), 6, "{hash}");
    assert!(!hash.contains("correct horse"));
}

#[test]
fn unknown_user_and_wrong_password_are_indistinguishable() {
    let (_d, db) = test_db();
    db.create_user("alice", "correct horse battery", false)
        .unwrap();
    assert_eq!(db.verify_login("alice", "wrong password!!").unwrap(), None);
    assert_eq!(
        db.verify_login("mallory", "wrong password!!").unwrap(),
        None
    );
    let ok = db.verify_login("ALICE", "correct horse battery").unwrap();
    assert_eq!(ok.map(|u| u.username), Some("alice".to_string()));
}

#[test]
fn duplicate_and_invalid_usernames_are_rejected() {
    let (_d, db) = test_db();
    db.create_user("alice", "correct horse battery", false)
        .unwrap();
    assert!(db
        .create_user("Alice", "correct horse battery", false)
        .is_err());
    assert!(db
        .create_user("bad name", "correct horse battery", false)
        .is_err());
}

#[test]
fn the_last_admin_cannot_be_deleted_or_demoted() {
    let (_d, db) = test_db();
    let admin = db
        .create_user("root", "correct horse battery", true)
        .unwrap();
    assert!(db.delete_user(admin.id).is_err());
    assert!(db.set_admin(admin.id, false).is_err());
    let second = db
        .create_user("ops", "correct horse battery", true)
        .unwrap();
    db.set_admin(admin.id, false).unwrap();
    assert!(db.delete_user(second.id).is_err());
}
