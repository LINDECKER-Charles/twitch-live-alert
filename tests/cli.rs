use std::{
    path::Path,
    process::{Command, Output},
};
use tempfile::TempDir;

fn cli(dir: &Path, args: &[&str], vars: &[(&str, &str)]) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_alert-bloodborne-fr-live"));
    command.current_dir(dir).args(args);
    for key in [
        "TWITCH_CLIENT_ID",
        "TWITCH_CLIENT_SECRET",
        "POLL_INTERVAL_SECONDS",
        "DATABASE_PATH",
        "NOTIFY_EXISTING_ON_FIRST_RUN",
        "RUST_LOG",
    ] {
        command.env_remove(key);
    }
    for (key, value) in vars {
        command.env(key, value);
    }
    command.output().unwrap()
}

#[test]
fn help_and_conflicting_modes_need_no_credentials() {
    let dir = TempDir::new().unwrap();
    let output = cli(dir.path(), &["--help"], &[]);
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("--test-notification"));
    let output = cli(dir.path(), &["--once", "--dry-run"], &[]);
    assert!(!output.status.success());
    assert!(!dir.path().join("data").exists());
}

#[test]
fn missing_credentials_fail_clearly_without_creating_state() {
    let dir = TempDir::new().unwrap();
    let output = cli(dir.path(), &["--once"], &[]);
    assert!(!output.status.success());
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(text.contains("TWITCH_CLIENT_ID"));
    assert!(!dir.path().join("data").exists());
}

#[test]
fn configuration_check_is_offline_and_does_not_echo_secrets() {
    let dir = TempDir::new().unwrap();
    let output = cli(
        dir.path(),
        &["--check-config"],
        &[
            ("TWITCH_CLIENT_ID", "test-client"),
            ("TWITCH_CLIENT_SECRET", "private-secret"),
        ],
    );
    assert!(output.status.success());
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!text.contains("private-secret"));
    assert!(!dir.path().join("data").exists());
}
