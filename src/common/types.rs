use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct EncryptedPassword {
    pub iv_text: String,
    pub encrypted_text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Password {
    Encrypted(EncryptedPassword),
    Plain(String),
}

impl Default for Password {
    fn default() -> Self {
        Password::Plain(String::new())
    }
}

impl Password {
    pub fn as_plain(&self) -> Option<&str> {
        match self {
            Password::Plain(s) => Some(s.as_str()),
            Password::Encrypted(_) => None,
        }
    }

    pub fn is_empty(&self) -> bool {
        match self {
            Password::Plain(s) => s.is_empty(),
            Password::Encrypted(e) => e.iv_text.is_empty() && e.encrypted_text.is_empty(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SshConfig {
    pub user: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub password: Option<Password>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub passphrase: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub private_key: Option<String>,
    pub host: String,
    pub port: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub private_key_with_passphrase: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub use_agent: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SslDetails {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ca: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cert: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum SslConfig {
    Bool(bool),
    Details(SslDetails),
}

impl Default for SslConfig {
    fn default() -> Self {
        SslConfig::Bool(false)
    }
}

impl SslConfig {
    pub fn is_enabled(&self) -> bool {
        match self {
            SslConfig::Bool(b) => *b,
            SslConfig::Details(_) => true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct FilterList {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub only: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ignore: Option<Vec<String>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum FilterClause {
    Exact(String),
    List(FilterList),
}

impl Default for FilterClause {
    fn default() -> Self {
        FilterClause::List(FilterList::default())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct DatabaseFilter {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub database: Option<FilterClause>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SchemaFilter {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schema: Option<FilterClause>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ServerFilter {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub database: Option<FilterClause>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schema: Option<FilterClause>,
}

impl ServerFilter {
    pub fn to_database_filter(&self) -> DatabaseFilter {
        DatabaseFilter {
            database: self.database.clone(),
        }
    }

    pub fn to_schema_filter(&self) -> SchemaFilter {
        SchemaFilter {
            schema: self.schema.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Server {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub client: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub adapter: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub socket_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub port: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub local_host: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub local_port: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub password: Option<Password>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub application_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub domain: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ssh: Option<SshConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ssl: Option<SslConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub encrypted: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub database: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schema: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filter: Option<ServerFilter>,
}

pub type ValidationErrors = HashMap<String, String>;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ServerResult {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<Server>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub validation_errors: Option<ValidationErrors>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LogOptions {
    #[serde(default)]
    pub console: bool,
    #[serde(default)]
    pub file: bool,
    #[serde(default = "default_log_level")]
    pub level: String,
    #[serde(default = "default_log_path")]
    pub path: String,
}

fn default_log_level() -> String {
    "error".to_string()
}

fn default_log_path() -> String {
    "~/.sqlectron.log".to_string()
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct BaseConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub log: Option<LogOptions>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub zoom_factor: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit_query_default_select_top: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enabled_auto_complete: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enabled_live_auto_complete: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enabled_dark_theme: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub disabled_open_animation: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub csv_delimiter: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub connections_as_list: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub custom_font: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ConfigFile {
    #[serde(default)]
    pub servers: Vec<Server>,
    #[serde(flatten)]
    pub base: BaseConfig,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct RepositoryInfo {
    pub url: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct CryptoConfig {
    pub secret: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Config {
    pub servers: Vec<Server>,
    pub log: LogOptions,
    pub zoom_factor: f64,
    pub limit_query_default_select_top: u32,
    pub enabled_auto_complete: bool,
    pub enabled_live_auto_complete: bool,
    pub enabled_dark_theme: bool,
    pub disabled_open_animation: bool,
    pub csv_delimiter: String,
    pub connections_as_list: bool,
    pub custom_font: String,
    pub name: String,
    pub version: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub homepage: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bugs: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repository: Option<RepositoryInfo>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dev_mode: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub print_version: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub crypto: Option<CryptoConfig>,
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
            connections_as_list: false,
            custom_font: "Lato".to_string(),
            name: "Rust-SQL".to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            path: None,
            homepage: Some("https://sqlectron.github.io".to_string()),
            bugs: Some("https://github.com/sqlectron/sqlectron-gui/issues".to_string()),
            repository: Some(RepositoryInfo {
                url: "https://github.com/sqlectron/sqlectron-gui".to_string(),
            }),
            dev_mode: Some(false),
            print_version: Some(false),
            crypto: Some(CryptoConfig {
                secret: "j[F6Y6NoWT}+YG|4c|-<89:ByJ83-9Aj?O8>$Zk/[WFk_~gFbg7<wm+*V|A{xQZ,"
                    .to_string(),
            }),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AdapterInfo {
    pub key: String,
    pub name: String,
    pub protocol: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_port: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_database: Option<String>,
    pub disabled_features: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AdapterVersion {
    pub name: String,
    pub version: String,
    pub string: String,
}

impl Default for AdapterVersion {
    fn default() -> Self {
        Self {
            name: "UNKNOWN".to_string(),
            version: "0.0.0".to_string(),
            string: "UNKNOWN 0.0.0".to_string(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FieldInfo {
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QueryRowResult {
    pub command: String,
    pub rows: Vec<serde_json::Map<String, Value>>,
    pub fields: Vec<FieldInfo>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub row_count: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub affected_rows: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListTableResult {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schema: Option<String>,
    pub name: String,
}

pub type DbTable = ListTableResult;
pub type ListViewResult = ListTableResult;
pub type DbView = ListViewResult;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListRoutineResult {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schema: Option<String>,
    pub routine_name: String,
    pub routine_type: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListTableColumnsResult {
    pub column_name: String,
    pub data_type: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TableKeysResult {
    pub column_name: String,
    pub key_type: String,
    pub constraint_name: Option<String>,
    pub referenced_table: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DialogFilter {
    pub name: String,
    pub extensions: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MenuItem {
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "type")]
    pub item_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MenuOptions {
    pub menu_items: Vec<MenuItem>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SavedQueryInfo {
    pub name: String,
    pub filename: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenedQueryInfo {
    pub name: String,
    pub query: String,
    pub filename: String,
}
