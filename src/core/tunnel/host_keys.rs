//! Trust-on-first-use host-key verification with a 180-day re-confirmation timer.
//!
//! Pins live in the `sshHostKeys` list of the configuration store. That store is not encrypted
//! yet; Workstream 4 moves it into the encrypted store. Every approval and deletion is written
//! to the `audit_events` table of the application database; if that write fails, the call
//! returns an error.

use super::error::TunnelError;
use crate::common::types::{
    HostKeyChallenge, HostKeyChallengeKind, SshHostKeyPin, HOST_KEY_RECONFIRM_AFTER_DAYS,
};
use crate::core::appdb::{AppDb, AuditActor, AuditEvent, AuditKind};
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

fn audit(db: Option<&AppDb>, event: AuditEvent) -> Result<(), TunnelError> {
    let result = match db {
        Some(db) => db.record_audit(&event),
        None => crate::core::appdb::audit::record_to_default_db(&event),
    };
    result.map_err(|e| TunnelError::PinStore(format!("audit log write failed: {e}")))
}

/// Persists the local user's approval (or re-confirmation) of `challenge` and audits it.
pub fn approve_host_key(challenge: &HostKeyChallenge) -> Result<(), TunnelError> {
    approve_host_key_inner(challenge, None, &AuditActor::local())
}

/// Persists `actor`'s approval of `challenge`, auditing it in `db`.
pub fn approve_host_key_audited(
    challenge: &HostKeyChallenge,
    db: &AppDb,
    actor: &AuditActor,
) -> Result<(), TunnelError> {
    approve_host_key_inner(challenge, Some(db), actor)
}

fn approve_host_key_inner(
    challenge: &HostKeyChallenge,
    db: Option<&AppDb>,
    actor: &AuditActor,
) -> Result<(), TunnelError> {
    let mut file = config::get_sync().map_err(TunnelError::PinStore)?;
    apply_approval(
        &mut file.ssh_host_keys,
        challenge,
        OffsetDateTime::now_utc(),
    )?;
    config::save_sync(&file).map_err(TunnelError::PinStore)?;
    let detail = format!(
        "{:?} {}:{} {} {}",
        challenge.kind,
        challenge.host,
        challenge.port,
        challenge.key_algorithm,
        challenge.fingerprint_sha256
    );
    audit(
        db,
        AuditEvent::new(AuditKind::HostKeyApproved, actor).detail(detail),
    )
}

/// Deletes the pin for `host:port` on behalf of the local user. Returns whether one was removed.
pub fn delete_host_key(host: &str, port: u32) -> Result<bool, TunnelError> {
    delete_host_key_inner(host, port, None, &AuditActor::local())
}

/// Deletes the pin for `host:port` on behalf of `actor`, auditing it in `db`.
pub fn delete_host_key_audited(
    host: &str,
    port: u32,
    db: &AppDb,
    actor: &AuditActor,
) -> Result<bool, TunnelError> {
    delete_host_key_inner(host, port, Some(db), actor)
}

fn delete_host_key_inner(
    host: &str,
    port: u32,
    db: Option<&AppDb>,
    actor: &AuditActor,
) -> Result<bool, TunnelError> {
    let mut file = config::get_sync().map_err(TunnelError::PinStore)?;
    let before = file.ssh_host_keys.len();
    file.ssh_host_keys.retain(|p| !same_endpoint(p, host, port));
    let removed = file.ssh_host_keys.len() != before;
    if removed {
        config::save_sync(&file).map_err(TunnelError::PinStore)?;
        let event =
            AuditEvent::new(AuditKind::HostKeyDeleted, actor).detail(format!("{host}:{port}"));
        audit(db, event)?;
    }
    Ok(removed)
}

#[cfg(test)]
#[path = "host_keys_tests.rs"]
mod tests;
