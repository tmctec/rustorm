//! The settings form: Enter on a host edits every keyword it can set,
//! grouped, each value checked against its keyword (docs/tui.md, Settings
//! form). Catalog cases: tf-1 .. tf-15.

mod common;
use common::*;
use crossterm::event::KeyCode;
use rustorm_core::{Config, SettingChange};

const LAB: &str = "\
# my ssh config
Host *
    User fallback
    ServerAliveInterval 60

# the lab box
Host lab
    # keep this comment
    HostName lab.example.com
    Compression yes
    LocalForward 8080 localhost:80

Host other
    HostName other.example.com
";

fn core(text: &str, changes: &[SettingChange]) -> String {
    let mut c = Config::parse(text).unwrap();
    c.apply_settings("lab", changes).unwrap();
    c.render()
}

/// Opens the settings form on `host`.
fn open(app: &mut rustorm_tui::App, host: &str) {
    select(app, host);
    app.handle(key(KeyCode::Enter));
    assert!(app.settings_row().is_some(), "form did not open");
}

/// Moves the form's focus to the `nth` row of `key`.
fn goto(app: &mut rustorm_tui::App, key: &str, nth: usize) {
    app.handle(key_ev(KeyCode::Home));
    let mut seen = 0;
    for _ in 0..400 {
        if app.settings_row().unwrap().0 == key {
            if seen == nth {
                return;
            }
            seen += 1;
        }
        app.handle(key_ev(KeyCode::Down));
    }
    panic!("no row {nth} of {key}");
}

fn key_ev(code: KeyCode) -> crossterm::event::KeyEvent {
    key(code)
}

fn value(app: &rustorm_tui::App) -> String {
    app.settings_row().unwrap().1.to_string()
}

/// tf-1: Enter opens the grouped form, prefilled; inherited values show
/// dimmed as coming from Host *.
#[test]
fn tf_1_enter_opens_the_grouped_prefilled_form() {
    let f = Fixture::new(LAB);
    let mut app = f.app();
    open(&mut app, "lab");
    let s = screen(&mut app);
    assert!(s.contains("Settings lab"), "{s}");
    assert!(s.contains("── Connection"), "{s}");
    goto(&mut app, "HostName", 0);
    assert_eq!(value(&app), "lab.example.com");
    goto(&mut app, "LocalForward", 0);
    assert_eq!(value(&app), "8080 localhost:80");
    goto(&mut app, "Compression", 0);
    let s = screen(&mut app);
    assert!(s.contains("(fallback from Host *)"), "{s}");
    for g in ["Authentication", "Forwarding"] {
        assert!(
            s.contains(g) || {
                goto(
                    &mut app,
                    if g == "Forwarding" {
                        "ForwardAgent"
                    } else {
                        "IdentityFile"
                    },
                    0,
                );
                screen(&mut app).contains(g)
            },
            "{g}"
        );
    }
    goto(&mut app, "ProxyCommand", 0);
    assert!(screen(&mut app).contains("── Proxy"));
    goto(&mut app, "ControlMaster", 0);
    assert!(screen(&mut app).contains("── Multiplexing"));
}

/// tf-2: toggling ForwardAgent to no writes it, with a backup; comments
/// and the order of untouched keys survive (tf-12).
#[test]
fn tf_2_and_12_toggle_writes_through_the_core() {
    let f = Fixture::new(LAB);
    let mut app = f.app();
    open(&mut app, "lab");
    goto(&mut app, "ForwardAgent", 0);
    press(&mut app, ' ');
    assert_eq!(value(&app), "yes");
    press(&mut app, ' ');
    assert_eq!(value(&app), "no");
    app.handle(key(KeyCode::Enter));
    assert!(app.settings_row().is_none());
    assert_eq!(
        f.read(),
        core(LAB, &[SettingChange::set("ForwardAgent", "no")])
    );
    assert!(f
        .read()
        .contains("    # keep this comment\n    HostName lab.example.com\n    Compression yes\n"));
    assert_eq!(f.backup().unwrap(), LAB);
    assert_eq!(app.message().unwrap(), "✔ lab updated.");
}

/// tf-3: a fixed choice is picked by cycling.
#[test]
fn tf_3_choice_cycles_to_auto() {
    let f = Fixture::new(LAB);
    let mut app = f.app();
    open(&mut app, "lab");
    goto(&mut app, "ControlMaster", 0);
    for _ in 0..4 {
        app.handle(key(KeyCode::Right));
    }
    assert_eq!(value(&app), "auto");
    app.handle(key(KeyCode::Left));
    assert_eq!(value(&app), "ask");
    app.handle(key(KeyCode::Right));
    app.handle(key(KeyCode::Enter));
    assert!(f.read().contains("    ControlMaster auto\n"));
}

/// tf-4: a value that does not fit its key is refused inline; nothing is
/// written.
#[test]
fn tf_4_bad_port_is_refused() {
    let f = Fixture::new(LAB);
    let mut app = f.app();
    open(&mut app, "lab");
    goto(&mut app, "Port", 0);
    typ(&mut app, "abc");
    app.handle(key(KeyCode::Enter));
    assert_eq!(app.settings_row(), Some(("Port", "abc")));
    let s = screen(&mut app);
    assert!(
        s.contains("Error: Port must be a port from 1 to 65535."),
        "{s}"
    );
    assert_eq!(f.read(), LAB);
    assert!(f.backup().is_none());
}

/// tf-5: clearing a value, or cycling it to unset, removes the key.
#[test]
fn tf_5_clear_or_cycle_unsets() {
    let expected = core(LAB, &[SettingChange::unset("Compression")]);
    for how in ["ctrl-u", "cycle"] {
        let f = Fixture::new(LAB);
        let mut app = f.app();
        open(&mut app, "lab");
        goto(&mut app, "Compression", 0);
        assert_eq!(value(&app), "yes");
        match how {
            "ctrl-u" => app.handle(ctrl('u')),
            _ => app.handle(key(KeyCode::Left)),
        }
        assert_eq!(value(&app), "", "{how}");
        app.handle(key(KeyCode::Enter));
        assert_eq!(f.read(), expected, "{how}");
    }
}

/// tf-6: a second LocalForward goes in the empty row below the first.
#[test]
fn tf_6_multi_valued_key_gains_a_row() {
    let f = Fixture::new(LAB);
    let mut app = f.app();
    open(&mut app, "lab");
    goto(&mut app, "LocalForward", 1);
    assert_eq!(value(&app), "");
    typ(&mut app, "8443 localhost:443");
    // A new empty row follows the filled one.
    goto(&mut app, "LocalForward", 2);
    assert_eq!(value(&app), "");
    app.handle(key(KeyCode::Enter));
    assert!(
        f.read()
            .contains("    LocalForward 8080 localhost:80\n    LocalForward 8443 localhost:443\n"),
        "{}",
        f.read()
    );
}

/// tf-7: free text is written verbatim, tokens and spaces included.
#[test]
fn tf_7_free_text_is_verbatim() {
    let f = Fixture::new(LAB);
    let mut app = f.app();
    open(&mut app, "lab");
    goto(&mut app, "ProxyCommand", 0);
    typ(&mut app, "ssh -W %h:%p jump");
    app.handle(key(KeyCode::Enter));
    assert!(f.read().contains("    ProxyCommand ssh -W %h:%p jump\n"));
}

/// tf-8: Esc after edits writes nothing.
#[test]
fn tf_8_escape_writes_nothing() {
    let f = Fixture::new(LAB);
    let mut app = f.app();
    open(&mut app, "lab");
    goto(&mut app, "User", 0);
    typ(&mut app, "someone");
    app.handle(key(KeyCode::Esc));
    assert!(app.settings_row().is_none());
    assert_eq!(f.read(), LAB);
    assert!(f.backup().is_none());
    assert_eq!(app.message().unwrap(), "Cancelled.");
}

/// tf-9: Enter with nothing changed writes nothing.
#[test]
fn tf_9_unchanged_submit_writes_nothing() {
    let f = Fixture::new(LAB);
    let mut app = f.app();
    open(&mut app, "lab");
    goto(&mut app, "ForwardAgent", 0);
    press(&mut app, ' ');
    app.handle(key(KeyCode::Left));
    assert_eq!(value(&app), "");
    app.handle(key(KeyCode::Enter));
    assert_eq!(app.message().unwrap(), "No changes.");
    assert_eq!(f.read(), LAB);
    assert!(f.backup().is_none());
}

/// tf-10: a host in an included file is written there, not in the root.
#[test]
fn tf_10_included_host_writes_its_own_file() {
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path().join("config.d");
    std::fs::create_dir(&d).unwrap();
    let root = dir.path().join("config");
    let root_text = format!("Include {}/*\n\nHost github\n    User git\n", d.display());
    std::fs::write(&root, &root_text).unwrap();
    let ranch = d.join("ranch");
    std::fs::write(&ranch, "Host ranch-nas\n    HostName nas.ranch.lan\n").unwrap();
    let mut app = rustorm_tui::App::with_options(&root, options()).unwrap();
    open(&mut app, "ranch-nas");
    goto(&mut app, "ForwardAgent", 0);
    press(&mut app, ' ');
    app.handle(key(KeyCode::Enter));
    assert_eq!(
        std::fs::read_to_string(&ranch).unwrap(),
        "Host ranch-nas\n    HostName nas.ranch.lan\n    ForwardAgent yes\n"
    );
    assert_eq!(std::fs::read_to_string(&root).unwrap(), root_text);
    assert!(
        app.message().unwrap().contains("ranch"),
        "{:?}",
        app.message()
    );
}

/// tf-11: the form will not open over unsaved editor edits.
#[test]
fn tf_11_editor_guard() {
    let f = Fixture::new(LAB);
    let mut app = f.app();
    app.handle(key(KeyCode::Tab));
    assert_eq!(app.focus(), rustorm_tui::Focus::Editor);
    typ(&mut app, "# edit\n");
    app.handle(key(KeyCode::Esc));
    select(&mut app, "lab");
    app.handle(key(KeyCode::Enter));
    assert!(app.settings_row().is_none());
    assert_eq!(
        app.message().unwrap(),
        "Error: Save or discard the editor's changes first."
    );
}

/// tf-13: `e` still opens the quick form.
#[test]
fn tf_13_e_opens_the_quick_form() {
    let f = Fixture::new(LAB);
    let mut app = f.app();
    select(&mut app, "lab");
    press(&mut app, 'e');
    assert!(app.settings_row().is_none());
    assert!(screen(&mut app).contains("Edit host lab"));
}

/// tf-14: typing a value for an inherited key gives the host its own line;
/// Host * is untouched.
#[test]
fn tf_14_inherited_key_gets_its_own_line() {
    let f = Fixture::new(LAB);
    let mut app = f.app();
    open(&mut app, "lab");
    goto(&mut app, "User", 0);
    assert_eq!(value(&app), "");
    typ(&mut app, "me");
    app.handle(key(KeyCode::Enter));
    let text = f.read();
    assert!(text.contains("Host *\n    User fallback\n"), "{text}");
    assert_eq!(text, core(LAB, &[SettingChange::set("User", "me")]));
}

/// tf-15: after a save the editor shows the host's new block.
#[test]
fn tf_15_editor_shows_the_updated_block() {
    let f = Fixture::new(LAB);
    let mut app = f.app();
    open(&mut app, "lab");
    goto(&mut app, "ForwardAgent", 0);
    press(&mut app, ' ');
    app.handle(key(KeyCode::Enter));
    assert!(app.editor_text().contains("    ForwardAgent yes\n"));
    let line = app
        .editor_text()
        .lines()
        .position(|l| l == "Host lab")
        .unwrap();
    assert_eq!(app.editor_cursor(), (line, 0));
}
