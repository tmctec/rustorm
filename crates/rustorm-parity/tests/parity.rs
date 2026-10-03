//! TUI / GUI parity (catalog par-1 .. par-18): each scenario runs one
//! operation in `rustorm-tui` (key events) and in `rustorm-gui` (kittest
//! clicks and typing) on identical workspaces, and in `rustorm-core`
//! directly on a third copy. All three must leave byte-identical files
//! and backups, and the two UIs must report the same message. The
//! reconcile scenarios (par-16 ..) also run the `rustorm` binary on a
//! fourth copy under a temporary HOME.

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::OnceLock;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;
use ratatui::backend::TestBackend;
use ratatui::Terminal;
use rustorm_core::{
    AddSpec, CloneSpec, Config, Decision, EditSpec, Env, HostSelector, PairKind, SettingChange,
    Workspace, WriteOptions,
};
use rustorm_gui::App as Gui;
use rustorm_tui::{App as Tui, Focus, Options, Theme};

fn env() -> Env {
    Env {
        user: Some("tester".into()),
        home: None,
    }
}

// ----- the workspace -----

const RANCH: &str = "\
# ranch machines
Host dcevant
    HostName dcevant.ranch.lan

Host ranch-nas
    HostName nas.ranch.lan
    User admin
    IdentityFile ~/.ssh/nas
";

/// `lab` in section `bench`, `lab2` in the catch-all `other`.
fn lab_text() -> String {
    let mut c = Config::parse(
        "Host lab\n    HostName lab.example.com\n    Compression yes\n    LocalForward 8080 localhost:80\n\nHost lab2\n    HostName lab2.example.com\n",
    )
    .unwrap();
    c.move_host("lab", None, Some("bench")).unwrap();
    c.render()
}

// docs/cli.md's reconcile example: `cypress.bak` is a stray copy of
// `cypress` with `cypressPro` changed, `cypressPro-ext` identical, `lab-1`
// without its location, and the orphan `printer`.
const RECONCILE_ROOT: &str =
    "Include ~/.ssh/config.d/*\n\nHost github\n    HostName github.com\n    User git\n";
const CYPRESS: &str = "Host cypressPro\n    HostName 10.10.0.2\n    User travis\n\nHost cypressPro-ext\n    HostName cypress.example.com\n    User travis\n    Port 2222\n\n# location: Austin DC, rack 4\nHost lab-1\n    HostName 10.10.0.30\n    User travis\n";
const BAK: &str = "Host cypressPro\n    HostName 10.10.0.9\n    User travis\n\n# old box\nHost cypressPro-ext\n    hostname   cypress.example.com\n    User travis\n    Port 2222\n\nHost lab-1\n    HostName 10.10.0.30\n    User travis\n\nHost printer\n    HostName 192.168.1.20\n";

/// A root that includes `config.d/*`, plus `ranch` and `lab`; or, from
/// [`Fixture::reconcile`], a temporary home holding `~/.ssh`.
struct Fixture {
    dir: tempfile::TempDir,
    root: PathBuf,
    /// The home directory `~` stands for, when the fixture is one.
    home: Option<PathBuf>,
}

impl Fixture {
    fn new() -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        let d = dir.path().join("config.d");
        std::fs::create_dir(&d).unwrap();
        std::fs::write(d.join("ranch"), RANCH).unwrap();
        std::fs::write(d.join("benchfile"), lab_text()).unwrap();
        let root = dir.path().join("config");
        std::fs::write(
            &root,
            format!(
                "# root\nInclude {}/*\n\nHost *\n    ServerAliveInterval 60\n\nHost github\n    HostName github.com\n    User git\n",
                d.display()
            ),
        )
        .unwrap();
        Fixture {
            dir,
            root,
            home: None,
        }
    }

    /// The reconcile example under a temporary home: `~/.ssh/config`,
    /// `~/.ssh/config.d/cypress` and `~/.ssh/config.d/cypress.bak`.
    fn reconcile() -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        let ssh = dir.path().join(".ssh");
        std::fs::create_dir_all(ssh.join("config.d")).unwrap();
        std::fs::write(ssh.join("config"), RECONCILE_ROOT).unwrap();
        std::fs::write(ssh.join("config.d/cypress"), CYPRESS).unwrap();
        std::fs::write(ssh.join("config.d/cypress.bak"), BAK).unwrap();
        let home = Some(dir.path().to_path_buf());
        Fixture {
            root: ssh.join("config"),
            dir,
            home,
        }
    }

    fn env(&self) -> Env {
        Env {
            home: self.home.clone(),
            ..env()
        }
    }

    fn load(&self) -> Workspace {
        Workspace::load_with_home(&self.root, self.home.as_deref()).unwrap()
    }

    fn file(&self, rel: &str) -> PathBuf {
        self.dir.path().join(rel)
    }

    /// Every file under the fixture with its text, the fixture's own path
    /// replaced by `<DIR>` so two fixtures compare.
    fn snapshot(&self) -> BTreeMap<String, String> {
        fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
            for e in std::fs::read_dir(dir).unwrap() {
                let p = e.unwrap().path();
                if p.is_dir() {
                    walk(&p, out);
                } else {
                    out.push(p);
                }
            }
        }
        let mut files = Vec::new();
        walk(self.dir.path(), &mut files);
        files
            .into_iter()
            .map(|p| {
                let rel = p
                    .strip_prefix(self.dir.path())
                    .unwrap()
                    .display()
                    .to_string();
                (rel, self.norm(&std::fs::read_to_string(&p).unwrap()))
            })
            .collect()
    }

    fn norm(&self, s: &str) -> String {
        let real = self.dir.path().canonicalize().unwrap();
        s.replace(&real.display().to_string(), "<DIR>")
            .replace(&self.dir.path().display().to_string(), "<DIR>")
    }
}

// ----- the TUI -----

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn ctrl(c: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
}

fn press(app: &mut Tui, c: char) {
    app.handle(key(KeyCode::Char(c)));
}

fn typ(app: &mut Tui, s: &str) {
    for c in s.chars() {
        match c {
            '\n' => app.handle(key(KeyCode::Enter)),
            c => press(app, c),
        }
    }
}

fn backspace(app: &mut Tui, n: usize) {
    for _ in 0..n {
        app.handle(key(KeyCode::Backspace));
    }
}

fn select(app: &mut Tui, name: &str) {
    while app.focus() != Focus::Table {
        app.handle(key(KeyCode::Esc));
    }
    press(app, 'g');
    for _ in 0..50 {
        if app.selected() == Some(name) {
            return;
        }
        press(app, 'j');
    }
    panic!("{name} not in the TUI table");
}

/// The TUI screen as text.
fn screen(app: &mut Tui) -> String {
    let mut term = Terminal::new(TestBackend::new(150, 60)).unwrap();
    term.draw(|f| app.render(f)).unwrap();
    let buf = term.backend().buffer().clone();
    (0..buf.area.height)
        .map(|y| {
            (0..buf.area.width)
                .map(|x| buf[(x, y)].symbol().to_string())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Moves the settings form's focus to the `nth` row of `key`.
fn goto(app: &mut Tui, k: &str, nth: usize) {
    app.handle(key(KeyCode::Home));
    let mut seen = 0;
    for _ in 0..400 {
        if app.settings_row().unwrap().0 == k {
            if seen == nth {
                return;
            }
            seen += 1;
        }
        app.handle(key(KeyCode::Down));
    }
    panic!("no TUI settings row {nth} of {k}");
}

// ----- the GUI -----

fn harness(f: &Fixture) -> Harness<'static, Gui> {
    let app = Gui::with_env(&f.root, f.env()).unwrap();
    let mut h = Harness::builder()
        .with_size([1500.0, 950.0])
        .build_ui_state(|ui, app: &mut Gui| app.show(ui), app);
    h.run();
    h
}

fn click(h: &mut Harness<'static, Gui>, label: &str) {
    h.get_all_by_label(label).last().unwrap().click();
    h.run();
}

fn type_into(h: &mut Harness<'static, Gui>, label: &str, text: &str) {
    h.get_all_by_label(label).last().unwrap().click();
    h.run();
    h.get_all_by_label(label).last().unwrap().type_text(text);
    h.run();
}

fn replace_in(h: &mut Harness<'static, Gui>, label: &str, text: &str) {
    h.get_all_by_label(label).last().unwrap().click();
    h.run();
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    h.run();
    h.key_press(egui::Key::Backspace);
    h.run();
    if !text.is_empty() {
        h.get_all_by_label(label).last().unwrap().type_text(text);
        h.run();
    }
}

fn pick(h: &mut Harness<'static, Gui>, label: &str, word: &str) {
    click(h, label);
    click(h, word);
}

// ----- the comparison -----

/// Three identical workspaces: one for each UI and one for the core.
struct Pair {
    t: Fixture,
    g: Fixture,
    c: Fixture,
    tui: Tui,
    gui: Harness<'static, Gui>,
    /// Makes the workspace every fixture starts as.
    fresh: fn() -> Fixture,
}

impl Pair {
    fn new() -> Pair {
        Pair::of(Fixture::new)
    }

    fn of(fresh: fn() -> Fixture) -> Pair {
        let (t, g, c) = (fresh(), fresh(), fresh());
        let tui = Tui::with_options(
            &t.root,
            Options {
                no_backup: false,
                theme: Theme::plain(),
                env: t.env(),
            },
        )
        .unwrap();
        let gui = harness(&g);
        Pair {
            t,
            g,
            c,
            tui,
            gui,
            fresh,
        }
    }

    /// Runs `op` on the core's workspace and saves it as the UIs do.
    fn core(&self, op: impl FnOnce(&mut Workspace)) {
        let mut ws = self.c.load();
        op(&mut ws);
        ws.save(WriteOptions::default()).unwrap();
    }

    /// The three workspaces hold the same files, backups included, and
    /// something was written.
    fn same_files(&self) {
        let (t, g, c) = (self.t.snapshot(), self.g.snapshot(), self.c.snapshot());
        assert_eq!(t, c, "TUI files differ from the core's");
        assert_eq!(g, c, "GUI files differ from the core's");
        assert_ne!(c, (self.fresh)().snapshot(), "nothing was written");
    }

    /// Neither UI wrote anything.
    fn untouched(&self) {
        let fresh = (self.fresh)().snapshot();
        assert_eq!(self.t.snapshot(), fresh, "the TUI wrote something");
        assert_eq!(self.g.snapshot(), fresh, "the GUI wrote something");
    }

    /// The two UIs' last messages, without their UI prefixes, compared.
    fn same_message(&mut self) -> String {
        let t = self.t.norm(&strip(&self.tui.message().unwrap_or_default()));
        let g = self.g.norm(&strip(self.gui.state().status()));
        assert_eq!(t, g, "TUI and GUI messages differ");
        t
    }
}

fn strip(s: &str) -> String {
    let s = s.trim();
    for p in ["✔ ", "[ok] ", "Error: ", "error: "] {
        if let Some(rest) = s.strip_prefix(p) {
            return rest.to_string();
        }
    }
    s.to_string()
}

/// The first sentence, for refusals whose remedy is phrased per UI.
fn first_sentence(s: &str) -> &str {
    s.split(". ").next().unwrap_or(s).trim_end_matches('.')
}

// ----- the scenarios -----

/// par-1: add a host with URI, identity file and section.
#[test]
fn par_1_add() {
    let mut p = Pair::new();
    press(&mut p.tui, 'a');
    typ(&mut p.tui, "newbox");
    p.tui.handle(key(KeyCode::Tab));
    typ(&mut p.tui, "root@newbox.example.com:2200");
    p.tui.handle(key(KeyCode::Tab));
    typ(&mut p.tui, "~/.ssh/k.pem");
    p.tui.handle(key(KeyCode::Tab));
    typ(&mut p.tui, "bench");
    p.tui.handle(key(KeyCode::Enter));

    click(&mut p.gui, "Add Host");
    type_into(&mut p.gui, "Name", "newbox");
    type_into(&mut p.gui, "Connection URI", "root@newbox.example.com:2200");
    type_into(&mut p.gui, "Identity file", "~/.ssh/k.pem");
    let field = p
        .gui
        .get_all_by_label("Section")
        .find(|n| n.value().is_some())
        .unwrap();
    field.click();
    p.gui.run();
    p.gui
        .get_all_by_label("Section")
        .find(|n| n.value().is_some())
        .unwrap()
        .type_text("bench");
    p.gui.run();
    click(&mut p.gui, "Save");

    p.core(|ws| {
        ws.add(
            &AddSpec {
                name: "newbox".into(),
                uri: "root@newbox.example.com:2200".into(),
                identity: Some("~/.ssh/k.pem".into()),
                options: Vec::new(),
                section: Some("bench".into()),
            },
            None,
            &env(),
        )
        .unwrap();
    });
    p.same_files();
    p.same_message();
}

/// par-2: quick edit — a new URI, and an emptied identity file removes it.
#[test]
fn par_2_quick_edit() {
    let mut p = Pair::new();
    select(&mut p.tui, "ranch-nas");
    press(&mut p.tui, 'e');
    backspace(&mut p.tui, 80);
    typ(&mut p.tui, "root@nas2.ranch.lan:2222");
    p.tui.handle(key(KeyCode::Tab));
    backspace(&mut p.tui, 80);
    p.tui.handle(key(KeyCode::Enter));

    click(&mut p.gui, "ranch-nas");
    replace_in(&mut p.gui, "Connection URI", "root@nas2.ranch.lan:2222");
    replace_in(&mut p.gui, "Identity file", "");
    click(&mut p.gui, "Save");

    p.core(|ws| {
        ws.edit(
            &EditSpec {
                name: "ranch-nas".into(),
                uri: "root@nas2.ranch.lan:2222".into(),
                identity: None,
                options: Vec::new(),
                section: None,
            },
            None,
            &env(),
        )
        .unwrap();
        ws.unset(
            &HostSelector::Name("ranch-nas".into()),
            &["IdentityFile".into()],
            None,
        )
        .unwrap();
    });
    p.same_files();
    p.same_message();
}

/// par-3: delete a host after confirming.
#[test]
fn par_3_delete() {
    let mut p = Pair::new();
    select(&mut p.tui, "dcevant");
    press(&mut p.tui, 'd');
    press(&mut p.tui, 'y');

    click(&mut p.gui, "dcevant");
    click(&mut p.gui, "Delete…");
    click(&mut p.gui, "Delete");

    p.core(|ws| {
        ws.delete(&["dcevant".to_string()], None).unwrap();
    });
    p.same_files();
    p.same_message();
}

/// par-4: clone a host; the copy joins the source's section.
#[test]
fn par_4_clone() {
    let mut p = Pair::new();
    select(&mut p.tui, "lab2");
    press(&mut p.tui, 'c');
    typ(&mut p.tui, "lab3");
    p.tui.handle(key(KeyCode::Enter));

    click(&mut p.gui, "lab2");
    click(&mut p.gui, "Clone…");
    type_into(&mut p.gui, "New name", "lab3");
    click(&mut p.gui, "Clone");

    p.core(|ws| {
        ws.clone_host(
            &CloneSpec {
                source: "lab2".into(),
                new_name: "lab3".into(),
                keep_hostname: false,
                overrides: Vec::new(),
                section: None,
            },
            None,
        )
        .unwrap();
    });
    p.same_files();
    p.same_message();
}

/// par-5: move a host to a section that does not exist yet.
#[test]
fn par_5_move() {
    let mut p = Pair::new();
    select(&mut p.tui, "github");
    press(&mut p.tui, 'm');
    p.tui.handle(key(KeyCode::Tab));
    backspace(&mut p.tui, 40);
    typ(&mut p.tui, "work");
    p.tui.handle(key(KeyCode::Enter));

    click(&mut p.gui, "github");
    click(&mut p.gui, "Move to Section…");
    type_into(&mut p.gui, "Section", "work");
    click(&mut p.gui, "Move");

    p.core(|ws| {
        ws.move_host("github", None, Some("work"), None).unwrap();
    });
    p.same_files();
    p.same_message();
}

/// par-6: add an empty section.
#[test]
fn par_6_add_section() {
    let mut p = Pair::new();
    select(&mut p.tui, "github");
    press(&mut p.tui, 'n');
    typ(&mut p.tui, "ops");
    p.tui.handle(key(KeyCode::Enter));

    click(&mut p.gui, "New section…");
    type_into(&mut p.gui, "Section name", "ops");
    click(&mut p.gui, "Add section");

    p.core(|ws| {
        ws.add_section("ops", None, None).unwrap();
    });
    p.same_files();
    p.same_message();
}

/// Renames section `old` to `new` in both UIs.
fn rename_section(p: &mut Pair, old: &str, new: &str) {
    select(&mut p.tui, "github");
    for _ in 0..4 {
        if p.tui.focus() == Focus::Sections {
            break;
        }
        p.tui.handle(key(KeyCode::Tab));
    }
    assert_eq!(p.tui.focus(), Focus::Sections);
    press(&mut p.tui, 'g');
    for _ in 0..10 {
        press(&mut p.tui, 'R');
        if screen(&mut p.tui).contains(&format!("Rename section {old}")) {
            break;
        }
        p.tui.handle(key(KeyCode::Esc));
        press(&mut p.tui, 'j');
    }
    backspace(&mut p.tui, 40);
    typ(&mut p.tui, new);
    p.tui.handle(key(KeyCode::Enter));

    click(&mut p.gui, &format!("{old}  1"));
    click(&mut p.gui, "Rename section…");
    replace_in(&mut p.gui, "New section name", new);
    click(&mut p.gui, "Rename");
}

/// par-7: rename a section; renaming onto an existing name merges.
#[test]
fn par_7_rename_section() {
    let mut p = Pair::new();
    rename_section(&mut p, "bench", "rack");
    p.core(|ws| {
        ws.rename_section("bench", "rack", None).unwrap();
    });
    p.same_files();
    p.same_message();

    let mut p = Pair::new();
    rename_section(&mut p, "bench", "other");
    p.core(|ws| {
        ws.rename_section("bench", "other", None).unwrap();
    });
    p.same_files();
    assert!(p.same_message().contains("merged into other"));
}

/// par-8: settings — a flag, a choice, a second LocalForward, a cleared
/// value — in one write.
#[test]
fn par_8_settings() {
    let mut p = Pair::new();
    select(&mut p.tui, "lab");
    p.tui.handle(key(KeyCode::Enter));
    p.tui.handle(ctrl('t'));
    goto(&mut p.tui, "Compression", 0);
    p.tui.handle(key(KeyCode::Right));
    goto(&mut p.tui, "ControlMaster", 0);
    for _ in 0..4 {
        p.tui.handle(key(KeyCode::Right));
    }
    goto(&mut p.tui, "LocalForward", 1);
    typ(&mut p.tui, "8443 localhost:443");
    goto(&mut p.tui, "LocalForward", 0);
    p.tui.handle(ctrl('u'));
    p.tui.handle(key(KeyCode::Enter));

    click(&mut p.gui, "lab");
    click(&mut p.gui, "All settings");
    click(&mut p.gui, "All");
    click(&mut p.gui, "Connection");
    pick(&mut p.gui, "Compression", "no");
    click(&mut p.gui, "Connection");
    click(&mut p.gui, "Multiplexing");
    pick(&mut p.gui, "ControlMaster", "auto");
    click(&mut p.gui, "Multiplexing");
    click(&mut p.gui, "Forwarding");
    type_into(&mut p.gui, "LocalForward", "8443 localhost:443");
    p.gui.get_all_by_label("−").next().unwrap().click();
    p.gui.run();
    click(&mut p.gui, "Save settings");

    p.core(|ws| {
        ws.apply_settings(
            "lab",
            &[
                SettingChange::set("Compression", "no"),
                SettingChange::set("ControlMaster", "auto"),
                SettingChange {
                    key: "LocalForward".into(),
                    values: vec!["8443 localhost:443".into()],
                },
            ],
            None,
        )
        .unwrap();
    });
    p.same_files();
    p.same_message();
}

/// par-9: Add setting — HostKeyAlias by typing its name, then a value.
#[test]
fn par_9_add_setting() {
    let mut p = Pair::new();
    select(&mut p.tui, "lab");
    p.tui.handle(key(KeyCode::Enter));
    p.tui.handle(key(KeyCode::End));
    typ(&mut p.tui, "hostk");
    p.tui.handle(key(KeyCode::Tab));
    typ(&mut p.tui, "alias1");
    p.tui.handle(key(KeyCode::Enter));

    click(&mut p.gui, "lab");
    click(&mut p.gui, "All settings");
    type_into(&mut p.gui, "Add setting", "hostk");
    p.gui.key_press(egui::Key::Tab);
    p.gui.run();
    p.gui
        .get_all_by_label("HostKeyAlias")
        .last()
        .unwrap()
        .type_text("alias1");
    p.gui.run();
    click(&mut p.gui, "Save settings");

    p.core(|ws| {
        ws.apply_settings("lab", &[SettingChange::set("HostKeyAlias", "alias1")], None)
            .unwrap();
    });
    p.same_files();
    p.same_message();
}

/// par-10: the raw editor — a completed `Port` line under `Host lab`,
/// saved.
#[test]
fn par_10_editor_completion_and_save() {
    let mut p = Pair::new();
    select(&mut p.tui, "lab");
    press(&mut p.tui, 'o');
    p.tui.handle(key(KeyCode::End));
    p.tui.handle(key(KeyCode::Enter));
    typ(&mut p.tui, "    por 2222");
    p.tui.handle(ctrl('s'));

    click(&mut p.gui, "lab");
    p.gui
        .key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Num2);
    p.gui.run();
    p.gui.key_press(egui::Key::End);
    p.gui.run();
    p.gui.key_press(egui::Key::Enter);
    p.gui.run();
    for t in ["    por", " ", "2222"] {
        p.gui.event(egui::Event::Text(t.into()));
        p.gui.run();
    }
    p.gui
        .key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::S);
    p.gui.run();

    let lab = p.c.file("config.d/benchfile");
    let text: String = std::fs::read_to_string(&lab)
        .unwrap()
        .replace("Host lab\n", "Host lab\n    Port 2222\n");
    let mut cfg = Config::parse(&text).unwrap();
    cfg.sort_sections();
    p.core(|ws| {
        let i = ws.files.iter().position(|f| f.path == lab).unwrap();
        ws.save_text(i, &cfg.render(), WriteOptions::default())
            .unwrap();
    });
    p.same_files();
    p.same_message();
}

/// par-11: a quick edit of a host in an included file writes only that
/// file (and its backup).
#[test]
fn par_11_included_file_host() {
    let mut p = Pair::new();
    select(&mut p.tui, "lab2");
    press(&mut p.tui, 'e');
    backspace(&mut p.tui, 80);
    typ(&mut p.tui, "ops@lab2.example.com:2200");
    p.tui.handle(key(KeyCode::Enter));

    click(&mut p.gui, "lab2");
    replace_in(&mut p.gui, "Connection URI", "ops@lab2.example.com:2200");
    click(&mut p.gui, "Save");

    p.core(|ws| {
        ws.edit(
            &EditSpec {
                name: "lab2".into(),
                uri: "ops@lab2.example.com:2200".into(),
                identity: None,
                options: Vec::new(),
                section: None,
            },
            None,
            &env(),
        )
        .unwrap();
    });
    p.same_files();
    assert_eq!(
        p.t.snapshot()["config"],
        Fixture::new().snapshot()["config"],
        "the root is untouched"
    );
    p.same_message();
}

/// par-12: selecting a host shows the same file and Host line in both.
#[test]
fn par_12_follow() {
    for (away, host) in [
        ("lab2", "github"),
        ("github", "ranch-nas"),
        ("github", "lab2"),
    ] {
        let mut p = Pair::new();
        select(&mut p.tui, away);
        select(&mut p.tui, host);
        click(&mut p.gui, away);
        click(&mut p.gui, host);
        let tui_file = p.tui.shown_file().file_name().unwrap().to_owned();
        let g = p.gui.state();
        let gui_file = g.workspace().files[g.current_file()]
            .path
            .file_name()
            .unwrap()
            .to_owned();
        assert_eq!(tui_file, gui_file, "{host}: file");
        assert_eq!(
            Some(p.tui.editor_cursor().0 + 1),
            g.pending_editor_line(),
            "{host}: Host line"
        );
    }
}

/// par-13: refusals — a duplicate name and a value that does not fit its
/// keyword — are refused alike and write nothing.
#[test]
fn par_13_refusals() {
    let mut p = Pair::new();
    press(&mut p.tui, 'a');
    typ(&mut p.tui, "github");
    p.tui.handle(key(KeyCode::Tab));
    typ(&mut p.tui, "x@y.example.com");
    p.tui.handle(key(KeyCode::Enter));
    let tui_err = screen(&mut p.tui)
        .lines()
        .find_map(|l| {
            l.split("Error: ")
                .nth(1)
                .map(|e| e.trim_end_matches(['║', '│', ' ']).to_string())
        })
        .expect("TUI shows the refusal");

    click(&mut p.gui, "Add Host");
    type_into(&mut p.gui, "Name", "github");
    type_into(&mut p.gui, "Connection URI", "x@y.example.com");
    click(&mut p.gui, "Save");
    let gui_err = strip(p.gui.state().status());
    assert_eq!(
        first_sentence(&p.t.norm(&tui_err)),
        first_sentence(&p.g.norm(&gui_err))
    );
    p.untouched();

    let mut p = Pair::new();
    select(&mut p.tui, "lab");
    p.tui.handle(key(KeyCode::Enter));
    p.tui.handle(ctrl('t'));
    goto(&mut p.tui, "Port", 0);
    typ(&mut p.tui, "abc");
    p.tui.handle(key(KeyCode::Enter));
    assert!(screen(&mut p.tui).contains("Error: Port must be a port from 1 to 65535."));

    click(&mut p.gui, "lab");
    click(&mut p.gui, "All settings");
    click(&mut p.gui, "All");
    click(&mut p.gui, "Connection");
    type_into(&mut p.gui, "Port", "abc");
    assert!(!p.gui.state_mut().save_settings());
    assert_eq!(
        p.gui.state().settings_error(),
        Some("Port must be a port from 1 to 65535.")
    );
    p.untouched();
}

/// par-14: host metadata through Add setting — a location and two tags land
/// as `# key: value` lines above `Host lab` in one write.
#[test]
fn par_14_metadata_settings() {
    let mut p = Pair::new();
    select(&mut p.tui, "lab");
    p.tui.handle(key(KeyCode::Enter));
    p.tui.handle(key(KeyCode::End));
    typ(&mut p.tui, "loca");
    p.tui.handle(key(KeyCode::Tab));
    typ(&mut p.tui, "Austin DC, rack 4");
    p.tui.handle(key(KeyCode::End));
    typ(&mut p.tui, "tags");
    p.tui.handle(key(KeyCode::Tab));
    typ(&mut p.tui, "prod, db");
    p.tui.handle(key(KeyCode::Enter));

    click(&mut p.gui, "lab");
    click(&mut p.gui, "All settings");
    type_into(&mut p.gui, "Add setting", "loca");
    p.gui.key_press(egui::Key::Tab);
    p.gui.run();
    p.gui
        .get_all_by_label("location")
        .last()
        .unwrap()
        .type_text("Austin DC, rack 4");
    p.gui.run();
    type_into(&mut p.gui, "Add setting", "tags");
    p.gui.key_press(egui::Key::Tab);
    p.gui.run();
    p.gui
        .get_all_by_label("tags")
        .last()
        .unwrap()
        .type_text("prod, db");
    p.gui.run();
    click(&mut p.gui, "Save settings");

    p.core(|ws| {
        ws.apply_settings(
            "lab",
            &[
                SettingChange::set("location", "Austin DC, rack 4"),
                SettingChange::set("tags", "prod, db"),
            ],
            None,
        )
        .unwrap();
    });
    p.same_files();
    p.same_message();
    let text = std::fs::read_to_string(p.c.file("config.d/benchfile")).unwrap();
    assert!(
        text.contains("# location: Austin DC, rack 4\n# tags: prod, db\nHost lab\n"),
        "{text}"
    );
}

/// par-15: key material in privateKeyLocation is refused alike and writes
/// nothing.
#[test]
fn par_15_key_material_refused() {
    const BLOB: &str = "-----BEGIN OPENSSH PRIVATE KEY-----";
    const MSG: &str = "privateKeyLocation holds a reference to a key, not the key itself.";
    let mut p = Pair::new();
    select(&mut p.tui, "lab");
    p.tui.handle(key(KeyCode::Enter));
    p.tui.handle(key(KeyCode::End));
    typ(&mut p.tui, "privatek");
    p.tui.handle(key(KeyCode::Tab));
    typ(&mut p.tui, BLOB);
    p.tui.handle(key(KeyCode::Enter));
    assert!(screen(&mut p.tui).contains(&format!("Error: {MSG}")));

    click(&mut p.gui, "lab");
    click(&mut p.gui, "All settings");
    type_into(&mut p.gui, "Add setting", "privatek");
    p.gui.key_press(egui::Key::Tab);
    p.gui.run();
    p.gui
        .get_all_by_label("privateKeyLocation")
        .last()
        .unwrap()
        .type_text(BLOB);
    p.gui.run();
    assert!(!p.gui.state_mut().save_settings());
    assert_eq!(p.gui.state().settings_error(), Some(MSG));
    p.untouched();
}

// ----- reconcile: the CLI, the TUI's Conflicts view, the GUI's dialog -----

/// The `rustorm` binary of this build. Cargo hands `CARGO_BIN_EXE_*` only
/// to the binary's own package, so it is built here once, into the target
/// directory and profile this test runs from.
fn rustorm_bin() -> &'static Path {
    static BIN: OnceLock<PathBuf> = OnceLock::new();
    BIN.get_or_init(|| {
        let exe = std::env::current_exe().unwrap();
        let profile = exe.parent().unwrap().parent().unwrap();
        let mut build = Command::new(env!("CARGO"));
        build
            .args(["build", "--quiet", "-p", "rustorm", "--bin", "rustorm"])
            .arg("--manifest-path")
            .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../Cargo.toml"))
            .env("CARGO_TARGET_DIR", profile.parent().unwrap());
        match profile.file_name().and_then(|n| n.to_str()) {
            Some("debug") | None => {}
            Some("release") => {
                build.arg("--release");
            }
            Some(other) => {
                build.args(["--profile", other]);
            }
        }
        assert!(build.status().unwrap().success(), "building rustorm");
        profile.join(format!("rustorm{}", std::env::consts::EXE_SUFFIX))
    })
}

/// `rustorm` with `f` as HOME, run without `--config`.
fn cli_cmd(f: &Fixture) -> Command {
    let mut c = Command::new(rustorm_bin());
    c.current_dir(f.dir.path())
        .env("HOME", f.dir.path())
        .env("XDG_CONFIG_HOME", f.dir.path().join(".config"))
        .env("USER", "tester")
        .env_remove("RUSTORM_CONFIG")
        .env_remove("RUSTORM_ASSUME_TTY")
        .env_remove("NO_COLOR")
        .stdin(Stdio::null());
    c
}

fn cli(f: &Fixture, args: &[&str]) -> Output {
    cli_cmd(f).args(args).output().expect("run rustorm")
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

/// The core's own reconcile, as `rustorm reconcile FILE... <decisions>`
/// runs it: decide by host, drop the identical copies when asked, save,
/// then retire every named file.
fn core_reconcile(
    f: &Fixture,
    files: &[&str],
    picks: &[(&str, Decision)],
    drop_identical: bool,
    retire: bool,
) {
    let mut ws = f.load();
    let files: Vec<String> = files.iter().map(|s| s.to_string()).collect();
    let scope = (!files.is_empty()).then(|| ws.reconcile_scope(&files).unwrap());
    let report = ws.reconcile_report(scope.as_deref());
    let mut d: Vec<(usize, Decision)> = picks
        .iter()
        .map(|(h, d)| (report.lookup(h).unwrap(), d.clone()))
        .collect();
    if drop_identical {
        let more = report.bulk(PairKind::Identical, &Decision::DropIdentical, &d);
        d.extend(more);
    }
    let applied = ws.apply_decisions(&report, &d, None).unwrap().value;
    ws.save(WriteOptions::default()).unwrap();
    if retire {
        for i in scope.unwrap() {
            ws.retire(i, &applied.decided, WriteOptions::default())
                .unwrap();
        }
    }
}

/// Highlights the row of `host` in the TUI's Conflicts view.
fn conflict(app: &mut Tui, host: &str) {
    press(app, 'g');
    for _ in 0..20 {
        if app.conflict_selected().as_deref() == Some(host) {
            return;
        }
        app.handle(key(KeyCode::Down));
    }
    panic!("{host} not in the TUI's Conflicts view");
}

/// Opens the GUI's Conflicts dialog.
fn conflicts(h: &mut Harness<'static, Gui>) {
    h.get_all_by_label_contains("Conflicts…")
        .last()
        .unwrap()
        .click();
    h.run();
    assert!(h.state().conflicts().is_some(), "the dialog opens");
}

/// The GUI dialog's row of `host`, whose copy is in cypress.bak.
fn row(host: &str) -> String {
    format!("{host} in ~/.ssh/config.d/cypress.bak")
}

/// The GUI's message: the status line, or the dialog's refusal.
fn gui_message(h: &Harness<'static, Gui>) -> String {
    let s = h.state();
    match s.conflicts().and_then(|v| v.error()) {
        Some(e) => e.to_string(),
        None => s.status().to_string(),
    }
}

/// The TUI and the GUI report the same message, and each of its
/// sentences is a line the CLI printed. Returns the message.
fn agree(p: &mut Pair, out: &Output) -> String {
    let t = strip(&p.tui.message().unwrap_or_default());
    let g = strip(&gui_message(&p.gui));
    assert_eq!(t, g, "TUI and GUI messages differ");
    let printed = format!("{}{}", stdout(out), stderr(out));
    let lines: Vec<&str> = printed
        .lines()
        .map(|l| l.strip_prefix("error: ").unwrap_or(l))
        .collect();
    for s in t.split_inclusive(". ") {
        assert!(
            lines.contains(&s.trim()),
            "the CLI did not say {s:?}: {printed}"
        );
    }
    t
}

/// par-16: reconcile — take the copy of one host, keep another live, drop
/// the identical copies, add the orphan, retire the copy file — through
/// CLI flags, TUI keys and GUI buttons leaves byte-identical trees, every
/// backup and the retired file included.
#[test]
fn par_16_reconcile_and_retire() {
    let mut p = Pair::of(Fixture::reconcile);
    let l = Fixture::reconcile();

    // Take the copy of cypressPro.
    press(&mut p.tui, 'C');
    conflict(&mut p.tui, "cypressPro");
    press(&mut p.tui, 'c');
    conflicts(&mut p.gui);
    click(&mut p.gui, &row("cypressPro"));
    click(&mut p.gui, "Take copy");
    let out = cli(&l, &["reconcile", "--take-copy", "cypressPro"]);
    assert_eq!(out.status.code(), Some(1), "{}", stderr(&out));
    agree(&mut p, &out);

    // Keep lab-1 live: nothing written; every later CLI run repeats it, as
    // the UIs remember it for the session.
    conflict(&mut p.tui, "lab-1");
    press(&mut p.tui, 'l');
    click(&mut p.gui, &row("lab-1"));
    click(&mut p.gui, "Keep live");
    p.same_message();

    // Drop every identical copy in cypress.bak (cypressPro is one now).
    conflict(&mut p.tui, "lab-1");
    press(&mut p.tui, 'D');
    click(&mut p.gui, &row("lab-1"));
    click(&mut p.gui, "Drop identical");
    let out = cli(
        &l,
        &["reconcile", "--keep-live", "lab-1", "--drop-identical"],
    );
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert!(agree(&mut p, &out).starts_with("2 identical copies dropped"));

    // Add the orphan printer to the root. The GUI offers it once Retire is
    // refused for it, which writes nothing.
    conflict(&mut p.tui, "printer");
    press(&mut p.tui, 'a');
    click(&mut p.gui, &row("lab-1"));
    click(&mut p.gui, "Retire cypress.bak");
    click(&mut p.gui, "Retire");
    assert_eq!(
        p.gui
            .state()
            .conflicts()
            .unwrap()
            .retire()
            .unwrap()
            .blockers,
        ["printer (orphan)"]
    );
    click(&mut p.gui, "Add printer");
    let out = cli(
        &l,
        &[
            "reconcile",
            "cypress.bak",
            "--keep-live",
            "lab-1",
            "--add",
            "printer",
        ],
    );
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    agree(&mut p, &out);

    // Retire cypress.bak.
    conflict(&mut p.tui, "lab-1");
    press(&mut p.tui, 'R');
    press(&mut p.tui, 'y');
    click(&mut p.gui, "Retire");
    let out = cli(
        &l,
        &[
            "reconcile",
            "cypress.bak",
            "--keep-live",
            "lab-1",
            "--retire",
        ],
    );
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert_eq!(
        agree(&mut p, &out),
        "~/.ssh/config.d/cypress.bak retired to ~/.ssh/retired/cypress.bak."
    );

    core_reconcile(
        &p.c,
        &[],
        &[("cypressPro", Decision::TakeCopy)],
        false,
        false,
    );
    core_reconcile(&p.c, &[], &[("lab-1", Decision::KeepLive)], true, false);
    core_reconcile(
        &p.c,
        &["cypress.bak"],
        &[("lab-1", Decision::KeepLive), ("printer", Decision::Add)],
        false,
        false,
    );
    core_reconcile(
        &p.c,
        &["cypress.bak"],
        &[("lab-1", Decision::KeepLive)],
        false,
        true,
    );
    p.same_files();
    assert_eq!(
        l.snapshot(),
        p.c.snapshot(),
        "CLI files differ from the core's"
    );
    let tree = p.c.snapshot();
    assert!(!tree.contains_key(".ssh/config.d/cypress.bak"));
    assert!(
        tree[".ssh/retired/cypress.bak"].contains("Host lab-1\n"),
        "{tree:?}"
    );
    assert!(tree[".ssh/config"].contains("Host printer\n"), "{tree:?}");
    assert!(tree[".ssh/config.d/cypress"].contains("HostName 10.10.0.9"));
}

/// par-17: retiring while conflicts remain is refused alike, with the same
/// blockers, and writes nothing anywhere.
#[test]
fn par_17_retire_refused() {
    let mut p = Pair::of(Fixture::reconcile);
    let l = Fixture::reconcile();
    press(&mut p.tui, 'C');
    conflict(&mut p.tui, "printer");
    press(&mut p.tui, 'R');
    conflicts(&mut p.gui);
    click(&mut p.gui, "Retire cypress.bak");
    click(&mut p.gui, "Retire");
    assert!(p.gui.state().conflicts().unwrap().retire().unwrap().refused);
    let out = cli(&l, &["reconcile", "cypress.bak", "--retire"]);
    assert_eq!(out.status.code(), Some(1), "{}", stderr(&out));
    assert_eq!(
        agree(&mut p, &out),
        "~/.ssh/config.d/cypress.bak not retired; undecided: cypressPro (conflict), lab-1 (conflict), printer (orphan)."
    );
    let ws = p.c.load();
    let bak = ws
        .files
        .iter()
        .position(|f| f.path.ends_with("cypress.bak"))
        .unwrap();
    assert_eq!(ws.retire_blockers(bak, &[]).len(), 3);
    p.untouched();
    assert_eq!(
        l.snapshot(),
        Fixture::reconcile().snapshot(),
        "the CLI wrote"
    );
}

/// par-18: a copy changed on disk after the report is refused alike as
/// stale; nothing is written.
#[test]
fn par_18_stale_refused() {
    let changed = BAK.replace("10.10.0.9", "10.10.0.7");
    let mut p = Pair::of(Fixture::reconcile);
    let l = Fixture::reconcile();

    press(&mut p.tui, 'C');
    conflict(&mut p.tui, "cypressPro");
    conflicts(&mut p.gui);
    click(&mut p.gui, &row("cypressPro"));
    for f in [&p.t, &p.g] {
        std::fs::write(f.file(".ssh/config.d/cypress.bak"), &changed).unwrap();
    }
    press(&mut p.tui, 'c');
    click(&mut p.gui, "Take copy");

    // The CLI on a terminal: the report is read, the file changes while
    // the prompt waits, then `c` takes the copy.
    let mut child = cli_cmd(&l)
        .env("RUSTORM_ASSUME_TTY", "1")
        .arg("reconcile")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut err = child.stderr.take().unwrap();
    let mut seen = Vec::new();
    let mut buf = [0u8; 256];
    while !String::from_utf8_lossy(&seen).contains("cypressPro: [l]ive") {
        let n = err.read(&mut buf).unwrap();
        assert!(n > 0, "no prompt: {}", String::from_utf8_lossy(&seen));
        seen.extend_from_slice(&buf[..n]);
    }
    std::fs::write(l.file(".ssh/config.d/cypress.bak"), &changed).unwrap();
    let mut input = child.stdin.take().unwrap();
    input.write_all(b"c\n").unwrap();
    drop(input);
    err.read_to_end(&mut seen).unwrap();
    let mut out = child.wait_with_output().unwrap();
    out.stderr = seen;
    assert_eq!(out.status.code(), Some(1), "{}", stderr(&out));

    let t = first_sentence(&strip(&p.tui.message().unwrap_or_default())).to_string();
    let g = first_sentence(&strip(&gui_message(&p.gui))).to_string();
    assert_eq!(t, "cypressPro changed since the report; reconcile again");
    assert_eq!(t, g, "TUI and GUI messages differ");
    assert!(
        stderr(&out).contains(&format!("error: {t}.\n")),
        "{}",
        stderr(&out)
    );
    let want = Fixture::reconcile();
    std::fs::write(want.file(".ssh/config.d/cypress.bak"), &changed).unwrap();
    let want = want.snapshot();
    assert_eq!(p.t.snapshot(), want, "the TUI wrote");
    assert_eq!(p.g.snapshot(), want, "the GUI wrote");
    assert_eq!(l.snapshot(), want, "the CLI wrote");
}
