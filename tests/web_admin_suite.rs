//! Admin account management over IPC: password policy, no hash in responses, last-admin
//! guard, and audit rows for every account change.

mod support;

use rust_sql::common::event::*;
use rust_sql::web::WebIpcState;
use serde_json::json;
use support::TestApp;

#[tokio::test]
async fn admins_manage_users_and_the_changes_are_audited() {
    let _g = support::lock().await;
    let t = TestApp::new();
    let admin = t.user("root", true);
    let s = WebIpcState::new(4);
    let short = t
        .ipc_as(
            &admin,
            &s,
            USERS_CREATE,
            vec![json!("eve"), json!("too short")],
        )
        .await;
    assert!(short["error"].as_str().unwrap().contains("at least 12"));
    let made = t
        .ipc_as(
            &admin,
            &s,
            USERS_CREATE,
            vec![json!("eve"), json!("a long enough password")],
        )
        .await;
    let eve_id = made["data"]["id"].as_i64().unwrap();
    assert!(made["data"].get("passwordHash").is_none());
    t.ipc_as(
        &admin,
        &s,
        USERS_SET_PASSWORD,
        vec![json!(eve_id), json!("another long password")],
    )
    .await;
    t.ipc_as(&admin, &s, USERS_DELETE, vec![json!(eve_id)])
        .await;
    let self_delete = t
        .ipc_as(&admin, &s, USERS_DELETE, vec![json!(admin.id)])
        .await;
    assert!(self_delete["error"]
        .as_str()
        .unwrap()
        .contains("last administrator"));
    let kinds: Vec<_> = t
        .app
        .db
        .list_audit(10)
        .unwrap()
        .into_iter()
        .map(|a| a.event_type)
        .collect();
    for k in ["user_created", "user_updated", "user_deleted"] {
        assert!(kinds.iter().any(|x| x == k), "{k} missing: {kinds:?}");
    }
}

fn run_cli(home: &std::path::Path, args: &[&str], stdin: &str) -> std::process::Output {
    use std::io::Write;
    let mut child = std::process::Command::new(env!("CARGO_BIN_EXE_Rust-SQL"))
        .args(args)
        .env("RUST_SQL_HOME", home)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("spawn Rust-SQL");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(stdin.as_bytes())
        .unwrap();
    child.wait_with_output().unwrap()
}

#[test]
fn users_create_admin_bootstraps_the_first_administrator() {
    let home = tempfile::TempDir::new().unwrap();
    let create = [
        "users",
        "create",
        "--admin",
        "--username",
        "root",
        "--password-stdin",
    ];
    let short = run_cli(home.path(), &create, "too-short\n");
    assert!(!short.status.success());
    assert!(String::from_utf8_lossy(&short.stderr).contains("at least 12"));

    let ok = run_cli(home.path(), &create, "a sufficiently long password\n");
    assert!(
        ok.status.success(),
        "{}",
        String::from_utf8_lossy(&ok.stderr)
    );
    let listed = run_cli(home.path(), &["users", "list"], "");
    let out = String::from_utf8_lossy(&listed.stdout);
    assert!(out.contains("root\tadmin"), "{out}");

    let db = rust_sql::core::appdb::AppDb::open(&home.path().join("rust-sql.db")).unwrap();
    let user = db
        .verify_login("root", "a sufficiently long password")
        .unwrap()
        .expect("login works");
    assert!(user.is_admin);
    assert_eq!(db.list_audit(5).unwrap()[0].event_type, "user_created");
}
