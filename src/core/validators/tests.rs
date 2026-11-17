//! Unit tests for [`super::validate`] and [`super::validate_unique_id`].

use super::*;
use crate::common::types::{EncryptedPassword, SshConfig, SslConfig};

#[test]
fn test_validate_valid_servers() {
    let s1 = Server {
        id: "c94cbafa-8977-4142-9f34-c84d382d8731".into(),
        name: "pg-vm".into(),
        client: "postgresql".into(),
        host: Some("10.10.10.10".into()),
        port: Some(5432),
        user: Some("user".into()),
        password: Some(Password::Plain("password".into())),
        database: Some("company".into()),
        ssl: Some(SslConfig::Bool(false)),
        ..Default::default()
    };
    assert!(validate(&s1).is_ok());

    let s2 = Server {
        password: Some(Password::Encrypted(EncryptedPassword {
            iv_text: "wGf6X9T+QSygOHqtgQPlcA==".into(),
            encrypted_text: "0LySDs9WPAvwSS9Qv+W3/A==".into(),
        })),
        ..s1.clone()
    };
    assert!(validate(&s2).is_ok());

    let s3 = Server {
        ssh: Some(SshConfig {
            host: "10.10.10.10".into(),
            port: 22,
            user: "core".into(),
            private_key: Some("~/.vagrant.d/insecure_private_key".into()),
            ..Default::default()
        }),
        ..s1.clone()
    };
    assert!(validate(&s3).is_ok());

    for client in ["mysql", "postgresql", "sqlserver", "sqlite", "cassandra"] {
        let mut srv = s1.clone();
        srv.client = client.to_string();
        assert!(validate(&srv).is_ok(), "client {client} should validate");
    }
}

#[test]
fn test_validate_invalid_client() {
    let srv = Server {
        id: "c94cbafa-8977-4142-9f34-c84d382d8731".into(),
        name: "pg-vm".into(),
        client: "fake-client".into(),
        host: Some("10.10.10.10".into()),
        port: Some(5432),
        user: Some("user".into()),
        password: Some(Password::Plain("password".into())),
        database: Some("company".into()),
        ssl: Some(SslConfig::Bool(false)),
        ..Default::default()
    };
    let err = validate(&srv).unwrap_err();
    assert!(err.validation_errors.contains_key("client"));
}

#[test]
fn test_validate_unique_id() {
    assert_eq!(
        validate_unique_id(&[], None),
        Err("serverId should be set".to_string())
    );
    assert_eq!(
        validate_unique_id(&[], Some("1c7cdae9-6065-46fa-a7d0-b89ccff78703")),
        Ok(true)
    );
    let existing = vec![Server {
        id: "1c7cdae9-6065-46fa-a7d0-b89ccff78703".into(),
        ..Default::default()
    }];
    assert_eq!(
        validate_unique_id(&existing, Some("1c7cdae9-6065-46fa-a7d0-b89ccff78703")),
        Ok(false)
    );
}
