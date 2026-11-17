//! Unit tests for the four host-key states and the approval rules.

use super::*;
use time::OffsetDateTime;

fn presented(fp: &str) -> PresentedHostKey {
    PresentedHostKey {
        host: "bastion.example".into(),
        port: 22,
        key_algorithm: "ssh-ed25519".into(),
        fingerprint_sha256: fp.into(),
    }
}

fn pin(fp: &str, approved_at: OffsetDateTime) -> SshHostKeyPin {
    SshHostKeyPin {
        host: "bastion.example".into(),
        port: 22,
        key_algorithm: "ssh-ed25519".into(),
        fingerprint_sha256: fp.into(),
        approved_at,
    }
}

#[test]
fn unknown_host_returns_host_key_unknown_with_fingerprint() {
    let now = OffsetDateTime::now_utc();
    let p = presented("SHA256:aaa");
    let status = evaluate(&[], &p, now);
    assert_eq!(status, HostKeyStatus::Unknown);
    match status_to_result(status, &p) {
        Err(TunnelError::HostKeyUnknown(c)) => {
            assert_eq!(c.fingerprint_sha256, "SHA256:aaa");
            assert_eq!(c.kind, HostKeyChallengeKind::Unknown);
        }
        other => panic!("expected HostKeyUnknown, got {other:?}"),
    }
}

#[test]
fn pin_for_other_port_is_not_used() {
    let now = OffsetDateTime::now_utc();
    let mut other = pin("SHA256:aaa", now);
    other.port = 2222;
    assert_eq!(
        evaluate(&[other], &presented("SHA256:aaa"), now),
        HostKeyStatus::Unknown
    );
}

#[test]
fn matching_recent_pin_is_trusted() {
    let now = OffsetDateTime::now_utc();
    let pins = [pin("SHA256:aaa", now - Duration::days(179))];
    let p = presented("SHA256:aaa");
    assert_eq!(evaluate(&pins, &p, now), HostKeyStatus::Trusted);
    assert!(status_to_result(HostKeyStatus::Trusted, &p).is_ok());
}

#[test]
fn pin_older_than_180_days_requires_reconfirmation() {
    let now = OffsetDateTime::now_utc();
    let pins = [pin("SHA256:aaa", now - Duration::days(181))];
    let p = presented("SHA256:aaa");
    let status = evaluate(&pins, &p, now);
    assert_eq!(status, HostKeyStatus::ReconfirmationRequired);
    assert!(matches!(
        status_to_result(status, &p),
        Err(TunnelError::HostKeyReconfirmationRequired(c)) if c.fingerprint_sha256 == "SHA256:aaa"
    ));
}

#[test]
fn pin_dated_in_the_future_requires_reconfirmation() {
    let now = OffsetDateTime::now_utc();
    let pins = [pin("SHA256:aaa", now + Duration::days(30))];
    assert_eq!(
        evaluate(&pins, &presented("SHA256:aaa"), now),
        HostKeyStatus::ReconfirmationRequired
    );
}

#[test]
fn changed_fingerprint_is_a_hard_failure_even_when_pin_is_old() {
    let now = OffsetDateTime::now_utc();
    for age in [1, 400] {
        let pins = [pin("SHA256:aaa", now - Duration::days(age))];
        let p = presented("SHA256:bbb");
        let status = evaluate(&pins, &p, now);
        assert!(matches!(status, HostKeyStatus::Changed { .. }));
        let err = status_to_result(status, &p).unwrap_err();
        assert!(matches!(err, TunnelError::HostKeyChanged { .. }));
        assert!(
            err.host_key_challenge().is_none(),
            "changed key must not be approvable"
        );
        assert!(err.to_string().contains("SHA256:aaa") && err.to_string().contains("SHA256:bbb"));
    }
}

#[test]
fn approval_inserts_then_reconfirmation_resets_timer() {
    let old = OffsetDateTime::now_utc() - Duration::days(200);
    let mut pins = Vec::new();
    let challenge = HostKeyChallenge {
        kind: HostKeyChallengeKind::Unknown,
        host: "bastion.example".into(),
        port: 22,
        key_algorithm: "ssh-ed25519".into(),
        fingerprint_sha256: "SHA256:aaa".into(),
    };
    apply_approval(&mut pins, &challenge, old).unwrap();
    assert_eq!(pins.len(), 1);
    let now = OffsetDateTime::now_utc();
    assert_eq!(
        evaluate(&pins, &presented("SHA256:aaa"), now),
        HostKeyStatus::ReconfirmationRequired
    );
    apply_approval(&mut pins, &challenge, now).unwrap();
    assert_eq!(pins.len(), 1);
    assert_eq!(
        evaluate(&pins, &presented("SHA256:aaa"), now),
        HostKeyStatus::Trusted
    );
}

#[test]
fn approval_cannot_override_a_changed_key() {
    let now = OffsetDateTime::now_utc();
    let mut pins = vec![pin("SHA256:aaa", now)];
    let challenge = HostKeyChallenge {
        kind: HostKeyChallengeKind::Unknown,
        host: "BASTION.example".into(),
        port: 22,
        key_algorithm: "ssh-ed25519".into(),
        fingerprint_sha256: "SHA256:evil".into(),
    };
    assert!(matches!(
        apply_approval(&mut pins, &challenge, now),
        Err(TunnelError::HostKeyChanged { .. })
    ));
    assert_eq!(pins[0].fingerprint_sha256, "SHA256:aaa");
}
