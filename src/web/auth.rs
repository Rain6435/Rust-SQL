//! Login, logout, session lookup, and CSRF verification for the web server.
//!
//! - The session cookie is `HttpOnly; Secure; SameSite=Strict; Path=/` and holds a random
//!   256-bit ID; the server stores only its SHA-256 hash.
//! - Every state-changing request must echo the session's CSRF token in `X-CSRF-Token`. All
//!   `POST` endpoints except `/api/login` are treated as state-changing.
//! - Missing or invalid sessions get 401; a missing or wrong CSRF token gets 403.
//! - Unknown users and wrong passwords get the same 401 response.

use crate::core::appdb::{AuditActor, AuditEvent, AuditKind, SessionInfo};
use crate::web::state::{WebApp, WebIpcState};
use axum::extract::{ConnectInfo, State};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Deserialize;
use serde_json::json;
use std::net::SocketAddr;
use std::sync::Arc;

/// Session cookie name.
pub const SESSION_COOKIE: &str = "rust_sql_session";
/// Header carrying the CSRF token.
pub const CSRF_HEADER: &str = "x-csrf-token";

/// A request with a valid session.
pub struct Authenticated {
    /// The session row and user.
    pub info: SessionInfo,
    /// The session's in-memory state.
    pub state: Arc<WebIpcState>,
}

/// JSON error body with `status`.
pub fn error_response(status: StatusCode, msg: &str) -> Response {
    (status, Json(json!({ "ok": false, "error": msg }))).into_response()
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// Extracts the session ID from the `Cookie` header(s).
pub fn session_token(headers: &HeaderMap) -> Option<String> {
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(';'))
        .filter_map(|pair| pair.trim().split_once('='))
        .find(|(name, _)| *name == SESSION_COOKIE)
        .map(|(_, value)| value.to_string())
        .filter(|v| !v.is_empty())
}

/// Client IP from the connection.
pub fn source_ip(addr: &SocketAddr) -> String {
    addr.ip().to_string()
}

/// Validates the session cookie and, for state-changing requests, the CSRF header.
pub fn authenticate(
    app: &WebApp,
    headers: &HeaderMap,
    state_changing: bool,
) -> Result<Authenticated, Box<Response>> {
    let unauthorized = || Box::new(error_response(StatusCode::UNAUTHORIZED, "Not signed in"));
    let token = session_token(headers).ok_or_else(unauthorized)?;
    let info = app
        .db
        .lookup_session(&token)
        .map_err(|e| {
            Box::new(error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                &e.to_string(),
            ))
        })?
        .ok_or_else(unauthorized)?;
    if state_changing {
        let sent = headers
            .get(CSRF_HEADER)
            .map(|v| v.as_bytes())
            .unwrap_or_default();
        if !constant_time_eq(sent, info.csrf_token.as_bytes()) {
            return Err(Box::new(error_response(
                StatusCode::FORBIDDEN,
                "Missing or invalid CSRF token",
            )));
        }
    }
    let state = app.session_state(&info.token_hash, info.user.id);
    Ok(Authenticated { info, state })
}

fn session_cookie(value: &str, max_age_secs: u64) -> HeaderValue {
    let cookie = format!(
        "{SESSION_COOKIE}={value}; HttpOnly; Secure; SameSite=Strict; Path=/; Max-Age={max_age_secs}"
    );
    HeaderValue::from_str(&cookie).unwrap_or_else(|_| HeaderValue::from_static(""))
}

/// Login request body.
#[derive(Deserialize)]
pub struct LoginBody {
    /// Account name.
    pub username: String,
    /// Password.
    pub password: String,
}

/// `POST /api/login`.
pub async fn handle_login(
    State(app): State<Arc<WebApp>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Json(body): Json<LoginBody>,
) -> Response {
    let ip = source_ip(&addr);
    let attempted = AuditActor {
        user_id: None,
        username: Some(body.username.clone()),
        source_ip: Some(ip.clone()),
    };
    if let Err(retry) = app.limiter.check(&body.username, Some(&ip)) {
        let secs = retry.as_secs().max(1);
        app.audit(AuditEvent::new(AuditKind::LoginRateLimited, &attempted).outcome("denied"));
        let mut resp = error_response(
            StatusCode::TOO_MANY_REQUESTS,
            &format!("Too many failed login attempts. Try again in {secs} seconds."),
        );
        resp.headers_mut()
            .insert(header::RETRY_AFTER, HeaderValue::from(secs));
        return resp;
    }

    let db = app.db.clone();
    let (username, password) = (body.username.clone(), body.password);
    let verified = tokio::task::spawn_blocking(move || db.verify_login(&username, &password))
        .await
        .map_err(|e| e.to_string())
        .and_then(|r| r.map_err(|e| e.to_string()));
    let user = match verified {
        Ok(Some(user)) => user,
        Ok(None) => {
            app.limiter.record_failure(&body.username, Some(&ip));
            app.audit(AuditEvent::new(AuditKind::LoginFailed, &attempted).outcome("failure"));
            return error_response(StatusCode::UNAUTHORIZED, "Invalid username or password");
        }
        Err(e) => return error_response(StatusCode::INTERNAL_SERVER_ERROR, &e),
    };

    app.limiter.record_success(&user.username);
    let ttl = app.settings.session_ttl;
    let session = match app.db.create_session(user.id, ttl) {
        Ok(s) => s,
        Err(e) => return error_response(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    };
    let actor = AuditActor {
        user_id: Some(user.id),
        username: Some(user.username.clone()),
        source_ip: Some(ip),
    };
    app.audit(AuditEvent::new(AuditKind::Login, &actor));
    let mut resp = Json(json!({
        "ok": true,
        "data": {
            "username": user.username,
            "isAdmin": user.is_admin,
            "csrfToken": session.csrf_token,
        }
    }))
    .into_response();
    resp.headers_mut().insert(
        header::SET_COOKIE,
        session_cookie(&session.token, ttl.as_secs()),
    );
    resp
}

/// `POST /api/logout`: revokes the session and clears the cookie.
pub async fn handle_logout(
    State(app): State<Arc<WebApp>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> Response {
    let auth = match authenticate(&app, &headers, true) {
        Ok(a) => a,
        Err(resp) => return *resp,
    };
    if let Err(e) = app.db.revoke_session(&auth.info.token_hash) {
        return error_response(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string());
    }
    app.drop_session_state(&auth.info.token_hash).await;
    let actor = AuditActor {
        user_id: Some(auth.info.user.id),
        username: Some(auth.info.user.username.clone()),
        source_ip: Some(source_ip(&addr)),
    };
    app.audit(AuditEvent::new(AuditKind::Logout, &actor));
    let mut resp = Json(json!({ "ok": true })).into_response();
    resp.headers_mut()
        .insert(header::SET_COOKIE, session_cookie("", 0));
    resp
}

/// `GET /api/session`: who is signed in, plus the CSRF token for this session.
pub async fn handle_session(State(app): State<Arc<WebApp>>, headers: HeaderMap) -> Response {
    match authenticate(&app, &headers, false) {
        Ok(auth) => Json(json!({
            "ok": true,
            "data": {
                "username": auth.info.user.username,
                "isAdmin": auth.info.user.is_admin,
                "csrfToken": auth.info.csrf_token,
            }
        }))
        .into_response(),
        Err(resp) => *resp,
    }
}
