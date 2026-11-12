//! The Cleanup dialog: find, probe and apply duplicates in the GUI (plan
//! coati, step 7; uicln-1..uicln-6).
//!
//! Every test runs under a temporary home directory holding docs/cli.md's
//! cleanup example workspace:
//!
//! ```text
//! ~/.ssh/config             Include ~/.ssh/config.d/*; nas twice (dead),
//!                           web and web-prod (same target), Host *
//! ~/.ssh/config.d/vps       vps, User admin (live)
//! ~/.ssh/config.d/vps.bak   vps, User travis (copy)
//! ```
//!
//! Probes go through an injected [`Gated`] prober: a fixed status per
//! (file name, host), each held until the test releases it, so a status
//! lands exactly when the test says. "Byte-identical to the CLI" compares
//! the GUI's home with a second home on which the CLI's own sequence ran
//! with the same prober: `cleanup_report`, `probe`, a fresh load,
//! `apply_cleanup` or `fold_host`, `save`.

mod common;

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use common::*;
use egui_kittest::kittest::{NodeT, Queryable};
use egui_kittest::Harness;
use rustorm_core::{
    remaining_findings, Env, ProbeResult, ProbeStatus, ProbeTarget, Prober, Workspace,
    WriteOptions, DEFAULT_JOBS, DEFAULT_TIMEOUT,
};
use rustorm_gui::App;

const ROOT: &str = "Include ~/.ssh/config.d/*\n\nHost nas\n    HostName 192.168.1.10\n    User travis\n\nHost web\n    HostName web.example.com\n    User deploy\n\nHost web-prod\n    HostName web.example.com\n    User deploy\n\nHost nas\n    HostName 192.168.1.10\n\nHost *\n    ServerAliveInterval 30\n";
const VPS: &str = "Host vps\n    HostName vps.example.com\n    User admin\n";
const VPS_BAK: &str = "Host vps\n    HostName vps.example.com\n    User travis\n";

type Key = (String, String);

fn key(file: &str, host: &str) -> Key {
    (file.to_string(), host.to_string())
}

/// A prober with a fixed status per (file name, host), each probe held
/// until released.
struct Gated {
    statuses: HashMap<Key, ProbeStatus>,
    open: Mutex<HashSet<Key>>,
    all: AtomicBool,
    cv: Condvar,
    started: AtomicUsize,
    finished: AtomicUsize,
}

impl Gated {
    fn new(released: bool) -> Arc<Gated> {
        let statuses = HashMap::from([
            (key("vps", "vps"), ProbeStatus::Refused),
            (key("vps.bak", "vps"), ProbeStatus::Works),
            (key("config", "nas"), ProbeStatus::Works),
            (key("config", "web"), ProbeStatus::Works),
            (key("config", "web-prod"), ProbeStatus::Works),
        ]);
        Arc::new(Gated {
            statuses,
            open: Mutex::new(HashSet::new()),
            all: AtomicBool::new(released),
            cv: Condvar::new(),
            started: AtomicUsize::new(0),
            finished: AtomicUsize::new(0),
        })
    }

    fn release(&self, file: &str, host: &str) {
        self.open.lock().unwrap().insert(key(file, host));
        self.cv.notify_all();
    }

    fn release_all(&self) {
        self.all.store(true, Ordering::SeqCst);
        self.cv.notify_all();
    }
}

impl Prober for Gated {
    fn probe(&self, t: &ProbeTarget) -> ProbeResult {
        self.started.fetch_add(1, Ordering::SeqCst);
        let k = key(&t.file.file_name().unwrap().to_string_lossy(), &t.name);
        let mut open = self.open.lock().unwrap();
        while !self.all.load(Ordering::SeqCst) && !open.contains(&k) {
            open = self
                .cv
                .wait_timeout(open, Duration::from_millis(10))
                .unwrap()
                .0;
        }
        drop(open);
        self.finished.fetch_add(1, Ordering::SeqCst);
        ProbeResult {
            status: self
                .statuses
                .get(&k)
                .copied()
                .unwrap_or(ProbeStatus::Timeout),
            detail: None,
        }
    }
}

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
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, text).unwrap();
    }

    fn read(&self, rel: &str) -> String {
        fs::read_to_string(self.path(rel)).unwrap()
    }

    fn env(&self) -> Env {
        Env {
            user: Some("travis".to_string()),
            home: Some(self.dir.path().to_path_buf()),
        }
    }

    fn harness(&self, prober: Arc<Gated>) -> Harness<'static, App> {
        let app = App::with_env(self.path(".ssh/config"), self.env())
            .unwrap()
            .with_prober(prober);
        let mut h = Harness::builder()
            .with_size([1500.0, 1000.0])
            .build_ui_state(|ui, app: &mut App| app.show(ui), app);
        h.run();
        h
    }

    fn load(&self) -> Workspace {
        Workspace::load_with_home(self.path(".ssh/config"), Some(self.dir.path())).unwrap()
    }

    fn snapshot(&self) -> BTreeMap<PathBuf, Vec<u8>> {
        fn walk(base: &Path, dir: &Path, out: &mut BTreeMap<PathBuf, Vec<u8>>) {
            for e in fs::read_dir(dir).unwrap() {
                let p = e.unwrap().path();
                if p.is_dir() {
                    walk(base, &p, out);
                } else {
                    let rel = p.strip_prefix(base).unwrap().to_path_buf();
                    out.insert(rel, fs::read(&p).unwrap());
                }
            }
        }
        let mut out = BTreeMap::new();
        walk(self.dir.path(), self.dir.path(), &mut out);
        out
    }

    /// `rustorm cleanup --apply` through the core calls the CLI makes,
    /// probing with `prober`. Returns the messages it prints.
    fn cli_apply(&self, prober: &dyn Prober) -> Vec<String> {
        let ws = self.load();
        let mut report = ws.cleanup_report(None, &self.env());
        report.probe(prober, DEFAULT_TIMEOUT, DEFAULT_JOBS, None, &mut |_, _| {});
        let mut ws = self.load();
        let change = ws.apply_cleanup(&report, &report.applicable()).unwrap();
        ws.save(WriteOptions::default()).unwrap();
        let mut out = change.messages;
        out.push(remaining_findings(
            ws.cleanup_report(None, &self.env()).findings.len(),
        ));
        out
    }

    /// `rustorm cleanup --fold HOST` through the core calls the CLI makes.
    fn cli_fold(&self, host: &str) -> Vec<String> {
        let mut ws = self.load();
        let fresh = ws.cleanup_report(None, &self.env());
        let change = ws.fold_host(&fresh, host).unwrap();
        ws.save(WriteOptions::default()).unwrap();
        let mut out = change.messages;
        out.push(remaining_findings(
            ws.cleanup_report(None, &self.env()).findings.len(),
        ));
        out
    }
}

fn click(h: &mut Harness<'static, App>, label: &str) {
    h.get_all_by_label(label)
        .last()
        .unwrap_or_else(|| panic!("no node labelled {label:?}"))
        .click();
    h.run();
}

fn disabled(h: &Harness<'static, App>, label: &str) -> bool {
    h.get_all_by_label(label)
        .last()
        .unwrap_or_else(|| panic!("no node labelled {label:?}"))
        .accesskit_node()
        .is_disabled()
}

fn open(h: &mut Harness<'static, App>) {
    h.get_all_by_label_contains("Cleanup…")
        .last()
        .unwrap()
        .click();
    h.run();
    assert!(h.state().cleanup().is_some(), "Cleanup dialog open");
}

/// Steps frames until `done` holds, at most five seconds.
fn wait(h: &mut Harness<'static, App>, what: &str, done: impl Fn(&App) -> bool) {
    let until = Instant::now() + Duration::from_secs(5);
    while !done(h.state()) {
        assert!(Instant::now() < until, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(5));
        h.step();
    }
    h.step();
}

/// The status the dialog shows for the definition `lead` of the selected
/// finding (`live`, `copy`, or a same-target name).
fn status_of(h: &App, lead: &str) -> String {
    let v = h.cleanup().unwrap();
    let f = &v.report().findings[v.selected().unwrap()];
    let d = f
        .definitions
        .iter()
        .find(|d| d.role.label() == lead || d.name == lead)
        .unwrap();
    v.status_text(d).to_string()
}

// TESTMASTER: id=uicln-1 tier=? parallel=yes
#[test]
fn uicln_1_gui_lists_the_findings_of_cleanup_no_probe() {
    let home = Home::new();
    let mut h = home.harness(Gated::new(true));
    assert_eq!(
        h.state().cleanup_count(),
        3,
        "the sidebar counts the findings"
    );
    assert!(shown(&h, "Cleanup…  3"));
    open(&mut h);
    let cli = home.load().cleanup_report(None, &home.env()).text();
    assert_eq!(h.state().cleanup().unwrap().report().text(), cli);
    for row in ["same name: vps", "dead: nas", "same target: web, web-prod"] {
        assert!(h.query_all_by_label(row).next().is_some(), "row {row}");
    }
    assert!(shown_contains(&h, "3 findings in 6 hosts."));
    assert!(shown_contains(
        &h,
        "drop the dead block at ~/.ssh/config:15"
    ));
    // The flag drops out of the fold sentence: Fold is the button.
    assert!(shown_contains(&h, "fold web-prod into web"));
    assert!(!shown_contains(&h, "--fold"));
    assert!(shown_contains(&h, "● not probed"));
}

// TESTMASTER: id=uicln-2 tier=? parallel=yes
#[test]
fn uicln_2_gui_statuses_land_one_by_one_while_frames_keep_drawing() {
    let home = Home::new();
    let prober = Gated::new(false);
    let mut h = home.harness(Arc::clone(&prober));
    open(&mut h);
    click(&mut h, "Probe");
    assert!(h.state().cleanup().unwrap().probing());
    assert_eq!(status_of(h.state(), "live"), "probing…");
    assert_eq!(h.state().cleanup().unwrap().progress(), Some((0, 6)));
    assert!(disabled(&h, "Probe") && disabled(&h, "Apply all (1)"));
    // Every probe is blocked; frames still draw at once.
    let t = Instant::now();
    for _ in 0..20 {
        h.step();
    }
    assert!(
        t.elapsed() < Duration::from_secs(2),
        "frames draw while probes block"
    );
    click(&mut h, "dead: nas");
    click(&mut h, "same name: vps");
    prober.release("vps.bak", "vps");
    wait(&mut h, "the copy's status", |a| {
        status_of(a, "copy") == "works"
    });
    assert_eq!(
        status_of(h.state(), "live"),
        "probing…",
        "live still running"
    );
    assert_eq!(h.state().cleanup().unwrap().progress(), Some((1, 6)));
    assert!(shown_contains(&h, "● works") && shown_contains(&h, "● probing…"));
    assert!(shown_contains(&h, "1 of 6 probed"));
    prober.release("vps", "vps");
    wait(&mut h, "the live status", |a| {
        status_of(a, "live") == "refused"
    });
    assert!(shown_contains(&h, "take the copy; only the copy works"));
    prober.release_all();
    wait(&mut h, "every probe", |a| !a.cleanup().unwrap().probing());
    assert_eq!(h.state().cleanup().unwrap().progress(), None);
    assert!(!disabled(&h, "Apply all (2)"));
}

// TESTMASTER: id=uicln-3 tier=? parallel=yes
#[test]
fn uicln_3_gui_apply_all_matches_cli_apply() {
    let gui = Home::new();
    let cli = Home::new();
    let mut h = gui.harness(Gated::new(true));
    open(&mut h);
    click(&mut h, "Probe");
    wait(&mut h, "every probe", |a| !a.cleanup().unwrap().probing());
    click(&mut h, "Apply all (2)");
    let said = cli.cli_apply(Gated::new(true).as_ref());
    assert_eq!(
        gui.snapshot(),
        cli.snapshot(),
        "files and backups match the CLI"
    );
    assert_eq!(h.state().status(), said.join(" "));
    assert_eq!(
        said,
        [
            "vps: moved the copy from ~/.ssh/config.d/vps.bak into ~/.ssh/config.d/vps; only the copy works.",
            "nas: dropped the dead block at ~/.ssh/config:15.",
            "1 finding remains.",
        ]
    );
    assert!(gui.read(".ssh/config.d/vps").contains("User travis"));
    let v = h
        .state()
        .cleanup()
        .expect("the dialog stays open on the result");
    assert_eq!(v.messages(), [said.join(" ")]);
    assert_eq!(v.report().findings.len(), 1);
    assert!(shown_contains(&h, "1 finding in 4 hosts."));
    // Apply on one finding writes only that finding.
    let one = Home::new();
    let mut h = one.harness(Gated::new(true));
    open(&mut h);
    click(&mut h, "dead: nas");
    click(&mut h, "Apply");
    assert_eq!(
        h.state().status(),
        "nas: dropped the dead block at ~/.ssh/config:15. 2 findings remain."
    );
    assert_eq!(one.read(".ssh/config.d/vps"), VPS);
}

// TESTMASTER: id=uicln-4 tier=? parallel=yes
#[test]
fn uicln_4_gui_fold_matches_cli_fold() {
    let gui = Home::new();
    let cli = Home::new();
    let mut h = gui.harness(Gated::new(true));
    open(&mut h);
    click(&mut h, "same name: vps");
    assert!(disabled(&h, "Fold"), "Fold is for same-target findings");
    click(&mut h, "same target: web, web-prod");
    assert!(disabled(&h, "Apply"), "a fold is not applied with Apply");
    click(&mut h, "Fold");
    let said = cli.cli_fold("web-prod");
    assert_eq!(gui.snapshot(), cli.snapshot());
    assert_eq!(h.state().status(), said.join(" "));
    assert_eq!(said[0], "web-prod folded into web in ~/.ssh/config.");
    assert!(gui.read(".ssh/config").contains("Host web web-prod\n"));
}

// TESTMASTER: id=uicln-5 tier=? parallel=yes
#[test]
fn uicln_5_gui_close_abandons_probes_and_writes_nothing() {
    let home = Home::new();
    let before = home.snapshot();
    let prober = Gated::new(false);
    let mut h = home.harness(Arc::clone(&prober));
    open(&mut h);
    click(&mut h, "Probe");
    assert!(h.state().cleanup().unwrap().probing());
    h.key_press(egui::Key::Escape);
    h.run();
    assert!(h.state().cleanup().is_none(), "Esc closes while probing");
    prober.release_all();
    let until = Instant::now() + Duration::from_secs(5);
    while prober.finished.load(Ordering::SeqCst) < prober.started.load(Ordering::SeqCst) {
        assert!(Instant::now() < until, "the abandoned probes end");
        std::thread::sleep(Duration::from_millis(5));
    }
    for _ in 0..5 {
        h.step();
    }
    assert!(h.state().cleanup().is_none());
    assert_eq!(home.snapshot(), before, "nothing written, no backup");
    // Close does the same.
    open(&mut h);
    click(&mut h, "Probe");
    click(&mut h, "Close");
    assert!(h.state().cleanup().is_none());
    assert_eq!(home.snapshot(), before);
}

// TESTMASTER: id=uicln-6 tier=? parallel=yes
#[test]
fn uicln_6_gui_no_duplicates_shows_none_and_disables_every_action() {
    let home = Home::new();
    home.write(
        ".ssh/config",
        "Host a\n    HostName a.example.com\n\nHost b\n    HostName b.example.com\n",
    );
    let mut h = home.harness(Gated::new(true));
    assert_eq!(h.state().cleanup_count(), 0);
    open(&mut h);
    assert!(shown_contains(&h, "no duplicates in 2 hosts."));
    for b in ["Probe", "Apply", "Apply all (0)", "Fold"] {
        assert!(disabled(&h, b), "{b} disabled");
    }
    assert!(!h.state_mut().cleanup_apply_all());
    // The editor guard refuses the dialog, as Conflicts is refused.
    click(&mut h, "Close");
    h.state_mut().set_editor_text("Host a\n    HostName x\n");
    h.run();
    assert!(disabled(&h, "Cleanup…"));
    h.state_mut().open_cleanup();
    assert!(h.state().cleanup().is_none());
}
