//! Shared helpers for suites that run against real database servers.
//!
//! A suite skips (and says so) when its server's environment variables are not set, so
//! `cargo test` stays green on machines without the containers from `docker-compose.yml`.
#![allow(dead_code)]

use rust_sql::common::types::{Password, Server};

fn env_or(key: &str, default: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| default.to_string())
}

/// The live PostgreSQL server from `RUST_SQL_TEST_PG_*`, or `None` (skipped) when unset.
pub fn live_pg_server() -> Option<Server> {
    let Ok(host) = std::env::var("RUST_SQL_TEST_PG_HOST") else {
        eprintln!("skipped: RUST_SQL_TEST_PG_HOST is not set");
        return None;
    };
    Some(Server {
        name: "pg".into(),
        client: "postgresql".into(),
        host: Some(host),
        port: Some(
            env_or("RUST_SQL_TEST_PG_PORT", "5432")
                .parse()
                .expect("port"),
        ),
        database: Some(env_or("RUST_SQL_TEST_PG_DB", "rust_sql")),
        user: Some(env_or("RUST_SQL_TEST_PG_USER", "postgres")),
        password: Some(Password::plain(env_or(
            "RUST_SQL_TEST_PG_PASSWORD",
            "password",
        ))),
        ..Default::default()
    })
}

/// A live server for `client` (`mysql`, `mariadb`) from `RUST_SQL_TEST_<PREFIX>_*`.
pub fn live_server_from_env(client: &str, prefix: &str, default_port: u32) -> Option<Server> {
    let Ok(host) = std::env::var(format!("RUST_SQL_TEST_{prefix}_HOST")) else {
        eprintln!("skipped: RUST_SQL_TEST_{prefix}_HOST is not set");
        return None;
    };
    let var = |k: &str, d: &str| env_or(&format!("RUST_SQL_TEST_{prefix}_{k}"), d);
    Some(Server {
        name: client.into(),
        client: client.into(),
        host: Some(host),
        port: Some(
            var("PORT", &default_port.to_string())
                .parse()
                .expect("port"),
        ),
        database: Some(var("DB", "rust_sql")),
        user: Some(var("USER", "root")),
        password: Some(Password::plain(var("PASSWORD", "password"))),
        ..Default::default()
    })
}

/// A file-backed SQLite server in `dir`.
pub fn sqlite_server(dir: &tempfile::TempDir) -> Server {
    Server {
        name: "sqlite".into(),
        client: "sqlite".into(),
        database: Some(dir.path().join("live.db").to_string_lossy().into_owned()),
        ..Default::default()
    }
}
