//! Web IPC handlers for SSH bastion host-key approval and pin management.
//!
//! `DB_CONNECT` answers an unknown or expiring bastion key with an error carrying
//! `hostKeyChallenge`. Only a challenge the server actually issued can be approved, so a client
//! cannot pin an arbitrary fingerprint. A changed key never produces a challenge.

use crate::common::event::{SSH_HOST_KEYS_LIST, SSH_HOST_KEY_APPROVE, SSH_HOST_KEY_DELETE};
use crate::common::types::HostKeyChallenge;
use crate::core::errors::DbCoreError;
use crate::core::tunnel;
use crate::web::{str_arg, IpcResponse, WebIpcState};
use serde_json::{json, Value};

/// Builds the error response for a failed connect, attaching any host-key challenge.
pub async fn connect_error_response(state: &WebIpcState, err: DbCoreError) -> IpcResponse {
    let mut resp = IpcResponse::err(err.to_string());
    if let DbCoreError::Tunnel(t) = &err {
        if let Some(challenge) = t.host_key_challenge() {
            let mut pending = state.pending_host_keys.lock().await;
            if !pending.contains(challenge) {
                pending.push(challenge.clone());
            }
            resp.host_key_challenge = Some(Box::new(challenge.clone()));
        }
    }
    resp
}

/// Handles host-key IPC events, returning `None` if `event` is unhandled.
pub async fn handle_ssh_host_keys_ipc(
    state: &WebIpcState,
    event: &str,
    args: &[Value],
) -> Option<IpcResponse> {
    let resp = match event {
        SSH_HOST_KEY_APPROVE => {
            let challenge: HostKeyChallenge =
                match args.first().cloned().map(serde_json::from_value) {
                    Some(Ok(c)) => c,
                    _ => return Some(IpcResponse::err("Missing host key challenge")),
                };
            let mut pending = state.pending_host_keys.lock().await;
            let Some(idx) = pending.iter().position(|p| *p == challenge) else {
                return Some(IpcResponse::err(
                    "This host key was not presented by a connection attempt; connect first",
                ));
            };
            match tunnel::approve_host_key(&challenge) {
                Ok(()) => {
                    pending.remove(idx);
                    IpcResponse::success(Value::Null)
                }
                Err(e) => IpcResponse::err(e.to_string()),
            }
        }
        SSH_HOST_KEYS_LIST => match tunnel::load_pins() {
            Ok(pins) => IpcResponse::success(pins),
            Err(e) => IpcResponse::err(e.to_string()),
        },
        SSH_HOST_KEY_DELETE => {
            let host = str_arg(args, 0).unwrap_or_default();
            let port = args.get(1).and_then(Value::as_u64).unwrap_or(22);
            let Ok(port) = u32::try_from(port) else {
                return Some(IpcResponse::err("Invalid port"));
            };
            match tunnel::delete_host_key(&host, port) {
                Ok(removed) => IpcResponse::success(json!({ "removed": removed })),
                Err(e) => IpcResponse::err(e.to_string()),
            }
        }
        _ => return None,
    };
    Some(resp)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::types::HostKeyChallengeKind;
    use serde_json::to_value;

    #[tokio::test]
    async fn approve_refuses_a_challenge_the_server_never_issued() {
        let state = WebIpcState::default();
        let forged = HostKeyChallenge {
            kind: HostKeyChallengeKind::Unknown,
            host: "bastion".into(),
            port: 22,
            key_algorithm: "ssh-ed25519".into(),
            fingerprint_sha256: "SHA256:attacker".into(),
        };
        let resp =
            handle_ssh_host_keys_ipc(&state, SSH_HOST_KEY_APPROVE, &[to_value(&forged).unwrap()])
                .await
                .unwrap();
        assert!(!resp.ok);
        assert!(resp.error.unwrap().contains("not presented"));
    }

    #[tokio::test]
    async fn connect_error_carries_challenge_and_records_it() {
        let state = WebIpcState::default();
        let challenge = HostKeyChallenge {
            kind: HostKeyChallengeKind::Unknown,
            host: "bastion".into(),
            port: 22,
            key_algorithm: "ssh-ed25519".into(),
            fingerprint_sha256: "SHA256:real".into(),
        };
        let err = DbCoreError::Tunnel(tunnel::TunnelError::HostKeyUnknown(challenge.clone()));
        let resp = connect_error_response(&state, err).await;
        assert_eq!(resp.host_key_challenge.as_deref(), Some(&challenge));
        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["hostKeyChallenge"]["fingerprintSha256"], "SHA256:real");
        assert_eq!(state.pending_host_keys.lock().await.as_slice(), [challenge]);
    }
}
