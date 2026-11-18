//! Trust-on-first-use host-key verification with a 180-day re-confirmation timer.
//!
//! This module holds the pure decision logic. Persistence lives in `super::pin_store`: the pin
//! list is stored encrypted under the OS keychain master key, and every approval and deletion
//! is written to the `audit_events` table of the application database.

use super::error::TunnelError;
use crate::common::types::{
    HostKeyChallenge, HostKeyChallengeKind, SshHostKeyPin, HOST_KEY_RECONFIRM_AFTER_DAYS,
};
use time::{Duration, OffsetDateTime};

pub use super::pin_store::{
    approve_host_key, approve_host_key_audited, decrypt_pins, delete_host_key,
    delete_host_key_audited, encrypt_pins, load_pins, PIN_STORE_FIELD, PIN_STORE_RECORD,
};

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

pub(super) fn same_endpoint(pin: &SshHostKeyPin, host: &str, port: u32) -> bool {
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

#[cfg(test)]
#[path = "host_keys_tests.rs"]
mod tests;
