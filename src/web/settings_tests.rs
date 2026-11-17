//! Unit tests for web server startup checks and `Host` allowlist normalization.

use super::*;

fn opts(bind: Option<&str>, insecure: bool) -> WebLaunchOptions {
    WebLaunchOptions {
        port: Some(8443),
        bind: bind.map(str::to_string),
        insecure_http: insecure,
    }
}

fn tls_cfg() -> WebConfig {
    WebConfig {
        tls_cert_path: Some("/etc/rust-sql/cert.pem".into()),
        tls_key_path: Some("/etc/rust-sql/key.pem".into()),
        ..Default::default()
    }
}

#[test]
fn non_loopback_bind_without_tls_refuses_to_start_and_explains_why() {
    for bind in ["0.0.0.0", "192.168.1.20", "::"] {
        let err = WebSettings::resolve(&WebConfig::default(), &opts(Some(bind), true)).unwrap_err();
        assert!(err.contains("Refusing to start"), "{err}");
        assert!(err.contains("TLS is not configured"), "{err}");
        assert!(err.contains("tlsCertPath"), "{err}");
    }
}

#[test]
fn loopback_plain_http_requires_explicit_development_opt_in() {
    let err = WebSettings::resolve(&WebConfig::default(), &opts(None, false)).unwrap_err();
    assert!(err.contains("--insecure-http"), "{err}");
    let ok = WebSettings::resolve(&WebConfig::default(), &opts(None, true)).unwrap();
    assert!(ok.tls.is_none());
    let cfg = WebConfig {
        allow_insecure_http: Some(true),
        ..Default::default()
    };
    assert!(WebSettings::resolve(&cfg, &opts(Some("::1"), false)).is_ok());
}

#[test]
fn tls_allows_any_bind_and_half_configured_tls_is_rejected() {
    let s = WebSettings::resolve(&tls_cfg(), &opts(Some("0.0.0.0"), false)).unwrap();
    assert!(s.tls.is_some());
    let half = WebConfig {
        tls_cert_path: Some("cert.pem".into()),
        ..Default::default()
    };
    assert!(WebSettings::resolve(&half, &opts(None, true)).is_err());
}

#[test]
fn default_bind_is_loopback_and_hosts_default_to_bind_address_and_port() {
    let s = WebSettings::resolve(&WebConfig::default(), &opts(None, true)).unwrap();
    assert_eq!(s.addr.to_string(), "127.0.0.1:8443");
    assert!(s.host_allowed("127.0.0.1:8443"));
    assert!(s.host_allowed("LOCALHOST:8443"));
    assert!(!s.host_allowed("evil.example:8443"));
    assert!(!s.host_allowed("127.0.0.1:9999"));
    assert!(!s.host_allowed("127.0.0.1"));
}

#[test]
fn configured_allowlist_uses_scheme_default_ports() {
    let cfg = WebConfig {
        allowed_hosts: Some(vec!["sql.example.com".into()]),
        ..tls_cfg()
    };
    let s = WebSettings::resolve(&cfg, &opts(Some("0.0.0.0"), false)).unwrap();
    assert!(s.host_allowed("sql.example.com"));
    assert!(s.host_allowed("sql.example.com:443"));
    assert!(!s.host_allowed("sql.example.com:8443"));
    assert!(!s.host_allowed("0.0.0.0:8443"));
}

#[test]
fn ipv6_hosts_are_normalized() {
    assert_eq!(normalize_host("[::1]", 443), "[::1]:443");
    assert_eq!(normalize_host("[::1]:8080", 443), "[::1]:8080");
    let s = WebSettings::resolve(&WebConfig::default(), &opts(Some("::1"), true)).unwrap();
    assert!(s.host_allowed("[::1]:8443"));
}

#[test]
fn session_ttl_and_concurrency_are_configurable_and_validated() {
    let cfg = WebConfig {
        session_ttl_hours: Some(2),
        max_concurrent_requests_per_session: Some(3),
        ..Default::default()
    };
    let s = WebSettings::resolve(&cfg, &opts(None, true)).unwrap();
    assert_eq!(s.session_ttl, Duration::from_secs(7200));
    assert_eq!(s.max_concurrent_per_session, 3);
    let bad = WebConfig {
        max_concurrent_requests_per_session: Some(0),
        ..Default::default()
    };
    assert!(WebSettings::resolve(&bad, &opts(None, true)).is_err());
    let d = WebSettings::resolve(&WebConfig::default(), &opts(None, true)).unwrap();
    assert_eq!(d.session_ttl, Duration::from_secs(12 * 3600));
}
