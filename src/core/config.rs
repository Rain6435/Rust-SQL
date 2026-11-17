use crate::common::types::{
    BaseConfig, Config, ConfigFile, CryptoConfig, LogOptions, Password, RepositoryInfo, Server,
    SslConfig,
};
use crate::core::crypto;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use uuid::Uuid;

pub const DEFAULT_CRYPTO_SECRET: &str =
    "j[F6Y6NoWT}+YG|4c|-<89:ByJ83-9Aj?O8>$Zk/[WFk_~gFbg7<wm+*V|A{xQZ,";

static CONFIG_CACHE: OnceLock<Mutex<Option<Config>>> = OnceLock::new();
static OVERRIDE_CONFIG_PATH: OnceLock<Mutex<Option<PathBuf>>> = OnceLock::new();

fn config_cache() -> &'static Mutex<Option<Config>> {
    CONFIG_CACHE.get_or_init(|| Mutex::new(None))
}

fn override_path_store() -> &'static Mutex<Option<PathBuf>> {
    OVERRIDE_CONFIG_PATH.get_or_init(|| Mutex::new(None))
}

/// Allows tests to override the config path (`utils-stub` equivalent).
pub fn set_config_path_override(path: Option<PathBuf>) {
    if let Ok(mut guard) = override_path_store().lock() {
        *guard = path;
    }
    if let Ok(mut cache) = config_cache().lock() {
        *cache = None;
    }
}

/// Resolves `~` to the user's home directory (`resolveHomePathToAbsolute`).
pub fn resolve_home_path_to_absolute(filename: &str) -> String {
    if let Some(rest) = filename.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(rest).to_string_lossy().into_owned();
        }
    }
    filename.to_string()
}

/// Resolves the Sqlectron config file path (`getConfigPath` in `src/browser/core/utils/index.ts`):
/// 1. Override path (for tests)
/// 2. `$SQLECTRON_HOME/sqlectron.json`
/// 3. `~/.sqlectron.json` (if it already exists)
/// 4. `~/.config/Sqlectron/sqlectron.json` (or OS-specific config dir)
pub fn get_config_path() -> PathBuf {
    if let Ok(guard) = override_path_store().lock() {
        if let Some(ref p) = *guard {
            return p.clone();
        }
    }

    if let Ok(sqlectron_home) = std::env::var("SQLECTRON_HOME") {
        if !sqlectron_home.is_empty() {
            return Path::new(&sqlectron_home).join("sqlectron.json");
        }
    }

    if let Some(home) = dirs::home_dir() {
        let legacy = home.join(".sqlectron.json");
        if legacy.is_file() {
            return legacy;
        }
    }

    if let Some(cfg_dir) = dirs::config_dir() {
        return cfg_dir.join("Sqlectron").join("sqlectron.json");
    }

    PathBuf::from("sqlectron.json")
}

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

pub fn sanitize_servers(data: &ConfigFile, crypto_secret: &str) -> Result<Vec<Server>, String> {
    data.servers
        .iter()
        .map(|srv| sanitize_server(srv, crypto_secret))
        .collect()
}

pub fn prepare_sync(crypto_secret: &str) -> Result<(), String> {
    let filename = get_config_path();
    if !filename.exists() {
        if let Some(parent) = filename.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let empty = ConfigFile::default();
        write_json_file_sync(&filename, &empty)?;
    }

    let mut result: ConfigFile = read_json_file_sync(&filename)?;
    result.servers = sanitize_servers(&result, crypto_secret)?;
    write_json_file_sync(&filename, &result)?;
    Ok(())
}

pub async fn prepare(crypto_secret: &str) -> Result<(), String> {
    prepare_sync(crypto_secret)
}

pub fn path() -> String {
    let filename = get_config_path();
    resolve_home_path_to_absolute(&filename.to_string_lossy())
}

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

pub async fn get() -> Result<ConfigFile, String> {
    get_sync()
}

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

pub async fn save(data: &ConfigFile) -> Result<(), String> {
    save_sync(data)
}

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

pub async fn save_settings(settings: &BaseConfig) -> Result<(), String> {
    save_settings_sync(settings)
}

/// Returns the full merged `Config` object (`getConfig` in `src/browser/config.ts`).
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
        name: "sqlectron".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        path: Some(config_path),
        homepage: Some("https://sqlectron.github.io".to_string()),
        bugs: Some("https://github.com/sqlectron/sqlectron-gui/issues".to_string()),
        repository: Some(RepositoryInfo {
            url: "https://github.com/sqlectron/sqlectron-gui".to_string(),
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

pub async fn get_full(force_clean_cache: bool) -> Result<Config, String> {
    get_full_sync(force_clean_cache)
}

fn read_json_file_sync<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T, String> {
    let content = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    serde_json::from_str(&content).map_err(|e| e.to_string())
}

fn write_json_file_sync<T: serde::Serialize>(path: &Path, data: &T) -> Result<(), String> {
    let mut json = serde_json::to_string_pretty(data).map_err(|e| e.to_string())?;
    json.push('\n');
    std::fs::write(path, json).map_err(|e| e.to_string())
}
