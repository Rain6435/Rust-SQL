//! Server configuration, SSH tunnel settings, SSL options, and database/schema filter types.

use super::secret::Password;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// SSH bastion/jump-host configuration for port-forwarding database connections.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SshConfig {
    /// SSH login username.
    pub user: String,
    /// Optional SSH password (plaintext in memory, encrypted on disk).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub password: Option<Password>,
    /// Optional saved passphrase for the private key (plaintext in memory, encrypted on disk).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub passphrase: Option<Password>,
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
    /// Database authentication password (plaintext in memory, encrypted on disk).
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
    /// Default database name, keyspace, or SQLite file path.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub database: Option<String>,
    /// Default database schema name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schema: Option<String>,
    /// Optional sidebar filter rules for databases and schemas.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filter: Option<ServerFilter>,
    /// Per-statement timeout in seconds. `None` or `0` means no timeout.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub statement_timeout_secs: Option<u32>,
    /// When `true`, queries run on this connection are never written to query history.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exclude_from_history: Option<bool>,
}

impl Server {
    /// The configured per-statement timeout, or `None` when there is no timeout.
    pub fn statement_timeout(&self) -> Option<std::time::Duration> {
        self.statement_timeout_secs
            .filter(|s| *s > 0)
            .map(|s| std::time::Duration::from_secs(u64::from(s)))
    }

    /// Short label for the UI, such as `"Timeout: 30s"` or `"Timeout: none"`.
    pub fn statement_timeout_label(&self) -> String {
        match self.statement_timeout_secs.filter(|s| *s > 0) {
            Some(s) => format!("Timeout: {s}s"),
            None => "Timeout: none".to_string(),
        }
    }

    /// Whether queries on this connection are kept out of query history.
    pub fn is_excluded_from_history(&self) -> bool {
        self.exclude_from_history == Some(true)
    }
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
