//! `Rust-SQL users ...`: bootstrap and manage web UI accounts from the command line.
//!
//! `users create --admin` creates the first administrator. The password is read from the
//! terminal without echo (or from one line of standard input with `--password-stdin`) and
//! is never accepted as a command-line argument, where it would land in shell history.

use rust_sql::core::appdb::{AppDb, AuditActor, AuditEvent, AuditKind, MIN_PASSWORD_LEN};
use std::io::BufRead;

const USAGE: &str = "Usage:
  Rust-SQL users create [--admin] [--username <NAME>] [--password-stdin]
  Rust-SQL users list";

fn flag_value(args: &[String], flag: &str) -> Option<String> {
    args.iter()
        .position(|a| a == flag)
        .and_then(|i| args.get(i + 1).cloned())
}

fn read_line(prompt: &str) -> Result<String, String> {
    eprint!("{prompt}");
    let mut line = String::new();
    std::io::stdin()
        .lock()
        .read_line(&mut line)
        .map_err(|e| e.to_string())?;
    Ok(line.trim_end_matches(['\r', '\n']).to_string())
}

fn read_password(from_stdin: bool) -> Result<String, String> {
    if from_stdin {
        return read_line("");
    }
    let first = rpassword::prompt_password(format!(
        "Password (at least {MIN_PASSWORD_LEN} characters): "
    ))
    .map_err(|e| e.to_string())?;
    let second = rpassword::prompt_password("Repeat password: ").map_err(|e| e.to_string())?;
    if first != second {
        return Err("Passwords do not match".into());
    }
    Ok(first)
}

fn create(args: &[String]) -> Result<(), String> {
    let is_admin = args.iter().any(|a| a == "--admin");
    let username = match flag_value(args, "--username") {
        Some(u) => u,
        None => read_line("Username: ")?,
    };
    let password = read_password(args.iter().any(|a| a == "--password-stdin"))?;
    let db = AppDb::open_default().map_err(|e| e.to_string())?;
    let user = db
        .create_user(&username, &password, is_admin)
        .map_err(|e| e.to_string())?;
    let event = AuditEvent::new(AuditKind::UserCreated, &AuditActor::local())
        .detail(format!("user {} admin={is_admin} (command line)", user.id));
    db.record_audit(&event).map_err(|e| e.to_string())?;
    let role = if is_admin { "administrator" } else { "user" };
    println!(
        "Created {role} '{}' in {}",
        user.username,
        db.path().display()
    );
    Ok(())
}

fn list() -> Result<(), String> {
    let db = AppDb::open_default().map_err(|e| e.to_string())?;
    for u in db.list_users().map_err(|e| e.to_string())? {
        let role = if u.is_admin { "admin" } else { "user" };
        println!("{}\t{}\t{}\t{}", u.id, u.username, role, u.created_at);
    }
    Ok(())
}

/// Runs `users <subcommand>`; `args` starts after `users`.
pub(crate) fn run_users_cli(args: &[String]) -> Result<(), String> {
    match args.first().map(String::as_str) {
        Some("create") => create(&args[1..]),
        Some("list") => list(),
        _ => Err(USAGE.to_string()),
    }
}
