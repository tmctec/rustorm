//! The Conflicts dialog: reconcile in the GUI (plan gannet, step 6;
//! rcg-1..rcg-9).
//!
//! Every test runs under a temporary home directory holding docs/cli.md's
//! reconcile example workspace:
//!
//! ```text
//! ~/.ssh/config                Include ~/.ssh/config.d/*; host github
//! ~/.ssh/config.d/cypress      hosts cypressPro, cypressPro-ext, lab-1
//! ~/.ssh/config.d/cypress.bak  the same three, then printer
//! ```

mod common;

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use common::*;
use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;
use rustorm_core::{Decision, Env, KeyPick, Pick, Workspace, WriteOptions};
use rustorm_gui::App;

const ROOT: &str =
    "Include ~/.ssh/config.d/*\n\nHost github\n    HostName github.com\n    User git\n";
const CYPRESS: &str = "Host cypressPro\n    HostName 10.10.0.2\n    User travis\n\nHost cypressPro-ext\n    HostName cypress.example.com\n    User travis\n    Port 2222\n\n# location: Austin DC, rack 4\nHost lab-1\n    HostName 10.10.0.30\n    User travis\n";
const BAK: &str = "Host cypressPro\n    HostName 10.10.0.9\n    User travis\n\n# old box\nHost cypressPro-ext\n    hostname   cypress.example.com\n    User travis\n    Port 2222\n\nHost lab-1\n    HostName 10.10.0.30\n    User travis\n\nHost printer\n    HostName 192.168.1.20\n";

struct Home {
    dir: tempfile::TempDir,
}

impl Home {
    fn with(cypress: &str, bak: &str) -> Home {
        let home = Home {
            dir: tempfile::tempdir().unwrap(),
        };
        home.write(".ssh/config", ROOT);
        home.write(".ssh/config.d/cypress", cypress);
        home.write(".ssh/config.d/cypress.bak", bak);
        home
    }

    fn new() -> Home {
        Home::with(CYPRESS, BAK)
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
            user: None,
            home: Some(self.dir.path().to_path_buf()),
        }
    }

    fn harness(&self) -> Harness<'static, App> {
        let app = App::with_env(self.path(".ssh/config"), self.env()).unwrap();
        let mut h = Harness::builder()
            .with_size([1500.0, 950.0])
            .build_ui_state(|ui, app: &mut App| app.show(ui), app);
        h.run();
        h
    }

    fn load(&self) -> Workspace {
        Workspace::load_with_home(self.path(".ssh/config"), Some(self.dir.path())).unwrap()
    }

    /// Every file under the home directory and its bytes, by relative path.
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

    /// The core's own reconcile: `decisions` by host, applied and saved.
    fn core_apply(&self, decisions: &[(&str, Decision)]) {
        let mut ws = self.load();
        let r = ws.reconcile_report(None);
        let picked: Vec<(usize, Decision)> = decisions
            .iter()
            .map(|(h, d)| (r.lookup(h).unwrap(), d.clone()))
            .collect();
        ws.apply_decisions(&r, &picked, None).unwrap();
        ws.save(WriteOptions::default()).unwrap();
    }
}

const BAK_LABEL: &str = "~/.ssh/config.d/cypress.bak";

/// Clicks the sidebar's Conflicts… button, whatever count it shows.
fn open(h: &mut Harness<'static, App>) {
    h.get_all_by_label_contains("Conflicts…")
        .last()
        .unwrap()
        .click();
    h.run();
}

/// Selects the pair `host` whose copy is in cypress.bak.
fn pick(h: &mut Harness<'static, App>, host: &str) {
    click(h, &format!("{host} in {BAK_LABEL}"));
}

/// (name, kind text) of the dialog's list, in list order.
fn listed(h: &Harness<'static, App>) -> Vec<(String, String)> {
    let s = h.state();
    let v = s.conflicts().expect("dialog open");
    v.order()
        .into_iter()
        .map(|i| {
            let p = &v.report().items[i];
            (p.name.clone(), v.kind_text(p))
        })
        .collect()
}

fn pairs(v: &[(&str, &str)]) -> Vec<(String, String)> {
    v.iter()
        .map(|(a, b)| (a.to_string(), b.to_string()))
        .collect()
}

// rcg-1: Conflicts… shows the count; the dialog lists every pair, conflicts first, labels-only marked
#[test]
fn rcg_1_lists_pairs_with_classification() {
    let home = Home::new();
    let mut h = home.harness();
    assert_eq!(h.state().conflict_count(), 3);
    assert!(shown(&h, "Conflicts…  3"));
    open(&mut h);
    assert_eq!(
        listed(&h),
        pairs(&[
            ("cypressPro", "conflict"),
            ("lab-1", "conflict · labels only"),
            ("cypressPro-ext", "identical"),
        ])
    );
    for host in ["cypressPro", "lab-1", "cypressPro-ext"] {
        assert!(shown(&h, &format!("{host} in {BAK_LABEL}")), "{host} row");
    }
    assert!(shown(&h, "conflict · labels only"));
    assert!(shown(&h, "identical"));
    assert!(shown(
        &h,
        "2 conflicts, 1 identical, 0 orphans across 2 files"
    ));
    click(&mut h, "Close");
    assert!(h.state().conflicts().is_none());
}

// rcg-2: selecting a pair shows live left and copy right with the differing lines marked
#[test]
fn rcg_2_side_by_side_labels_and_marks() {
    let home = Home::new();
    let mut h = home.harness();
    open(&mut h);
    pick(&mut h, "cypressPro");
    assert!(shown(&h, "live  ~/.ssh/config.d/cypress:1"));
    assert!(shown(&h, "copy  ~/.ssh/config.d/cypress.bak:1"));
    assert!(shown(&h, "    HostName 10.10.0.2"));
    assert!(shown(&h, "    HostName 10.10.0.9"));
    let (live, copy) = h.state().conflicts().unwrap().marked_lines();
    assert_eq!(live, ["    HostName 10.10.0.2"]);
    assert_eq!(copy, ["    HostName 10.10.0.9"]);
    for b in [
        "Keep live",
        "Take copy",
        "Apply keys",
        "Skip",
        "HostName live",
        "HostName copy",
    ] {
        assert!(shown(&h, b), "{b}");
    }
    pick(&mut h, "lab-1");
    let (live, copy) = h.state().conflicts().unwrap().marked_lines();
    assert_eq!(live, ["# location: Austin DC, rack 4"]);
    assert!(copy.is_empty());
    click(&mut h, "Skip");
    assert_eq!(
        h.state().conflicts().unwrap().selected().unwrap().name,
        "cypressPro-ext"
    );
}

// rcg-3: Take copy writes what the core's apply_decisions + save writes, byte for byte
#[test]
fn rcg_3_take_copy_matches_core() {
    let gui = Home::new();
    let core = Home::new();
    let mut h = gui.harness();
    open(&mut h);
    pick(&mut h, "cypressPro");
    click(&mut h, "Take copy");
    core.core_apply(&[("cypressPro", Decision::TakeCopy)]);
    assert_eq!(gui.snapshot(), core.snapshot());
    assert!(gui.path(".ssh/config.d/.cypress~").exists(), "backup");
    assert_eq!(
        h.state().status(),
        "cypressPro: took the copy from ~/.ssh/config.d/cypress.bak into ~/.ssh/config.d/cypress. 1 conflict remains."
    );
    // Re-reported: cypressPro is now identical; lab-1 is selected.
    assert_eq!(
        listed(&h),
        pairs(&[
            ("lab-1", "conflict · labels only"),
            ("cypressPro", "identical"),
            ("cypressPro-ext", "identical"),
        ])
    );
    assert_eq!(
        h.state().conflicts().unwrap().selected().unwrap().name,
        "lab-1"
    );
}

// rcg-4: Keep live leaves every file byte-identical and marks the pair kept
#[test]
fn rcg_4_keep_live_writes_nothing() {
    let home = Home::new();
    let before = home.snapshot();
    let mut h = home.harness();
    open(&mut h);
    pick(&mut h, "cypressPro");
    click(&mut h, "Keep live");
    assert_eq!(home.snapshot(), before);
    assert_eq!(
        h.state().status(),
        "cypressPro: kept ~/.ssh/config.d/cypress. 1 conflict remains."
    );
    assert_eq!(
        listed(&h),
        pairs(&[
            ("lab-1", "conflict · labels only"),
            ("cypressPro-ext", "identical"),
            ("cypressPro", "kept live"),
        ])
    );
    assert!(shown(
        &h,
        "2 conflicts, 1 identical, 0 orphans across 2 files; 1 kept live"
    ));
}

// rcg-5: the per-key chooser takes only the keys marked copy, as the core does
#[test]
fn rcg_5_per_key_picks() {
    let live = "Host box\n    HostName a.example.com\n    User alice\n    Port 22\n";
    let copy = "Host box\n    HostName b.example.com\n    User bob\n    Port 22\n";
    let gui = Home::with(live, copy);
    let core = Home::with(live, copy);
    let mut h = gui.harness();
    open(&mut h);
    pick(&mut h, "box");
    assert!(shown_contains(&h, "a.example.com") && shown_contains(&h, "b.example.com"));
    click(&mut h, "User copy");
    assert_eq!(
        h.state().conflicts().unwrap().picks(),
        [Pick::Live, Pick::Copy]
    );
    click(&mut h, "Apply keys");
    core.core_apply(&[(
        "box",
        Decision::Keys(vec![
            KeyPick {
                key: "HostName".into(),
                pick: Pick::Live,
            },
            KeyPick {
                key: "User".into(),
                pick: Pick::Copy,
            },
        ]),
    )]);
    assert_eq!(gui.snapshot(), core.snapshot());
    assert_eq!(
        gui.read(".ssh/config.d/cypress"),
        "Host box\n    HostName a.example.com\n    User bob\n    Port 22\n"
    );
    assert_eq!(gui.read(".ssh/config.d/cypress.bak"), copy);
    assert_eq!(
        h.state().status(),
        "box: took User from ~/.ssh/config.d/cypress.bak into ~/.ssh/config.d/cypress. no conflicts remain."
    );
    assert_eq!(listed(&h), pairs(&[("box", "kept live")]));
}

// rcg-6: Drop identical removes every identical copy from the selected pair's copy file only
#[test]
fn rcg_6_drop_identical() {
    let gui = Home::new();
    let core = Home::new();
    let mut h = gui.harness();
    open(&mut h);
    pick(&mut h, "cypressPro");
    click(&mut h, "Drop identical");
    core.core_apply(&[("cypressPro-ext", Decision::DropIdentical)]);
    assert_eq!(gui.snapshot(), core.snapshot());
    assert_eq!(gui.read(".ssh/config.d/cypress"), CYPRESS);
    assert!(!gui
        .read(".ssh/config.d/cypress.bak")
        .contains("cypressPro-ext"));
    assert_eq!(
        h.state().status(),
        "1 identical copy dropped from ~/.ssh/config.d/cypress.bak. 2 conflicts remain."
    );
    assert_eq!(
        listed(&h),
        pairs(&[
            ("cypressPro", "conflict"),
            ("lab-1", "conflict · labels only")
        ])
    );
    assert_eq!(h.state().conflict_count(), 2);
}

// rcg-7: Retire asks, is refused with the blockers, then moves the resolved copy into ~/.ssh/retired/
#[test]
fn rcg_7_retire_refused_then_allowed() {
    let home = Home::new();
    let mut h = home.harness();
    open(&mut h);
    click(&mut h, "Retire cypress.bak");
    assert!(shown(&h, "Retire ~/.ssh/config.d/cypress.bak?"));
    click(&mut h, "Retire");
    let r = h.state().conflicts().unwrap().retire().unwrap().clone();
    assert!(r.refused);
    assert_eq!(
        r.blockers,
        [
            "cypressPro (conflict)",
            "lab-1 (conflict)",
            "printer (orphan)"
        ]
    );
    assert!(shown(
        &h,
        "Not retired; undecided: cypressPro (conflict), lab-1 (conflict), printer (orphan)."
    ));
    assert!(home.path(".ssh/config.d/cypress.bak").exists());
    assert!(!home.path(".ssh/retired").exists());
    click(&mut h, "Cancel");
    assert!(h.state().conflicts().unwrap().retire().is_none());
    pick(&mut h, "cypressPro");
    click(&mut h, "Take copy");
    pick(&mut h, "lab-1");
    click(&mut h, "Keep live");
    click(&mut h, "Retire cypress.bak");
    click(&mut h, "Retire");
    let r = h.state().conflicts().unwrap().retire().unwrap().clone();
    assert_eq!(r.blockers, ["printer (orphan)"]);
    click(&mut h, "Add printer");
    assert_eq!(
        h.state().status(),
        "printer added to ~/.ssh/config from ~/.ssh/config.d/cypress.bak. no conflicts remain."
    );
    assert!(h
        .state()
        .conflicts()
        .unwrap()
        .retire()
        .unwrap()
        .blockers
        .is_empty());
    let bak = home.read(".ssh/config.d/cypress.bak");
    click(&mut h, "Retire");
    assert!(!home.path(".ssh/config.d/cypress.bak").exists());
    assert_eq!(home.read(".ssh/retired/cypress.bak"), bak);
    assert_eq!(
        h.state().status(),
        "~/.ssh/config.d/cypress.bak retired to ~/.ssh/retired/cypress.bak."
    );
    assert!(h.state().conflicts().is_none(), "nothing remains: closed");
    assert_eq!(h.state().conflict_count(), 0);
    assert!(home.read(".ssh/config").contains("Host printer"));
    assert_eq!(h.state().workspace().files.len(), 2);
}

// rcg-8: Conflicts… is disabled while the editor holds unsaved text
#[test]
fn rcg_8_disabled_while_editor_dirty() {
    let home = Home::new();
    let mut h = home.harness();
    let text = h.state().editor_text().to_string();
    h.state_mut().set_editor_text(format!("{text}# pending\n"));
    h.run();
    open(&mut h);
    assert!(h.state().conflicts().is_none());
    h.state_mut().set_editor_text(text);
    h.run();
    open(&mut h);
    assert!(h.state().conflicts().is_some());
}

// rcg-9: a pair changed on disk since the report is refused and re-reported, never decided blind
#[test]
fn rcg_9_stale_pair_rereports() {
    let home = Home::new();
    let mut h = home.harness();
    open(&mut h);
    pick(&mut h, "cypressPro");
    let changed = BAK.replace("10.10.0.9", "10.10.0.99");
    home.write(".ssh/config.d/cypress.bak", &changed);
    click(&mut h, "Take copy");
    assert_eq!(home.read(".ssh/config.d/cypress"), CYPRESS);
    assert_eq!(home.read(".ssh/config.d/cypress.bak"), changed);
    let err = "cypressPro changed since the report; reconcile again.";
    assert_eq!(h.state().conflicts().unwrap().error(), Some(err));
    assert!(shown_contains(&h, err));
    let (_, copy) = h.state().conflicts().unwrap().marked_lines();
    assert_eq!(copy, ["    HostName 10.10.0.99"]);
    click(&mut h, "Take copy");
    assert!(home
        .read(".ssh/config.d/cypress")
        .starts_with("Host cypressPro\n    HostName 10.10.0.99\n"));
}
