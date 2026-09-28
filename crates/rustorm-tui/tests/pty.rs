//! ui-1 on a real pseudo-terminal, driven by tests/pty.exp through
//! `script -q /dev/null`. Skipped when `expect` is not installed.

mod common;
use common::*;
use std::process::Command;

#[test]
fn ui_1_pty_quit_exits_zero_and_restores_terminal() {
    if Command::new("expect").arg("-v").output().is_err() {
        eprintln!("expect not installed; skipping the pty run");
        return;
    }
    let f = Fixture::new(&three_hosts());
    let out = Command::new("expect")
        .arg("-f")
        .arg(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/pty.exp"))
        .arg(env!("CARGO_BIN_EXE_rustorm-tui"))
        .arg(&f.path)
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&out.stdout);
    let result = text.lines().find(|l| l.starts_with("RESULT")).unwrap_or("");
    assert!(out.status.success(), "pty run failed: {result}\n{text}");
    assert!(result.contains("exit=0") && result.contains("terminal_restored=1"));
    assert_eq!(f.read(), three_hosts(), "quitting writes nothing");
}
