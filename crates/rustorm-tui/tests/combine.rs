//! The Combine view (`M`): two or more files merged into one, each
//! conflict decided before the write, through the core's combine
//! (docs/cli.md, combine). Catalog cases: uicmb-1 .. uicmb-9.

mod common;
use common::*;
use crossterm::event::KeyCode;
use rustorm_core::{CombineRequest, Env, HostDecision, HostDecisions, WriteOptions};
use rustorm_tui::App;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

const ROOT: &str = "Include ~/.ssh/config.d/*\n\nHost *\n    ServerAliveInterval 30\n\nHost nas\n    HostName nas.local\n\nHost vps\n    HostName vps.example.com\n    User root\n";
const LEGACY: &str = "Host *\n    ServerAliveInterval 60\n    Compression yes\n\nHost vps\n    HostName old-vps.example.com\n    User admin\n\nHost nas\n    HostName 192.168.1.10\n\nHost printer\n    HostName 192.168.1.20\n";

/// A temporary home: `~/.ssh/config` including `~/.ssh/config.d/*`, which
/// holds `legacy`; nas and vps are in both.
struct Home {
    dir: tempfile::TempDir,
}

impl Home {
    fn new() -> Home {
        let home = Home {
            dir: tempfile::tempdir().unwrap(),
        };
        home.write(".ssh/config", ROOT);
        home.write(".ssh/config.d/legacy", LEGACY);
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
    /// `rustorm combine ~/.ssh/config ~/.ssh/config.d/legacy` with
    /// `--keep`/`--replace`, `-o` and `--retire`, through the core call the
    /// CLI makes.
    fn cli_combine(&self, decisions: &[(&str, HostDecision)], output: Option<&str>, retire: bool) {
        let request = CombineRequest {
            inputs: vec![self.path(".ssh/config"), self.path(".ssh/config.d/legacy")],
            output: output.map(|o| self.path(o)),
            decisions: decisions
                .iter()
                .map(|(h, d)| (h.to_string(), *d))
                .collect::<HostDecisions>(),
            retire,
            ..CombineRequest::default()
        };
        request
            .commit(None, WriteOptions::default(), Some(self.dir.path()))
            .unwrap();
    }
}

/// Opens Combine, picks `legacy` as input 1 and previews.
fn preview(app: &mut App) {
    press(app, 'M');
    assert_eq!(app.view_name(), Some("combine files"));
    app.handle(key(KeyCode::Down));
    press(app, ' ');
    app.handle(key(KeyCode::Enter));
    assert_eq!(
        app.view_name(),
        Some("combine preview"),
        "{:?}",
        app.message()
    );
}

/// Highlights the conflict `host` in the preview.
fn goto(app: &mut App, host: &str) {
    press(app, 'g');
    for _ in 0..10 {
        if app.combine_conflict().as_deref() == Some(host) {
            return;
        }
        app.handle(key(KeyCode::Down));
    }
    panic!("{host} not a conflict");
}

/// Decides nas keep and vps replace.
fn decide(app: &mut App) {
    goto(app, "nas");
    press(app, 'k');
    goto(app, "vps");
    press(app, 'r');
}

/// uicmb-1
#[test]
fn uicmb_1_lists_loaded_files_and_a_path_entry() {
    let h = Home::new();
    h.write("old/extra", "Host extra\n    HostName extra.example.com\n");
    let mut app = h.app();
    press(&mut app, 'M');
    let s = screen(&mut app);
    assert!(s.contains("Combine  pick the base"), "{s}");
    assert!(s.contains("base      ~/.ssh/config "), "{s}");
    assert!(s.contains("          ~/.ssh/config.d/legacy"), "{s}");
    assert!(s.contains("output    ~/.ssh/config (the base)"), "{s}");
    press(&mut app, 't');
    assert!(screen(&mut app).contains("Add file: "));
    typ(&mut app, "~/old/extra\n");
    let s = screen(&mut app);
    assert!(s.contains("input 1   ~/old/extra"), "{s}");
    press(&mut app, 't');
    typ(&mut app, "~/old/none\n");
    assert_eq!(
        app.message().as_deref(),
        Some("Error: ~/old/none is not a file. Type the path of a config file.")
    );
}

/// uicmb-2
#[test]
fn uicmb_2_preview_shows_text_defaults_and_conflicts_and_writes_nothing() {
    let h = Home::new();
    let before = h.snapshot();
    let mut app = h.app();
    preview(&mut app);
    let s = screen(&mut app);
    assert!(
        s.contains("Host *: Compression yes added from ~/.ssh/config.d/legacy."),
        "{s}"
    );
    assert!(
        s.contains(
            "Host *: ServerAliveInterval 60 from ~/.ssh/config.d/legacy skipped, keeping 30."
        ),
        "{s}"
    );
    assert!(s.contains("2 conflicts, 2 undecided"), "{s}");
    assert!(s.contains("undecided"), "{s}");
    assert!(s.contains("merged text"), "{s}");
    assert!(
        s.contains("HostName 192.168.1.20"),
        "the merged text shows printer: {s}"
    );
    assert_eq!(h.snapshot(), before, "a preview writes nothing");
}

/// uicmb-3
#[test]
fn uicmb_3_decisions_match_cli() {
    let tui = Home::new();
    let cli = Home::new();
    let mut app = tui.app();
    preview(&mut app);
    decide(&mut app);
    assert!(screen(&mut app).contains("2 conflicts, 0 undecided"));
    app.handle(key(KeyCode::Enter));
    let msg = app.message().unwrap();
    assert_eq!(
        app.view_name(),
        None,
        "the view closes after the write: {msg}"
    );
    assert!(
        msg.contains("combined 2 files into") && msg.contains("3 hosts, 0 sections, 2 conflicts."),
        "{msg}"
    );
    cli.cli_combine(
        &[("nas", HostDecision::Keep), ("vps", HostDecision::Replace)],
        None,
        false,
    );
    assert_eq!(
        tui.snapshot(),
        cli.snapshot(),
        "files and backups match the CLI's"
    );
    assert!(tui.read(".ssh/config").contains("old-vps.example.com"));
}

/// uicmb-4
#[test]
fn uicmb_4_esc_writes_nothing() {
    let h = Home::new();
    let before = h.snapshot();
    let mut app = h.app();
    preview(&mut app);
    decide(&mut app);
    app.handle(key(KeyCode::Esc));
    assert_eq!(app.view_name(), Some("combine files"));
    app.handle(key(KeyCode::Esc));
    assert_eq!(app.view_name(), None);
    assert_eq!(
        app.message().as_deref(),
        Some("Combine cancelled; nothing written.")
    );
    assert_eq!(h.snapshot(), before, "no file and no backup written");
}

/// uicmb-5
#[test]
fn uicmb_5_write_refused_until_each_conflict_is_decided() {
    let h = Home::new();
    let before = h.snapshot();
    let mut app = h.app();
    preview(&mut app);
    goto(&mut app, "nas");
    press(&mut app, 'k');
    app.handle(key(KeyCode::Enter));
    assert_eq!(
        app.message().as_deref(),
        Some("Error: 1 conflict is left undecided; nothing written. Press k to keep or r to replace each one.")
    );
    assert_eq!(app.view_name(), Some("combine preview"));
    assert_eq!(h.snapshot(), before);
    goto(&mut app, "vps");
    press(&mut app, 'r');
    app.handle(key(KeyCode::Enter));
    assert_eq!(app.view_name(), None, "{:?}", app.message());
    assert_ne!(h.snapshot(), before);
}

/// uicmb-6
#[test]
fn uicmb_6_output_elsewhere_leaves_the_base() {
    let tui = Home::new();
    let cli = Home::new();
    let mut app = tui.app();
    press(&mut app, 'M');
    app.handle(key(KeyCode::Down));
    press(&mut app, ' ');
    press(&mut app, 'o');
    assert!(screen(&mut app).contains("Output (empty: the base): "));
    typ(&mut app, "~/merged\n");
    assert!(screen(&mut app).contains("output    ~/merged"));
    app.handle(key(KeyCode::Enter));
    decide(&mut app);
    app.handle(key(KeyCode::Enter));
    assert_eq!(app.view_name(), None, "{:?}", app.message());
    assert_eq!(tui.read(".ssh/config"), ROOT, "the base is untouched");
    cli.cli_combine(
        &[("nas", HostDecision::Keep), ("vps", HostDecision::Replace)],
        Some("merged"),
        false,
    );
    assert_eq!(tui.snapshot(), cli.snapshot());
}

/// uicmb-7
#[test]
fn uicmb_7_retire_moves_inputs_as_cli_and_refreshes_the_table() {
    let tui = Home::new();
    let cli = Home::new();
    let mut app = tui.app();
    assert_eq!(app.files().len(), 2);
    preview(&mut app);
    decide(&mut app);
    press(&mut app, 'R');
    assert_eq!(
        app.message().as_deref(),
        Some("Retire on: ~/.ssh/config.d/legacy moves to ~/.ssh/retired/ after the write.")
    );
    let s = screen(&mut app);
    assert!(
        s.contains("retire: on; moves ~/.ssh/config.d/legacy"),
        "{s}"
    );
    assert!(
        !s.contains("Warning: Include"),
        "a retiring input has no warning: {s}"
    );
    app.handle(key(KeyCode::Enter));
    let msg = app.message().unwrap();
    assert!(msg.contains("retired to"), "{msg}");
    assert!(tui.path(".ssh/retired/legacy").is_file());
    assert!(!tui.path(".ssh/config.d/legacy").exists());
    cli.cli_combine(
        &[("nas", HostDecision::Keep), ("vps", HostDecision::Replace)],
        None,
        true,
    );
    assert_eq!(tui.snapshot(), cli.snapshot());
    assert_eq!(app.files().len(), 1, "the workspace is read again");
    let names: Vec<String> = app.visible_rows().iter().map(|r| r.name.clone()).collect();
    assert!(names.contains(&"printer".to_string()), "{names:?}");
}

/// uicmb-8
#[test]
fn uicmb_8_stale_input_refused_and_preview_recomputed() {
    let h = Home::new();
    let mut app = h.app();
    preview(&mut app);
    decide(&mut app);
    let changed = format!("{LEGACY}\nHost scanner\n    HostName 192.168.1.30\n");
    h.write(".ssh/config.d/legacy", &changed);
    let before = h.snapshot();
    app.handle(key(KeyCode::Enter));
    let msg = app.message().unwrap();
    assert!(
        msg.starts_with("Error: ")
            && msg.contains("changed since the preview; the preview is read again."),
        "{msg}"
    );
    assert_eq!(app.view_name(), Some("combine preview"));
    assert_eq!(h.snapshot(), before, "nothing written");
    assert!(
        screen(&mut app).contains("HostName 192.168.1.30"),
        "the preview shows the new text"
    );
    app.handle(key(KeyCode::Enter));
    assert_eq!(app.view_name(), None, "{:?}", app.message());
    assert!(h.read(".ssh/config").contains("Host scanner"));
}

/// uicmb-9
#[test]
fn uicmb_9_refused_while_the_editor_holds_unsaved_text() {
    let h = Home::new();
    let before = h.snapshot();
    let mut app = h.app();
    press(&mut app, 'o');
    typ(&mut app, "x");
    app.handle(key(KeyCode::Esc));
    assert!(app.is_editor_modified());
    press(&mut app, 'M');
    assert_eq!(app.view_name(), None);
    assert_eq!(
        app.message().as_deref(),
        Some("Error: Save or discard the editor's changes first.")
    );
    press(&mut app, 'X');
    assert_eq!(app.view_name(), None, "Cleanup is refused too");
    assert_eq!(
        app.message().as_deref(),
        Some("Error: Save or discard the editor's changes first.")
    );
    assert_eq!(h.snapshot(), before);
}

#[test]
fn combine_needs_two_files() {
    let h = Home::new();
    let mut app = h.app();
    press(&mut app, 'M');
    app.handle(key(KeyCode::Enter));
    assert_eq!(
        app.message().as_deref(),
        Some("Error: Pick a base and at least one more file. Space picks the highlighted file.")
    );
    assert_eq!(app.view_name(), Some("combine files"));
}

#[test]
fn combine_b_makes_the_highlighted_file_the_base() {
    let h = Home::new();
    let mut app = h.app();
    press(&mut app, 'M');
    app.handle(key(KeyCode::Down));
    press(&mut app, 'b');
    let s = screen(&mut app);
    assert!(s.contains("input 1   ~/.ssh/config "), "{s}");
    assert!(s.contains("base      ~/.ssh/config.d/legacy"), "{s}");
}

#[test]
fn combine_help_overlay_lists_its_keys() {
    let h = Home::new();
    let mut app = h.app();
    press(&mut app, 'M');
    press(&mut app, '?');
    let s = screen(&mut app);
    assert!(s.contains("keep earlier / replace with later"), "{s}");
    app.handle(key(KeyCode::Esc));
    assert_eq!(app.view_name(), Some("combine files"));
}
