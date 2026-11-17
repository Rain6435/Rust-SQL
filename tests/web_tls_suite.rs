//! HTTPS end to end with rustls, and the startup refusals: a non-loopback bind without TLS,
//! plain HTTP without the development opt-in, and a server with no accounts.

mod support;

use rust_sql::common::types::WebConfig;
use rust_sql::core::appdb::AppDb;
use rust_sql::core::keys::KeyProvider;
use rust_sql::web::{run_web_server, serve_on, WebApp, WebLaunchOptions, WebSettings};
use std::sync::Arc;
use support::TestApp;

#[tokio::test]
async fn startup_refuses_non_loopback_plain_http_with_a_clear_message() {
    let _g = support::lock().await;
    let t = TestApp::new();
    t.user("root", true);
    let opts = WebLaunchOptions {
        port: Some(0),
        bind: Some("0.0.0.0".into()),
        insecure_http: true,
    };
    let err = run_web_server(opts).await.unwrap_err();
    assert!(
        err.contains("Refusing to start") && err.contains("TLS is not configured"),
        "{err}"
    );

    let opts = WebLaunchOptions {
        port: Some(0),
        bind: None,
        insecure_http: false,
    };
    let err = run_web_server(opts).await.unwrap_err();
    assert!(err.contains("--insecure-http"), "{err}");
}

#[tokio::test]
async fn startup_refuses_to_serve_without_any_account() {
    let _g = support::lock().await;
    let _t = TestApp::new();
    let opts = WebLaunchOptions {
        port: Some(0),
        bind: None,
        insecure_http: true,
    };
    let err = run_web_server(opts).await.unwrap_err();
    assert!(err.contains("users create --admin"), "{err}");
}

#[tokio::test]
async fn serves_https_with_the_configured_certificate() {
    let _g = support::lock().await;
    let t = TestApp::new();
    let cert = rcgen::generate_simple_self_signed(vec!["localhost".to_string()]).unwrap();
    let cert_path = t.dir.path().join("cert.pem");
    let key_path = t.dir.path().join("key.pem");
    std::fs::write(&cert_path, cert.cert.pem()).unwrap();
    std::fs::write(&key_path, cert.signing_key.serialize_pem()).unwrap();

    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let cfg = WebConfig {
        tls_cert_path: Some(cert_path.display().to_string()),
        tls_key_path: Some(key_path.display().to_string()),
        ..Default::default()
    };
    let opts = WebLaunchOptions {
        port: Some(addr.port()),
        ..Default::default()
    };
    let settings = WebSettings::resolve(&cfg, &opts).expect("TLS settings");
    let db = AppDb::open(&t.dir.path().join("tls.db")).unwrap();
    let keys: Arc<dyn KeyProvider> = t.keys.clone();
    let app = Arc::new(WebApp::new(db, keys, settings));
    let server = tokio::spawn(serve_on(app, listener));

    let client = reqwest::Client::builder()
        .add_root_certificate(reqwest::Certificate::from_pem(cert.cert.pem().as_bytes()).unwrap())
        .resolve("localhost", addr)
        .no_proxy()
        .build()
        .unwrap();
    let url = format!("https://localhost:{}/api/health", addr.port());
    let mut resp = None;
    for _ in 0..50 {
        match client.get(&url).send().await {
            Ok(r) => {
                resp = Some(r);
                break;
            }
            Err(_) => tokio::time::sleep(std::time::Duration::from_millis(50)).await,
        }
    }
    let resp = resp.expect("HTTPS server answered");
    assert_eq!(resp.status(), 200);
    assert!(resp.headers().contains_key("strict-transport-security"));

    let plain = reqwest::Client::builder().no_proxy().build().unwrap();
    let http_url = format!("http://localhost:{}/api/health", addr.port());
    let plain_result = plain.get(&http_url).send().await;
    assert!(plain_result
        .map(|r| !r.status().is_success())
        .unwrap_or(true));
    server.abort();
}
