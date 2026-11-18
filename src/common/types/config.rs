//! Application configuration file and runtime settings structures.

use crate::common::types::server::Server;
use serde::{Deserialize, Serialize};

/// Logging configuration controlling console and file output.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LogOptions {
    /// Whether to write log messages to standard output / console.
    #[serde(default)]
    pub console: bool,
    /// Whether to write log messages to a log file.
    #[serde(default)]
    pub file: bool,
    /// Minimum log severity level (`debug`, `info`, `warn`, `error`).
    #[serde(default = "default_log_level")]
    pub level: String,
    /// Path to the log file on disk.
    #[serde(default = "default_log_path")]
    pub path: String,
}

fn default_log_level() -> String {
    "error".to_string()
}

fn default_log_path() -> String {
    "~/.rust-sql.log".to_string()
}

impl Default for LogOptions {
    fn default() -> Self {
        Self {
            console: false,
            file: false,
            level: default_log_level(),
            path: default_log_path(),
        }
    }
}

/// User-editable application preferences saved via the Settings dialog.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct BaseConfig {
    /// Optional logging configuration.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub log: Option<LogOptions>,
    /// UI zoom factor (`0.4` to `3.0`, where `1.0` = 100%).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub zoom_factor: Option<f64>,
    /// Default row limit used for `Select Top N` table queries.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit_query_default_select_top: Option<u32>,
    /// Whether SQL keyword and schema autocomplete is enabled.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enabled_auto_complete: Option<bool>,
    /// Whether autocomplete triggers automatically while typing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enabled_live_auto_complete: Option<bool>,
    /// Whether the dark UI theme is active.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enabled_dark_theme: Option<bool>,
    /// Whether the startup intro animation is disabled.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub disabled_open_animation: Option<bool>,
    /// Field delimiter string used when exporting CSV data.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub csv_delimiter: Option<String>,
    /// Whether saved server connections are rendered as a list instead of a card grid.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub connections_as_list: Option<bool>,
    /// Custom UI font family name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub custom_font: Option<String>,
    /// Opt-in: emulate dialects that have no live driver (results are not from a real server).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub emulate_unsupported_dialects: Option<bool>,
    /// Opt-in: also keep the query with parameter values filled in (display only) in history.
    /// Off by default, so history keeps only the query with its placeholders.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub history_store_bound_values: Option<bool>,
    /// Rows fetched per page of query results (more are fetched on demand).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result_page_size: Option<u32>,
}

/// On-disk schema of `rust-sql.json`, containing the server list and optional user preferences.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ConfigFile {
    /// Saved server connection profiles.
    #[serde(default)]
    pub servers: Vec<Server>,
    /// User-customized base preferences.
    #[serde(flatten)]
    pub base: BaseConfig,
    /// Trusted SSH bastion host keys, one per `host:port` (trust on first use).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ssh_host_keys: Vec<super::SshHostKeyPin>,
    /// Multi-user web server settings.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub web: Option<super::WebConfig>,
}

/// Default number of rows fetched per result page.
pub fn default_result_page_size() -> u32 {
    crate::adapters::DEFAULT_LIMIT
}

/// Repository metadata attached to the runtime configuration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct RepositoryInfo {
    /// Canonical Git repository URL.
    pub url: String,
}

/// Fully resolved runtime configuration with all defaults, CLI flags, and package metadata merged.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Config {
    /// Saved server connection profiles.
    pub servers: Vec<Server>,
    /// Active logging options.
    pub log: LogOptions,
    /// Active UI zoom factor.
    pub zoom_factor: f64,
    /// Active row limit for default `SELECT TOP N` queries.
    pub limit_query_default_select_top: u32,
    /// Whether SQL autocomplete is enabled.
    pub enabled_auto_complete: bool,
    /// Whether live SQL autocomplete is enabled.
    pub enabled_live_auto_complete: bool,
    /// Whether dark theme is enabled.
    pub enabled_dark_theme: bool,
    /// Whether startup animation is disabled.
    pub disabled_open_animation: bool,
    /// Active CSV delimiter character/string.
    pub csv_delimiter: String,
    /// Whether dialects without a live driver may be emulated (off unless explicitly enabled).
    #[serde(default)]
    pub emulate_unsupported_dialects: bool,
    /// Whether history also keeps parameter values (display-only bound SQL). Off by default.
    #[serde(default)]
    pub history_store_bound_values: bool,
    /// Rows fetched per page of query results.
    #[serde(default = "default_result_page_size")]
    pub result_page_size: u32,
    /// Whether servers are displayed in list mode.
    pub connections_as_list: bool,
    /// Active custom font family.
    pub custom_font: String,
    /// Application name.
    pub name: String,
    /// Application semantic version.
    pub version: String,
    /// Resolved path to the configuration file on disk.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// Project homepage URL.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub homepage: Option<String>,
    /// Issue tracker URL.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bugs: Option<String>,
    /// Source repository metadata.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repository: Option<RepositoryInfo>,
    /// Whether development mode (`--dev`) is enabled.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dev_mode: Option<bool>,
    /// Whether `--version` / `-v` was requested on the command line.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub print_version: Option<bool>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            servers: Vec::new(),
            log: LogOptions::default(),
            zoom_factor: 1.0,
            limit_query_default_select_top: 100,
            enabled_auto_complete: true,
            enabled_live_auto_complete: true,
            enabled_dark_theme: false,
            disabled_open_animation: false,
            csv_delimiter: ",".to_string(),
            emulate_unsupported_dialects: false,
            history_store_bound_values: false,
            result_page_size: default_result_page_size(),
            connections_as_list: false,
            custom_font: "Lato".to_string(),
            name: "Rust-SQL".to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            path: None,
            homepage: Some("https://github.com/Rain6435/Rust-SQL".to_string()),
            bugs: Some("https://github.com/Rain6435/Rust-SQL/issues".to_string()),
            repository: Some(RepositoryInfo {
                url: "https://github.com/Rain6435/Rust-SQL".to_string(),
            }),
            dev_mode: Some(false),
            print_version: Some(false),
        }
    }
}
