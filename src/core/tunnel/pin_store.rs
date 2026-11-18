//! Encrypted persistence of SSH host-key pins.
//!
//! The whole pin list is serialized to JSON and encrypted as one value with
//! [`crypto::encrypt_for`] under [`Purpose::HOST_KEY_PINS`]: XChaCha20-Poly1305 with a subkey
//! derived by HKDF-SHA256 from the OS keychain master key (its own HKDF info string, so it never
//! shares a subkey with saved passwords), bound by associated data to [`PIN_STORE_RECORD`] and
//! [`PIN_STORE_FIELD`]. It is stored as `"sshHostKeys": {"ciphertext": "v2:..."}` in
//! `rust-sql.json`.
//!
//! Encrypting the list as a unit hides which bastions are trusted and means a pin cannot be
//! edited, added, or swapped in without the key. Deleting the value, or replacing it with an
//! older ciphertext, is not detectable; both fall back to the first-use prompt or an older
//! approval (see `docs/security/threat-model.md`).
//!
//! The keychain is mandatory: without the master key, pins can be neither read nor written, so
//! SSH tunnels fail with [`TunnelError::PinStore`] instead of skipping verification. Every
//! approval and deletion is audited; if the audit write fails, the call returns an error.

use super::error::TunnelError;
use super::host_keys::{apply_approval, same_endpoint};
use crate::common::types::{
    EncryptedPassword, HostKeyChallenge, SshHostKeyPin, StoredHostKeyPins,
    LEGACY_HOST_KEY_PINS_ERROR,
};
use crate::core::appdb::{AppDb, AuditActor, AuditEvent, AuditKind};
use crate::core::config;
use crate::core::crypto::{self, Purpose};
use crate::core::keys::KeyProvider;
use time::OffsetDateTime;
use zeroize::Zeroizing;

/// Record ID bound into the pin list's associated data.
pub const PIN_STORE_RECORD: &str = "sshHostKeys";
/// Field name bound into the pin list's associated data.
pub const PIN_STORE_FIELD: &str = "pins";

fn store_error(e: impl std::fmt::Display) -> TunnelError {
    TunnelError::PinStore(format!("SSH host-key pins: {e}"))
}

/// Encrypts `pins` into the on-disk form. An empty list is stored as nothing.
pub fn encrypt_pins(
    pins: &[SshHostKeyPin],
    keys: &dyn KeyProvider,
) -> Result<Option<StoredHostKeyPins>, TunnelError> {
    if pins.is_empty() {
        return Ok(None);
    }
    let key = keys.master_key().map_err(store_error)?;
    let json = Zeroizing::new(serde_json::to_string(pins).map_err(store_error)?);
    let enc = crypto::encrypt_for(
        Purpose::HOST_KEY_PINS,
        &json,
        &key,
        PIN_STORE_RECORD,
        PIN_STORE_FIELD,
    )
    .map_err(store_error)?;
    Ok(Some(StoredHostKeyPins::Encrypted(enc)))
}

/// Decrypts the on-disk pin list. Absent means no pins; a legacy plaintext list is an error.
///
/// The master key is requested only when there is something to decrypt.
pub fn decrypt_pins(
    stored: Option<&StoredHostKeyPins>,
    keys: &dyn KeyProvider,
) -> Result<Vec<SshHostKeyPin>, TunnelError> {
    match stored {
        None => Ok(Vec::new()),
        Some(StoredHostKeyPins::LegacyPlaintext(_)) => Err(store_error(LEGACY_HOST_KEY_PINS_ERROR)),
        Some(StoredHostKeyPins::Encrypted(enc)) => decrypt_list(enc, keys),
    }
}

fn decrypt_list(
    enc: &EncryptedPassword,
    keys: &dyn KeyProvider,
) -> Result<Vec<SshHostKeyPin>, TunnelError> {
    let key = keys.master_key().map_err(store_error)?;
    let plain = crypto::decrypt_for(
        Purpose::HOST_KEY_PINS,
        enc,
        &key,
        PIN_STORE_RECORD,
        PIN_STORE_FIELD,
    )
    .map_err(store_error)?;
    serde_json::from_str(&plain).map_err(store_error)
}

/// Loads and decrypts all host-key pins from the configuration store.
pub fn load_pins(keys: &dyn KeyProvider) -> Result<Vec<SshHostKeyPin>, TunnelError> {
    let file = config::get_sync().map_err(TunnelError::PinStore)?;
    decrypt_pins(file.ssh_host_keys.as_ref(), keys)
}

/// Reads the pins for a change. A legacy plaintext list is replaced rather than kept, so the
/// first write after an upgrade removes the plaintext from disk.
fn pins_for_update(
    stored: Option<&StoredHostKeyPins>,
    keys: &dyn KeyProvider,
) -> Result<Vec<SshHostKeyPin>, TunnelError> {
    match stored {
        Some(StoredHostKeyPins::LegacyPlaintext(_)) => Ok(Vec::new()),
        other => decrypt_pins(other, keys),
    }
}

fn audit(db: Option<&AppDb>, event: AuditEvent) -> Result<(), TunnelError> {
    let result = match db {
        Some(db) => db.record_audit(&event),
        None => crate::core::appdb::audit::record_to_default_db(&event),
    };
    result.map_err(|e| TunnelError::PinStore(format!("audit log write failed: {e}")))
}

/// Persists the local user's approval (or re-confirmation) of `challenge` and audits it.
pub fn approve_host_key(
    challenge: &HostKeyChallenge,
    keys: &dyn KeyProvider,
) -> Result<(), TunnelError> {
    approve_host_key_inner(challenge, keys, None, &AuditActor::local())
}

/// Persists `actor`'s approval of `challenge`, auditing it in `db`.
pub fn approve_host_key_audited(
    challenge: &HostKeyChallenge,
    keys: &dyn KeyProvider,
    db: &AppDb,
    actor: &AuditActor,
) -> Result<(), TunnelError> {
    approve_host_key_inner(challenge, keys, Some(db), actor)
}

fn approve_host_key_inner(
    challenge: &HostKeyChallenge,
    keys: &dyn KeyProvider,
    db: Option<&AppDb>,
    actor: &AuditActor,
) -> Result<(), TunnelError> {
    let mut file = config::get_sync().map_err(TunnelError::PinStore)?;
    let mut pins = pins_for_update(file.ssh_host_keys.as_ref(), keys)?;
    apply_approval(&mut pins, challenge, OffsetDateTime::now_utc())?;
    file.ssh_host_keys = encrypt_pins(&pins, keys)?;
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
pub fn delete_host_key(host: &str, port: u32, keys: &dyn KeyProvider) -> Result<bool, TunnelError> {
    delete_host_key_inner(host, port, keys, None, &AuditActor::local())
}

/// Deletes the pin for `host:port` on behalf of `actor`, auditing it in `db`.
pub fn delete_host_key_audited(
    host: &str,
    port: u32,
    keys: &dyn KeyProvider,
    db: &AppDb,
    actor: &AuditActor,
) -> Result<bool, TunnelError> {
    delete_host_key_inner(host, port, keys, Some(db), actor)
}

fn delete_host_key_inner(
    host: &str,
    port: u32,
    keys: &dyn KeyProvider,
    db: Option<&AppDb>,
    actor: &AuditActor,
) -> Result<bool, TunnelError> {
    let mut file = config::get_sync().map_err(TunnelError::PinStore)?;
    let mut pins = decrypt_pins(file.ssh_host_keys.as_ref(), keys)?;
    let before = pins.len();
    pins.retain(|p| !same_endpoint(p, host, port));
    let removed = pins.len() != before;
    if removed {
        file.ssh_host_keys = encrypt_pins(&pins, keys)?;
        config::save_sync(&file).map_err(TunnelError::PinStore)?;
        let event =
            AuditEvent::new(AuditKind::HostKeyDeleted, actor).detail(format!("{host}:{port}"));
        audit(db, event)?;
    }
    Ok(removed)
}

#[cfg(test)]
#[path = "pin_store_tests.rs"]
mod tests;
