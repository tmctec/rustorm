//! The Cleanup view (`X`): every duplicate host as `rustorm cleanup` finds
//! it, probed off the UI thread with an injected prober, applied and folded
//! through the core (docs/cli.md, cleanup). Catalog cases: uicln-1 ..
//! uicln-6.

mod common;
use common::*;
use crossterm::event::KeyCode;
use rustorm_core::{Env, ProbeResult, ProbeStatus, ProbeTarget, Prober, Workspace, WriteOptions};
use rustorm_tui::App;
use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

const ROOT: &str = "Include ~/.ssh/config.d/*\n\nHost nas\n    HostName 192.168.1.10\n\nHost web\n    HostName web.example.com\n    User deploy\n\nHost web-prod\n    HostName web.example.com\n    User deploy\n\nHost nas\n    HostName 192.168.1.10\n";
const VPS: &str = "Host vps\n    HostName vps.example.com\n    User admin\n";
const VPS_BAK: &str = "Host vps\n    HostName vps.example.com\n    User travis\n";

/// Every status by probe name, the live vps (in `vps`) refused and its
/// copy (in `vps.bak`) working.
#[derive(Default)]
struct Fixed;

impl Prober for Fixed {
    fn probe(&self, t: &ProbeTarget) -> ProbeResult {
        let status = if t.name == "vps" && t.file.ends_with("vps") {
            ProbeStatus::Refused
        } else {
            ProbeStatus::Works
        };
        ProbeResult {
            status,
            detail: None,
        }
    }
}

/// [`Fixed`]'s statuses, each held back until the test releases its name.
#[derive(Default)]
struct Gated {
    open: Mutex<HashSet<String>>,
    cv: Condvar,
}

impl Gated {
    fn release(&self, name: &str) {
        self.open.lock().unwrap().insert(name.to_string());
        self.cv.notify_all();
    }
}

impl Prober for Gated {
    fn probe(&self, t: &ProbeTarget) -> ProbeResult {
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut open = self.open.lock().unwrap();
        while !open.contains(&t.name) && !open.contains("*") && Instant::now() < deadline {
            open = self
                .cv
                .wait_timeout(open, Duration::from_millis(20))
                .unwrap()
                .0;
        }
        Fixed.probe(t)
    }
}

/// A temporary home: `~/.ssh/config` with a dead `nas` and the same-target
/// `web` and `web-prod`, including `~/.ssh/config.d/*` with `vps` and its
/// stray copy `vps.bak`.
struct Home {
    dir: tempfile::TempDir,
}

impl Home {
    fn new() -> Home {
        let home = Home {
            dir: tempfile::tempdir().unwrap(),
        };
        home.write(".ssh/config", ROOT);
        home.write(".ssh/config.d/vps", VPS);
        home.write(".ssh/config.d/vps.bak", VPS_BAK);
        home
    }
    fn path(&self, rel: &str) -> PathBuf {
        self.dir.path().join(rel)
    }
    fn write(&self, rel: &str, text: &str) {
        let p = self.path(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }
    fn env(&self) -> Env {
        Env {
            user: Some("tester".into()),
            home: Some(self.dir.path().to_path_buf()),
        }
    }
    fn app(&self) -> App {
        let mut opts = options();
        opts.env = self.env();
        let mut app = App::with_options(self.path(".ssh/config"), opts).unwrap();
        app.set_prober(Arc::new(Fixed));
        app
    }
    fn load(&self) -> Workspace {
        Workspace::load_with_home(self.path(".ssh/config"), Some(self.dir.path())).unwrap()
    }
    /// Every file under the home with its bytes, by path relative to it.
    fn snapshot(&self) -> BTreeMap<PathBuf, Vec<u8>> {
        fn walk(root: &Path, dir: &Path, out: &mut BTreeMap<PathBuf, Vec<u8>>) {
            for e in std::fs::read_dir(dir).unwrap() {
                let p = e.unwrap().path();
                if p.is_dir() {
                    walk(root, &p, out);
                } else {
                    let rel = p.strip_prefix(root).unwrap().to_path_buf();
                    out.insert(rel, std::fs::read(&p).unwrap());
                }
            }
        }
        let mut out = BTreeMap::new();
        walk(self.dir.path(), self.dir.path(), &mut out);
        out
    }
    /// `rustorm cleanup --apply` through the core calls the CLI makes,
    /// probing with [`Fixed`].
    fn cli_apply(&self) {
        let ws = self.load();
        let mut report = ws.cleanup_report(None, &self.env());
        report.probe(&Fixed, Duration::from_secs(5), 8, None, &mut |_, _| {});
        let chosen = report.applicable();
        let mut ws = self.load();
        ws.apply_cleanup(&report, &chosen).unwrap();
        ws.save(WriteOptions::default()).unwrap();
    }
    /// `rustorm cleanup --fold <host>` through the core calls the CLI
    /// makes.
    fn cli_fold(&self, host: &str) {
        let mut ws = self.load();
        let report = ws.cleanup_report(None, &self.env());
        ws.fold_host(&report, host).unwrap();
        ws.save(WriteOptions::default()).unwrap();
    }
}

/// The Cleanup view's body: each line inside the border without its
/// two-column marker, trailing blank lines dropped.
fn body(app: &mut App) -> String {
    let all = lines(&draw(app));
    let mut out: Vec<String> = Vec::new();
    for l in &all[1..] {
        if l.starts_with('╚') {
            break;
        }
        let inner = l.trim_start_matches('║').trim_end().trim_end_matches('║');
        out.push(
            inner
                .chars()
                .skip(2)
                .collect::<String>()
                .trim_end()
                .to_string(),
        );
    }
    while out.last().is_some_and(String::is_empty) {
        out.pop();
    }
    out.join("\n")
}

/// Ticks until `done` holds, at most 5 s.
fn wait(app: &mut App, done: impl Fn(&mut App) -> bool) {
    let end = Instant::now() + Duration::from_secs(5);
    while !done(app) {
        assert!(Instant::now() < end, "timed out");
        app.tick();
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn probe_all(app: &mut App) {
    press(app, 'p');
    wait(app, |a| !a.is_probing());
}

/// Highlights the finding whose first line starts with `head`.
fn goto(app: &mut App, head: &str) {
    press(app, 'g');
    for _ in 0..10 {
        if app.cleanup_selected().is_some_and(|h| h.starts_with(head)) {
            return;
        }
        app.handle(key(KeyCode::Down));
    }
    panic!("no finding {head}");
}

/// uicln-1
#[test]
fn uicln_1_lists_the_same_findings_as_cleanup_no_probe() {
    let h = Home::new();
    let mut app = h.app();
    press(&mut app, 'X');
    assert_eq!(app.view_name(), Some("cleanup"));
    let want = h.load().cleanup_report(None, &h.env()).text();
    assert!(
        want.contains("same name: vps")
            && want.contains("dead: nas")
            && want.contains("same target: web, web-prod")
    );
    assert_eq!(body(&mut app), want.trim_end());
    let s = screen(&mut app);
    assert!(s.contains("Cleanup  3 findings in 6 hosts."), "{s}");
    assert!(
        s.contains("> same name: vps"),
        "the first finding is highlighted: {s}"
    );
}

/// uicln-2
#[test]
fn uicln_2_probes_off_the_ui_thread_and_statuses_land_one_by_one() {
    let h = Home::new();
    let gate = Arc::new(Gated::default());
    let mut app = h.app();
    app.set_prober(gate.clone());
    press(&mut app, 'X');
    press(&mut app, 'p');
    assert!(app.is_probing());
    assert_eq!(app.message().as_deref(), Some("Probing 6 definitions."));
    let s = screen(&mut app);
    assert!(s.contains("probing 0/6"), "{s}");
    // The UI keeps taking keys while every probe is held.
    app.handle(key(KeyCode::Down));
    assert_eq!(app.cleanup_selected().as_deref(), Some("dead: nas"));
    gate.release("web");
    wait(&mut app, |a| screen(a).contains("probing 1/6"));
    let b = body(&mut app);
    assert!(
        b.lines()
            .any(|l| l.starts_with("  web ") && l.ends_with("works")),
        "web landed: {b}"
    );
    assert!(
        b.lines()
            .any(|l| l.starts_with("  web-prod") && l.ends_with("not probed")),
        "web-prod still waits: {b}"
    );
    assert!(b.contains("recommendation: probe to decide."));
    gate.release("*");
    wait(&mut app, |a| !a.is_probing());
    assert_eq!(app.message().as_deref(), Some("Probed 6 definitions."));
    let b = body(&mut app);
    assert!(
        b.contains("recommendation: take the copy; only the copy works."),
        "{b}"
    );
    assert!(!b.contains("not probed"), "{b}");
}

/// uicln-3
#[test]
fn uicln_3_apply_matches_cli_apply() {
    let tui = Home::new();
    let cli = Home::new();
    let mut app = tui.app();
    press(&mut app, 'X');
    probe_all(&mut app);
    press(&mut app, 'A');
    let msg = app.message().unwrap();
    assert_eq!(
        msg,
        "✔ vps: moved the copy from ~/.ssh/config.d/vps.bak into ~/.ssh/config.d/vps; only the copy works. nas: dropped the dead block at ~/.ssh/config:14. 1 finding remains."
    );
    cli.cli_apply();
    assert_eq!(
        tui.snapshot(),
        cli.snapshot(),
        "files and backups match the CLI's"
    );
    let s = screen(&mut app);
    assert!(s.contains("Cleanup  1 finding in 4 hosts."), "{s}");
}

/// uicln-3, one finding at a time with `a`.
#[test]
fn uicln_3_apply_one_then_the_other_matches_cli_apply() {
    let tui = Home::new();
    let cli = Home::new();
    let mut app = tui.app();
    press(&mut app, 'X');
    probe_all(&mut app);
    goto(&mut app, "dead: nas");
    press(&mut app, 'a');
    assert_eq!(
        app.message().as_deref(),
        Some("✔ nas: dropped the dead block at ~/.ssh/config:14. 2 findings remain.")
    );
    goto(&mut app, "same name: vps");
    assert!(
        body(&mut app).contains("take the copy; only the copy works"),
        "probe results carry across the write"
    );
    press(&mut app, 'a');
    assert!(app.message().unwrap().ends_with("1 finding remains."));
    cli.cli_apply();
    assert_eq!(
        tui.snapshot().get(Path::new(".ssh/config")),
        cli.snapshot().get(Path::new(".ssh/config"))
    );
    assert_eq!(
        tui.snapshot().get(Path::new(".ssh/config.d/vps")),
        cli.snapshot().get(Path::new(".ssh/config.d/vps"))
    );
    assert_eq!(
        tui.snapshot().get(Path::new(".ssh/config.d/vps.bak")),
        cli.snapshot().get(Path::new(".ssh/config.d/vps.bak"))
    );
}

/// uicln-4
#[test]
fn uicln_4_fold_matches_cli_fold() {
    let tui = Home::new();
    let cli = Home::new();
    let mut app = tui.app();
    press(&mut app, 'X');
    goto(&mut app, "same target");
    press(&mut app, 'f');
    assert_eq!(
        app.message().as_deref(),
        Some("✔ web-prod folded into web in ~/.ssh/config. 2 findings remain.")
    );
    cli.cli_fold("web-prod");
    assert_eq!(tui.snapshot(), cli.snapshot());
    goto(&mut app, "dead");
    press(&mut app, 'f');
    assert_eq!(
        app.message().as_deref(),
        Some("Error: f folds a same-target finding; nas is dead.")
    );
}

/// uicln-5
#[test]
fn uicln_5_esc_abandons_probes_and_writes_nothing() {
    let h = Home::new();
    let before = h.snapshot();
    let gate = Arc::new(Gated::default());
    let mut app = h.app();
    app.set_prober(gate.clone());
    press(&mut app, 'X');
    press(&mut app, 'p');
    press(&mut app, 'A');
    assert_eq!(
        app.message().as_deref(),
        Some("Error: Probes are running. Wait for them, or press Esc to abandon them.")
    );
    app.handle(key(KeyCode::Esc));
    assert_eq!(app.view_name(), None);
    assert!(!app.is_probing());
    assert_eq!(
        app.message().as_deref(),
        Some("Probes abandoned; nothing written.")
    );
    gate.release("*");
    std::thread::sleep(Duration::from_millis(50));
    assert!(!app.tick(), "abandoned results never land");
    assert_eq!(h.snapshot(), before, "no file and no backup written");
    press(&mut app, 'X');
    assert!(body(&mut app).contains("not probed"));
}

/// uicln-6
#[test]
fn uicln_6_no_duplicates_shows_none_and_applies_nothing() {
    let f = Fixture::new(&three_hosts());
    let mut app = f.app();
    app.set_prober(Arc::new(Fixed));
    press(&mut app, 'X');
    let s = screen(&mut app);
    assert!(s.contains("no duplicates in 3 hosts."), "{s}");
    assert!(
        s.contains(" ?:help  Esc:close"),
        "a, A and f are not offered: {s}"
    );
    assert!(!s.contains("a:apply"));
    for c in ['a', 'A', 'f'] {
        press(&mut app, c);
        assert_eq!(
            app.message().as_deref(),
            Some("Nothing to apply; no duplicates in 3 hosts.")
        );
    }
    press(&mut app, 'p');
    assert!(!app.is_probing());
    assert_eq!(f.read(), three_hosts());
    assert_eq!(f.backup(), None);
}

#[test]
fn cleanup_stale_block_refused_and_findings_read_again() {
    let h = Home::new();
    let mut app = h.app();
    press(&mut app, 'X');
    probe_all(&mut app);
    h.write(
        ".ssh/config.d/vps.bak",
        "Host vps\n    HostName vps.example.com\n    User other\n",
    );
    let before = h.snapshot();
    goto(&mut app, "same name: vps");
    press(&mut app, 'a');
    let msg = app.message().unwrap();
    assert!(
        msg.starts_with("Error: ")
            && msg.ends_with("changed since the report; the findings are read again."),
        "{msg}"
    );
    assert_eq!(h.snapshot(), before);
}

#[test]
fn cleanup_without_recommendation_names_the_key() {
    let h = Home::new();
    let mut app = h.app();
    press(&mut app, 'X');
    press(&mut app, 'a');
    assert_eq!(
        app.message().as_deref(),
        Some("Error: vps: nothing to apply; probe to decide; press p.")
    );
}
