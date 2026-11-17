//! Server entry sanitization, runtime configuration merging, and JSON file I/O helpers.

use crate::common::types::{
    Config, ConfigFile, CryptoConfig, LogOptions, Password, RepositoryInfo, Server, SslConfig,
};
use crate::core::config::{config_cache, get_sync, path, prepare_sync, DEFAULT_CRYPTO_SECRET};
use crate::core::crypto;
use std::path::Path;
use uuid::Uuid;

/// Sanitizes a single [`Server`] entry by assigning a UUID if missing, defaulting `ssl` to `false`,
/// and encrypting any plaintext passwords using `crypto_secret`.
pub fn sanitize_server(server: &Server, crypto_secret: &str) -> Result<Server, String> {
    let mut srv = server.clone();

    if srv.id.trim().is_empty() {
        srv.id = Uuid::new_v4().to_string();
    }

    if srv.ssl.is_none() {
        srv.ssl = Some(SslConfig::Bool(false));
    }

    match srv.encrypted {
        None => {
            srv.encrypted = Some(true);
            if let Some(Password::Plain(ref plain)) = srv.password {
                if !plain.is_empty() {
                    let enc = crypto::encrypt(plain, crypto_secret).map_err(|e| e.to_string())?;
                    srv.password = Some(Password::Encrypted(enc));
                }
            }
            if let Some(ref mut ssh) = srv.ssh {
                if let Some(Password::Plain(ref plain)) = ssh.password {
                    if !plain.is_empty() {
                        let enc =
                            crypto::encrypt(plain, crypto_secret).map_err(|e| e.to_string())?;
                        ssh.password = Some(Password::Encrypted(enc));
                    }
                }
            }
        }
        Some(true) => {
            if let Some(Password::Plain(ref plain)) = srv.password {
                if !plain.is_empty() {
                    let dec =
                        crypto::unsafe_decrypt(plain, crypto_secret).map_err(|e| e.to_string())?;
                    let enc = crypto::encrypt(&dec, crypto_secret).map_err(|e| e.to_string())?;
                    srv.password = Some(Password::Encrypted(enc));
                }
            }
            if let Some(ref mut ssh) = srv.ssh {
                if let Some(Password::Plain(ref plain)) = ssh.password {
                    if !plain.is_empty() {
                        let dec = crypto::unsafe_decrypt(plain, crypto_secret)
                            .map_err(|e| e.to_string())?;
                        let enc =
                            crypto::encrypt(&dec, crypto_secret).map_err(|e| e.to_string())?;
                        ssh.password = Some(Password::Encrypted(enc));
                    }
                }
            }
        }
        Some(false) => {}
    }

    Ok(srv)
}

/// Sanitizes all server entries within a [`ConfigFile`].
pub fn sanitize_servers(data: &ConfigFile, crypto_secret: &str) -> Result<Vec<Server>, String> {
    data.servers
        .iter()
        .map(|srv| sanitize_server(srv, crypto_secret))
        .collect()
}

/// Returns the fully merged runtime [`Config`] object, optionally bypassing the cache.
pub fn get_full_sync(force_clean_cache: bool) -> Result<Config, String> {
    if !force_clean_cache {
        if let Ok(guard) = config_cache().lock() {
            if let Some(ref cached) = *guard {
                return Ok(cached.clone());
            }
        }
    }

    let _ = prepare_sync(DEFAULT_CRYPTO_SECRET);
    let app_config = get_sync().unwrap_or_default();
    let config_path = path();

    let args: Vec<String> = std::env::args().collect();
    let dev_mode = args.iter().any(|a| a == "--dev");
    let print_version = args.iter().any(|a| a == "--version" || a == "-v");
    let is_dev = std::env::var("NODE_ENV")
        .map(|v| v != "production")
        .unwrap_or(false)
        || dev_mode;

    let default_level = if std::env::var("DEBUG").is_ok() {
        "debug".to_string()
    } else {
        "error".to_string()
    };

    let log = match &app_config.base.log {
        Some(l) => LogOptions {
            console: l.console,
            file: l.file,
            level: if l.level.is_empty() {
                default_level
            } else {
                l.level.clone()
            },
            path: if l.path.is_empty() {
                config_path.replace(".json", ".log")
            } else {
                l.path.clone()
            },
        },
        None => LogOptions {
            console: is_dev,
            file: false,
            level: default_level,
            path: config_path.replace(".json", ".log"),
        },
    };

    let full = Config {
        servers: app_config.servers,
        log,
        zoom_factor: app_config.base.zoom_factor.unwrap_or(1.0),
        limit_query_default_select_top: app_config
            .base
            .limit_query_default_select_top
            .unwrap_or(100),
        enabled_auto_complete: app_config.base.enabled_auto_complete.unwrap_or(true),
        enabled_live_auto_complete: app_config.base.enabled_live_auto_complete.unwrap_or(true),
        enabled_dark_theme: app_config.base.enabled_dark_theme.unwrap_or(false),
        disabled_open_animation: app_config.base.disabled_open_animation.unwrap_or(false),
        csv_delimiter: app_config
            .base
            .csv_delimiter
            .unwrap_or_else(|| ",".to_string()),
        connections_as_list: app_config.base.connections_as_list.unwrap_or(false),
        custom_font: app_config
            .base
            .custom_font
            .unwrap_or_else(|| "Lato".to_string()),
        name: "Rust-SQL".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        path: Some(config_path),
        homepage: Some("https://github.com/Rain6435/Rust-SQL".to_string()),
        bugs: Some("https://github.com/Rain6435/Rust-SQL/issues".to_string()),
        repository: Some(RepositoryInfo {
            url: "https://github.com/Rain6435/Rust-SQL".to_string(),
        }),
        dev_mode: Some(dev_mode),
        print_version: Some(print_version),
        crypto: Some(CryptoConfig {
            secret: DEFAULT_CRYPTO_SECRET.to_string(),
        }),
    };

    if let Ok(mut guard) = config_cache().lock() {
        *guard = Some(full.clone());
    }

    Ok(full)
}

/// Asynchronously returns the fully merged runtime [`Config`] object.
pub async fn get_full(force_clean_cache: bool) -> Result<Config, String> {
    get_full_sync(force_clean_cache)
}

/// Reads and deserializes a JSON file from `path`.
pub fn read_json_file_sync<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T, String> {
    let content = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    serde_json::from_str(&content).map_err(|e| e.to_string())
}

/// Serializes `data` as pretty-printed JSON with a trailing newline and writes it to `path`.
pub fn write_json_file_sync<T: serde::Serialize>(path: &Path, data: &T) -> Result<(), String> {
    let mut json = serde_json::to_string_pretty(data).map_err(|e| e.to_string())?;
    json.push('\n');
    std::fs::write(path, json).map_err(|e| e.to_string())
}
