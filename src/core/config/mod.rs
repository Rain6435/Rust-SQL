//! Configuration file path resolution, loading, and saving.
//!
//! Manages the lifecycle of `rust-sql.json` on disk and exposes [`get_full_sync`]
//! to merge CLI flags, environment variables, and default preferences into [`Config`].

pub mod sanitize;

pub use sanitize::{
    get_full, get_full_sync, read_json_file_sync, sanitize_server, sanitize_servers,
    write_json_file_sync,
};

use crate::common::types::{BaseConfig, Config, ConfigFile};
use crate::core::keys::KeyProvider;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

/// Serializes unit tests that read or write the process-wide configuration file or its path
/// override, so one test cannot truncate or overwrite another's config mid-test.
#[cfg(test)]
pub(crate) static TEST_CONFIG_LOCK: Mutex<()> = Mutex::new(());

pub(crate) static CONFIG_CACHE: OnceLock<Mutex<Option<Config>>> = OnceLock::new();
static OVERRIDE_CONFIG_PATH: OnceLock<Mutex<Option<PathBuf>>> = OnceLock::new();

pub(crate) fn config_cache() -> &'static Mutex<Option<Config>> {
    CONFIG_CACHE.get_or_init(|| Mutex::new(None))
}

fn override_path_store() -> &'static Mutex<Option<PathBuf>> {
    OVERRIDE_CONFIG_PATH.get_or_init(|| Mutex::new(None))
}

/// Overrides the resolved configuration file path (primarily for isolated test suites).
pub fn set_config_path_override(path: Option<PathBuf>) {
    if let Ok(mut guard) = override_path_store().lock() {
        *guard = path;
    }
    if let Ok(mut cache) = config_cache().lock() {
        *cache = None;
    }
}

/// Expands a leading `~/` prefix in `filename` to the current user's home directory.
pub fn resolve_home_path_to_absolute(filename: &str) -> String {
    if let Some(rest) = filename.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(rest).to_string_lossy().into_owned();
        }
    }
    filename.to_string()
}

/// Resolves the active `rust-sql.json` configuration file path according to priority:
/// 1. Explicit test override set via [`set_config_path_override`]
/// 2. `$RUST_SQL_HOME/rust-sql.json`
/// 3. `~/.rust-sql.json` (if the file already exists)
/// 4. OS config directory (`~/.config/Rust-SQL/rust-sql.json` on Linux)
pub fn get_config_path() -> PathBuf {
    if let Ok(guard) = override_path_store().lock() {
        if let Some(ref p) = *guard {
            return p.clone();
        }
    }

    if let Ok(rust_sql_home) = std::env::var("RUST_SQL_HOME") {
        if !rust_sql_home.is_empty() {
            return Path::new(&rust_sql_home).join("rust-sql.json");
        }
    }

    if let Some(home) = dirs::home_dir() {
        let legacy = home.join(".rust-sql.json");
        if legacy.is_file() {
            return legacy;
        }
    }

    if let Some(cfg_dir) = dirs::config_dir() {
        return cfg_dir.join("Rust-SQL").join("rust-sql.json");
    }

    PathBuf::from("rust-sql.json")
}

/// Synchronously ensures the configuration file exists on disk and sanitizes/encrypts all server entries.
///
/// The master key is requested from `keys` only if a plaintext password must be encrypted.
pub fn prepare_sync(keys: &dyn KeyProvider) -> Result<(), String> {
    let filename = get_config_path();
    if !filename.exists() {
        if let Some(parent) = filename.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        write_json_file_sync(&filename, &ConfigFile::default())?;
    }

    let mut result: ConfigFile = read_json_file_sync(&filename)?;
    result.servers = sanitize_servers(&result, keys)?;
    write_json_file_sync(&filename, &result)?;
    Ok(())
}

/// Asynchronously prepares and sanitizes the configuration file on disk.
pub async fn prepare(keys: &dyn KeyProvider) -> Result<(), String> {
    prepare_sync(keys)
}

/// Returns the resolved absolute path of the active configuration file as a string.
pub fn path() -> String {
    let filename = get_config_path();
    resolve_home_path_to_absolute(&filename.to_string_lossy())
}

/// Synchronously reads the raw [`ConfigFile`] from disk, creating an empty one if missing.
pub fn get_sync() -> Result<ConfigFile, String> {
    let filename = get_config_path();
    if !filename.exists() {
        if let Some(parent) = filename.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let empty = ConfigFile::default();
        write_json_file_sync(&filename, &empty)?;
        return Ok(empty);
    }
    read_json_file_sync(&filename)
}

/// Asynchronously reads the raw [`ConfigFile`] from disk.
pub async fn get() -> Result<ConfigFile, String> {
    get_sync()
}

/// Synchronously writes `data` to the configuration file and invalidates the in-memory cache.
pub fn save_sync(data: &ConfigFile) -> Result<(), String> {
    let filename = get_config_path();
    if let Some(parent) = filename.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    write_json_file_sync(&filename, data)?;
    if let Ok(mut cache) = config_cache().lock() {
        *cache = None;
    }
    Ok(())
}

/// Asynchronously writes `data` to the configuration file.
pub async fn save(data: &ConfigFile) -> Result<(), String> {
    save_sync(data)
}

/// Synchronously merges `settings` into the existing configuration file on disk.
pub fn save_settings_sync(settings: &BaseConfig) -> Result<(), String> {
    let mut full = get_sync()?;
    if settings.log.is_some() {
        full.base.log = settings.log.clone();
    }
    if settings.zoom_factor.is_some() {
        full.base.zoom_factor = settings.zoom_factor;
    }
    if settings.limit_query_default_select_top.is_some() {
        full.base.limit_query_default_select_top = settings.limit_query_default_select_top;
    }
    if settings.enabled_auto_complete.is_some() {
        full.base.enabled_auto_complete = settings.enabled_auto_complete;
    }
    if settings.enabled_live_auto_complete.is_some() {
        full.base.enabled_live_auto_complete = settings.enabled_live_auto_complete;
    }
    if settings.enabled_dark_theme.is_some() {
        full.base.enabled_dark_theme = settings.enabled_dark_theme;
    }
    if settings.disabled_open_animation.is_some() {
        full.base.disabled_open_animation = settings.disabled_open_animation;
    }
    if settings.csv_delimiter.is_some() {
        full.base.csv_delimiter = settings.csv_delimiter.clone();
    }
    if settings.connections_as_list.is_some() {
        full.base.connections_as_list = settings.connections_as_list;
    }
    if settings.custom_font.is_some() {
        full.base.custom_font = settings.custom_font.clone();
    }
    save_sync(&full)
}

/// Asynchronously merges `settings` into the existing configuration file on disk.
pub async fn save_settings(settings: &BaseConfig) -> Result<(), String> {
    save_settings_sync(settings)
}
