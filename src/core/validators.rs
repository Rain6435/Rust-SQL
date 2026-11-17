use crate::adapters::CLIENTS;
use crate::common::types::{Password, Server, ValidationErrors};
use std::collections::HashSet;
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("Server validation failed")]
pub struct ServerValidationError {
    pub validation_errors: ValidationErrors,
}

impl ServerValidationError {
    pub fn new(validation_errors: ValidationErrors) -> Self {
        Self { validation_errors }
    }
}

const ADDRESS_FIELDS: [&str; 3] = ["host", "port", "socketPath"];

/// Validates and sanitizes a `Server` configuration, matching `src/browser/core/validators/server.ts`.
pub fn validate(server: &Server) -> Result<Server, ServerValidationError> {
    let mut sanitized = server.clone();
    let mut errors = ValidationErrors::new();
    let mut disabled_fields = HashSet::new();

    sanitized.name = sanitized.name.trim().to_string();
    sanitized.client = sanitized.client.trim().to_string();

    let client_config = CLIENTS.iter().find(|c| c.key == sanitized.client);
    if let Some(cfg) = client_config {
        for item in &cfg.disabled_features {
            if let Some((region, field)) = item.split_once(':') {
                if region == "server" {
                    disabled_fields.insert(field.to_string());
                }
            }
        }
    }

    // 1. name: required, trimmed, min(1)
    if !disabled_fields.contains("name") && sanitized.name.is_empty() {
        errors.insert(
            "name".to_string(),
            "\"name\" is not allowed to be empty".to_string(),
        );
    }

    // 2. client: required, must be in CLIENTS
    if sanitized.client.is_empty() || client_config.is_none() {
        errors.insert(
            "client".to_string(),
            "\"client\" must be one of [mysql, mariadb, postgresql, redshift, sqlserver, sqlite, cassandra]"
                .to_string(),
        );
    }

    // 3. ssl: required unless disabled
    if !disabled_fields.contains("ssl") && sanitized.ssl.is_none() {
        errors.insert("ssl".to_string(), "\"ssl\" is required".to_string());
    }

    // Sanitize optional string fields (trim; if empty and present, that's a min(1) violation if enabled)
    let mut check_optional_string = |field_name: &str, opt: &mut Option<String>| {
        if disabled_fields.contains(field_name) {
            return;
        }
        if let Some(val) = opt {
            let trimmed = val.trim().to_string();
            if trimmed.is_empty() {
                errors.insert(
                    field_name.to_string(),
                    format!("\"{field_name}\" is not allowed to be empty"),
                );
            }
            *val = trimmed;
        }
    };

    check_optional_string("host", &mut sanitized.host);
    check_optional_string("socketPath", &mut sanitized.socket_path);
    check_optional_string("database", &mut sanitized.database);
    check_optional_string("user", &mut sanitized.user);

    // 4. port: integer 0..=99999
    if !disabled_fields.contains("port") {
        if let Some(p) = sanitized.port {
            if p > 99999 {
                errors.insert(
                    "port".to_string(),
                    "\"port\" must be less than or equal to 99999".to_string(),
                );
            }
        }
    }

    // 5. password: PASSWORD_SCHEMA
    if !disabled_fields.contains("password") {
        if let Some(pw) = &mut sanitized.password {
            validate_password(pw, "password", &mut errors);
        }
    }

    // 6. ssh: SSH_SCHEMA
    if !disabled_fields.contains("ssh") {
        if let Some(ssh) = &mut sanitized.ssh {
            ssh.host = ssh.host.trim().to_string();
            if ssh.host.is_empty() {
                errors.insert(
                    "ssh.host".to_string(),
                    "\"ssh.host\" is not allowed to be empty".to_string(),
                );
            }
            if ssh.port > 99999 {
                errors.insert(
                    "ssh.port".to_string(),
                    "\"ssh.port\" must be less than or equal to 99999".to_string(),
                );
            }
            ssh.user = ssh.user.trim().to_string();
            if ssh.user.is_empty() {
                errors.insert(
                    "ssh.user".to_string(),
                    "\"ssh.user\" is required".to_string(),
                );
            }
            if let Some(pk) = &mut ssh.private_key {
                *pk = pk.trim().to_string();
                if pk.is_empty() {
                    errors.insert(
                        "ssh.privateKey".to_string(),
                        "\"ssh.privateKey\" is not allowed to be empty".to_string(),
                    );
                }
            }
            if let Some(pw) = &mut ssh.password {
                validate_password(pw, "ssh.password", &mut errors);
            }
        }
    }

    // 7. Address field relationships: xor('host', 'socketPath') and and('host', 'port')
    let has_host_field = !disabled_fields.contains("host");
    let has_port_field = !disabled_fields.contains("port");
    let has_socket_field = !disabled_fields.contains("socketPath");

    let active_address_fields: Vec<&str> = ADDRESS_FIELDS
        .iter()
        .copied()
        .filter(|f| !disabled_fields.contains(*f))
        .collect();

    let host_present = sanitized
        .host
        .as_ref()
        .map(|s| !s.is_empty())
        .unwrap_or(false);
    let port_present = sanitized.port.is_some();
    let socket_present = sanitized
        .socket_path
        .as_ref()
        .map(|s| !s.is_empty())
        .unwrap_or(false);

    if has_host_field && has_socket_field {
        // xor(host, socketPath): exactly one must be present
        if host_present == socket_present {
            let msg = if !host_present && !socket_present {
                "\"value\" must contain at least one of [host, socketPath]".to_string()
            } else {
                "\"value\" contains a conflict between exclusive peers [host, socketPath]"
                    .to_string()
            };
            for field in &active_address_fields {
                errors.insert((*field).to_string(), msg.clone());
            }
        }
    } else if has_host_field && !has_socket_field {
        // e.g. cassandra or sqlserver where socketPath is disabled or not used
        if has_port_field && (host_present != port_present) {
            let msg = "\"value\" contains [host, port] without its required peers".to_string();
            for field in &active_address_fields {
                errors.insert((*field).to_string(), msg.clone());
            }
        }
    }

    if has_host_field && has_port_field && (host_present != port_present) {
        let msg = "\"value\" contains [host, port] without its required peers".to_string();
        for field in &active_address_fields {
            errors.insert((*field).to_string(), msg.clone());
        }
    }

    if !errors.is_empty() {
        return Err(ServerValidationError::new(errors));
    }

    Ok(sanitized)
}

fn validate_password(pw: &mut Password, path: &str, errors: &mut ValidationErrors) {
    match pw {
        Password::Plain(s) => {
            let trimmed = s.trim().to_string();
            if trimmed.is_empty() {
                errors.insert(
                    path.to_string(),
                    format!("\"{path}\" is not allowed to be empty"),
                );
            }
            *s = trimmed;
        }
        Password::Encrypted(enc) => {
            enc.iv_text = enc.iv_text.trim().to_string();
            enc.encrypted_text = enc.encrypted_text.trim().to_string();
            if enc.iv_text.is_empty() && enc.encrypted_text.is_empty() {
                errors.insert(
                    path.to_string(),
                    format!("\"{path}\" contains an invalid value"),
                );
            }
        }
    }
}

pub fn validate_unique_id(servers: &[Server], server_id: Option<&str>) -> Result<bool, String> {
    let Some(id) = server_id.filter(|s| !s.is_empty()) else {
        return Err("serverId should be set".to_string());
    };
    Ok(!servers.iter().any(|srv| srv.id == id))
}

#[cfg(test)]
mod tests {
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
}
