//! Conversion from [`ServerFormDraft`] back into a normalized [`Server`] configuration struct.

use crate::adapters::CLIENTS;
use crate::common::types::{
    FilterClause, FilterList, Password, Server, ServerFilter, SshConfig, SslConfig,
};
use crate::gui::ServerFormDraft;
use std::collections::HashSet;

impl ServerFormDraft {
    /// Converts the form draft into a [`Server`], stripping fields disabled by the selected dialect.
    pub fn to_server(&self) -> Server {
        let disabled: HashSet<String> = CLIENTS
            .iter()
            .find(|c| c.key == self.client)
            .map(|c| c.disabled_features.iter().cloned().collect())
            .unwrap_or_default();

        let opt_str = |s: &str, feat: &str| -> Option<String> {
            if disabled.contains(feat) {
                return None;
            }
            let t = s.trim();
            (!t.is_empty()).then(|| t.to_string())
        };

        let port = if disabled.contains("server:port") || !self.socket_path.trim().is_empty() {
            None
        } else {
            self.port.trim().parse::<u32>().ok().or_else(|| {
                (!self.host.trim().is_empty()).then(|| {
                    CLIENTS
                        .iter()
                        .find(|c| c.key == self.client)
                        .and_then(|c| c.default_port)
                })?
            })
        };

        let ssh = if self.ssh_enabled && !disabled.contains("server:ssh") {
            Some(SshConfig {
                host: self.ssh_host.trim().to_string(),
                port: self.ssh_port.trim().parse::<u32>().unwrap_or(22),
                user: self.ssh_user.trim().to_string(),
                password: if self.ssh_password.trim().is_empty()
                    || !self.ssh_private_key.trim().is_empty()
                    || self.ssh_use_agent
                {
                    None
                } else {
                    Some(Password::Plain(self.ssh_password.trim().to_string()))
                },
                passphrase: None,
                private_key: if self.ssh_private_key.trim().is_empty() || self.ssh_use_agent {
                    None
                } else {
                    Some(self.ssh_private_key.trim().to_string())
                },
                private_key_with_passphrase: Some(self.ssh_private_key_with_passphrase),
                use_agent: Some(self.ssh_use_agent),
            })
        } else {
            None
        };

        let parse_lines = |s: &str| -> Option<Vec<String>> {
            let v: Vec<String> = s
                .lines()
                .map(|l| l.trim().to_string())
                .filter(|l| !l.is_empty())
                .collect();
            (!v.is_empty()).then_some(v)
        };

        let filter = if self.filter_enabled {
            let db_only = parse_lines(&self.filter_db_only);
            let db_ignore = parse_lines(&self.filter_db_ignore);
            let sc_only = parse_lines(&self.filter_schema_only);
            let sc_ignore = parse_lines(&self.filter_schema_ignore);
            Some(ServerFilter {
                database: (db_only.is_some() || db_ignore.is_some()).then_some(FilterClause::List(
                    FilterList {
                        only: db_only,
                        ignore: db_ignore,
                    },
                )),
                schema: (sc_only.is_some() || sc_ignore.is_some()).then_some(FilterClause::List(
                    FilterList {
                        only: sc_only,
                        ignore: sc_ignore,
                    },
                )),
            })
        } else {
            None
        };

        Server {
            id: self.id.clone(),
            name: self.name.trim().to_string(),
            client: self.client.trim().to_string(),
            adapter: None,
            host: if !self.socket_path.trim().is_empty() {
                None
            } else {
                opt_str(&self.host, "server:host")
            },
            socket_path: if !self.host.trim().is_empty() {
                None
            } else {
                opt_str(&self.socket_path, "server:socketPath")
            },
            port,
            local_host: None,
            local_port: None,
            user: opt_str(&self.user, "server:user"),
            password: opt_str(&self.password, "server:password").map(Password::Plain),
            application_name: None,
            domain: opt_str(&self.domain, "server:domain"),
            ssh,
            ssl: (!disabled.contains("server:ssl")).then_some(SslConfig::Bool(self.ssl)),
            encrypted: None,
            database: {
                let d = self.database.trim();
                (!d.is_empty()).then(|| d.to_string())
            },
            schema: opt_str(&self.schema, "server:schema"),
            filter,
        }
    }
}
