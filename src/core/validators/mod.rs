//! Server profile schema validation enforcing client-specific `disabledFeatures` and address rules.

#[cfg(test)]
mod tests;

use crate::adapters::CLIENTS;
use crate::common::types::{Password, Server, ValidationErrors};
use std::collections::HashSet;
use thiserror::Error;

/// Error returned when a [`Server`] configuration fails schema validation.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("Server validation failed")]
pub struct ServerValidationError {
    /// Map of invalid field paths to human-readable error messages.
    pub validation_errors: ValidationErrors,
}

impl ServerValidationError {
    /// Creates a new [`ServerValidationError`] wrapping `validation_errors`.
    pub fn new(validation_errors: ValidationErrors) -> Self {
        Self { validation_errors }
    }
}

const ADDRESS_FIELDS: [&str; 3] = ["host", "port", "socketPath"];

/// Validates and sanitizes a [`Server`] configuration, trimming string fields and verifying
/// address/port/socket/SSH constraints for the selected database client.
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

    if !disabled_fields.contains("name") && sanitized.name.is_empty() {
        errors.insert(
            "name".to_string(),
            "\"name\" is not allowed to be empty".to_string(),
        );
    }

    if sanitized.client.is_empty() || client_config.is_none() {
        errors.insert(
            "client".to_string(),
            "\"client\" must be one of [mysql, mariadb, postgresql, redshift, sqlserver, sqlite, cassandra]"
                .to_string(),
        );
    }

    if !disabled_fields.contains("ssl") && sanitized.ssl.is_none() {
        errors.insert("ssl".to_string(), "\"ssl\" is required".to_string());
    }

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

    if !disabled_fields.contains("password") {
        if let Some(pw) = &mut sanitized.password {
            validate_password(pw, "password", &mut errors);
        }
    }

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

    if has_host_field && has_socket_field && (host_present == socket_present) {
        let msg = if !host_present && !socket_present {
            "\"value\" must contain at least one of [host, socketPath]".to_string()
        } else {
            "\"value\" contains a conflict between exclusive peers [host, socketPath]".to_string()
        };
        for field in &active_address_fields {
            errors.insert((*field).to_string(), msg.clone());
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
            *s = trimmed.into();
        }
        Password::Encrypted(enc) => {
            if !enc
                .ciphertext
                .starts_with(crate::core::crypto::FORMAT_V2_PREFIX)
            {
                errors.insert(
                    path.to_string(),
                    format!("\"{path}\" contains an invalid value"),
                );
            }
        }
    }
}

/// Checks whether `server_id` is unique among `servers`, returning an error if `server_id` is `None` or empty.
pub fn validate_unique_id(servers: &[Server], server_id: Option<&str>) -> Result<bool, String> {
    let Some(id) = server_id.filter(|s| !s.is_empty()) else {
        return Err("serverId should be set".to_string());
    };
    Ok(!servers.iter().any(|srv| srv.id == id))
}
