//! Trust-on-first-use host-key verification with a 180-day re-confirmation timer.
//!
//! Pins live in the `sshHostKeys` list of the configuration store. That store is not encrypted
//! yet; Workstream 4 moves it into the encrypted store, and Workstream 3 adds the audit table.
//! Until then approvals and deletions are logged with `log::warn!` under the `audit` target.

use super::error::TunnelError;
use crate::common::types::{
    HostKeyChallenge, HostKeyChallengeKind, SshHostKeyPin, HOST_KEY_RECONFIRM_AFTER_DAYS,
};
use crate::core::config;
use time::{Duration, OffsetDateTime};

/// Result of comparing a presented host key against the stored pins.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostKeyStatus {
    /// The key matches an approval that is still within the re-confirmation period.
    Trusted,
    /// No pin exists for this `host:port`.
    Unknown,
    /// The key matches, but the approval is older than the re-confirmation period.
    ReconfirmationRequired,
    /// A pin exists and the presented key differs from it.
    Changed {
        /// The pin that was expected.
        pinned: SshHostKeyPin,
    },
}

/// The key a bastion presented during the handshake.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PresentedHostKey {
    /// Bastion host as configured.
    pub host: String,
    /// Bastion port.
    pub port: u32,
    /// Key algorithm name.
    pub key_algorithm: String,
    /// `SHA256:` fingerprint.
    pub fingerprint_sha256: String,
}

impl PresentedHostKey {
    fn challenge(&self, kind: HostKeyChallengeKind) -> HostKeyChallenge {
        HostKeyChallenge {
            kind,
            host: self.host.clone(),
            port: self.port,
            key_algorithm: self.key_algorithm.clone(),
            fingerprint_sha256: self.fingerprint_sha256.clone(),
        }
    }
}

fn same_endpoint(pin: &SshHostKeyPin, host: &str, port: u32) -> bool {
    pin.port == port && pin.host.eq_ignore_ascii_case(host)
}

/// Returns the pin stored for `host:port`, if any.
pub fn find_pin<'a>(pins: &'a [SshHostKeyPin], host: &str, port: u32) -> Option<&'a SshHostKeyPin> {
    pins.iter().find(|p| same_endpoint(p, host, port))
}

/// Classifies `presented` against `pins` at time `now`.
pub fn evaluate(
    pins: &[SshHostKeyPin],
    presented: &PresentedHostKey,
    now: OffsetDateTime,
) -> HostKeyStatus {
    let Some(pin) = find_pin(pins, &presented.host, presented.port) else {
        return HostKeyStatus::Unknown;
    };
    if pin.fingerprint_sha256 != presented.fingerprint_sha256
        || pin.key_algorithm != presented.key_algorithm
    {
        return HostKeyStatus::Changed {
            pinned: pin.clone(),
        };
    }
    let max_age = Duration::days(HOST_KEY_RECONFIRM_AFTER_DAYS);
    // An approval dated in the future (clock change) is treated as expired: fail closed.
    if now - pin.approved_at > max_age || pin.approved_at > now + Duration::days(1) {
        return HostKeyStatus::ReconfirmationRequired;
    }
    HostKeyStatus::Trusted
}

/// Maps a [`HostKeyStatus`] to `Ok(())` when trusted, or the matching [`TunnelError`].
pub fn status_to_result(
    status: HostKeyStatus,
    presented: &PresentedHostKey,
) -> Result<(), TunnelError> {
    match status {
        HostKeyStatus::Trusted => Ok(()),
        HostKeyStatus::Unknown => Err(TunnelError::HostKeyUnknown(
            presented.challenge(HostKeyChallengeKind::Unknown),
        )),
        HostKeyStatus::ReconfirmationRequired => Err(TunnelError::HostKeyReconfirmationRequired(
            presented.challenge(HostKeyChallengeKind::ReconfirmationRequired),
        )),
        HostKeyStatus::Changed { pinned } => Err(TunnelError::HostKeyChanged {
            host: presented.host.clone(),
            port: presented.port,
            expected: format!("{} {}", pinned.key_algorithm, pinned.fingerprint_sha256),
            presented: format!(
                "{} {}",
                presented.key_algorithm, presented.fingerprint_sha256
            ),
        }),
    }
}

/// Applies a user's approval of `challenge` to `pins` at time `now`.
///
/// A first-use approval inserts a pin; a re-confirmation resets `approved_at`. If a different
/// key is pinned for the endpoint the approval is refused: a changed key has no in-app override.
pub fn apply_approval(
    pins: &mut Vec<SshHostKeyPin>,
    challenge: &HostKeyChallenge,
    now: OffsetDateTime,
) -> Result<(), TunnelError> {
    if let Some(pin) = pins
        .iter_mut()
        .find(|p| same_endpoint(p, &challenge.host, challenge.port))
    {
        if pin.fingerprint_sha256 != challenge.fingerprint_sha256
            || pin.key_algorithm != challenge.key_algorithm
        {
            return Err(TunnelError::HostKeyChanged {
                host: challenge.host.clone(),
                port: challenge.port,
                expected: format!("{} {}", pin.key_algorithm, pin.fingerprint_sha256),
                presented: format!(
                    "{} {}",
                    challenge.key_algorithm, challenge.fingerprint_sha256
                ),
            });
        }
        pin.approved_at = now;
        return Ok(());
    }
    pins.push(SshHostKeyPin {
        host: challenge.host.clone(),
        port: challenge.port,
        key_algorithm: challenge.key_algorithm.clone(),
        fingerprint_sha256: challenge.fingerprint_sha256.clone(),
        approved_at: now,
    });
    Ok(())
}

/// Loads all host-key pins from the configuration store.
pub fn load_pins() -> Result<Vec<SshHostKeyPin>, TunnelError> {
    Ok(config::get_sync()
        .map_err(TunnelError::PinStore)?
        .ssh_host_keys)
}

/// Persists the user's approval (or re-confirmation) of `challenge`.
pub fn approve_host_key(challenge: &HostKeyChallenge) -> Result<(), TunnelError> {
    let mut file = config::get_sync().map_err(TunnelError::PinStore)?;
    apply_approval(
        &mut file.ssh_host_keys,
        challenge,
        OffsetDateTime::now_utc(),
    )?;
    config::save_sync(&file).map_err(TunnelError::PinStore)?;
    log::warn!(
        target: "audit",
        "ssh host key approved ({:?}): {}:{} {} {}",
        challenge.kind,
        challenge.host,
        challenge.port,
        challenge.key_algorithm,
        challenge.fingerprint_sha256
    );
    Ok(())
}

/// Deletes the pin for `host:port`. Returns whether a pin was removed.
pub fn delete_host_key(host: &str, port: u32) -> Result<bool, TunnelError> {
    let mut file = config::get_sync().map_err(TunnelError::PinStore)?;
    let before = file.ssh_host_keys.len();
    file.ssh_host_keys.retain(|p| !same_endpoint(p, host, port));
    let removed = file.ssh_host_keys.len() != before;
    if removed {
        config::save_sync(&file).map_err(TunnelError::PinStore)?;
        log::warn!(target: "audit", "ssh host key pin deleted: {host}:{port}");
    }
    Ok(removed)
}

#[cfg(test)]
#[path = "host_keys_tests.rs"]
mod tests;
