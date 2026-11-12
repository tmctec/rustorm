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

/// A fixture home for the Combine and Cleanup pty run: `legacy` conflicts
/// with the root on nas and vps; the root holds a dead `lab`; `box` in
/// `config.d/box` points at a closed port and its stray copy `box.bak` at
/// `open`, a live listener.
fn flows_home(open: u16, closed: u16) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let w = |rel: &str, text: String| {
        let p = dir.path().join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    };
    w(
        ".ssh/config",
        format!("Include ~/.ssh/config.d/*\n\nHost nas\n    HostName nas.local\n\nHost vps\n    HostName vps.example.com\n    User root\n\nHost lab\n    HostName 127.0.0.1\n    Port {open}\n    User labuser\n\nHost lab\n    HostName 127.0.0.1\n"),
    );
    w(
        ".ssh/config.d/legacy",
        "Host vps\n    HostName old-vps.example.com\n    User admin\n\nHost nas\n    HostName 192.168.1.10\n\nHost printer\n    HostName 192.168.1.20\n".into(),
    );
    w(
        ".ssh/config.d/box",
        format!("Host box\n    HostName 127.0.0.1\n    Port {closed}\n"),
    );
    w(
        ".ssh/config.d/box.bak",
        format!("Host box\n    HostName 127.0.0.1\n    Port {open}\n    User tester\n"),
    );
    dir
}

/// Every file under `root` with its bytes, by relative path.
fn tree(root: &std::path::Path) -> std::collections::BTreeMap<std::path::PathBuf, Vec<u8>> {
    fn walk(
        root: &std::path::Path,
        dir: &std::path::Path,
        out: &mut std::collections::BTreeMap<std::path::PathBuf, Vec<u8>>,
    ) {
        for e in std::fs::read_dir(dir).unwrap() {
            let p = e.unwrap().path();
            if p.is_dir() {
                walk(root, &p, out);
            } else {
                out.insert(
                    p.strip_prefix(root).unwrap().to_path_buf(),
                    std::fs::read(&p).unwrap(),
                );
            }
        }
    }
    let mut out = std::collections::BTreeMap::new();
    walk(root, root, &mut out);
    out
}

/// uicmb-3, uicmb-7, uicln-3 on a real pseudo-terminal, driven by
/// tests/pty_flows.exp: Combine and Cleanup leave the files the CLI leaves.
/// The CLI is the `rustorm` binary beside `rustorm-tui` when it is built,
/// else the core calls it makes.
#[test]
fn ui_flows_pty_combine_and_cleanup_match_cli() {
    if Command::new("expect").arg("-v").output().is_err() {
        eprintln!("expect not installed; skipping the pty run");
        return;
    }
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let open = listener.local_addr().unwrap().port();
    let closed = {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        l.local_addr().unwrap().port()
    };
    let tui = flows_home(open, closed);
    let cli = flows_home(open, closed);
    let out = Command::new("expect")
        .arg("-f")
        .arg(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/pty_flows.exp"))
        .arg(env!("CARGO_BIN_EXE_rustorm-tui"))
        .arg(tui.path())
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&out.stdout);
    let result = text.lines().find(|l| l.starts_with("RESULT")).unwrap_or("");
    assert!(out.status.success(), "pty run failed: {result}\n{text}");
    let bin = std::path::Path::new(env!("CARGO_BIN_EXE_rustorm-tui")).with_file_name("rustorm");
    let home = cli.path();
    if bin.is_file() {
        let run = |args: &[&str]| {
            let o = Command::new(&bin)
                .args(args)
                .env("HOME", home)
                .env("USER", "tester")
                .env_remove("RUSTORM_CONFIG")
                .stdin(std::process::Stdio::null())
                .output()
                .unwrap();
            String::from_utf8_lossy(&o.stdout).to_string() + &String::from_utf8_lossy(&o.stderr)
        };
        let root = home.join(".ssh/config");
        let legacy = home.join(".ssh/config.d/legacy");
        let combined = run(&[
            "combine",
            root.to_str().unwrap(),
            legacy.to_str().unwrap(),
            "--keep",
            "vps",
            "--replace",
            "nas",
            "--retire",
        ]);
        assert!(combined.contains("retired to"), "{combined}");
        let cleaned = run(&["cleanup", "--apply", "--timeout", "2"]);
        assert!(cleaned.contains("no findings remain."), "{cleaned}");
    } else {
        use rustorm_core::{CombineRequest, HostDecision, TcpProber, Workspace, WriteOptions};
        let request = CombineRequest {
            inputs: vec![home.join(".ssh/config"), home.join(".ssh/config.d/legacy")],
            decisions: [
                ("vps".to_string(), HostDecision::Keep),
                ("nas".to_string(), HostDecision::Replace),
            ]
            .into_iter()
            .collect(),
            retire: true,
            ..CombineRequest::default()
        };
        request
            .commit(None, WriteOptions::default(), Some(home))
            .unwrap();
        let env = rustorm_core::Env {
            user: Some("tester".into()),
            home: Some(home.to_path_buf()),
        };
        let load = || Workspace::load_with_home(home.join(".ssh/config"), Some(home)).unwrap();
        let mut report = load().cleanup_report(None, &env);
        report.probe(
            &TcpProber,
            std::time::Duration::from_secs(2),
            8,
            None,
            &mut |_, _| {},
        );
        let mut ws = load();
        ws.apply_cleanup(&report, &report.applicable()).unwrap();
        ws.save(WriteOptions::default()).unwrap();
    }
    drop(listener);
    assert_eq!(
        tree(tui.path()),
        tree(cli.path()),
        "the TUI leaves the CLI's files"
    );
    assert!(tui.path().join(".ssh/retired/legacy").is_file());
}
