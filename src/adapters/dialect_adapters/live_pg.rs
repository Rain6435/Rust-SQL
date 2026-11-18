//! Live PostgreSQL / Redshift connection setup and error reporting via `tokio-postgres`.
//!
//! Connection failures are returned to the caller. They name the host, port, and database,
//! and never the password. There is no fallback to the emulator.

use crate::adapters::dialect_adapters::seed::{dialect_display_name, DialectAdapter};
use crate::adapters::dialect_adapters::sql_templates::pg_sql;
use crate::adapters::sqlite::connection::set_query_only;
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

/// Maps a query error, reporting PostgreSQL's `statement_timeout` cancel as a timeout.
pub(crate) fn pg_query_error(e: &tokio_postgres::Error) -> DbCoreError {
    if let Some(db) = e.as_db_error() {
        if db.code() == &tokio_postgres::error::SqlState::QUERY_CANCELED
            && db.message().contains("statement timeout")
        {
            return DbCoreError::StatementTimeout(format!(
                "{} (SQLSTATE {}). The connection's statement timeout stopped it.",
                db.message(),
                db.code().code()
            ));
        }
    }
    DbCoreError::Message(describe_pg_error(e))
}

/// `SET` command for the session's `statement_timeout` (`0` turns it off).
pub fn pg_statement_timeout_command(timeout: Option<Duration>) -> String {
    let ms = timeout.map(|t| t.as_millis()).unwrap_or(0);
    format!("SET statement_timeout = {ms}")
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
    if let Some(timeout) = server.statement_timeout() {
        client
            .batch_execute(&pg_statement_timeout_command(Some(timeout)))
            .await
            .map_err(|e| {
                fail(&format!(
                    "could not set statement_timeout: {}",
                    describe_pg_error(&e)
                ))
            })?;
    }

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

/// `SET` command that makes every later transaction on the session read-only (or not).
pub fn pg_read_only_command(read_only: bool) -> &'static str {
    if read_only {
        "SET SESSION CHARACTERISTICS AS TRANSACTION READ ONLY"
    } else {
        "SET SESSION CHARACTERISTICS AS TRANSACTION READ WRITE"
    }
}

impl DialectAdapter {
    /// Enforces Read-Only mode in the database session: a read-only transaction default on a live
    /// PostgreSQL/Redshift session, `PRAGMA query_only` on the emulator.
    pub(crate) async fn apply_read_only(&self, read_only: bool) -> Result<(), DbCoreError> {
        if let Some(client) = &self.pg_client {
            return client
                .batch_execute(pg_read_only_command(read_only))
                .await
                .map_err(|e| {
                    DbCoreError::Message(format!(
                        "Could not set the {} session read-{}: {}",
                        dialect_display_name(&self.adapter_key),
                        if read_only { "only" } else { "write" },
                        describe_pg_error(&e)
                    ))
                });
        }
        match &self.emulated_conn {
            Some(conn) => {
                let conn = conn
                    .lock()
                    .map_err(|e| DbCoreError::Message(e.to_string()))?;
                set_query_only(&conn, read_only)
            }
            None => Err(DbCoreError::NoConnectionAvailable),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn statement_timeout_command_uses_milliseconds() {
        assert_eq!(
            pg_statement_timeout_command(Some(Duration::from_secs(30))),
            "SET statement_timeout = 30000"
        );
        assert_eq!(
            pg_statement_timeout_command(None),
            "SET statement_timeout = 0"
        );
    }

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
