//! SSH bastion host-key pins (trust on first use) and the approval challenges shown to users.
//!
//! One [`SshHostKeyPin`] is stored per bastion `host:port`. A pin is approved by a person after
//! comparing the fingerprint out of band, and must be re-confirmed after
//! [`HOST_KEY_RECONFIRM_AFTER_DAYS`] days. A changed fingerprint is never overridable in-app.
//!
//! On disk the whole pin list is one encrypted value ([`StoredHostKeyPins::Encrypted`]); see
//! [`crate::core::tunnel::host_keys`] for the format.

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

/// Number of days after which an approved host key must be re-confirmed by the user.
pub const HOST_KEY_RECONFIRM_AFTER_DAYS: i64 = 180;

/// The `sshHostKeys` value of `rust-sql.json`.
///
/// JSON form: an object `{"ciphertext": "v2:..."}` is the encrypted pin list. An array is the
/// plaintext list written by builds before pins were encrypted; it is accepted on read only so
/// it can be reported and discarded, and it is never written back (there is no migration).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum StoredHostKeyPins {
    /// The encrypted, JSON-serialized `Vec<SshHostKeyPin>`.
    Encrypted(super::EncryptedPassword),
    /// A plaintext pin list from an older build. Never serialized.
    LegacyPlaintext(Vec<serde_json::Value>),
}

/// Error shown when the configuration still holds plaintext pins from an older build.
pub const LEGACY_HOST_KEY_PINS_ERROR: &str = "rust-sql.json holds SSH host-key pins in the \
     removed plaintext format; there is no migration. They are discarded the next time \
     Rust-SQL starts, and each bastion's fingerprint must then be approved again";

impl StoredHostKeyPins {
    /// `serde` helper: only the encrypted form is ever written to disk.
    pub fn skip_serializing(value: &Option<Self>) -> bool {
        !matches!(value, Some(Self::Encrypted(_)))
    }
}

/// A trusted SSH bastion host key, persisted (encrypted) in the configuration store.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SshHostKeyPin {
    /// Bastion hostname or IP address exactly as configured.
    pub host: String,
    /// Bastion SSH port.
    pub port: u32,
    /// SSH key algorithm name, such as `ssh-ed25519`.
    pub key_algorithm: String,
    /// OpenSSH-style SHA-256 fingerprint, such as `SHA256:abc...`.
    pub fingerprint_sha256: String,
    /// When the user last approved or re-confirmed this key (UTC, RFC 3339).
    #[serde(with = "time::serde::rfc3339")]
    pub approved_at: OffsetDateTime,
}

/// Why the user is being asked to approve a host key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum HostKeyChallengeKind {
    /// The bastion has never been seen before.
    Unknown,
    /// The key matches the pin, but the pin is older than the re-confirmation period.
    ReconfirmationRequired,
}

/// A host key the user must approve before the tunnel connects.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HostKeyChallenge {
    /// Whether this is a first-use approval or a periodic re-confirmation.
    pub kind: HostKeyChallengeKind,
    /// Bastion hostname or IP address.
    pub host: String,
    /// Bastion SSH port.
    pub port: u32,
    /// SSH key algorithm name presented by the server.
    pub key_algorithm: String,
    /// SHA-256 fingerprint presented by the server.
    pub fingerprint_sha256: String,
}

impl HostKeyChallenge {
    /// Returns a human-readable prompt asking the user to verify the fingerprint out of band.
    pub fn prompt_text(&self) -> String {
        let lead = match self.kind {
            HostKeyChallengeKind::Unknown => "The SSH bastion's host key is not yet trusted.",
            HostKeyChallengeKind::ReconfirmationRequired => {
                "The SSH bastion's host key was approved more than 180 days ago and must be \
                 re-confirmed."
            }
        };
        format!(
            "{lead} Host {}:{} presented a {} key with fingerprint {}. Approve it only if \
             this matches the fingerprint your administrator gave you.",
            self.host, self.port, self.key_algorithm, self.fingerprint_sha256
        )
    }
}
