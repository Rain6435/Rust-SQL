use crate::adapters::CLIENTS;
use crate::common::types::{Password, Server};

/// Parses a connection URI string (`postgres://user:pass@host:5432/db`, `mysql://...`, `file:///path.db`, etc.)
/// into a `Server` object, matching `ServerModalForm` (`connection-string` package).
pub fn parse_connection_uri(uri: &str, existing: Option<&Server>) -> Option<Server> {
    let trimmed = uri.trim();
    if trimmed.is_empty() {
        return None;
    }

    let parsed = url::Url::parse(trimmed).ok()?;
    let scheme = parsed.scheme();

    let client = CLIENTS
        .iter()
        .find(|c| c.protocol == scheme || c.key == scheme)
        .map(|c| c.key.clone())?;

    let mut srv = existing.cloned().unwrap_or_default();
    srv.client = client.clone();

    let username = parsed.username();
    if !username.is_empty() {
        srv.user = Some(urlencoding_decode(username));
    }

    if let Some(pass) = parsed.password() {
        srv.password = Some(Password::Plain(urlencoding_decode(pass)));
    }

    if let Some(host) = parsed.host_str() {
        if !host.is_empty() {
            srv.host = Some(host.to_string());
        }
    }

    if let Some(port) = parsed.port() {
        srv.port = Some(port as u32);
    } else if let Some(adapter) = CLIENTS.iter().find(|c| c.key == client) {
        if srv.port.is_none() {
            srv.port = adapter.default_port;
        }
    }

    let path = parsed.path();
    if client == "sqlite" {
        if !path.is_empty() {
            srv.database = Some(path.to_string());
        }
    } else {
        let db = path.trim_start_matches('/');
        if !db.is_empty() {
            srv.database = Some(urlencoding_decode(db));
        }
    }

    if srv.name.trim().is_empty() {
        srv.name = srv
            .database
            .clone()
            .or_else(|| srv.host.clone())
            .unwrap_or_else(|| client.clone());
    }

    Some(srv)
}

/// Builds a connection URI string from a `Server`, masking the password with `*` unless `show_plain_password` is true.
pub fn build_connection_uri(server: &Server, show_plain_password: bool) -> String {
    let Some(client_info) = CLIENTS.iter().find(|c| c.key == server.client) else {
        return String::new();
    };

    let host = server
        .host
        .as_deref()
        .filter(|s| !s.is_empty())
        .or_else(|| server.socket_path.as_deref().filter(|s| !s.is_empty()));
    let user = server.user.as_deref().filter(|s| !s.is_empty());
    let db = server.database.as_deref().filter(|s| !s.is_empty());

    if host.is_none() && user.is_none() && db.is_none() {
        return String::new();
    }

    let protocol = &client_info.protocol;
    let mut uri = format!("{protocol}://");

    if let Some(u) = user {
        uri.push_str(u);
        if let Some(pw) = &server.password {
            if !pw.is_empty() {
                let raw_pw = match pw {
                    Password::Plain(s) => s.clone(),
                    Password::Encrypted(e) => e.encrypted_text.clone(),
                };
                let formatted_pw = if show_plain_password {
                    raw_pw
                } else {
                    "*".repeat(raw_pw.len())
                };
                uri.push(':');
                uri.push_str(&formatted_pw);
            }
        }
        if host.is_some() {
            uri.push('@');
        }
    }

    if let Some(h) = host {
        uri.push_str(h);
        if let Some(port) = server.port.or(client_info.default_port) {
            uri.push(':');
            uri.push_str(&port.to_string());
        }
    }

    if let Some(d) = db {
        if !d.starts_with('/') {
            uri.push('/');
        }
        uri.push_str(d);
    }

    uri
}

fn urlencoding_decode(input: &str) -> String {
    let mut out = Vec::with_capacity(input.len());
    let bytes = input.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(hex) = std::str::from_utf8(&bytes[i + 1..i + 3]) {
                if let Ok(val) = u8::from_str_radix(hex, 16) {
                    out.push(val);
                    i += 3;
                    continue;
                }
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_and_build_uri() {
        let srv =
            parse_connection_uri("postgres://admin:secret@localhost:5432/mydb", None).unwrap();
        assert_eq!(srv.client, "postgresql");
        assert_eq!(srv.user.as_deref(), Some("admin"));
        assert_eq!(srv.password, Some(Password::Plain("secret".to_string())));
        assert_eq!(srv.host.as_deref(), Some("localhost"));
        assert_eq!(srv.port, Some(5432));
        assert_eq!(srv.database.as_deref(), Some("mydb"));

        assert_eq!(
            build_connection_uri(&srv, false),
            "postgres://admin:******@localhost:5432/mydb"
        );
        assert_eq!(
            build_connection_uri(&srv, true),
            "postgres://admin:secret@localhost:5432/mydb"
        );
    }
}
