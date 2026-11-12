//! The Combine dialog: merge files in the GUI (plan coati, step 7;
//! uicmb-1..uicmb-9).
//!
//! Every test runs under a temporary home directory:
//!
//! ```text
//! ~/.ssh/config              Include ~/.ssh/config.d/*; Host *, nas, vps
//! ~/.ssh/config.d/cypress    cypress
//! ~/.ssh/config.d/legacy     Host *, nas, vps (both conflicts), printer
//! ~/.ssh/old/extra           extra; no Include loads it
//! ```
//!
//! "Byte-identical to the CLI" compares the GUI's home with a second home
//! on which the CLI's own sequence ran: `CombineRequest::commit` with the
//! decisions `--keep`/`--replace` build, as `rustorm combine` does.

mod common;

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use common::*;
use egui_kittest::kittest::{NodeT, Queryable};
use egui_kittest::Harness;
use rustorm_core::{host_decisions, CombineRequest, Env, HostDecision, OnConflict, WriteOptions};
use rustorm_gui::App;

const ROOT: &str = "Include ~/.ssh/config.d/*\n\nHost *\n    ServerAliveInterval 30\n\nHost nas\n    HostName nas.local\n\nHost vps\n    HostName vps.example.com\n    User admin\n";
const CYPRESS: &str = "Host cypress\n    HostName cypress.example.com\n";
const LEGACY: &str = "Host *\n    ServerAliveInterval 60\n    Compression yes\n\nHost nas\n    HostName 192.168.1.10\n\nHost vps\n    HostName old-vps.example.com\n    User root\n\nHost printer\n    HostName 192.168.1.20\n";
const EXTRA: &str = "Host extra\n    HostName extra.lan\n";

struct Home {
    dir: tempfile::TempDir,
}

impl Home {
    fn new() -> Home {
        let home = Home {
            dir: tempfile::tempdir().unwrap(),
        };
        home.write(".ssh/config", ROOT);
        home.write(".ssh/config.d/cypress", CYPRESS);
        home.write(".ssh/config.d/legacy", LEGACY);
        home.write(".ssh/old/extra", EXTRA);
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
            user: None,
            home: Some(self.dir.path().to_path_buf()),
        }
    }

    fn harness(&self) -> Harness<'static, App> {
        let app = App::with_env(self.path(".ssh/config"), self.env()).unwrap();
        let mut h = Harness::builder()
            .with_size([1500.0, 1000.0])
            .build_ui_state(|ui, app: &mut App| app.show(ui), app);
        h.run();
        h
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

    /// `rustorm combine <inputs> [-o output] --keep .. --replace .. [--retire]`
    /// through the core calls the CLI makes.
    fn cli_combine(
        &self,
        inputs: &[&str],
        output: Option<&str>,
        keep: &[&str],
        replace: &[&str],
        retire: bool,
    ) -> String {
        let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        let request = CombineRequest {
            inputs: inputs.iter().map(|r| self.path(r)).collect(),
            output: output.map(|o| self.path(o)),
            policy: OnConflict::Fail,
            decisions: host_decisions(&s(keep), &s(replace)).unwrap(),
            retire,
        };
        let outcome = request
            .commit(None, WriteOptions::default(), Some(self.dir.path()))
            .unwrap();
        let mut lines = Vec::new();
        for a in &outcome.report.added {
            lines.push(format!(
                "Host *: {} {} added from {}.",
                a.key,
                a.value,
                a.from.display()
            ));
        }
        for k in &outcome.report.skipped {
            lines.push(format!(
                "Host *: {} {} from {} skipped, keeping {}.",
                k.key,
                k.value,
                k.from.display(),
                k.kept
            ));
        }
        // The CLI prints this line to stderr as `warning: ...`; the UIs
        // put it in the status line.
        let moved = |p: &Path| {
            outcome
                .retired
                .iter()
                .any(|r| r.to.is_some() && r.from == p)
        };
        for w in outcome.report.includes.iter().filter(|w| !moved(&w.loads)) {
            lines.push(format!(
                "Warning: Include {} in {} still loads {}.",
                w.pattern,
                w.file.display(),
                w.loads.display()
            ));
        }
        lines.push(outcome.report.summary(Some(&outcome.output)));
        lines.extend(outcome.retired.iter().map(|r| r.message()));
        lines.join(" ")
    }
}

/// Clicks the last node labelled `label`, naming it when none is.
fn click(h: &mut Harness<'static, App>, label: &str) {
    h.get_all_by_label(label)
        .last()
        .unwrap_or_else(|| panic!("no node labelled {label:?}"))
        .click();
    h.run();
}

/// Clicks the sidebar's Combine… button.
fn open(h: &mut Harness<'static, App>) {
    click(h, "Combine…");
    assert!(h.state().combine().is_some(), "Combine dialog open");
}

fn disabled(h: &Harness<'static, App>, label: &str) -> bool {
    h.get_all_by_label(label)
        .last()
        .unwrap()
        .accesskit_node()
        .is_disabled()
}

/// Opens Combine with the root as base and legacy chosen.
fn open_legacy(h: &mut Harness<'static, App>) {
    open(h);
    click(h, "Merge ~/.ssh/config.d/legacy");
}

// TESTMASTER: id=uicmb-1 tier=? parallel=yes
#[test]
fn uicmb_1_gui_lists_loaded_files_and_path_entry() {
    let home = Home::new();
    let mut h = home.harness();
    open(&mut h);
    let labels: Vec<String> = h
        .state()
        .combine()
        .unwrap()
        .files()
        .iter()
        .map(|f| f.label.clone())
        .collect();
    assert_eq!(
        labels,
        [
            "~/.ssh/config",
            "~/.ssh/config.d/cypress",
            "~/.ssh/config.d/legacy"
        ]
    );
    for l in &labels {
        assert!(shown(&h, &format!("Base {l}")), "Base {l}");
        assert!(shown(&h, &format!("Merge {l}")), "Merge {l}");
    }
    assert_eq!(h.state().combine().unwrap().base(), 0);
    type_into(&mut h, "File to add", "~/.ssh/old/extra");
    click(&mut h, "Add");
    let v = h.state().combine().unwrap();
    let extra = v.files().last().unwrap();
    assert_eq!(extra.label, "~/.ssh/old/extra");
    assert!(extra.typed && extra.chosen);
    assert!(shown(&h, "Merge ~/.ssh/old/extra"));
    assert!(v.preview().is_some(), "two files give a preview");
    // A path that does not exist is refused at once.
    type_into(&mut h, "File to add", "~/.ssh/nope");
    click(&mut h, "Add");
    assert_eq!(
        h.state().combine().unwrap().error(),
        Some("~/.ssh/nope: no such file.")
    );
}

// TESTMASTER: id=uicmb-2 tier=? parallel=yes
#[test]
fn uicmb_2_gui_preview_shows_merge_without_writing() {
    let home = Home::new();
    let before = home.snapshot();
    let mut h = home.harness();
    open(&mut h);
    assert!(h.state().combine().unwrap().preview().is_none());
    assert!(disabled(&h, "Write"));
    click(&mut h, "Merge ~/.ssh/config.d/legacy");
    let p = h.state().combine().unwrap().preview().unwrap().clone();
    assert!(p.text.contains("Host printer"));
    assert!(p.text.contains("Compression yes"));
    let names: Vec<&str> = p.conflicts.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(names, ["nas", "vps"]);
    assert!(p.conflicts.iter().all(|c| c.decision.is_none()));
    assert!(shown_contains(
        &h,
        "Host *: Compression yes added from ~/.ssh/config.d/legacy."
    ));
    assert!(shown_contains(
        &h,
        "Host *: ServerAliveInterval 60 from ~/.ssh/config.d/legacy skipped, keeping 30."
    ));
    assert!(shown_contains(
        &h,
        "Include ~/.ssh/config.d/* in ~/.ssh/config still loads ~/.ssh/config.d/legacy."
    ));
    assert!(shown_contains(&h, "Conflicts: 2 · 2 undecided"));
    assert!(shown(&h, "Keep nas") && shown(&h, "Replace vps"));
    assert!(shown_contains(&h, "HostName old-vps.example.com"));
    assert_eq!(home.snapshot(), before, "a preview writes nothing");
}

// TESTMASTER: id=uicmb-3 tier=? parallel=yes
#[test]
fn uicmb_3_gui_decisions_match_cli() {
    let gui = Home::new();
    let cli = Home::new();
    let mut h = gui.harness();
    open_legacy(&mut h);
    click(&mut h, "Keep nas");
    click(&mut h, "Replace vps");
    assert!(!disabled(&h, "Write"));
    click(&mut h, "Write");
    assert!(
        h.state().combine().is_none(),
        "the dialog closes after a write"
    );
    let said = cli.cli_combine(
        &[".ssh/config", ".ssh/config.d/legacy"],
        None,
        &["nas"],
        &["vps"],
        false,
    );
    assert_eq!(
        gui.snapshot(),
        cli.snapshot(),
        "files and backups match the CLI"
    );
    let gui_said = h
        .state()
        .status()
        .replace(&gui.dir.path().display().to_string(), "HOME");
    assert_eq!(
        gui_said,
        said.replace(&cli.dir.path().display().to_string(), "HOME")
    );
    assert!(
        gui_said.contains(
            "Warning: Include ~/.ssh/config.d/* in HOME/.ssh/config still loads HOME/.ssh/config.d/legacy. combined 2 files"
        ),
        "the include warning precedes the summary: {gui_said}"
    );
    assert!(gui.read(".ssh/config").contains("old-vps.example.com"));
    assert!(gui.read(".ssh/config").contains("nas.local"));
    assert!(gui.path(".ssh/config~").exists());
    // The table shows the merged base.
    assert!(h.state().rows().iter().any(|r| r.name == "printer"));
}

// TESTMASTER: id=uicmb-4 tier=? parallel=yes
#[test]
fn uicmb_4_gui_cancel_or_esc_writes_nothing() {
    let home = Home::new();
    let before = home.snapshot();
    let mut h = home.harness();
    open_legacy(&mut h);
    click(&mut h, "Keep nas");
    click(&mut h, "Replace vps");
    click(&mut h, "Cancel");
    assert!(h.state().combine().is_none());
    open_legacy(&mut h);
    click(&mut h, "Keep nas");
    h.key_press(egui::Key::Escape);
    h.run();
    assert!(h.state().combine().is_none(), "Esc closes the dialog");
    assert_eq!(home.snapshot(), before, "no file and no backup written");
    assert!(!home.path(".ssh/config~").exists());
}

// TESTMASTER: id=uicmb-5 tier=? parallel=yes
#[test]
fn uicmb_5_gui_write_refused_until_every_conflict_is_decided() {
    let home = Home::new();
    let before = home.snapshot();
    let mut h = home.harness();
    open_legacy(&mut h);
    click(&mut h, "Keep nas");
    assert!(disabled(&h, "Write"));
    assert!(shown(&h, "Decide every conflict first"));
    assert!(!h.state_mut().combine_write(), "Write is refused");
    h.run();
    assert_eq!(home.snapshot(), before);
    assert_eq!(
        h.state().combine().unwrap().decision("nas"),
        Some(HostDecision::Keep)
    );
    click(&mut h, "Keep vps");
    assert!(!disabled(&h, "Write"));
}

// TESTMASTER: id=uicmb-6 tier=? parallel=yes
#[test]
fn uicmb_6_gui_output_to_another_path_leaves_base_untouched() {
    let gui = Home::new();
    let cli = Home::new();
    let mut h = gui.harness();
    open_legacy(&mut h);
    click(&mut h, "Another file:");
    assert!(disabled(&h, "Write"), "the output file needs a name");
    type_into(&mut h, "Output file", "~/.ssh/merged");
    click(&mut h, "Replace nas");
    click(&mut h, "Keep vps");
    click(&mut h, "Write");
    assert!(h.state().combine().is_none());
    assert_eq!(gui.read(".ssh/config"), ROOT, "base untouched");
    assert!(!gui.path(".ssh/config~").exists());
    cli.cli_combine(
        &[".ssh/config", ".ssh/config.d/legacy"],
        Some(".ssh/merged"),
        &["vps"],
        &["nas"],
        false,
    );
    assert_eq!(gui.snapshot(), cli.snapshot());
    assert!(gui.read(".ssh/merged").contains("192.168.1.10"));
}

// TESTMASTER: id=uicmb-7 tier=? parallel=yes
#[test]
fn uicmb_7_gui_retire_moves_inputs_as_cli_retire_and_table_refreshes() {
    let gui = Home::new();
    let cli = Home::new();
    let mut h = gui.harness();
    assert!(!h.state().rows().iter().any(|r| r.name == "extra"));
    open(&mut h);
    click(&mut h, "Merge ~/.ssh/config.d/cypress");
    type_into(&mut h, "File to add", "~/.ssh/old/extra");
    click(&mut h, "Add");
    click(&mut h, "Retire merged files");
    assert!(h.state().combine().unwrap().retire());
    assert!(shown_contains(
        &h,
        "~/.ssh/config.d/cypress moves to ~/.ssh/retired/"
    ));
    assert!(shown_contains(
        &h,
        "~/.ssh/old/extra stays: no Include loads it"
    ));
    click(&mut h, "Write");
    assert!(h.state().combine().is_none());
    let said = cli.cli_combine(
        &[".ssh/config", ".ssh/config.d/cypress", ".ssh/old/extra"],
        None,
        &[],
        &[],
        true,
    );
    assert_eq!(gui.snapshot(), cli.snapshot());
    assert!(gui.path(".ssh/retired/cypress").exists());
    assert!(!gui.path(".ssh/config.d/cypress").exists());
    assert!(gui.path(".ssh/old/extra").exists());
    let gui_said = h
        .state()
        .status()
        .replace(&gui.dir.path().display().to_string(), "HOME");
    assert_eq!(
        gui_said,
        said.replace(&cli.dir.path().display().to_string(), "HOME")
    );
    assert!(gui_said.contains("retired to HOME/.ssh/retired/cypress."));
    assert!(
        !gui_said.contains("Warning:"),
        "a retired input gets no include warning: {gui_said}"
    );
    // The table and Files list read the workspace again.
    let s = h.state();
    assert!(s.rows().iter().any(|r| r.name == "extra"));
    assert_eq!(
        s.rows().iter().filter(|r| r.name == "cypress").count(),
        1,
        "cypress is read once, from the root"
    );
    assert!(!(0..s.workspace().files.len())
        .any(|i| s.workspace().display(i).ends_with("config.d/cypress")));
}

// TESTMASTER: id=uicmb-8 tier=? parallel=yes
#[test]
fn uicmb_8_gui_stale_input_refused_and_preview_recomputed() {
    let home = Home::new();
    let mut h = home.harness();
    open_legacy(&mut h);
    click(&mut h, "Keep nas");
    click(&mut h, "Keep vps");
    let changed = LEGACY.replace("192.168.1.20", "192.168.1.99");
    home.write(".ssh/config.d/legacy", &changed);
    click(&mut h, "Write");
    let v = h.state().combine().expect("the dialog stays open");
    let e = v.error().unwrap();
    assert!(e.contains("legacy"), "names the changed input: {e}");
    assert!(
        v.preview().unwrap().text.contains("192.168.1.99"),
        "the preview is read again"
    );
    assert_eq!(home.read(".ssh/config"), ROOT, "nothing written");
    assert!(!home.path(".ssh/config~").exists());
    assert_eq!(
        v.decision("nas"),
        Some(HostDecision::Keep),
        "decisions kept"
    );
    click(&mut h, "Write");
    assert!(
        h.state().combine().is_none(),
        "the recomputed preview writes"
    );
    assert!(home.read(".ssh/config").contains("192.168.1.99"));
}

// TESTMASTER: id=uicmb-9 tier=? parallel=yes
#[test]
fn uicmb_9_gui_combine_disabled_while_editor_dirty() {
    let home = Home::new();
    let mut h = home.harness();
    h.state_mut().set_editor_text(format!("{ROOT}# edit\n"));
    h.run();
    assert!(disabled(&h, "Combine…"));
    h.state_mut().open_combine();
    h.run();
    assert!(h.state().combine().is_none(), "refused as Conflicts is");
    assert!(!shown(&h, "Combine files"));
}

/// File > Combine Files… and Edit > Clean Up Duplicates… open the same
/// dialogs as the sidebar buttons.
#[test]
fn gui_menu_opens_combine_and_cleanup() {
    let home = Home::new();
    let mut h = home.harness();
    click(&mut h, "File");
    click(&mut h, "Combine Files…");
    assert!(h.state().combine().is_some());
    click(&mut h, "Cancel");
    click(&mut h, "Edit");
    click(&mut h, "Clean Up Duplicates…");
    assert!(h.state().cleanup().is_some());
}

/// `--no-backup` (`App::with_write_options`): Combine writes the base
/// without `<file>~`, and the files match the CLI's `--no-backup` run.
#[test]
fn gui_no_backup_combine_writes_no_backup() {
    let gui = Home::new();
    let cli = Home::new();
    let app = App::with_env(gui.path(".ssh/config"), gui.env())
        .unwrap()
        .with_write_options(WriteOptions { no_backup: true });
    let mut h = Harness::builder()
        .with_size([1500.0, 1000.0])
        .build_ui_state(|ui, app: &mut App| app.show(ui), app);
    h.run();
    open_legacy(&mut h);
    click(&mut h, "Keep nas");
    click(&mut h, "Replace vps");
    click(&mut h, "Write");
    assert!(h.state().combine().is_none());
    assert!(!gui.path(".ssh/config~").exists());
    assert_eq!(h.state().last_backup(), None);
    let request = CombineRequest {
        inputs: vec![cli.path(".ssh/config"), cli.path(".ssh/config.d/legacy")],
        output: None,
        policy: OnConflict::Fail,
        decisions: host_decisions(&["nas".to_string()], &["vps".to_string()]).unwrap(),
        retire: false,
    };
    request
        .commit(None, WriteOptions { no_backup: true }, Some(cli.dir.path()))
        .unwrap();
    assert_eq!(gui.snapshot(), cli.snapshot());
}
