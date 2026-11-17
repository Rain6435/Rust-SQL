//! Configuration of the multi-user web server (`"web"` section of `rust-sql.json`).

use serde::{Deserialize, Serialize};

/// Web server settings. Every field is optional; see `web::settings` for defaults and checks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct WebConfig {
    /// IP address to bind (default `127.0.0.1`). A non-loopback address requires TLS.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bind_address: Option<String>,
    /// TCP port (default `8080`; `--port` overrides it).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
    /// PEM certificate chain for HTTPS.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tls_cert_path: Option<String>,
    /// PEM private key for HTTPS.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tls_key_path: Option<String>,
    /// Development only: allow plain HTTP on a loopback bind address.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allow_insecure_http: Option<bool>,
    /// Accepted `Host` header values (`host` or `host:port`). Default: the bind address and port.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allowed_hosts: Option<Vec<String>>,
    /// Session lifetime in hours (default 12).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_ttl_hours: Option<u64>,
    /// Maximum concurrent IPC requests per session (default 8).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_concurrent_requests_per_session: Option<usize>,
}
