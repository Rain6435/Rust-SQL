//! Resolved web server settings: bind address, TLS, `Host` allowlist, session lifetime, and
//! per-session concurrency, with the startup safety checks.
//!
//! Startup refuses to serve plain HTTP on a non-loopback address, and allows plain HTTP on
//! loopback only when development mode is explicitly enabled (`--insecure-http` or
//! `"web": { "allowInsecureHttp": true }`).

use crate::common::types::WebConfig;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::time::Duration;

/// Default port.
pub const DEFAULT_PORT: u16 = 8080;
/// Default session lifetime.
pub const DEFAULT_SESSION_TTL_HOURS: u64 = 12;
/// Default cap on concurrent IPC requests per session.
pub const DEFAULT_MAX_CONCURRENT: usize = 8;
/// Request body limit (1 MiB).
pub const BODY_LIMIT_BYTES: usize = 1024 * 1024;

/// Certificate and key paths for HTTPS.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TlsPaths {
    /// PEM certificate chain.
    pub cert: PathBuf,
    /// PEM private key.
    pub key: PathBuf,
}

/// Command-line overrides for the web server.
#[derive(Debug, Clone, Default)]
pub struct WebLaunchOptions {
    /// `--port`.
    pub port: Option<u16>,
    /// `--bind`.
    pub bind: Option<String>,
    /// `--insecure-http` (development only, loopback only).
    pub insecure_http: bool,
}

/// Validated settings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WebSettings {
    /// Socket address to bind.
    pub addr: SocketAddr,
    /// HTTPS certificate and key; `None` means development plain HTTP on loopback.
    pub tls: Option<TlsPaths>,
    /// Normalized `host:port` values accepted in the `Host` header.
    pub allowed_hosts: Vec<String>,
    /// Session lifetime.
    pub session_ttl: Duration,
    /// Concurrent IPC requests allowed per session.
    pub max_concurrent_per_session: usize,
}

fn parse_bind(raw: &str) -> Result<IpAddr, String> {
    if raw.eq_ignore_ascii_case("localhost") {
        return Ok(IpAddr::V4(Ipv4Addr::LOCALHOST));
    }
    raw.trim_matches(['[', ']'])
        .parse()
        .map_err(|_| format!("Invalid web bind address '{raw}': expected an IP address"))
}

/// Lower-cases `host` and appends the scheme's default port when it has none.
pub fn normalize_host(host: &str, default_port: u16) -> String {
    let host = host.trim().to_ascii_lowercase();
    let has_port = if host.starts_with('[') {
        host.contains("]:")
    } else {
        host.matches(':').count() == 1
    };
    if has_port {
        host
    } else {
        format!("{host}:{default_port}")
    }
}

impl WebSettings {
    /// Merges config and CLI options and applies the startup safety checks.
    pub fn resolve(cfg: &WebConfig, cli: &WebLaunchOptions) -> Result<Self, String> {
        let bind_raw = cli
            .bind
            .clone()
            .or_else(|| cfg.bind_address.clone())
            .unwrap_or_else(|| "127.0.0.1".to_string());
        let ip = parse_bind(&bind_raw)?;
        let port = cli.port.or(cfg.port).unwrap_or(DEFAULT_PORT);
        let addr = SocketAddr::new(ip, port);

        let tls = match (&cfg.tls_cert_path, &cfg.tls_key_path) {
            (Some(cert), Some(key)) => Some(TlsPaths {
                cert: PathBuf::from(cert),
                key: PathBuf::from(key),
            }),
            (None, None) => None,
            _ => {
                return Err("TLS is half-configured: set both web.tlsCertPath and \
                     web.tlsKeyPath, or neither"
                    .into())
            }
        };
        if tls.is_none() {
            if !ip.is_loopback() {
                return Err(format!(
                    "Refusing to start: the web server would listen on {addr}, which other \
                     machines can reach, but TLS is not configured. Passwords and session \
                     cookies would cross the network in plain text. Set web.tlsCertPath and \
                     web.tlsKeyPath in {}, or bind to 127.0.0.1.",
                    crate::core::config::path()
                ));
            }
            if !(cli.insecure_http || cfg.allow_insecure_http == Some(true)) {
                return Err(
                    "Refusing to start: TLS is not configured. Configure web.tlsCertPath and \
                     web.tlsKeyPath, or, for local development only, pass --insecure-http \
                     to serve plain HTTP on the loopback address."
                        .into(),
                );
            }
        }

        let default_port = if tls.is_some() { 443 } else { 80 };
        let allowed_hosts = match &cfg.allowed_hosts {
            Some(list) if !list.is_empty() => list
                .iter()
                .map(|h| normalize_host(h, default_port))
                .collect(),
            _ => {
                let mut hosts = vec![normalize_host(&addr.to_string(), default_port)];
                if ip.is_loopback() {
                    hosts.push(format!("localhost:{port}"));
                }
                hosts
            }
        };

        let ttl_hours = cfg.session_ttl_hours.unwrap_or(DEFAULT_SESSION_TTL_HOURS);
        if ttl_hours == 0 {
            return Err("web.sessionTtlHours must be at least 1".into());
        }
        let max_concurrent = cfg
            .max_concurrent_requests_per_session
            .unwrap_or(DEFAULT_MAX_CONCURRENT);
        if max_concurrent == 0 {
            return Err("web.maxConcurrentRequestsPerSession must be at least 1".into());
        }
        Ok(Self {
            addr,
            tls,
            allowed_hosts,
            session_ttl: Duration::from_secs(ttl_hours.saturating_mul(3600)),
            max_concurrent_per_session: max_concurrent,
        })
    }

    /// Whether `host` (a `Host` header value) is in the allowlist.
    pub fn host_allowed(&self, host: &str) -> bool {
        let default_port = if self.tls.is_some() { 443 } else { 80 };
        let host = normalize_host(host, default_port);
        self.allowed_hosts.contains(&host)
    }

    /// Settings for an in-process test server on loopback plain HTTP.
    pub fn loopback_for_tests(port: u16) -> Self {
        let cli = WebLaunchOptions {
            port: Some(port),
            bind: None,
            insecure_http: true,
        };
        Self::resolve(&WebConfig::default(), &cli).expect("loopback settings are valid")
    }
}

#[cfg(test)]
#[path = "settings_tests.rs"]
mod tests;
