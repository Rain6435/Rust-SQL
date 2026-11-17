//! Unit tests for [`config_tunnel`] validation and secret handling.

use super::*;
use crate::common::types::SshConfig;

fn server_with(ssh: SshConfig) -> Server {
    Server {
        host: Some("db.internal".into()),
        port: Some(5432),
        ssh: Some(ssh),
        ..Default::default()
    }
}

fn base_ssh() -> SshConfig {
    SshConfig {
        host: "10.0.0.1".into(),
        port: 22,
        user: "root".into(),
        ..Default::default()
    }
}

#[test]
fn test_config_tunnel_validation() {
    let msg = |s: &Server| config_tunnel(s).unwrap_err().to_string();
    assert!(msg(&Server::default()).contains("Host and port not specified for tunnel"));
    let missing_ssh = Server {
        host: Some("localhost".into()),
        port: Some(5432),
        ..Default::default()
    };
    assert!(msg(&missing_ssh).contains("SSH information not specified"));
    assert!(msg(&server_with(base_ssh())).contains("No SSH authentication method configured"));
}

#[test]
fn password_is_selected_and_never_printed() {
    let mut ssh = base_ssh();
    ssh.password = Some(Password::plain("s3cret-pw"));
    let cfg = config_tunnel(&server_with(ssh)).unwrap();
    assert_eq!(cfg.auth_method(), Some(SshAuthMethod::Password));
    assert_eq!(
        cfg.password.as_deref().map(String::as_str),
        Some("s3cret-pw")
    );
    assert!(!format!("{cfg:?}").contains("s3cret-pw"));
}

#[test]
fn private_key_takes_priority_over_password() {
    let dir = tempfile::tempdir().unwrap();
    let key_path = dir.path().join("id_test");
    std::fs::write(&key_path, "KEY MATERIAL THAT IS SECRET").unwrap();
    let mut ssh = base_ssh();
    ssh.password = Some(Password::plain("pw"));
    ssh.private_key = Some(key_path.to_string_lossy().into_owned());
    ssh.passphrase = Some("phrase".into());
    let cfg = config_tunnel(&server_with(ssh)).unwrap();
    assert_eq!(cfg.auth_method(), Some(SshAuthMethod::PrivateKey));
    assert!(cfg.password.is_none(), "only one method may be configured");
    let dbg = format!("{cfg:?}");
    assert!(!dbg.contains("KEY MATERIAL") && !dbg.contains("phrase\""));
}

#[test]
fn agent_requires_socket() {
    let mut ssh = base_ssh();
    ssh.use_agent = Some(true);
    ssh.password = Some(Password::plain("pw"));
    let prev = std::env::var("SSH_AUTH_SOCK").ok();
    std::env::remove_var("SSH_AUTH_SOCK");
    let res = config_tunnel(&server_with(ssh.clone()));
    if cfg!(not(windows)) {
        assert!(res.unwrap_err().to_string().contains("SSH_AUTH_SOCK"));
    }
    std::env::set_var("SSH_AUTH_SOCK", "/tmp/agent.sock");
    let cfg = config_tunnel(&server_with(ssh)).unwrap();
    assert_eq!(cfg.auth_method(), Some(SshAuthMethod::Agent));
    assert!(!format!("{cfg:?}").contains("/tmp/agent.sock"));
    match prev {
        Some(v) => std::env::set_var("SSH_AUTH_SOCK", v),
        None => std::env::remove_var("SSH_AUTH_SOCK"),
    }
}
