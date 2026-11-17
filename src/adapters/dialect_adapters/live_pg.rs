//! Live PostgreSQL / Redshift connection setup and error reporting via `tokio-postgres`.
//!
//! Connection failures are returned to the caller. They name the host, port, and database,
//! and never the password. There is no fallback to the emulator.

use crate::adapters::dialect_adapters::seed::dialect_display_name;
use crate::adapters::dialect_adapters::sql_templates::pg_sql;
use crate::common::types::{AdapterVersion, Server};
use crate::core::errors::DbCoreError;
use std::sync::Arc;
use std::time::Duration;
use tokio_postgres::NoTls;

/// How long a live connection attempt may take before it fails.
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);

/// A successfully established live PostgreSQL or Redshift session.
pub struct LivePostgres {
    /// Connected client.
    pub client: Arc<tokio_postgres::Client>,
    /// `current_schema()` reported by the server, if any.
    pub schema: Option<String>,
    /// Version reported by `select version()`.
    pub version: AdapterVersion,
}

/// Replaces every occurrence of `secret` in `text` with `***`.
pub(crate) fn redact(text: &str, secret: Option<&str>) -> String {
    match secret.filter(|s| !s.is_empty()) {
        Some(s) => text.replace(s, "***"),
        None => text.to_string(),
    }
}

/// Renders a driver error with its full cause chain (`tokio_postgres::Error` hides it otherwise).
pub(crate) fn describe_pg_error(e: &tokio_postgres::Error) -> String {
    if let Some(db) = e.as_db_error() {
        return format!("{db} (SQLSTATE {})", db.code().code());
    }
    let mut out = e.to_string();
    let mut source = std::error::Error::source(e);
    while let Some(cause) = source {
        let text = cause.to_string();
        if !out.contains(&text) {
            out.push_str(": ");
            out.push_str(&text);
        }
        source = cause.source();
    }
    out
}

/// Opens a live connection for `adapter_key` (`postgresql` or `redshift`).
pub async fn connect_live_postgres(
    adapter_key: &str,
    server: &Server,
    database: &str,
) -> Result<LivePostgres, DbCoreError> {
    let host = server
        .local_host
        .as_deref()
        .or(server.host.as_deref())
        .filter(|h| !h.is_empty())
        .unwrap_or("localhost")
        .to_string();
    let port = server
        .local_port
        .map(u32::from)
        .or(server.port)
        .unwrap_or(5432);
    let password = server.password.as_ref().and_then(|p| p.as_plain());
    let fail = |reason: &str| DbCoreError::ConnectionFailed {
        dialect: dialect_display_name(adapter_key),
        host: host.clone(),
        port,
        database: database.to_string(),
        reason: redact(reason, password),
    };

    if server
        .password
        .as_ref()
        .is_some_and(|p| p.as_plain().is_none())
    {
        return Err(fail("the saved password could not be decrypted"));
    }
    if server.ssl.as_ref().is_some_and(|s| s.is_enabled()) {
        return Err(fail(
            "SSL/TLS is enabled for this connection, but TLS is not implemented for live \
             PostgreSQL connections yet. Refusing to connect without encryption.",
        ));
    }
    let port_u16 = u16::try_from(port).map_err(|_| fail("port is out of range"))?;

    let mut cfg = tokio_postgres::Config::new();
    cfg.host(&host)
        .port(port_u16)
        .user(server.user.as_deref().unwrap_or("postgres"))
        .dbname(database)
        .connect_timeout(CONNECT_TIMEOUT);
    if let Some(pw) = password {
        cfg.password(pw);
    }

    let (client, connection) = cfg
        .connect(NoTls)
        .await
        .map_err(|e| fail(&describe_pg_error(&e)))?;
    tokio::spawn(async move {
        if let Err(e) = connection.await {
            log::warn!("PostgreSQL connection closed: {e}");
        }
    });

    let schema = client
        .query_one(pg_sql::GET_SCHEMA, &[])
        .await
        .ok()
        .and_then(|r| r.try_get::<_, Option<String>>(0).ok().flatten());
    let ver_str: String = client
        .query_one(pg_sql::GET_VERSION, &[])
        .await
        .and_then(|r| r.try_get(0))
        .map_err(|e| {
            fail(&format!(
                "could not read server version: {}",
                describe_pg_error(&e)
            ))
        })?;
    let parts: Vec<&str> = ver_str.split(' ').collect();
    let version = AdapterVersion {
        name: parts.first().copied().unwrap_or("PostgreSQL").to_string(),
        version: parts.get(1).copied().unwrap_or("").to_string(),
        string: ver_str.clone(),
    };

    Ok(LivePostgres {
        client: Arc::new(client),
        schema,
        version,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redact_hides_every_occurrence() {
        assert_eq!(
            redact("pw=s3cr3t and s3cr3t", Some("s3cr3t")),
            "pw=*** and ***"
        );
        assert_eq!(redact("nothing here", Some("")), "nothing here");
        assert_eq!(redact("nothing here", None), "nothing here");
    }
}
