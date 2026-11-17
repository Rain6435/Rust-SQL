//! Local listener that forwards each accepted TCP connection over an SSH `direct-tcpip` channel.

use super::error::TunnelError;
use super::host_keys::load_pins;
use super::session::{open_session, ClientHandler};
use super::{config_tunnel, TunnelConfig};
use crate::common::types::{Server, SshHostKeyPin};
use russh::client::Handle;
use russh::Disconnect;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io::copy_bidirectional;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::watch;
use tokio::task::{JoinHandle, JoinSet};

/// Active tunnel: a loopback listener plus the authenticated SSH session behind it.
///
/// [`TunnelHandle::close`] and `Drop` stop the listener, abort forwarded connections, and
/// disconnect the SSH session.
#[derive(Debug)]
pub struct TunnelHandle {
    /// Local socket address (`127.0.0.1:<port>`) where the tunnel is listening.
    pub local_addr: SocketAddr,
    shutdown: watch::Sender<bool>,
    task: Option<JoinHandle<()>>,
}

impl TunnelHandle {
    /// Returns the local IP address string (`"127.0.0.1"`).
    pub fn local_host(&self) -> String {
        self.local_addr.ip().to_string()
    }

    /// Returns the ephemeral local TCP port bound by the tunnel.
    pub fn local_port(&self) -> u16 {
        self.local_addr.port()
    }

    /// Stops the listener and tears down the SSH session.
    pub fn close(&self) {
        let _ = self.shutdown.send(true);
    }

    /// Closes the tunnel and waits until the listener and SSH session are gone.
    pub async fn close_and_wait(mut self) {
        self.close();
        if let Some(task) = self.task.take() {
            let _ = task.await;
        }
    }
}

impl Drop for TunnelHandle {
    fn drop(&mut self) {
        let _ = self.shutdown.send(true);
    }
}

/// Opens an SSH session for `server_info` using the stored host-key pins and starts forwarding.
pub async fn connect_tunnel(server_info: &Server) -> Result<TunnelHandle, TunnelError> {
    let cfg = config_tunnel(server_info)?;
    connect_tunnel_with_pins(&cfg, load_pins()?).await
}

/// Opens an SSH session for `cfg`, verifying the host key against `pins`, and starts forwarding.
pub async fn connect_tunnel_with_pins(
    cfg: &TunnelConfig,
    pins: Vec<SshHostKeyPin>,
) -> Result<TunnelHandle, TunnelError> {
    let session = Arc::new(open_session(cfg, pins).await?);
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .map_err(|e| TunnelError::InvalidConfig(format!("cannot bind local tunnel port: {e}")))?;
    let local_addr = listener
        .local_addr()
        .map_err(|e| TunnelError::InvalidConfig(format!("cannot read local tunnel port: {e}")))?;
    let (shutdown, shutdown_rx) = watch::channel(false);
    let task = tokio::spawn(run_listener(
        listener,
        session,
        shutdown_rx,
        cfg.dst_host.clone(),
        cfg.dst_port,
    ));
    Ok(TunnelHandle {
        local_addr,
        shutdown,
        task: Some(task),
    })
}

async fn run_listener(
    listener: TcpListener,
    session: Arc<Handle<ClientHandler>>,
    mut shutdown: watch::Receiver<bool>,
    dst_host: String,
    dst_port: u32,
) {
    let mut conns = JoinSet::new();
    loop {
        tokio::select! {
            changed = shutdown.changed() => {
                if changed.is_err() || *shutdown.borrow() {
                    break;
                }
            }
            accepted = listener.accept() => match accepted {
                Ok((inbound, peer)) => {
                    conns.spawn(forward(session.clone(), inbound, peer, dst_host.clone(), dst_port));
                }
                Err(e) => log::warn!("SSH tunnel: accept failed: {e}"),
            },
            Some(_) = conns.join_next(), if !conns.is_empty() => {}
        }
    }
    drop(listener);
    conns.abort_all();
    let _ = session
        .disconnect(Disconnect::ByApplication, "tunnel closed", "en")
        .await;
}

/// Forwards one local connection. A channel error closes only this connection.
async fn forward(
    session: Arc<Handle<ClientHandler>>,
    mut inbound: TcpStream,
    peer: SocketAddr,
    dst_host: String,
    dst_port: u32,
) {
    let channel = match session
        .channel_open_direct_tcpip(
            dst_host.clone(),
            dst_port,
            peer.ip().to_string(),
            u32::from(peer.port()),
        )
        .await
    {
        Ok(ch) => ch,
        Err(e) => {
            log::warn!("SSH tunnel: bastion refused channel to {dst_host}:{dst_port}: {e}");
            return;
        }
    };
    let mut remote = channel.into_stream();
    if let Err(e) = copy_bidirectional(&mut inbound, &mut remote).await {
        log::debug!("SSH tunnel: forwarded connection to {dst_host}:{dst_port} ended: {e}");
    }
}
