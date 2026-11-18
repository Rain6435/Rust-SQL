//! SSH host-key approval actions on [`AppStore`], shared by the GUI and TUI.
//!
//! A connection attempt that hits an unknown (or 180-day-old) bastion key stores the challenge
//! in [`ConnectionState::host_key_prompt`](crate::state::ConnectionState). The front end shows
//! the fingerprint; only that exact challenge can then be approved. A changed key is never
//! turned into a prompt, so it cannot be approved in-app.

use crate::common::types::{HostKeyChallenge, SshHostKeyPin};
use crate::core::errors::DbCoreError;
use crate::core::tunnel;
use crate::state::AppStore;

impl AppStore {
    /// Records a host-key challenge carried by `err` and returns the error text for display.
    pub(crate) fn record_host_key_challenge(
        &mut self,
        err: &DbCoreError,
        retry_server_id: Option<&str>,
    ) -> String {
        if let DbCoreError::Tunnel(t) = err {
            if let Some(challenge) = t.host_key_challenge() {
                self.connections.host_key_prompt = Some(challenge.clone());
                self.connections.host_key_retry_server_id = retry_server_id.map(str::to_string);
            }
        }
        err.to_string()
    }

    /// Approves the pending host-key prompt and returns the server ID to reconnect, if any.
    pub fn approve_host_key(&mut self) -> Result<Option<String>, String> {
        let challenge = self
            .connections
            .host_key_prompt
            .take()
            .ok_or_else(|| "No SSH host key is waiting for approval".to_string())?;
        let retry = self.connections.host_key_retry_server_id.take();
        self.approve_host_key_challenge(&challenge)?;
        self.connections.error = None;
        self.connections.test_error = None;
        Ok(retry)
    }

    /// Persists approval of `challenge`. Refuses if a different key is pinned for that host.
    pub fn approve_host_key_challenge(
        &mut self,
        challenge: &HostKeyChallenge,
    ) -> Result<(), String> {
        tunnel::approve_host_key(challenge, self.key_provider()).map_err(|e| e.to_string())?;
        self.status = format!(
            "Trusted SSH host key {} for {}:{}",
            challenge.fingerprint_sha256, challenge.host, challenge.port
        );
        Ok(())
    }

    /// Dismisses the pending host-key prompt without trusting the key.
    pub fn reject_host_key(&mut self) {
        self.connections.host_key_prompt = None;
        self.connections.host_key_retry_server_id = None;
    }

    /// Lists trusted SSH bastion host keys.
    pub fn list_host_keys(&self) -> Result<Vec<SshHostKeyPin>, String> {
        tunnel::load_pins(self.key_provider()).map_err(|e| e.to_string())
    }

    /// Deletes the pin for `host:port`, the only way to accept a rotated bastion key.
    pub fn delete_host_key(&mut self, host: &str, port: u32) -> Result<bool, String> {
        let removed =
            tunnel::delete_host_key(host, port, self.key_provider()).map_err(|e| e.to_string())?;
        if removed {
            self.status = format!("Deleted SSH host key pin for {host}:{port}");
        }
        Ok(removed)
    }
}

#[cfg(test)]
#[path = "host_keys_tests.rs"]
mod tests;
