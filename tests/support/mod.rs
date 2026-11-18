//! Shared helpers for the web server integration suites: an isolated app (temporary config,
//! application database, and an in-memory test key), direct IPC calls, and HTTP requests
//! through the real router with a mocked client address.

#![allow(dead_code)]

pub mod editor;
pub mod fake_pg;

use axum::body::Body;
use axum::extract::connect_info::MockConnectInfo;
use axum::http::{header, Request, StatusCode};
use axum::Router;
use http_body_util::BodyExt;
use rust_sql::core::appdb::{AppDb, User};
use rust_sql::core::keys::{InMemoryKeyProvider, KeyProvider};
use rust_sql::web::{
    build_web_router, dispatch_ipc_event, IpcCtx, IpcRequest, WebApp, WebIpcState, WebSettings,
};
use serde_json::{json, Value};
use std::net::SocketAddr;
use std::sync::Arc;
use tempfile::TempDir;

/// Password used for every test account.
pub const PASSWORD: &str = "correct horse battery staple";
/// Host header accepted by [`WebSettings::loopback_for_tests`] on port 8080.
pub const HOST: &str = "127.0.0.1:8080";

/// Serializes tests in one binary: the configuration path override is process-wide.
pub async fn lock() -> tokio::sync::MutexGuard<'static, ()> {
    static LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
    LOCK.lock().await
}

/// An isolated web app.
pub struct TestApp {
    pub dir: TempDir,
    pub app: Arc<WebApp>,
    pub keys: Arc<InMemoryKeyProvider>,
}

impl TestApp {
    /// Fresh config file, application database, and test key.
    pub fn new() -> Self {
        let dir = TempDir::new().expect("tempdir");
        rust_sql::core::config::set_config_path_override(Some(dir.path().join("rust-sql.json")));
        let keys = Arc::new(InMemoryKeyProvider::new_for_tests());
        let db = AppDb::open(&dir.path().join("rust-sql.db")).expect("app db");
        let provider: Arc<dyn KeyProvider> = keys.clone();
        let app = Arc::new(WebApp::new(
            db,
            provider,
            WebSettings::loopback_for_tests(8080),
        ));
        Self { dir, app, keys }
    }

    /// Creates an account with [`PASSWORD`].
    pub fn user(&self, name: &str, admin: bool) -> User {
        self.app
            .db
            .create_user(name, PASSWORD, admin)
            .expect("create user")
    }

    /// The router as served, with every request coming from `ip`.
    pub fn router_from(&self, ip: [u8; 4]) -> Router {
        build_web_router(self.app.clone()).layer(MockConnectInfo(SocketAddr::from((ip, 40000))))
    }

    /// The router with requests from 192.0.2.1.
    pub fn router(&self) -> Router {
        self.router_from([192, 0, 2, 1])
    }

    /// Calls an IPC event directly as `user`, bypassing HTTP.
    pub async fn ipc_as(
        &self,
        user: &User,
        session: &WebIpcState,
        event: &str,
        args: Vec<Value>,
    ) -> Value {
        let ctx = IpcCtx {
            app: &self.app,
            session,
            user,
            source_ip: Some("192.0.2.1".into()),
        };
        let req = IpcRequest {
            event: event.into(),
            args,
        };
        serde_json::to_value(dispatch_ipc_event(&ctx, req).await).expect("serialize")
    }
}

impl Drop for TestApp {
    fn drop(&mut self) {
        rust_sql::core::config::set_config_path_override(None);
    }
}

/// A response: status, headers, and JSON body (`Null` if not JSON).
pub struct Resp {
    pub status: StatusCode,
    pub headers: axum::http::HeaderMap,
    pub body: Value,
}

/// Sends one request through `router`.
pub async fn send(router: &Router, req: Request<Body>) -> Resp {
    use tower::ServiceExt;
    let resp = router.clone().oneshot(req).await.expect("router");
    let status = resp.status();
    let headers = resp.headers().clone();
    let bytes = resp.into_body().collect().await.expect("body").to_bytes();
    let body = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    Resp {
        status,
        headers,
        body,
    }
}

/// Builds a JSON `POST` with the test `Host` header.
pub fn post(path: &str) -> axum::http::request::Builder {
    Request::post(path)
        .header(header::HOST, HOST)
        .header(header::CONTENT_TYPE, "application/json")
}

/// Attempts a login.
pub async fn login(router: &Router, username: &str, password: &str) -> Resp {
    let body = json!({ "username": username, "password": password });
    send(
        router,
        post("/api/login")
            .body(Body::from(body.to_string()))
            .unwrap(),
    )
    .await
}

/// Signs in successfully and returns the `Cookie` header value and CSRF token.
pub async fn session(router: &Router, username: &str) -> (String, String) {
    let r = login(router, username, PASSWORD).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    let set_cookie = r.headers[header::SET_COOKIE].to_str().unwrap().to_string();
    let cookie = set_cookie.split(';').next().unwrap().to_string();
    (
        cookie,
        r.body["data"]["csrfToken"].as_str().unwrap().to_string(),
    )
}

/// Calls `/api/ipc` with the given cookie and CSRF header.
pub async fn ipc_http(
    router: &Router,
    cookie: Option<&str>,
    csrf: Option<&str>,
    event: &str,
    args: Vec<Value>,
) -> Resp {
    ipc_http_at(router, "/api/ipc", cookie, csrf, event, args).await
}

/// Calls an IPC endpoint at `path` with the given cookie and CSRF header.
pub async fn ipc_http_at(
    router: &Router,
    path: &str,
    cookie: Option<&str>,
    csrf: Option<&str>,
    event: &str,
    args: Vec<Value>,
) -> Resp {
    let body = json!({ "event": event, "args": args });
    let mut b = post(path);
    if let Some(c) = cookie {
        b = b.header(header::COOKIE, c);
    }
    if let Some(t) = csrf {
        b = b.header("X-CSRF-Token", t);
    }
    send(router, b.body(Body::from(body.to_string())).unwrap()).await
}
