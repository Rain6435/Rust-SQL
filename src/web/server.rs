//! HTTP routing, request guards, and the HTTPS listener.
//!
//! Guards applied to every route: `Host` allowlist (421 otherwise), 1 MiB body limit (413),
//! and security headers. `/api/ipc` additionally requires a session (401), a CSRF token (403),
//! and a free slot in the session's concurrency cap (429).

use crate::core::appdb::AppDb;
use crate::core::keys::system_key_provider;
use crate::web::auth::{
    authenticate, error_response, handle_login, handle_logout, handle_session, source_ip,
};
use crate::web::settings::{WebLaunchOptions, WebSettings, BODY_LIMIT_BYTES};
use crate::web::state::{IpcCtx, WebApp};
use crate::web::{dispatch_ipc_event, IpcRequest, RUST_SQL_WEB_HTML};
use axum::body::Bytes;
use axum::extract::{ConnectInfo, DefaultBodyLimit, Request, State};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{Html, IntoResponse, Json, Response};
use axum::routing::{get, post};
use axum::Router;
use serde_json::{json, Value};
use std::net::SocketAddr;
use std::sync::Arc;

async fn host_guard(State(app): State<Arc<WebApp>>, req: Request, next: Next) -> Response {
    let host = req
        .headers()
        .get(header::HOST)
        .and_then(|h| h.to_str().ok())
        .map(str::to_string)
        .or_else(|| req.uri().authority().map(|a| a.to_string()));
    match host {
        Some(h) if app.settings.host_allowed(&h) => next.run(req).await,
        _ => error_response(
            StatusCode::MISDIRECTED_REQUEST,
            "Host header is not in the web.allowedHosts allowlist",
        ),
    }
}

async fn security_headers(State(app): State<Arc<WebApp>>, req: Request, next: Next) -> Response {
    let mut resp = next.run(req).await;
    let h = resp.headers_mut();
    h.insert(
        "x-content-type-options",
        HeaderValue::from_static("nosniff"),
    );
    h.insert("x-frame-options", HeaderValue::from_static("DENY"));
    h.insert("referrer-policy", HeaderValue::from_static("no-referrer"));
    h.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    h.insert(
        "content-security-policy",
        HeaderValue::from_static("frame-ancestors 'none'; base-uri 'none'; form-action 'self'"),
    );
    if app.settings.tls.is_some() {
        h.insert(
            header::STRICT_TRANSPORT_SECURITY,
            HeaderValue::from_static("max-age=31536000"),
        );
    }
    resp
}

async fn handle_ipc(
    State(app): State<Arc<WebApp>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let auth = match authenticate(&app, &headers, true) {
        Ok(a) => a,
        Err(resp) => return *resp,
    };
    let Ok(_permit) = auth.state.permits.clone().try_acquire_owned() else {
        return error_response(
            StatusCode::TOO_MANY_REQUESTS,
            "Too many concurrent requests for this session",
        );
    };
    let req: IpcRequest = match serde_json::from_slice(&body) {
        Ok(r) => r,
        Err(e) => return error_response(StatusCode::BAD_REQUEST, &e.to_string()),
    };
    let ctx = IpcCtx {
        app: &app,
        session: &auth.state,
        user: &auth.info.user,
        source_ip: Some(source_ip(&addr)),
    };
    let resp = dispatch_ipc_event(&ctx, req).await;
    let status = match (resp.ok, resp.status) {
        (_, Some(code)) => StatusCode::from_u16(code).unwrap_or(StatusCode::BAD_REQUEST),
        (true, None) => StatusCode::OK,
        (false, None) => StatusCode::BAD_REQUEST,
    };
    (status, Json(resp)).into_response()
}

async fn handle_index() -> Html<&'static str> {
    Html(RUST_SQL_WEB_HTML)
}

async fn handle_health() -> Json<Value> {
    Json(json!({
        "name": "Rust-SQL",
        "version": env!("CARGO_PKG_VERSION"),
        "status": "ok"
    }))
}

/// Builds the router. Callers serve it with `into_make_service_with_connect_info`.
pub fn build_web_router(app: Arc<WebApp>) -> Router {
    Router::new()
        .route("/", get(handle_index))
        .route("/api/health", get(handle_health))
        .route("/api/login", post(handle_login))
        .route("/api/logout", post(handle_logout))
        .route("/api/session", get(handle_session))
        .route("/api/ipc", post(handle_ipc))
        .layer(DefaultBodyLimit::max(BODY_LIMIT_BYTES))
        .layer(middleware::from_fn_with_state(
            app.clone(),
            security_headers,
        ))
        .layer(middleware::from_fn_with_state(app.clone(), host_guard))
        .with_state(app)
}

/// Loads the PEM certificate chain and key into a rustls server config (ring provider).
pub fn load_tls_config(
    cert: &std::path::Path,
    key: &std::path::Path,
) -> Result<rustls::ServerConfig, String> {
    use rustls::pki_types::pem::PemObject;
    use rustls::pki_types::{CertificateDer, PrivateKeyDer};
    let certs = CertificateDer::pem_file_iter(cert)
        .and_then(|it| it.collect::<Result<Vec<_>, _>>())
        .map_err(|e| format!("Cannot read TLS certificate {}: {e}", cert.display()))?;
    if certs.is_empty() {
        return Err(format!("No certificate found in {}", cert.display()));
    }
    let key = PrivateKeyDer::from_pem_file(key)
        .map_err(|e| format!("Cannot read TLS private key {}: {e}", key.display()))?;
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let mut cfg = rustls::ServerConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(|e| e.to_string())?
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .map_err(|e| format!("Invalid TLS certificate or key: {e}"))?;
    cfg.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];
    Ok(cfg)
}

/// Serves `app` on an already-bound listener, over HTTPS when TLS is configured.
pub async fn serve_on(app: Arc<WebApp>, listener: std::net::TcpListener) -> Result<(), String> {
    let tls = app.settings.tls.clone();
    let service = build_web_router(app).into_make_service_with_connect_info::<SocketAddr>();
    listener.set_nonblocking(true).map_err(|e| e.to_string())?;
    match tls {
        Some(paths) => {
            let cfg = load_tls_config(&paths.cert, &paths.key)?;
            let rustls_cfg = axum_server::tls_rustls::RustlsConfig::from_config(Arc::new(cfg));
            axum_server::from_tcp_rustls(listener, rustls_cfg)
                .map_err(|e| e.to_string())?
                .serve(service)
                .await
                .map_err(|e| e.to_string())
        }
        None => {
            let listener =
                tokio::net::TcpListener::from_std(listener).map_err(|e| e.to_string())?;
            axum::serve(listener, service)
                .await
                .map_err(|e| e.to_string())
        }
    }
}

/// Starts the multi-user web server after the startup safety checks.
pub async fn run_web_server(opts: WebLaunchOptions) -> Result<(), String> {
    let cfg = crate::core::config::get_sync()?.web.unwrap_or_default();
    let settings = WebSettings::resolve(&cfg, &opts)?;
    let db = AppDb::open_default().map_err(|e| e.to_string())?;
    if db.user_count().map_err(|e| e.to_string())? == 0 {
        return Err(format!(
            "No web accounts exist in {}. Create the first administrator with:\n  \
             Rust-SQL users create --admin --username <name>",
            db.path().display()
        ));
    }
    // The OS keychain is mandatory: without the master key the server could neither store
    // nor use saved credentials, so it refuses to start instead of running degraded.
    let keys = system_key_provider();
    keys.master_key().map_err(|e| {
        format!(
            "{e}\nThe web server needs the OS keychain master key and will not start without it."
        )
    })?;
    if let Some(tls) = &settings.tls {
        load_tls_config(&tls.cert, &tls.key)?;
    }
    let scheme = if settings.tls.is_some() {
        "https"
    } else {
        "http"
    };
    let listener = std::net::TcpListener::bind(settings.addr)
        .map_err(|e| format!("Cannot bind {}: {e}", settings.addr))?;
    println!("Rust-SQL Web UI listening on {scheme}://{}", settings.addr);
    if settings.tls.is_none() {
        println!("Development mode: plain HTTP on loopback only. Do not expose this port.");
    }
    let app = Arc::new(WebApp::new(db, keys, settings));
    serve_on(app, listener).await
}
