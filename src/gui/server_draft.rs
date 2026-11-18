//! Editable draft state (`ServerFormDraft`) for the Server Add/Edit modal and TUI form.

use crate::common::types::{FilterClause, Server};
use crate::core::connection_string::build_connection_uri;

/// Editable form fields for creating or updating a [`Server`] configuration.
#[derive(Debug, Clone, Default)]
pub struct ServerFormDraft {
    /// Server UUID (empty when creating a new server).
    pub id: String,
    /// Display name of the server.
    pub name: String,
    /// Dialect key (`postgresql`, `mysql`, `sqlite`, `sqlserver`, `cassandra`, etc.).
    pub client: String,
    /// Whether SSL/TLS is enabled.
    pub ssl: bool,
    /// Hostname or IP address.
    pub host: String,
    /// Port number string.
    pub port: String,
    /// Unix domain socket path.
    pub socket_path: String,
    /// Windows/NTLM domain name (SQL Server).
    pub domain: String,
    /// Database username.
    pub user: String,
    /// Database password (plain text during editing).
    pub password: String,
    /// Whether the password and full connection URI are revealed in plain text.
    pub show_plain_password: bool,
    /// Default database, file path, or keyspace.
    pub database: String,
    /// Default schema name.
    pub schema: String,
    /// Synchronized connection URI string (`postgres://...`, `mysql://...`, etc.).
    pub conn_uri: String,
    /// Whether SSH tunneling is enabled.
    pub ssh_enabled: bool,
    /// SSH bastion host.
    pub ssh_host: String,
    /// SSH port string (defaults to `"22"`).
    pub ssh_port: String,
    /// SSH username.
    pub ssh_user: String,
    /// SSH password.
    pub ssh_password: String,
    /// Path to SSH private key file.
    pub ssh_private_key: String,
    /// Whether the SSH private key requires an interactive passphrase.
    pub ssh_private_key_with_passphrase: bool,
    /// Whether to authenticate via the system SSH agent.
    pub ssh_use_agent: bool,
    /// Whether database/schema inclusion/exclusion filters are enabled.
    pub filter_enabled: bool,
    /// Newline-separated list of included databases.
    pub filter_db_only: String,
    /// Newline-separated list of excluded databases.
    pub filter_db_ignore: String,
    /// Newline-separated list of included schemas.
    pub filter_schema_only: String,
    /// Newline-separated list of excluded schemas.
    pub filter_schema_ignore: String,
    /// Per-statement timeout in seconds (empty or `0` = none).
    pub statement_timeout_secs: String,
    /// Whether queries on this connection are kept out of query history.
    pub exclude_from_history: bool,
}

impl ServerFormDraft {
    /// Populates a [`ServerFormDraft`] from an optional existing [`Server`].
    pub fn from_server(srv: Option<&Server>) -> Self {
        let Some(s) = srv else {
            return Self {
                client: "postgresql".to_string(),
                port: "5432".to_string(),
                ssh_port: "22".to_string(),
                ..Default::default()
            };
        };

        let extract_clause = |clause: &Option<FilterClause>| -> (String, String) {
            match clause {
                Some(FilterClause::List(l)) => (
                    l.only.as_ref().map(|v| v.join("\n")).unwrap_or_default(),
                    l.ignore.as_ref().map(|v| v.join("\n")).unwrap_or_default(),
                ),
                Some(FilterClause::Exact(e)) => (e.clone(), String::new()),
                None => (String::new(), String::new()),
            }
        };

        let (db_only, db_ignore, schema_only, schema_ignore, filter_enabled) =
            if let Some(ref f) = s.filter {
                let (dbo, dbi) = extract_clause(&f.database);
                let (so, si) = extract_clause(&f.schema);
                (dbo, dbi, so, si, true)
            } else {
                (
                    String::new(),
                    String::new(),
                    String::new(),
                    String::new(),
                    false,
                )
            };

        let (
            ssh_enabled,
            ssh_host,
            ssh_port,
            ssh_user,
            ssh_password,
            ssh_private_key,
            ssh_passphrase_flag,
            ssh_use_agent,
        ) = if let Some(ref ssh) = s.ssh {
            (
                true,
                ssh.host.clone(),
                ssh.port.to_string(),
                ssh.user.clone(),
                ssh.password
                    .as_ref()
                    .and_then(|p| p.as_plain())
                    .unwrap_or("")
                    .to_string(),
                ssh.private_key.clone().unwrap_or_default(),
                ssh.private_key_with_passphrase.unwrap_or(false),
                ssh.use_agent.unwrap_or(false),
            )
        } else {
            (
                false,
                String::new(),
                "22".to_string(),
                String::new(),
                String::new(),
                String::new(),
                false,
                false,
            )
        };

        let mut draft = Self {
            id: s.id.clone(),
            name: s.name.clone(),
            client: s.client.clone(),
            ssl: s.ssl.as_ref().map(|ssl| ssl.is_enabled()).unwrap_or(false),
            host: s.host.clone().unwrap_or_default(),
            port: s.port.map(|p| p.to_string()).unwrap_or_default(),
            socket_path: s.socket_path.clone().unwrap_or_default(),
            domain: s.domain.clone().unwrap_or_default(),
            user: s.user.clone().unwrap_or_default(),
            password: s
                .password
                .as_ref()
                .and_then(|p| p.as_plain())
                .unwrap_or("")
                .to_string(),
            show_plain_password: false,
            database: s.database.clone().unwrap_or_default(),
            schema: s.schema.clone().unwrap_or_default(),
            conn_uri: String::new(),
            ssh_enabled,
            ssh_host,
            ssh_port,
            ssh_user,
            ssh_password,
            ssh_private_key,
            ssh_private_key_with_passphrase: ssh_passphrase_flag,
            ssh_use_agent,
            filter_enabled,
            filter_db_only: db_only,
            filter_db_ignore: db_ignore,
            filter_schema_only: schema_only,
            filter_schema_ignore: schema_ignore,
            statement_timeout_secs: s
                .statement_timeout_secs
                .map(|t| t.to_string())
                .unwrap_or_default(),
            exclude_from_history: s.is_excluded_from_history(),
        };
        draft.conn_uri = build_connection_uri(&draft.to_server(), false);
        draft
    }
}
