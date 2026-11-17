//! Server configuration, SSH tunnel settings, SSL options, and database/schema filter types.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// AES-256-CBC encrypted password payload containing base64-encoded IV and ciphertext.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct EncryptedPassword {
    /// Base64-encoded 16-byte initialization vector.
    pub iv_text: String,
    /// Base64-encoded PKCS#7-padded AES-256-CBC ciphertext.
    pub encrypted_text: String,
}

/// Represents a password that may be either plaintext in memory or AES-256-CBC encrypted on disk.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Password {
    /// Encrypted password structure stored in `rust-sql.json`.
    Encrypted(EncryptedPassword),
    /// Decrypted or unsaved plaintext password.
    Plain(String),
}

impl Default for Password {
    fn default() -> Self {
        Password::Plain(String::new())
    }
}

impl Password {
    /// Returns the plaintext string if the password is in `Plain` form.
    pub fn as_plain(&self) -> Option<&str> {
        match self {
            Password::Plain(s) => Some(s.as_str()),
            Password::Encrypted(_) => None,
        }
    }

    /// Returns `true` if the plaintext or encrypted payload is empty.
    pub fn is_empty(&self) -> bool {
        match self {
            Password::Plain(s) => s.is_empty(),
            Password::Encrypted(e) => e.iv_text.is_empty() && e.encrypted_text.is_empty(),
        }
    }
}

/// SSH bastion/jump-host configuration for port-forwarding database connections.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SshConfig {
    /// SSH login username.
    pub user: String,
    /// Optional SSH password (plaintext or encrypted).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub password: Option<Password>,
    /// Optional passphrase for decrypting an encrypted private key at runtime.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub passphrase: Option<String>,
    /// Path to an OpenSSH private key file (e.g., `~/.ssh/id_ed25519`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub private_key: Option<String>,
    /// SSH bastion hostname or IP address.
    pub host: String,
    /// SSH port (defaults to `22`).
    pub port: u32,
    /// Whether the private key requires a passphrase prompt on connection.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub private_key_with_passphrase: Option<bool>,
    /// Whether to authenticate via the local `SSH_AUTH_SOCK` agent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub use_agent: Option<bool>,
}

/// Custom PEM certificate paths for SSL/TLS database connections.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SslDetails {
    /// Client private key PEM content or path.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    /// Certificate Authority (CA) PEM content or path.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ca: Option<String>,
    /// Client certificate PEM content or path.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cert: Option<String>,
}

/// SSL/TLS configuration supporting either a simple boolean toggle or explicit certificate details.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum SslConfig {
    /// Simple boolean toggle enabling or disabling SSL/TLS.
    Bool(bool),
    /// Detailed certificate configuration.
    Details(SslDetails),
}

impl Default for SslConfig {
    fn default() -> Self {
        SslConfig::Bool(false)
    }
}

impl SslConfig {
    /// Returns `true` if SSL/TLS is enabled.
    pub fn is_enabled(&self) -> bool {
        match self {
            SslConfig::Bool(b) => *b,
            SslConfig::Details(_) => true,
        }
    }
}

/// Allowlist (`only`) and blocklist (`ignore`) filter arrays for databases or schemas.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct FilterList {
    /// Only include items matching these names.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub only: Option<Vec<String>>,
    /// Exclude items matching these names.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ignore: Option<Vec<String>>,
}

/// Filter specification supporting either an exact name match or `only`/`ignore` lists.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum FilterClause {
    /// Match a single exact database or schema name.
    Exact(String),
    /// Match against `only` and/or `ignore` lists.
    List(FilterList),
}

impl Default for FilterClause {
    fn default() -> Self {
        FilterClause::List(FilterList::default())
    }
}

/// Filter applied when listing databases on a server.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct DatabaseFilter {
    /// Database filter clause.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub database: Option<FilterClause>,
}

/// Filter applied when listing schemas, tables, views, or routines.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SchemaFilter {
    /// Schema filter clause.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schema: Option<FilterClause>,
}

/// Combined database and schema filter stored on a `Server` configuration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ServerFilter {
    /// Optional database filter clause.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub database: Option<FilterClause>,
    /// Optional schema filter clause.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schema: Option<FilterClause>,
}

impl ServerFilter {
    /// Extracts the `DatabaseFilter` portion of this server filter.
    pub fn to_database_filter(&self) -> DatabaseFilter {
        DatabaseFilter {
            database: self.database.clone(),
        }
    }

    /// Extracts the `SchemaFilter` portion of this server filter.
    pub fn to_schema_filter(&self) -> SchemaFilter {
        SchemaFilter {
            schema: self.schema.clone(),
        }
    }
}

/// Complete database server connection profile.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Server {
    /// Unique UUID identifier for the server.
    #[serde(default)]
    pub id: String,
    /// Human-readable display name.
    #[serde(default)]
    pub name: String,
    /// Database client key (`postgresql`, `redshift`, `mysql`, `mariadb`, `sqlserver`, `sqlite`, `cassandra`).
    #[serde(default)]
    pub client: String,
    /// Optional explicit adapter key override.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub adapter: Option<String>,
    /// Database server hostname or IP address.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
    /// Path to a local Unix domain socket.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub socket_path: Option<String>,
    /// Database server TCP port.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub port: Option<u32>,
    /// Ephemeral local host bound by an active SSH tunnel.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub local_host: Option<String>,
    /// Ephemeral local port bound by an active SSH tunnel.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub local_port: Option<u16>,
    /// Database authentication username.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user: Option<String>,
    /// Database authentication password (plaintext or encrypted).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub password: Option<Password>,
    /// Optional application name reported to the database server.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub application_name: Option<String>,
    /// Windows Active Directory / NTLM domain for SQL Server authentication.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub domain: Option<String>,
    /// Optional SSH tunnel configuration.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ssh: Option<SshConfig>,
    /// Optional SSL/TLS configuration.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ssl: Option<SslConfig>,
    /// Flag indicating whether secret fields on this struct are currently encrypted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub encrypted: Option<bool>,
    /// Default database name, keyspace, or SQLite file path.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub database: Option<String>,
    /// Default database schema name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schema: Option<String>,
    /// Optional sidebar filter rules for databases and schemas.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filter: Option<ServerFilter>,
}

/// Map of field paths (e.g., `"host"`, `"ssh.user"`) to human-readable validation error messages.
pub type ValidationErrors = HashMap<String, String>;

/// Result of adding or updating a server configuration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ServerResult {
    /// Persisted server configuration when validation succeeds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<Server>,
    /// Validation errors keyed by field name when validation fails.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub validation_errors: Option<ValidationErrors>,
}
