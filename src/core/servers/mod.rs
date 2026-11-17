//! Server connection profile CRUD operations and unique name generation.

pub mod secrets;

pub use secrets::{decrypt_secrets, encrypt_secrets, has_plaintext_secrets};

use crate::common::types::{Server, ServerResult};
use crate::core::config;
use crate::core::keys::KeyProvider;
use crate::core::validators::{validate, validate_unique_id};
use uuid::Uuid;

/// Asynchronously returns all saved server profiles from the configuration file.
pub async fn get_all() -> Result<Vec<Server>, String> {
    get_all_sync()
}

/// Synchronously returns all saved server profiles from the configuration file.
pub fn get_all_sync() -> Result<Vec<Server>, String> {
    let cfg = config::get_sync()?;
    Ok(cfg.servers)
}

/// Asynchronously validates, encrypts secrets with the master key, assigns a UUID, and persists a new server profile.
pub async fn add(server: &Server, keys: &dyn KeyProvider) -> Result<ServerResult, String> {
    add_sync(server, keys)
}

/// Synchronously validates, encrypts secrets with the master key, assigns a UUID, and persists a new server profile.
pub fn add_sync(server: &Server, keys: &dyn KeyProvider) -> Result<ServerResult, String> {
    let mut srv = match validate(server) {
        Ok(s) => s,
        Err(e) => {
            return Ok(ServerResult {
                data: None,
                validation_errors: Some(e.validation_errors),
            });
        }
    };

    let mut data = config::get_sync()?;
    let new_id = loop {
        let candidate = Uuid::new_v4().to_string();
        if validate_unique_id(&data.servers, Some(&candidate)).unwrap_or(false) {
            break candidate;
        }
    };

    // The ID is part of each ciphertext's associated data, so assign it before encrypting.
    srv.id = new_id;
    srv = encrypt_secrets(&srv, keys)?;
    data.servers.push(srv.clone());
    config::save_sync(&data)?;

    Ok(ServerResult {
        data: Some(srv),
        validation_errors: None,
    })
}

/// Asynchronously validates, encrypts secrets with the master key, and updates an existing server profile in place.
pub async fn update(server: &Server, keys: &dyn KeyProvider) -> Result<ServerResult, String> {
    update_sync(server, keys)
}

/// Synchronously validates, encrypts secrets with the master key, and updates an existing server profile in place.
pub fn update_sync(server: &Server, keys: &dyn KeyProvider) -> Result<ServerResult, String> {
    let mut srv = match validate(server) {
        Ok(s) => s,
        Err(e) => {
            return Ok(ServerResult {
                data: None,
                validation_errors: Some(e.validation_errors),
            });
        }
    };

    let mut data = config::get_sync()?;
    if srv.id.trim().is_empty() {
        return Err("Cannot update a saved connection without an ID".to_string());
    }
    let index = data.servers.iter().position(|item| item.id == srv.id);
    srv = encrypt_secrets(&srv, keys)?;

    if let Some(i) = index {
        data.servers[i] = srv.clone();
    } else {
        data.servers.push(srv.clone());
    }

    config::save_sync(&data)?;

    Ok(ServerResult {
        data: Some(srv),
        validation_errors: None,
    })
}

/// Asynchronously adds a new server or updates an existing server if `server.id` is non-empty.
pub async fn add_or_update(
    server: &Server,
    keys: &dyn KeyProvider,
) -> Result<ServerResult, String> {
    add_or_update_sync(server, keys)
}

/// Synchronously adds a new server or updates an existing server if `server.id` is non-empty.
pub fn add_or_update_sync(server: &Server, keys: &dyn KeyProvider) -> Result<ServerResult, String> {
    let has_id = !server.id.trim().is_empty();
    if has_id {
        update_sync(server, keys)
    } else {
        add_sync(server, keys)
    }
}

/// Asynchronously removes a server profile by its UUID `id`.
pub async fn remove_by_id(id: &str) -> Result<(), String> {
    remove_by_id_sync(id)
}

/// Synchronously removes a server profile by its UUID `id`.
pub fn remove_by_id_sync(id: &str) -> Result<(), String> {
    let mut data = config::get_sync()?;
    data.servers.retain(|srv| srv.id != id);
    config::save_sync(&data)
}

/// Generates a unique server name for duplicated servers by appending `-1`, `-2`, etc.
pub fn get_unique_name(server: &Server, existing_servers: &[Server]) -> String {
    let base_name = server.name.clone();
    let exists = |candidate: &str| existing_servers.iter().any(|s| s.name == candidate);
    if !exists(&base_name) {
        return base_name;
    }
    let mut num = 1usize;
    loop {
        let candidate = format!("{base_name}-{num}");
        if !exists(&candidate) {
            return candidate;
        }
        num += 1;
    }
}
