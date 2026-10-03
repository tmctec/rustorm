//! The Conflicts view (`C`): hosts defined in two or more workspace files,
//! decided through the core's reconcile (docs/cli.md, reconcile).
//! Catalog cases: rct-1 .. rct-10.

mod common;
use common::*;
use crossterm::event::KeyCode;
use ratatui::style::Modifier;
use rustorm_core::{Decision, Env, KeyPick, Pick, Workspace, WriteOptions};
use rustorm_tui::App;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

const ROOT: &str =
    "Include ~/.ssh/config.d/*\n\nHost github\n    HostName github.com\n    User git\n";
const CYPRESS: &str = "Host cypressPro\n    HostName 10.10.0.2\n    User travis\n\nHost cypressPro-ext\n    HostName cypress.example.com\n    User travis\n    Port 2222\n\n# location: Austin DC, rack 4\nHost lab-1\n    HostName 10.10.0.30\n    User travis\n";
const BAK: &str = "Host cypressPro\n    HostName 10.10.0.9\n    User travis\n\n# old box\nHost cypressPro-ext\n    hostname   cypress.example.com\n    User travis\n    Port 2222\n\nHost lab-1\n    HostName 10.10.0.30\n    User travis\n\nHost printer\n    HostName 192.168.1.20\n";

/// A temporary home: `~/.ssh/config` including `~/.ssh/config.d/*`, which
/// holds `cypress` and the stray `cypress.bak`.
struct Home {
    dir: tempfile::TempDir,
}

impl Home {
    fn with(bak: &str) -> Home {
        let home = Home {
            dir: tempfile::tempdir().unwrap(),
        };
        home.write(".ssh/config", ROOT);
        home.write(".ssh/config.d/cypress", CYPRESS);
        home.write(".ssh/config.d/cypress.bak", bak);
        home
    }
    fn new() -> Home {
        Home::with(BAK)
    }
    fn path(&self, rel: &str) -> PathBuf {
        self.dir.path().join(rel)
    }
    fn write(&self, rel: &str, text: &str) {
        let p = self.path(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }
    fn read(&self, rel: &str) -> String {
        std::fs::read_to_string(self.path(rel)).unwrap()
    }
    fn app(&self) -> App {
        let mut opts = options();
        opts.env = Env {
            user: Some("tester".into()),
            home: Some(self.dir.path().to_path_buf()),
        };
        App::with_options(self.path(".ssh/config"), opts).unwrap()
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
    /// `decisions` by host through the core, then saved, as the CLI does.
    fn core_apply(&self, decisions: &[(&str, Decision)]) {
        let mut ws = self.load();
        let report = ws.reconcile_report(None);
        let d: Vec<(usize, Decision)> = decisions
            .iter()
            .map(|(h, d)| (report.lookup(h).unwrap(), d.clone()))
            .collect();
        ws.apply_decisions(&report, &d, None).unwrap();
        ws.save(WriteOptions::default()).unwrap();
    }
}

/// Opens the Conflicts view.
fn open(app: &mut App) {
    press(app, 'C');
    assert!(
        app.conflict_selected().is_some(),
        "C opens the Conflicts view"
    );
}

/// Highlights the row of `host` in the Conflicts view.
fn goto(app: &mut App, host: &str) {
    press(app, 'g');
    for _ in 0..20 {
        if app.conflict_selected().as_deref() == Some(host) {
            return;
        }
        app.handle(key(KeyCode::Down));
    }
    panic!("{host} not in the Conflicts view");
}

/// The listed rows: host and kind, in screen order.
fn listed(app: &mut App) -> Vec<(String, String)> {
    let text = screen(app);
    let mut out = Vec::new();
    for l in text.lines() {
        let inner = l.trim_matches(|c: char| "║│ ".contains(c));
        let tokens: Vec<&str> = inner.split_whitespace().collect();
        if tokens.len() >= 4 && tokens[2].starts_with("~/.ssh/") {
            let kind = inner
                .split_once(tokens[2])
                .map(|(_, k)| k.trim().to_string())
                .unwrap_or_default();
            out.push((tokens[0].to_string(), kind));
        }
    }
    out
}

fn msg(app: &App) -> String {
    app.message().unwrap_or_default()
}

// rct-1: C lists every pair, conflicts first, then identical, then the
// orphan of the copy file; a metadata-only conflict says labels only.
#[test]
fn rct_1_list_shows_pairs_and_classification() {
    let h = Home::new();
    let mut app = h.app();
    open(&mut app);
    let text = screen(&mut app);
    assert!(
        text.contains("Duplicates  2 conflicts, 1 identical, 1 orphan"),
        "{text}"
    );
    assert_eq!(
        listed(&mut app),
        vec![
            ("cypressPro".into(), "conflict".into()),
            ("lab-1".into(), "conflict, labels only".into()),
            ("cypressPro-ext".into(), "identical".into()),
            ("printer".into(), "orphan".into()),
        ]
    );
    assert!(text.contains("~/.ssh/config.d/cypress:1"), "{text}");
    assert!(text.contains("~/.ssh/config.d/cypress.bak:1"), "{text}");
}

// rct-2: Enter shows the live block left and the copy right on the same
// rows; the differing HostName lines stand out, the shared User line not.
#[test]
fn rct_2_enter_shows_blocks_side_by_side() {
    let h = Home::new();
    let mut app = h.app();
    open(&mut app);
    goto(&mut app, "cypressPro");
    app.handle(key(KeyCode::Enter));
    let buf = draw(&mut app);
    let rows = lines(&buf);
    let y = rows
        .iter()
        .position(|l| l.contains("HostName 10.10.0.2"))
        .expect("live HostName shown");
    let row = &rows[y];
    let (l, r) = (
        row.find("HostName 10.10.0.2").unwrap(),
        row.find("HostName 10.10.0.9")
            .expect("copy HostName on the same row"),
    );
    assert!(l < r, "live left, copy right: {row}");
    assert!(rows
        .iter()
        .any(|l| l.contains(" live ~/.ssh/config.d/cypress:1 ")));
    assert!(rows
        .iter()
        .any(|l| l.contains(" copy ~/.ssh/config.d/cypress.bak:1 ")));
    assert!(rows
        .iter()
        .any(|l| l.contains("HostName: 10.10.0.2 | 10.10.0.9")));
    let col = |s: &str| row.chars().take(row.find(s).unwrap()).count() as u16;
    let x = col("HostName 10.10.0.2");
    assert!(buf[(x, y as u16)].modifier.contains(Modifier::BOLD));
    let xr = col("HostName 10.10.0.9");
    assert!(buf[(xr, y as u16)].modifier.contains(Modifier::BOLD));
    let uy = rows.iter().position(|l| l.contains("User travis")).unwrap();
    let urow = &rows[uy];
    let ux = urow.chars().take(urow.find("User travis").unwrap()).count() as u16;
    assert!(!buf[(ux, uy as u16)].modifier.contains(Modifier::BOLD));
    app.handle(key(KeyCode::Esc));
    assert_eq!(listed(&mut app).len(), 4, "Esc goes back to the list");
}

// rct-3: c writes the copy's body into the live file, byte-equal to the
// core's apply_decisions + save on an identical fixture, after a backup.
#[test]
fn rct_3_take_copy_matches_core() {
    let (h, want) = (Home::new(), Home::new());
    let mut app = h.app();
    open(&mut app);
    goto(&mut app, "cypressPro");
    press(&mut app, 'c');
    want.core_apply(&[("cypressPro", Decision::TakeCopy)]);
    assert_eq!(h.snapshot(), want.snapshot());
    assert!(h
        .read(".ssh/config.d/cypress")
        .contains("HostName 10.10.0.9"));
    assert_eq!(
        msg(&app),
        "✔ cypressPro: took the copy from ~/.ssh/config.d/cypress.bak into ~/.ssh/config.d/cypress. 1 conflict remains."
    );
    // Re-read: the pair is identical now, and the table shows the new value.
    assert!(listed(&mut app).contains(&("cypressPro".into(), "identical".into())));
    let row = app
        .visible_rows()
        .into_iter()
        .find(|r| r.name == "cypressPro")
        .unwrap()
        .hostname
        .clone();
    assert_eq!(row.as_deref(), Some("10.10.0.9"));
}

// rct-4: l keeps the live definition: every file stays byte-identical and
// the row says kept live.
#[test]
fn rct_4_keep_live_writes_nothing() {
    let h = Home::new();
    let before = h.snapshot();
    let mut app = h.app();
    open(&mut app);
    goto(&mut app, "lab-1");
    press(&mut app, 'l');
    assert_eq!(h.snapshot(), before);
    assert_eq!(
        msg(&app),
        "✔ lab-1: kept ~/.ssh/config.d/cypress. 1 conflict remains."
    );
    assert!(listed(&mut app).contains(&("lab-1".into(), "conflict, labels only, kept live".into())));
}

// rct-5: k steps through the differing keys; each l / c picks a side and
// the last pick writes, byte-equal to the core's per-key decision.
#[test]
fn rct_5_key_by_key_matches_core() {
    let bak = BAK.replace(
        "HostName 10.10.0.9\n    User travis\n",
        "HostName 10.10.0.9\n    User root\n",
    );
    let (h, want) = (Home::with(&bak), Home::with(&bak));
    let mut app = h.app();
    open(&mut app);
    goto(&mut app, "cypressPro");
    press(&mut app, 'k');
    let text = screen(&mut app);
    assert!(text.contains("cypressPro key by key"), "{text}");
    assert!(text.contains("> HostName"), "{text}");
    press(&mut app, 'c');
    assert!(screen(&mut app).contains("> User"));
    assert!(h.snapshot() == want.snapshot(), "nothing written mid-picks");
    press(&mut app, 'l');
    want.core_apply(&[(
        "cypressPro",
        Decision::Keys(vec![
            KeyPick {
                key: "HostName".into(),
                pick: Pick::Copy,
            },
            KeyPick {
                key: "User".into(),
                pick: Pick::Live,
            },
        ]),
    )]);
    assert_eq!(h.snapshot(), want.snapshot());
    assert!(msg(&app).contains("cypressPro: took HostName from ~/.ssh/config.d/cypress.bak"));
}

// rct-6: D drops every identical copy in the highlighted pair's copy file;
// the live file is untouched.
#[test]
fn rct_6_drop_identical() {
    let h = Home::new();
    let live = h.read(".ssh/config.d/cypress");
    let mut app = h.app();
    open(&mut app);
    goto(&mut app, "cypressPro");
    press(&mut app, 'D');
    let bak = h.read(".ssh/config.d/cypress.bak");
    assert!(!bak.contains("cypressPro-ext"), "{bak}");
    assert!(!bak.contains("# old box"), "{bak}");
    assert_eq!(h.read(".ssh/config.d/cypress"), live);
    assert_eq!(
        msg(&app),
        "✔ 1 identical copy dropped from ~/.ssh/config.d/cypress.bak. 2 conflicts remain."
    );
    assert!(!listed(&mut app).iter().any(|(n, _)| n == "cypressPro-ext"));
}

// rct-7: R on a copy file with undecided hosts is refused with the
// blockers; once every host is decided it asks [y/N] and y moves the file
// into ~/.ssh/retired/, and the workspace drops it.
#[test]
fn rct_7_retire_refused_then_allowed() {
    let h = Home::new();
    let mut app = h.app();
    open(&mut app);
    goto(&mut app, "printer");
    press(&mut app, 'R');
    assert_eq!(
        msg(&app),
        "Error: ~/.ssh/config.d/cypress.bak not retired; undecided: cypressPro (conflict), lab-1 (conflict), printer (orphan)."
    );
    assert!(h.path(".ssh/config.d/cypress.bak").exists());
    goto(&mut app, "cypressPro");
    press(&mut app, 'c');
    goto(&mut app, "lab-1");
    press(&mut app, 'l');
    goto(&mut app, "printer");
    press(&mut app, 'a');
    assert!(h.read(".ssh/config").contains("Host printer"));
    goto(&mut app, "lab-1");
    press(&mut app, 'R');
    assert!(
        screen(&mut app).contains("Retire ~/.ssh/config.d/cypress.bak to ~/.ssh/retired/? [y/N]")
    );
    press(&mut app, 'n');
    assert!(
        h.path(".ssh/config.d/cypress.bak").exists(),
        "n keeps the file"
    );
    press(&mut app, 'R');
    press(&mut app, 'y');
    assert!(!h.path(".ssh/config.d/cypress.bak").exists());
    assert!(h.path(".ssh/retired/cypress.bak").exists());
    assert_eq!(
        msg(&app),
        "✔ ~/.ssh/config.d/cypress.bak retired to ~/.ssh/retired/cypress.bak."
    );
    assert_eq!(app.files().len(), 2);
    assert!(screen(&mut app).contains("no host is defined in two workspace files."));
}

// rct-8: Esc leaves the side-by-side view for the list and the list for the
// table, writing nothing.
#[test]
fn rct_8_esc_leaves() {
    let h = Home::new();
    let before = h.snapshot();
    let mut app = h.app();
    open(&mut app);
    app.handle(key(KeyCode::Enter));
    assert!(screen(&mut app).contains(" copy ~/.ssh/config.d/cypress.bak:1 "));
    app.handle(key(KeyCode::Esc));
    assert!(screen(&mut app).contains("Duplicates  2 conflicts"));
    app.handle(key(KeyCode::Esc));
    assert_eq!(app.conflict_selected(), None);
    assert!(!screen(&mut app).contains("Duplicates"));
    assert_eq!(app.focus(), rustorm_tui::Focus::Table);
    assert_eq!(h.snapshot(), before);
}

// rct-9: the table's help bar names C, the view's help bars and the ?
// overlay list the keys, every bar within 150 columns; a single file has
// no C.
#[test]
fn rct_9_help_lists_keys() {
    let h = Home::new();
    let mut app = h.app();
    let bar = lines(&draw(&mut app)).pop().unwrap();
    assert!(bar.contains("C:conflicts"), "{bar}");
    assert!(bar.trim_end().chars().count() <= 150);
    open(&mut app);
    let bar = lines(&draw(&mut app)).pop().unwrap();
    for k in [
        "l:keep live",
        "c:take copy",
        "k:key by key",
        "s:skip",
        "D:drop identical",
        "R:retire file",
        "Esc:back",
    ] {
        assert!(bar.contains(k), "{k} in {bar}");
    }
    press(&mut app, '?');
    let text = screen(&mut app);
    for k in [
        "drop identical copies in file",
        "retire the copy file",
        "keep live / take copy",
        "pick key by key",
    ] {
        assert!(text.contains(k), "{k} in overlay");
    }
    press(&mut app, '?');
    assert!(
        app.conflict_selected().is_some(),
        "? closes back to the view"
    );
    app.handle(key(KeyCode::Enter));
    let bar = lines(&draw(&mut app)).pop().unwrap();
    assert!(bar.contains("Esc:back to list"), "{bar}");
    press(&mut app, 'k');
    let bar = lines(&draw(&mut app)).pop().unwrap();
    assert!(bar.contains("l:live value"), "{bar}");
    let f = Fixture::new(&three_hosts());
    let mut single = f.app();
    press(&mut single, 'C');
    assert_eq!(single.conflict_selected(), None);
}

// rct-10: a copy changed on disk since the list was read: the decision is
// refused as stale, the list is read again, and nothing is written.
#[test]
fn rct_10_stale_rereads() {
    let h = Home::new();
    let mut app = h.app();
    open(&mut app);
    goto(&mut app, "cypressPro");
    let bak = BAK.replace("10.10.0.9", "10.10.0.7");
    h.write(".ssh/config.d/cypress.bak", &bak);
    let before = h.snapshot();
    press(&mut app, 'c');
    assert_eq!(
        msg(&app),
        "Error: cypressPro changed since the report; reconcile again. Read the duplicates again."
    );
    assert_eq!(h.snapshot(), before);
    app.handle(key(KeyCode::Enter));
    assert!(screen(&mut app).contains("HostName 10.10.0.7"));
    press(&mut app, 'c');
    assert!(h
        .read(".ssh/config.d/cypress")
        .contains("HostName 10.10.0.7"));
}
