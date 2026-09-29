//! The settings form: Enter on a host edits every keyword it can set,
//! grouped, each value checked against its keyword (docs/tui.md, Settings
//! form). Catalog cases: tf-1 .. tf-15 (in the all view), fv-1 .. fv-14.

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

/// Opens the settings form on `host`, in the filled view.
fn open_filled(app: &mut rustorm_tui::App, host: &str) {
    select(app, host);
    app.handle(key(KeyCode::Enter));
    assert!(app.settings_row().is_some(), "form did not open");
    assert!(screen(app).contains(&format!("Settings {host} (filled)")));
}

/// Opens the settings form on `host` and switches to the all view.
fn open(app: &mut rustorm_tui::App, host: &str) {
    open_filled(app, host);
    app.handle(ctrl('t'));
    assert!(screen(app).contains(&format!("Settings {host} (all)")));
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

const FIVE: &str = "\
Host *
    User fallback

Host five
    HostName five.example.com
    User deploy
    Port 2222
    IdentityFile ~/.ssh/five
    Compression yes

Host other
    HostName other.example.com
";

/// The form's drawn lines between its title and its help line, trimmed.
fn form_lines(app: &mut rustorm_tui::App) -> Vec<String> {
    let all = lines(&draw(app));
    let start = all.iter().position(|l| l.contains("[Settings ")).unwrap();
    let end = all.iter().position(|l| l.contains("Enter: save")).unwrap();
    let chars: Vec<char> = all[start].chars().collect();
    let left = chars.iter().position(|c| *c == '╔').unwrap();
    let right = chars.iter().position(|c| *c == '╗').unwrap();
    all[start + 1..end]
        .iter()
        .map(|l| {
            let row: String = l.chars().skip(left + 1).take(right - left - 1).collect();
            row.trim().to_string()
        })
        .filter(|l| !l.is_empty())
        .collect()
}

/// The keywords (and group headings) the form draws, in order.
fn form_keys(app: &mut rustorm_tui::App) -> Vec<String> {
    form_lines(app)
        .iter()
        .filter_map(|l| {
            let l = l.trim_start_matches(['>', '*', ' ']);
            if let Some(g) = l.strip_prefix("── ") {
                return Some(format!("── {}", g.trim()));
            }
            if l.starts_with("Add setting") {
                return Some("Add setting".into());
            }
            l.split_whitespace().next().map(str::to_string)
        })
        .filter(|w| !w.starts_with("Error"))
        .collect()
}

fn add(app: &mut rustorm_tui::App, text: &str) {
    app.handle(key(KeyCode::End));
    assert_eq!(app.settings_row().unwrap().0, "Add setting");
    typ(app, text);
}

/// fv-1: a host with 5 keys opens showing only those rows under their
/// groups, empty groups hidden, and an Add setting row.
#[test]
fn fv_1_opens_in_filled_view() {
    let f = Fixture::new(FIVE);
    let mut app = f.app();
    open_filled(&mut app, "five");
    assert_eq!(
        form_keys(&mut app),
        [
            "── Connection",
            "Compression",
            "HostName",
            "Port",
            "User",
            "── Authentication",
            "IdentityFile",
            "Add setting"
        ]
    );
    assert_eq!(app.settings_row(), Some(("Compression", "yes")));
    let s = screen(&mut app);
    assert!(s.contains("Ctrl-T"), "{s}");
}

/// fv-2: Ctrl-T shows every keyword, Ctrl-T again only the filled ones;
/// an edit made in the all view is kept and shows in the filled view.
#[test]
fn fv_2_ctrl_t_toggles_and_keeps_edits() {
    let f = Fixture::new(FIVE);
    let mut app = f.app();
    open_filled(&mut app, "five");
    app.handle(ctrl('t'));
    assert!(screen(&mut app).contains("Settings five (all)"));
    goto(&mut app, "ForwardAgent", 0);
    press(&mut app, ' ');
    assert_eq!(value(&app), "yes");
    app.handle(ctrl('t'));
    assert!(screen(&mut app).contains("Settings five (filled)"));
    let keys = form_keys(&mut app);
    assert!(keys.contains(&"ForwardAgent".to_string()), "{keys:?}");
    assert!(!keys.contains(&"ProxyJump".to_string()), "{keys:?}");
    assert_eq!(keys.len(), 10, "{keys:?}");
    app.handle(key(KeyCode::Enter));
    assert!(f.read().contains("    ForwardAgent yes\n"), "{}", f.read());
}

/// fv-3: the all view edits rows as before and has no Add setting row.
#[test]
fn fv_3_all_view_has_no_add_row() {
    let f = Fixture::new(FIVE);
    let mut app = f.app();
    open(&mut app, "five");
    app.handle(key(KeyCode::End));
    assert_ne!(app.settings_row().unwrap().0, "Add setting");
    assert!(!screen(&mut app).contains("Add setting"));
    goto(&mut app, "Tag", 0);
    typ(&mut app, "lab");
    assert_eq!(app.settings_row(), Some(("Tag", "lab")));
    app.handle(key(KeyCode::Enter));
    assert!(f.read().contains("    Tag lab\n"), "{}", f.read());
}

/// fv-4: typing hostk offers HostKeyAlias; Tab adds it and focuses its
/// value; the save writes it.
#[test]
fn fv_4_add_setting_hostkeyalias() {
    let f = Fixture::new(FIVE);
    let mut app = f.app();
    open_filled(&mut app, "five");
    add(&mut app, "hostk");
    assert!(screen(&mut app).contains("hostk▏eyAlias"));
    app.handle(key(KeyCode::Tab));
    assert_eq!(app.settings_row(), Some(("HostKeyAlias", "")));
    typ(&mut app, "alias1");
    let keys = form_keys(&mut app);
    assert!(keys.contains(&"HostKeyAlias".to_string()), "{keys:?}");
    app.handle(key(KeyCode::Enter));
    assert!(
        f.read().contains("    HostKeyAlias alias1\n"),
        "{}",
        f.read()
    );
}

/// fv-5: stricth, Tab adds StrictHostKeyChecking with focus on its value;
/// Space cycles its choices.
#[test]
fn fv_5_add_setting_choice_cycles() {
    let f = Fixture::new(FIVE);
    let mut app = f.app();
    open_filled(&mut app, "five");
    add(&mut app, "stricth");
    app.handle(key(KeyCode::Tab));
    assert_eq!(app.settings_row(), Some(("StrictHostKeyChecking", "yes")));
    press(&mut app, ' ');
    assert_eq!(value(&app), "no");
    press(&mut app, ' ');
    assert_eq!(value(&app), "ask");
}

/// fv-6: forward fills in ForwardAgent and lists the other matches; more
/// text narrows them; Tab takes the first.
#[test]
fn fv_6_first_prefix_match_wins() {
    let f = Fixture::new(FIVE);
    let mut app = f.app();
    open_filled(&mut app, "five");
    add(&mut app, "forward");
    let s = screen(&mut app);
    assert!(s.contains("forward▏Agent"), "{s}");
    assert!(
        s.contains("also: ForwardX11, ForwardX11Timeout, ForwardX11Trusted"),
        "{s}"
    );
    typ(&mut app, "x11t");
    let s = screen(&mut app);
    assert!(s.contains("forwardx11t▏imeout"), "{s}");
    assert!(s.contains("also: ForwardX11Trusted"), "{s}");
    app.handle(key(KeyCode::Tab));
    assert_eq!(app.settings_row(), Some(("ForwardX11Timeout", "")));
}

/// fv-7: text matching no keyword adds nothing and says so.
#[test]
fn fv_7_no_match_adds_nothing() {
    let f = Fixture::new(FIVE);
    let mut app = f.app();
    open_filled(&mut app, "five");
    let before = form_keys(&mut app);
    add(&mut app, "zzz");
    assert!(screen(&mut app).contains("no matching keyword"));
    app.handle(key(KeyCode::Tab));
    assert_eq!(app.settings_row(), Some(("Add setting", "zzz")));
    assert!(screen(&mut app).contains("no matching keyword"));
    assert_eq!(form_keys(&mut app), before);
}

/// fv-8: adding a single-value key the host sets focuses its row.
#[test]
fn fv_8_existing_key_is_focused_not_duplicated() {
    let f = Fixture::new(FIVE);
    let mut app = f.app();
    open_filled(&mut app, "five");
    let before = form_keys(&mut app);
    add(&mut app, "user");
    app.handle(key(KeyCode::Tab));
    assert_eq!(app.settings_row(), Some(("User", "deploy")));
    assert!(!app.settings_value_selected());
    assert_eq!(form_keys(&mut app), before);
}

/// fv-9: adding a repeatable key the host sets gives it another row after
/// the existing one (prefilled with the premade value, selected).
#[test]
fn fv_9_repeatable_key_gains_a_row() {
    let f = Fixture::new(FIVE);
    let mut app = f.app();
    open_filled(&mut app, "five");
    add(&mut app, "identityf");
    app.handle(key(KeyCode::Tab));
    assert_eq!(
        app.settings_row(),
        Some(("IdentityFile", "~/.ssh/id_ed25519"))
    );
    assert!(app.settings_value_selected());
    let keys = form_keys(&mut app);
    let at = keys.iter().position(|k| k == "IdentityFile").unwrap();
    assert_eq!(keys[at + 1], "IdentityFile", "{keys:?}");
    typ(&mut app, "~/.ssh/other");
    assert_eq!(value(&app), "~/.ssh/other");
    app.handle(key(KeyCode::Enter));
    assert!(
        f.read()
            .contains("    IdentityFile ~/.ssh/five\n    IdentityFile ~/.ssh/other\n"),
        "{}",
        f.read()
    );
}

/// fv-10: a cleared filled row stays visible, marked changed, until the
/// form closes.
#[test]
fn fv_10_cleared_row_stays_visible() {
    let f = Fixture::new(FIVE);
    let mut app = f.app();
    open_filled(&mut app, "five");
    let before = form_keys(&mut app);
    goto(&mut app, "Port", 0);
    app.handle(ctrl('u'));
    assert_eq!(value(&app), "");
    app.handle(key(KeyCode::Down));
    assert_eq!(form_keys(&mut app), before);
    let port = form_lines(&mut app)
        .into_iter()
        .find(|l| l.contains("Port"))
        .unwrap();
    assert!(port.starts_with("* Port"), "{port}");
    app.handle(key(KeyCode::Esc));
    assert_eq!(f.read(), FIVE);
    open_filled(&mut app, "five");
    goto(&mut app, "Port", 0);
    assert_eq!(value(&app), "2222");
}

/// fv-11: Esc clears Add setting's text and keeps the form; a second Esc
/// cancels it.
#[test]
fn fv_11_esc_clears_then_cancels() {
    let f = Fixture::new(FIVE);
    let mut app = f.app();
    open_filled(&mut app, "five");
    add(&mut app, "hostk");
    app.handle(key(KeyCode::Esc));
    assert_eq!(app.settings_row(), Some(("Add setting", "")));
    app.handle(key(KeyCode::Esc));
    assert!(app.settings_row().is_none());
    assert_eq!(app.message().unwrap(), "Cancelled.");
    assert_eq!(f.read(), FIVE);
}

/// fv-12: a key added and given a value, then Esc twice, writes nothing.
#[test]
fn fv_12_added_key_then_escape_writes_nothing() {
    let f = Fixture::new(FIVE);
    let mut app = f.app();
    open_filled(&mut app, "five");
    add(&mut app, "hostk");
    app.handle(key(KeyCode::Tab));
    typ(&mut app, "alias1");
    app.handle(key(KeyCode::Esc));
    app.handle(key(KeyCode::Esc));
    assert!(app.settings_row().is_none());
    assert_eq!(f.read(), FIVE);
    assert!(f.backup().is_none());
}

/// fv-13: the form opens in the filled view every time.
#[test]
fn fv_13_filled_is_the_default_every_time() {
    let f = Fixture::new(FIVE);
    let mut app = f.app();
    open(&mut app, "five");
    app.handle(key(KeyCode::Esc));
    open_filled(&mut app, "five");
    assert_eq!(form_keys(&mut app).len(), 8);
}

/// fv-14 (TUI): accepting Port prefills 22, selected; typing replaces it.
#[test]
fn fv_14_premade_value_prefilled_and_selected() {
    let f = Fixture::new(LAB);
    let mut app = f.app();
    open_filled(&mut app, "lab");
    add(&mut app, "port");
    app.handle(key(KeyCode::Tab));
    assert_eq!(app.settings_row(), Some(("Port", "22")));
    assert!(app.settings_value_selected());
    let buf = draw(&mut app);
    let all = lines(&buf);
    let y = all.iter().position(|l| l.contains("> Port")).unwrap();
    let x = all[y].find("22").unwrap();
    let x = all[y][..x].chars().count() as u16;
    assert!(buf[(x, y as u16)]
        .modifier
        .contains(ratatui::style::Modifier::REVERSED));
    typ(&mut app, "2200");
    assert_eq!(value(&app), "2200");
    assert!(!app.settings_value_selected());
    app.handle(key(KeyCode::Enter));
    assert!(f.read().contains("    Port 2200\n"), "{}", f.read());
}
