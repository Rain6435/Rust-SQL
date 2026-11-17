//! Web server authentication guards: 401 without a session, 403 without the CSRF token,
//! 421 for a forged `Host`, 429 after 5 failed logins, 413 above 1 MiB, the per-session
//! concurrency cap, cookie flags, logout, and expiry.

mod support;

use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use rust_sql::common::event::SERVERS_GET_ALL;
use rust_sql::core::appdb::sessions::hash_token;
use serde_json::json;
use std::time::Duration;
use support::{ipc_http, login, post, send, session, TestApp, HOST, PASSWORD};

#[tokio::test]
async fn requests_without_a_session_cookie_get_401() {
    let _g = support::lock().await;
    let t = TestApp::new();
    let r = t.router();
    assert_eq!(
        ipc_http(&r, None, None, SERVERS_GET_ALL, vec![])
            .await
            .status,
        StatusCode::UNAUTHORIZED
    );
    let bogus = ipc_http(
        &r,
        Some("rust_sql_session=forged"),
        Some("x"),
        SERVERS_GET_ALL,
        vec![],
    )
    .await;
    assert_eq!(bogus.status, StatusCode::UNAUTHORIZED);
    let get = Request::get("/api/session")
        .header(header::HOST, HOST)
        .body(Body::empty())
        .unwrap();
    assert_eq!(send(&r, get).await.status, StatusCode::UNAUTHORIZED);
    let logout = post("/api/logout").body(Body::from("{}")).unwrap();
    assert_eq!(send(&r, logout).await.status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn state_changing_requests_without_the_csrf_token_get_403() {
    let _g = support::lock().await;
    let t = TestApp::new();
    t.user("alice", false);
    let r = t.router();
    let (cookie, csrf) = session(&r, "alice").await;
    let missing = ipc_http(&r, Some(&cookie), None, SERVERS_GET_ALL, vec![]).await;
    assert_eq!(missing.status, StatusCode::FORBIDDEN, "{}", missing.body);
    let wrong = ipc_http(
        &r,
        Some(&cookie),
        Some("not-the-token"),
        SERVERS_GET_ALL,
        vec![],
    )
    .await;
    assert_eq!(wrong.status, StatusCode::FORBIDDEN);
    let logout = post("/api/logout")
        .header(header::COOKIE, &cookie)
        .body(Body::from("{}"))
        .unwrap();
    assert_eq!(send(&r, logout).await.status, StatusCode::FORBIDDEN);
    let ok = ipc_http(&r, Some(&cookie), Some(&csrf), SERVERS_GET_ALL, vec![]).await;
    assert_eq!(ok.status, StatusCode::OK, "{}", ok.body);
}

#[tokio::test]
async fn forged_host_header_gets_421_on_every_route() {
    let _g = support::lock().await;
    let t = TestApp::new();
    t.user("alice", false);
    let r = t.router();
    for (method, path) in [
        ("GET", "/"),
        ("GET", "/api/health"),
        ("POST", "/api/login"),
        ("POST", "/api/ipc"),
    ] {
        let req = Request::builder()
            .method(method)
            .uri(path)
            .header(header::HOST, "attacker.example:8080")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                json!({ "username": "alice", "password": PASSWORD }).to_string(),
            ))
            .unwrap();
        assert_eq!(
            send(&r, req).await.status,
            StatusCode::MISDIRECTED_REQUEST,
            "{path}"
        );
    }
    let no_host = Request::get("/api/health").body(Body::empty()).unwrap();
    assert_eq!(
        send(&r, no_host).await.status,
        StatusCode::MISDIRECTED_REQUEST
    );
}

#[tokio::test]
async fn login_sets_a_hardened_cookie_and_hides_whether_the_user_exists() {
    let _g = support::lock().await;
    let t = TestApp::new();
    t.user("alice", false);
    let r = t.router();
    let wrong = login(&r, "alice", "wrong password here").await;
    let unknown = login(&r, "nobody", "wrong password here").await;
    assert_eq!(wrong.status, StatusCode::UNAUTHORIZED);
    assert_eq!((unknown.status, &unknown.body), (wrong.status, &wrong.body));
    assert!(wrong.headers.get(header::SET_COOKIE).is_none());

    let ok = login(&r, "alice", PASSWORD).await;
    assert_eq!(ok.status, StatusCode::OK);
    let cookie = ok.headers[header::SET_COOKIE].to_str().unwrap();
    for attr in [
        "HttpOnly",
        "Secure",
        "SameSite=Strict",
        "Path=/",
        "Max-Age=43200",
    ] {
        assert!(cookie.contains(attr), "{attr} missing from {cookie}");
    }
    assert_eq!(ok.body["data"]["csrfToken"].as_str().unwrap().len(), 43);
    assert_eq!(ok.headers["x-frame-options"], "DENY");
}

#[tokio::test]
async fn five_failed_logins_lock_out_the_username_and_the_ip_with_429() {
    let _g = support::lock().await;
    let t = TestApp::new();
    t.user("alice", false);
    t.user("bob", false);
    let r = t.router_from([198, 51, 100, 1]);
    for _ in 0..5 {
        assert_eq!(
            login(&r, "alice", "wrong password here").await.status,
            StatusCode::UNAUTHORIZED
        );
    }
    let locked = login(&r, "alice", PASSWORD).await;
    assert_eq!(
        locked.status,
        StatusCode::TOO_MANY_REQUESTS,
        "{}",
        locked.body
    );
    let retry: u64 = locked.headers[header::RETRY_AFTER]
        .to_str()
        .unwrap()
        .parse()
        .unwrap();
    assert!(retry > 0 && retry <= 900);
    // The username is locked from every address.
    let other_ip = t.router_from([198, 51, 100, 2]);
    assert_eq!(
        login(&other_ip, "alice", PASSWORD).await.status,
        StatusCode::TOO_MANY_REQUESTS
    );
    // The address is locked for every username.
    assert_eq!(
        login(&r, "bob", PASSWORD).await.status,
        StatusCode::TOO_MANY_REQUESTS
    );
    assert_eq!(
        login(&other_ip, "bob", PASSWORD).await.status,
        StatusCode::OK
    );

    let audit = t.app.db.list_audit(50).unwrap();
    assert_eq!(
        audit
            .iter()
            .filter(|a| a.event_type == "login_failed")
            .count(),
        5
    );
    assert!(audit.iter().any(|a| a.event_type == "login_rate_limited"));
}

#[tokio::test]
async fn bodies_over_one_mebibyte_get_413() {
    let _g = support::lock().await;
    let t = TestApp::new();
    t.user("alice", false);
    let r = t.router();
    let (cookie, csrf) = session(&r, "alice").await;
    let big = json!({ "event": "LOGGER_ERROR", "args": ["x".repeat(1024 * 1024 + 1)] }).to_string();
    let req = post("/api/ipc")
        .header(header::COOKIE, &cookie)
        .header("X-CSRF-Token", &csrf)
        .body(Body::from(big))
        .unwrap();
    assert_eq!(send(&r, req).await.status, StatusCode::PAYLOAD_TOO_LARGE);
    let login_big = post("/api/login")
        .body(Body::from("x".repeat(2 * 1024 * 1024)))
        .unwrap();
    assert_eq!(
        send(&r, login_big).await.status,
        StatusCode::PAYLOAD_TOO_LARGE
    );
}

#[tokio::test]
async fn concurrent_requests_beyond_the_session_cap_get_429() {
    let _g = support::lock().await;
    let t = TestApp::new();
    let alice = t.user("alice", false);
    let r = t.router();
    let (cookie, csrf) = session(&r, "alice").await;
    let token = cookie.trim_start_matches("rust_sql_session=");
    let state = t.app.session_state(&hash_token(token), alice.id);
    let held = state.permits.clone().acquire_many_owned(8).await.unwrap();
    let busy = ipc_http(&r, Some(&cookie), Some(&csrf), SERVERS_GET_ALL, vec![]).await;
    assert_eq!(busy.status, StatusCode::TOO_MANY_REQUESTS);
    drop(held);
    let ok = ipc_http(&r, Some(&cookie), Some(&csrf), SERVERS_GET_ALL, vec![]).await;
    assert_eq!(ok.status, StatusCode::OK);
}

#[tokio::test]
async fn logout_and_expiry_end_the_session() {
    let _g = support::lock().await;
    let t = TestApp::new();
    let alice = t.user("alice", false);
    let r = t.router();
    let (cookie, csrf) = session(&r, "alice").await;
    let logout = post("/api/logout")
        .header(header::COOKIE, &cookie)
        .header("X-CSRF-Token", &csrf)
        .body(Body::from("{}"))
        .unwrap();
    let out = send(&r, logout).await;
    assert_eq!(out.status, StatusCode::OK);
    assert!(out.headers[header::SET_COOKIE]
        .to_str()
        .unwrap()
        .contains("Max-Age=0"));
    let after = ipc_http(&r, Some(&cookie), Some(&csrf), SERVERS_GET_ALL, vec![]).await;
    assert_eq!(after.status, StatusCode::UNAUTHORIZED);

    let expired = t.app.db.create_session(alice.id, Duration::ZERO).unwrap();
    let cookie = format!("rust_sql_session={}", expired.token);
    let resp = ipc_http(
        &r,
        Some(&cookie),
        Some(&expired.csrf_token),
        SERVERS_GET_ALL,
        vec![],
    )
    .await;
    assert_eq!(resp.status, StatusCode::UNAUTHORIZED);
    let kinds: Vec<_> = t
        .app
        .db
        .list_audit(10)
        .unwrap()
        .into_iter()
        .map(|a| a.event_type)
        .collect();
    assert!(kinds.contains(&"logout".to_string()) && kinds.contains(&"login".to_string()));
}
