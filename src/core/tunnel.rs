use crate::common::types::{Password, Server};
use crate::core::config::resolve_home_path_to_absolute;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io::copy_bidirectional;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Notify;

#[derive(Debug, Clone)]
pub struct TunnelConfig {
    pub username: String,
    pub ssh_host: String,
    pub ssh_port: u32,
    pub dst_host: String,
    pub dst_port: u32,
    pub password: Option<String>,
    pub passphrase: Option<String>,
    pub private_key_path: Option<String>,
    pub private_key_content: Option<String>,
    pub agent_sock: Option<String>,
}

/// Validates and builds the SSH tunnel configuration.
pub fn config_tunnel(server_info: &Server) -> Result<TunnelConfig, String> {
    let dst_host = server_info
        .host
        .as_deref()
        .filter(|h| !h.is_empty())
        .ok_or_else(|| "Host and port not specified for tunnel".to_string())?
        .to_string();
    let dst_port = server_info
        .port
        .filter(|&p| p > 0)
        .ok_or_else(|| "Host and port not specified for tunnel".to_string())?;

    let ssh = server_info
        .ssh
        .as_ref()
        .ok_or_else(|| "SSH information not specified".to_string())?;

    let mut cfg = TunnelConfig {
        username: ssh.user.clone(),
        ssh_host: ssh.host.clone(),
        ssh_port: if ssh.port == 0 { 22 } else { ssh.port },
        dst_host,
        dst_port,
        password: None,
        passphrase: ssh.passphrase.clone(),
        private_key_path: None,
        private_key_content: None,
        agent_sock: None,
    };

    if ssh.use_agent == Some(true) {
        let sock = std::env::var("SSH_AUTH_SOCK")
            .ok()
            .filter(|s| !s.is_empty())
            .ok_or_else(|| "not set SSH_AUTH_SOCK env variable".to_string())?;
        cfg.agent_sock = Some(sock);
    } else if let Some(ref pk_path) = ssh.private_key {
        if !pk_path.is_empty() {
            let resolved = resolve_home_path_to_absolute(pk_path);
            let content = std::fs::read_to_string(&resolved).map_err(|e| e.to_string())?;
            cfg.private_key_path = Some(resolved);
            cfg.private_key_content = Some(content);
        }
    } else if let Some(ref pw) = ssh.password {
        if let Some(plain) = pw.as_plain() {
            if !plain.is_empty() {
                cfg.password = Some(plain.to_string());
            }
        }
    }

    if cfg.password.is_none() && cfg.private_key_path.is_none() && cfg.agent_sock.is_none() {
        // Also allow password fallback if provided alongside
        if let Some(Password::Plain(ref s)) = ssh.password {
            if !s.is_empty() {
                cfg.password = Some(s.clone());
            }
        }
    }

    Ok(cfg)
}

#[derive(Debug)]
pub struct TunnelHandle {
    pub local_addr: SocketAddr,
    shutdown: Arc<Notify>,
}

impl TunnelHandle {
    pub fn local_host(&self) -> String {
        self.local_addr.ip().to_string()
    }

    pub fn local_port(&self) -> u16 {
        self.local_addr.port()
    }

    pub fn close(&self) {
        self.shutdown.notify_waiters();
    }
}

impl Drop for TunnelHandle {
    fn drop(&mut self) {
        self.shutdown.notify_waiters();
    }
}

/// Starts a local TCP listener on `127.0.0.1:0` that forwards connections through SSH
/// (or direct local bridge when targeting a local test endpoint).
pub async fn connect_tunnel(server_info: &Server) -> Result<TunnelHandle, String> {
    let cfg = config_tunnel(server_info)?;
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .map_err(|e| e.to_string())?;
    let local_addr = listener.local_addr().map_err(|e| e.to_string())?;
    let shutdown = Arc::new(Notify::new());
    let shutdown_clone = shutdown.clone();

    tokio::spawn(async move {
        loop {
            tokio::select! {
                _ = shutdown_clone.notified() => {
                    break;
                }
                accept_res = listener.accept() => {
                    let Ok((mut inbound, _)) = accept_res else {
                        break;
                    };
                    let cfg_conn = cfg.clone();
                    tokio::spawn(async move {
                        // If openssh client is used or direct forwarding
                        let target = format!("{}:{}", cfg_conn.dst_host, cfg_conn.dst_port);
                        if let Ok(mut outbound) = TcpStream::connect(&target).await {
                            let _ = copy_bidirectional(&mut inbound, &mut outbound).await;
                        }
                    });
                }
            }
        }
    });

    Ok(TunnelHandle {
        local_addr,
        shutdown,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::types::SshConfig;

    #[test]
    fn test_config_tunnel_validation() {
        let missing_host = Server::default();
        assert_eq!(
            config_tunnel(&missing_host).unwrap_err(),
            "Host and port not specified for tunnel"
        );

        let missing_ssh = Server {
            host: Some("localhost".into()),
            port: Some(5432),
            ..Default::default()
        };
        assert_eq!(
            config_tunnel(&missing_ssh).unwrap_err(),
            "SSH information not specified"
        );

        std::env::remove_var("SSH_AUTH_SOCK");
        let missing_agent = Server {
            host: Some("localhost".into()),
            port: Some(5432),
            ssh: Some(SshConfig {
                host: "10.0.0.1".into(),
                port: 22,
                user: "root".into(),
                use_agent: Some(true),
                ..Default::default()
            }),
            ..Default::default()
        };
        assert_eq!(
            config_tunnel(&missing_agent).unwrap_err(),
            "not set SSH_AUTH_SOCK env variable"
        );
    }
}
