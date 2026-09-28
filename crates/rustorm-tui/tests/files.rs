//! The file list editor on a workspace of several files (docs/tui.md,
//! Files). Catalog cases: inc-tui-1 .. inc-tui-4.

mod common;
use common::*;
use crossterm::event::KeyCode;
use std::path::Path;
use std::process::Command;

/// Golden frames recorded from the TUI before the Include plan. Set
/// `RUSTORM_BLESS=1` to rewrite them (only ever from a pre-plan build).
fn golden(name: &str, actual: &str) {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join(name);
    if std::env::var_os("RUSTORM_BLESS").is_some() {
        std::fs::write(&path, actual).unwrap();
        return;
    }
    let want = std::fs::read_to_string(&path).unwrap();
    assert!(
        want == actual,
        "{name} differs from the pre-plan frame\n--- want\n{want}\n--- got\n{actual}"
    );
}

/// The screen with the fixture path replaced by `<CONFIG>` and the border
/// run after it trimmed, so the frame does not depend on the temp dir.
fn normalized(app: &mut rustorm_tui::App, f: &Fixture, w: u16, h: u16) -> String {
    let shown = [
        f.path.display().to_string(),
        std::env::var("HOME")
            .ok()
            .and_then(|home| {
                f.path
                    .strip_prefix(home)
                    .ok()
                    .map(|r| format!("~/{}", r.display()))
            })
            .unwrap_or_default(),
    ];
    lines(&draw_sized(app, w, h))
        .into_iter()
        .map(|l| {
            match shown
                .iter()
                .find(|p| !p.is_empty() && l.contains(p.as_str()))
            {
                Some(p) => l
                    .replace(p.as_str(), "<CONFIG>")
                    .trim_end_matches(['─', '┐', '═', '╗'])
                    .to_string(),
                None => l,
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// inc-tui-4: a config without Include opens exactly as before the plan:
/// no file list, no file column, the same help bar, keys and overlay.
#[test]
fn inc_tui_4_no_include_frame_matches_pre_plan() {
    let f = Fixture::new(&three_hosts());
    let mut app = f.app();
    golden("three_hosts_150x40.txt", &normalized(&mut app, &f, 150, 40));
    // Tab cycle: sections, table, editor; each help bar as before.
    let mut bars = Vec::new();
    for _ in 0..3 {
        app.handle(key(KeyCode::Tab));
        bars.push(format!("{:?}", app.focus()));
        bars.push(lines(&draw_sized(&mut app, 150, 40)).pop().unwrap());
    }
    app.handle(key(KeyCode::Esc));
    press(&mut app, '?');
    bars.push(normalized(&mut app, &f, 150, 40));
    golden("three_hosts_keys.txt", &bars.join("\n"));
    // F is not a key on a single file: it neither moves focus nor opens anything.
    press(&mut app, '?');
    let before = normalized(&mut app, &f, 150, 40);
    press(&mut app, 'F');
    assert_eq!(normalized(&mut app, &f, 150, 40), before);
    let s = screen(&mut app);
    assert!(!s.contains("Files"), "{s}");
    let table = table_lines(&draw(&mut app)).join("\n");
    assert!(!table.contains("File "), "{table}");
}

#[test]
fn inc_tui_4_no_include_six_hosts_frame_matches_pre_plan() {
    let f = Fixture::new(&six_hosts());
    let mut app = f.app();
    golden("six_hosts_150x60.txt", &normalized(&mut app, &f, W, H));
}

/// inc-tui-4 on a real pty: the no-Include fixture draws the same bytes
/// as before the plan. Skipped when `expect` is not installed.
#[test]
fn inc_tui_4_pty_no_include_frame_matches_pre_plan() {
    if Command::new("expect").arg("-v").output().is_err() {
        eprintln!("expect not installed; skipping the pty run");
        return;
    }
    let f = Fixture::new(&three_hosts());
    let out = Command::new("expect")
        .arg("-f")
        .arg(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/pty_frame.exp"))
        .arg(env!("CARGO_BIN_EXE_rustorm-tui"))
        .arg(&f.path)
        .env("HOME", f.dir.path())
        .env("TERM", "xterm-256color")
        .env("LANG", "en_US.UTF-8")
        .env_remove("LC_ALL")
        .env_remove("LC_CTYPE")
        .env_remove("NO_COLOR")
        .env_remove("RUSTORM_CONFIG")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "pty run failed: {}",
        String::from_utf8_lossy(&out.stdout)
    );
    let raw = String::from_utf8_lossy(&out.stdout).to_string();
    let start = raw.find("\x1b[?1049h").expect("alternate screen entered");
    let end = raw[start..]
        .find("\x1b[?1049l")
        .map(|e| start + e)
        .expect("alternate screen left");
    let frame = raw[start..end].replace("\r\n", "\n");
    golden("three_hosts_pty.ansi", &frame);
    assert_eq!(f.read(), three_hosts(), "quitting writes nothing");
}

// ----- a workspace of three files -----

use rustorm_core::{AddSpec, Config, Workspace};
use std::collections::BTreeMap;
use std::path::PathBuf;

/// A root that includes `config.d/*`, which holds `cypress` and `ranch`;
/// both hold a section `lab`.
struct Multi {
    dir: tempfile::TempDir,
    root: PathBuf,
    cypress: PathBuf,
    ranch: PathBuf,
}

fn sectioned(base: &str, hosts: &[(&str, &str)]) -> String {
    let mut c = Config::parse(base).unwrap();
    for (name, uri) in hosts {
        c.add(
            &AddSpec {
                name: name.to_string(),
                uri: uri.to_string(),
                section: Some("lab".into()),
                ..Default::default()
            },
            &env(),
        )
        .unwrap();
    }
    c.render()
}

impl Multi {
    fn new() -> Multi {
        let dir = tempfile::tempdir().unwrap();
        let d = dir.path().join("config.d");
        std::fs::create_dir(&d).unwrap();
        let root = dir.path().join("config");
        std::fs::write(
            &root,
            format!(
                "# root\nInclude {}/*\n\nHost github\n    HostName github.com\n    User git\n",
                d.display()
            ),
        )
        .unwrap();
        let cypress = d.join("cypress");
        std::fs::write(
            &cypress,
            sectioned(
                "Host cypressPro\n    HostName 10.0.0.2\n    User travis\n",
                &[("cypress-lab", "root@lab.cypress.example.com")],
            ),
        )
        .unwrap();
        let ranch = d.join("ranch");
        std::fs::write(
            &ranch,
            sectioned(
                "",
                &[
                    ("dcevant", "admin@dcevant.example.com"),
                    ("ranch-nas", "nas@10.1.0.9:2222"),
                ],
            ),
        )
        .unwrap();
        Multi {
            dir,
            root,
            cypress,
            ranch,
        }
    }

    fn app(&self) -> rustorm_tui::App {
        rustorm_tui::App::with_options(&self.root, options()).unwrap()
    }

    /// Every file under the temp dir, hidden ones included, with its bytes.
    fn snapshot(&self) -> BTreeMap<PathBuf, Vec<u8>> {
        fn walk(dir: &Path, out: &mut BTreeMap<PathBuf, Vec<u8>>) {
            for e in std::fs::read_dir(dir).unwrap() {
                let p = e.unwrap().path();
                if p.is_dir() {
                    walk(&p, out);
                } else {
                    out.insert(p.clone(), std::fs::read(&p).unwrap());
                }
            }
        }
        let mut out = BTreeMap::new();
        walk(self.dir.path(), &mut out);
        out
    }

    /// Where the core backs up `path` (core's per-file backup rule).
    fn backup_of(&self, path: &Path) -> PathBuf {
        let ws = Workspace::load_with_home(&self.root, None).unwrap();
        let i = ws.files.iter().position(|f| f.path == path).unwrap();
        ws.backup_path_for(i)
    }
}

fn read(p: &Path) -> String {
    std::fs::read_to_string(p).unwrap()
}

/// Focuses the file list and shows the file at `pos` in load order.
fn open_file(app: &mut rustorm_tui::App, pos: usize) {
    press(app, 'F');
    assert_eq!(app.focus(), rustorm_tui::Focus::Files);
    press(app, 'g');
    for _ in 0..pos {
        press(app, 'j');
    }
    app.handle(key(KeyCode::Enter));
    assert_eq!(app.focus(), rustorm_tui::Focus::Editor);
}

/// inc-tui-1: selecting a file in the file list loads it into the editor
/// with its path in the title; Ctrl-S writes that file and its backup
/// and nothing else, while another file's unsaved buffer keeps its edits.
#[test]
fn inc_tui_1_file_list_selects_file_and_ctrl_s_writes_only_it() {
    let m = Multi::new();
    let mut app = m.app();
    assert!(app.is_multi());
    assert_eq!(app.files(), vec![&*m.root, &*m.cypress, &*m.ranch]);
    let s = screen(&mut app);
    assert!(s.contains("Files"), "{s}");
    let table = table_lines(&draw(&mut app)).join("\n");
    assert!(table.contains("File"), "{table}");
    for name in ["cypress", "ranch", "github", "ranch-nas"] {
        assert!(table.contains(name), "{name}: {table}");
    }

    // An unsaved edit in the root stays unsaved throughout.
    open_file(&mut app, 0);
    assert_eq!(app.shown_file(), m.root.as_path());
    typ(&mut app, "# root edit\n");
    app.handle(key(KeyCode::Esc));
    assert_eq!(app.dirty_files(), vec![m.root.as_path()]);

    let before = m.snapshot();
    let cypress_old = read(&m.cypress);
    open_file(&mut app, 1);
    assert_eq!(app.shown_file(), m.cypress.as_path());
    assert_eq!(app.editor_text(), cypress_old);
    let s = screen(&mut app);
    let title = s.lines().find(|l| l.contains("Editor ")).unwrap();
    assert!(title.contains("cypress"), "{title}");
    // The root is marked dirty in the file list.
    assert!(
        s.lines().any(|l| l.contains("• ") && l.contains("config ")),
        "{s}"
    );

    assert_eq!(app.focus(), rustorm_tui::Focus::Editor);
    typ(&mut app, "# cypress edit\n");
    app.handle(ctrl('s'));
    let msg = app.message().unwrap();
    assert!(
        msg.starts_with("✔ Saved ") && msg.contains("cypress"),
        "{msg}"
    );

    let after = m.snapshot();
    let backup = m.backup_of(&m.cypress);
    // `config.d/cypress~` would match `Include config.d/*`, so the core
    // backs up to `config.d/.cypress~` instead.
    assert_eq!(backup, m.dir.path().join("config.d/.cypress~"));
    let changed: Vec<&PathBuf> = after
        .keys()
        .filter(|p| before.get(*p) != after.get(*p))
        .collect();
    assert_eq!(changed, {
        let mut v = vec![&m.cypress, &backup];
        v.sort();
        v
    });
    assert!(read(&m.cypress).starts_with("# cypress edit\n"));
    assert_eq!(read(&backup), cypress_old);
    assert_eq!(app.dirty_files(), vec![m.root.as_path()]);
    assert!(!after.contains_key(&m.dir.path().join("config~")));
}

/// inc-tui-2: `o` on a host from an included file shows that file with
/// the cursor on its Host line.
#[test]
fn inc_tui_2_o_opens_the_file_holding_the_host_at_its_host_line() {
    let m = Multi::new();
    let mut app = m.app();
    assert_eq!(app.shown_file(), m.root.as_path());
    select(&mut app, "ranch-nas");
    press(&mut app, 'o');
    assert_eq!(app.focus(), rustorm_tui::Focus::Editor);
    assert_eq!(app.shown_file(), m.ranch.as_path());
    let text = read(&m.ranch);
    let line = text
        .lines()
        .position(|l| l.trim() == "Host ranch-nas")
        .unwrap();
    assert_eq!(app.editor_cursor(), (line, 0));
    let s = screen(&mut app);
    let title = s.lines().find(|l| l.contains("Editor ")).unwrap();
    assert!(title.contains("ranch"), "{title}");
    // A root host goes back to the root.
    app.handle(key(KeyCode::Esc));
    select(&mut app, "github");
    press(&mut app, 'o');
    assert_eq!(app.shown_file(), m.root.as_path());
    assert_eq!(app.editor_cursor(), (3, 0));
}

fn two_dirty(m: &Multi) -> rustorm_tui::App {
    let mut app = m.app();
    open_file(&mut app, 0);
    typ(&mut app, "# root edit\n");
    app.handle(key(KeyCode::Esc));
    open_file(&mut app, 2);
    typ(&mut app, "# ranch edit\n");
    app.handle(key(KeyCode::Esc));
    assert_eq!(app.dirty_files(), vec![m.root.as_path(), m.ranch.as_path()]);
    press(&mut app, 'q');
    assert!(!app.should_quit());
    let s = screen(&mut app);
    assert!(s.contains("Unsaved changes in "), "{s}");
    assert!(s.contains("[s]ave all / [d]iscard all / [c]ancel"), "{s}");
    let prompt: String = s
        .lines()
        .filter(|l| l.contains('║'))
        .collect::<Vec<_>>()
        .join("");
    assert!(prompt.contains("config,"), "{prompt}");
    assert!(prompt.contains("ranch."), "{prompt}");
    app
}

/// inc-tui-3: `q` with two dirty buffers asks once, listing both; `s`
/// saves both and quits.
#[test]
fn inc_tui_3_quit_with_two_dirty_files_save_all_writes_both() {
    let m = Multi::new();
    let root_old = read(&m.root);
    let ranch_old = read(&m.ranch);
    let cypress_old = read(&m.cypress);
    let mut app = two_dirty(&m);
    press(&mut app, 's');
    assert!(app.should_quit());
    assert_eq!(read(&m.root), format!("# root edit\n{root_old}"));
    assert_eq!(read(&m.ranch), format!("# ranch edit\n{ranch_old}"));
    assert_eq!(read(&m.cypress), cypress_old);
    assert_eq!(read(&m.backup_of(&m.root)), root_old);
    assert_eq!(read(&m.backup_of(&m.ranch)), ranch_old);
}

/// inc-tui-3: `d` discards both and writes neither; `c` returns.
#[test]
fn inc_tui_3_quit_with_two_dirty_files_discard_all_writes_neither() {
    let m = Multi::new();
    let before = m.snapshot();
    let mut app = two_dirty(&m);
    press(&mut app, 'c');
    assert!(!app.should_quit());
    assert_eq!(app.dirty_files().len(), 2);
    press(&mut app, 'q');
    press(&mut app, 'd');
    assert!(app.should_quit());
    assert_eq!(m.snapshot(), before);
}

/// inc-tui-3: a save refused during save-all leaves the TUI open on that file.
#[test]
fn inc_tui_3_refused_save_keeps_the_tui_open_on_that_file() {
    let m = Multi::new();
    let mut app = m.app();
    open_file(&mut app, 1);
    typ(&mut app, "Host\n");
    app.handle(key(KeyCode::Esc));
    open_file(&mut app, 2);
    typ(&mut app, "# ranch edit\n");
    app.handle(key(KeyCode::Esc));
    let before = m.snapshot();
    press(&mut app, 'q');
    press(&mut app, 's');
    assert!(!app.should_quit());
    assert_eq!(app.shown_file(), m.cypress.as_path());
    assert_eq!(app.focus(), rustorm_tui::Focus::Editor);
    assert!(app
        .message()
        .unwrap()
        .starts_with("Error: Not saved: line 1"));
    assert_eq!(m.snapshot(), before);
}

/// One dirty file in a workspace of several keeps the single-file prompt.
#[test]
fn quit_with_one_dirty_file_uses_the_single_prompt() {
    let m = Multi::new();
    let mut app = m.app();
    open_file(&mut app, 1);
    typ(&mut app, "# c\n");
    app.handle(key(KeyCode::Esc));
    press(&mut app, 'q');
    let s = screen(&mut app);
    assert!(
        s.contains("The editor has unsaved changes. [s]ave / [d]iscard / [c]ancel"),
        "{s}"
    );
    press(&mut app, 's');
    assert!(app.should_quit());
    assert!(read(&m.cypress).starts_with("# c\n"));
}

#[test]
fn file_column_sorts_with_1_and_filters_with_f_1() {
    let m = Multi::new();
    let mut app = m.app();
    let all = [
        "github",
        "cypressPro",
        "cypress-lab",
        "dcevant",
        "ranch-nas",
    ];
    // File order: root, cypress, ranch.
    assert_eq!(drawn_order(&mut app, &all)[0], "github");
    press(&mut app, '1');
    press(&mut app, '1');
    assert_eq!(drawn_order(&mut app, &all)[..2], ["dcevant", "ranch-nas"]);
    press(&mut app, '0');
    press(&mut app, 'f');
    press(&mut app, '1');
    typ(&mut app, "ranch\n");
    assert_eq!(drawn_order(&mut app, &all), ["dcevant", "ranch-nas"]);
    assert!(screen(&mut app).contains("filter: file~ranch"));
    // 8 sorts by jump; the section column moved to 2.
    press(&mut app, 'x');
    press(&mut app, '8');
    assert!(table_lines(&draw(&mut app)).join("").contains("Jump▲"));
}

#[test]
fn shared_section_shows_once_per_file_and_add_to_it_is_refused() {
    let m = Multi::new();
    let mut app = m.app();
    let s = screen(&mut app);
    assert!(s.contains("lab cypress"), "{s}");
    assert!(s.contains("lab ranch"), "{s}");
    let before = m.snapshot();
    press(&mut app, 'a');
    typ(&mut app, "newbox");
    app.handle(key(KeyCode::Tab));
    typ(&mut app, "root@newbox.example.com");
    app.handle(key(KeyCode::Tab));
    app.handle(key(KeyCode::Tab));
    typ(&mut app, "lab\n");
    let s = screen(&mut app);
    assert!(s.contains("Error: section lab exists in "), "{s}");
    assert_eq!(m.snapshot(), before);
}

#[test]
fn edit_routes_to_the_file_holding_the_host_and_names_it() {
    let m = Multi::new();
    let root_old = read(&m.root);
    let cypress_old = read(&m.cypress);
    let mut app = m.app();
    select(&mut app, "ranch-nas");
    press(&mut app, 'e');
    backspace(&mut app, 40);
    typ(&mut app, "nas@10.1.0.10:2222\n");
    let msg = app.message().unwrap();
    assert!(msg.starts_with("✔ ranch-nas updated in "), "{msg}");
    assert!(msg.ends_with("ranch."), "{msg}");
    assert!(read(&m.ranch).contains("HostName 10.1.0.10"));
    assert_eq!(read(&m.root), root_old);
    assert_eq!(read(&m.cypress), cypress_old);
    assert!(m.backup_of(&m.ranch).exists());
    // The editor buffer of that file shows the new text.
    select(&mut app, "ranch-nas");
    press(&mut app, 'o');
    assert!(app.editor_text().contains("HostName 10.1.0.10"));
}

#[test]
fn forms_refuse_while_any_buffer_is_dirty() {
    let m = Multi::new();
    let mut app = m.app();
    open_file(&mut app, 2);
    typ(&mut app, "# x\n");
    app.handle(key(KeyCode::Esc));
    open_file(&mut app, 0);
    app.handle(key(KeyCode::Esc));
    assert!(!app.is_editor_modified());
    press(&mut app, 'a');
    assert_eq!(
        app.message().unwrap(),
        "Error: Save or discard the editor's changes first."
    );
}

#[test]
fn unreadable_include_is_listed_and_cannot_be_opened() {
    use std::os::unix::fs::PermissionsExt;
    let m = Multi::new();
    let private = m.dir.path().join("config.d/private");
    std::fs::write(&private, "Host secret\n").unwrap();
    std::fs::set_permissions(&private, std::fs::Permissions::from_mode(0o000)).unwrap();
    if std::fs::read(&private).is_ok() {
        return; // running as root
    }
    let mut app = m.app();
    let s = screen(&mut app);
    assert!(s.contains("cannot read"), "{s}");
    open_file_expect_error(&mut app, 2);
    assert!(app.message().unwrap().starts_with("Error: Cannot open "));
    assert_ne!(app.shown_file(), private.as_path());
}

fn open_file_expect_error(app: &mut rustorm_tui::App, pos: usize) {
    press(app, 'F');
    press(app, 'g');
    for _ in 0..pos {
        press(app, 'j');
    }
    app.handle(key(KeyCode::Enter));
    assert_eq!(app.focus(), rustorm_tui::Focus::Files);
}

#[test]
fn tab_cycle_includes_the_file_list_first() {
    let m = Multi::new();
    let mut app = m.app();
    let mut seen = Vec::new();
    for _ in 0..4 {
        app.handle(key(KeyCode::Tab));
        seen.push(app.focus());
    }
    use rustorm_tui::Focus::*;
    assert_eq!(seen, vec![Editor, Files, Sections, Table]);
    press(&mut app, '?');
    let s = screen(&mut app);
    assert!(s.contains("file list"), "{s}");
    assert!(s.contains("1-8"), "{s}");
}
