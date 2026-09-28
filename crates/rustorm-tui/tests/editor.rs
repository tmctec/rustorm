//! The embedded editor: highlighting, save through the core, the
//! discard-or-save prompt, refusal of invalid edits, external changes.
//! Catalog cases: ed-1, ed-3, ed-4, ed-5.

mod common;
use common::*;
use crossterm::event::KeyCode;
use ratatui::buffer::Buffer;
use ratatui::style::{Color, Modifier, Style};
use rustorm_core::Config;
use rustorm_tui::{App, Focus, Theme};

fn highlight_fixture() -> String {
    let mut c = Config::parse(
        "# plain comment\nHost *\n    User me\n\nHost a b\n    HostName a.example.com\n    ProxyCommand ssh -W %h:%p gw\n    ProxyJump jumpbox\n",
    )
    .unwrap();
    c.ensure_section("bob");
    c.render()
}

/// Style of the first cell of `needle` inside the editor pane.
fn style_of(buf: &Buffer, needle: &str) -> Style {
    let all = lines(buf);
    let start = all.iter().position(|l| l.contains("Editor ")).unwrap();
    for (y, l) in all.iter().enumerate().skip(start + 1) {
        if let Some(bx) = l.find(needle) {
            let x = l[..bx].chars().count() as u16;
            return buf[(x, y as u16)].style();
        }
    }
    panic!("{needle} not in editor pane");
}

/// ed-1
#[test]
fn ed_1_distinct_styles_per_span_kind() {
    let f = Fixture::new(&highlight_fixture());
    let mut app = f.app();
    let buf = draw_sized(&mut app, W, 100);
    let comment = style_of(&buf, "# plain comment");
    let banner = style_of(&buf, "#-----");
    let label = style_of(&buf, "section: bob");
    let host_kw = style_of(&buf, "Host a b");
    let host_name = style_of(&buf, "a b");
    let key = style_of(&buf, "HostName a.example");
    let value = style_of(&buf, "a.example.com");
    let proxy = style_of(&buf, "ssh -W %h:%p gw");
    let jump = style_of(&buf, "jumpbox");
    assert_eq!(comment.fg, Some(Color::DarkGray));
    assert!(comment.add_modifier.contains(Modifier::ITALIC));
    assert_eq!(banner.fg, Some(Color::Magenta));
    assert_eq!(label, banner, "every banner line is Banner");
    assert_eq!(host_kw.fg, Some(Color::Yellow));
    assert_eq!(host_name.fg, Some(Color::Green));
    assert_eq!(key.fg, Some(Color::Cyan));
    assert_eq!(proxy.fg, Some(Color::LightRed));
    assert!(proxy.add_modifier.contains(Modifier::UNDERLINED));
    assert_eq!(jump.fg, Some(Color::LightBlue));
    assert!(jump
        .add_modifier
        .contains(Modifier::BOLD | Modifier::UNDERLINED));
    let all = [comment, banner, host_kw, host_name, key, value, proxy, jump];
    for (i, a) in all.iter().enumerate() {
        for b in &all[i + 1..] {
            assert_ne!(a, b, "styles must differ");
        }
    }
}

#[test]
fn ed_1_styles_stay_distinct_without_color() {
    let f = Fixture::new(&highlight_fixture());
    let mut opts = options();
    opts.theme = Theme::plain();
    let mut app = App::with_options(&f.path, opts).unwrap();
    let buf = draw_sized(&mut app, W, 100);
    let styles = [
        style_of(&buf, "# plain comment"),
        style_of(&buf, "#-----"),
        style_of(&buf, "Host a b"),
        style_of(&buf, "ssh -W %h:%p gw"),
        style_of(&buf, "jumpbox"),
    ];
    for s in &styles {
        assert!(
            matches!(s.fg, None | Some(Color::Reset)),
            "no color under the plain theme"
        );
    }
    for (i, a) in styles.iter().enumerate() {
        for b in &styles[i + 1..] {
            assert_ne!(a, b);
        }
    }
}

/// Focuses the editor at `name`'s Host line.
fn open_at(app: &mut App, name: &str) {
    select(app, name);
    press(app, 'o');
    assert_eq!(app.focus(), Focus::Editor);
}

/// ed-3
#[test]
fn ed_3_edit_value_and_save_through_core() {
    let orig = three_hosts();
    let f = Fixture::new(&orig);
    let mut app = f.app();
    open_at(&mut app, "vps");
    app.handle(key(KeyCode::Down));
    app.handle(key(KeyCode::Down)); // "    User root"
    app.handle(key(KeyCode::End));
    backspace(&mut app, 4);
    typ(&mut app, "admin");
    assert!(app.is_editor_modified());
    assert!(screen(&mut app).contains("[modified]"));
    app.handle(ctrl('s'));
    let expected = orig.replace("    User root\n", "    User admin\n");
    assert_eq!(f.read(), expected, "only the edited line changes");
    assert_eq!(f.backup().as_deref(), Some(orig.as_str()));
    assert!(!app.is_editor_modified());
    let s = screen(&mut app);
    assert!(s.contains(&format!("✔ Saved {}.", f.path.display())), "{s}");
    assert!(table_lines(&draw(&mut app)).join("\n").contains("admin"));
}

/// ed-3: a save re-sorts hosts inside each section.
#[test]
fn ed_3_save_resorts_sections() {
    let orig = three_hosts();
    let f = Fixture::new(&orig);
    let mut app = f.app();
    open_at(&mut app, "db");
    app.handle(key(KeyCode::End));
    backspace(&mut app, 2);
    typ(&mut app, "zz");
    app.handle(ctrl('s'));
    let edited = orig.replace("Host db\n", "Host zz\n");
    let mut c = Config::parse(&edited).unwrap();
    c.sort_sections();
    let expected = c.render();
    let now = f.read();
    assert_eq!(now, expected);
    assert!(now.find("Host web").unwrap() < now.find("Host zz").unwrap());
}

/// ed-4
#[test]
fn ed_4_quit_with_unsaved_edits_asks_discard_or_save() {
    let orig = three_hosts();
    let f = Fixture::new(&orig);
    let mut app = f.app();
    open_at(&mut app, "web");
    typ(&mut app, "# note\n");
    app.handle(key(KeyCode::Esc));
    press(&mut app, 'q');
    let s = screen(&mut app);
    assert!(
        s.contains("The editor has unsaved changes. [s]ave / [d]iscard / [c]ancel"),
        "{s}"
    );
    assert!(!app.should_quit());
    press(&mut app, 'c');
    assert!(!app.should_quit());
    assert_eq!(f.read(), orig);
    // Ctrl-C takes the same path; d quits without writing.
    app.handle(ctrl('c'));
    assert!(screen(&mut app).contains("[s]ave / [d]iscard / [c]ancel"));
    press(&mut app, 'd');
    assert!(app.should_quit());
    assert_eq!(f.read(), orig);
    assert!(f.backup().is_none());
}

/// ed-4: s saves, then quits.
#[test]
fn ed_4_quit_prompt_save_writes_then_quits() {
    let orig = three_hosts();
    let f = Fixture::new(&orig);
    let mut app = f.app();
    open_at(&mut app, "web");
    typ(&mut app, "# note\n");
    app.handle(ctrl('c'));
    press(&mut app, 's');
    assert!(app.should_quit());
    assert!(f.read().contains("# note\n"));
}

#[test]
fn quit_without_edits_exits_immediately() {
    let f = Fixture::new(&three_hosts());
    let mut app = f.app();
    press(&mut app, 'q');
    assert!(app.should_quit());
}

/// ed-5
#[test]
fn ed_5_invalid_edit_refused_with_line_number() {
    let orig = three_hosts();
    let f = Fixture::new(&orig);
    let mut app = f.app();
    open_at(&mut app, "web");
    let (row, _) = app.editor_cursor();
    typ(&mut app, "Host\n");
    app.handle(ctrl('s'));
    let msg = format!("Error: Not saved: line {}: cannot parse: Host", row + 1);
    assert_eq!(app.message().as_deref(), Some(msg.as_str()));
    assert!(screen(&mut app).contains(&msg));
    assert_eq!(f.read(), orig, "file unchanged");
    assert!(f.backup().is_none());
    assert_eq!(app.editor_cursor(), (row, 0), "cursor on the bad line");
}

/// Reload-then-apply for the editor: an external change prompts; reload
/// takes the disk text, overwrite saves the buffer.
#[test]
fn editor_save_after_external_change_prompts() {
    let orig = three_hosts();
    let f = Fixture::new(&orig);
    let mut app = f.app();
    open_at(&mut app, "web");
    typ(&mut app, "# mine\n");
    let hand = format!("{orig}# hand edit\n");
    std::fs::write(&f.path, &hand).unwrap();
    app.handle(ctrl('s'));
    assert!(screen(&mut app)
        .contains("The file changed on disk. [r]eload (drop your edits) / [o]verwrite / [c]ancel"));
    press(&mut app, 'r');
    assert_eq!(app.editor_text(), hand);
    assert_eq!(f.read(), hand);
    // Overwrite path.
    assert_eq!(app.focus(), Focus::Editor);
    typ(&mut app, "# mine\n");
    std::fs::write(&f.path, format!("{hand}# again\n")).unwrap();
    app.handle(ctrl('s'));
    press(&mut app, 'o');
    assert!(f.read().contains("# mine\n"));
    assert!(!f.read().contains("# again"));
}

#[test]
fn discard_reloads_from_disk() {
    let orig = three_hosts();
    let f = Fixture::new(&orig);
    let mut app = f.app();
    open_at(&mut app, "web");
    typ(&mut app, "junk");
    app.handle(ctrl('r'));
    assert!(screen(&mut app).contains("Discard the editor's changes? [y/N]"));
    press(&mut app, 'y');
    assert_eq!(app.editor_text(), orig);
    assert!(!app.is_editor_modified());
}

#[test]
fn help_overlay_lists_keys() {
    let f = Fixture::new(&three_hosts());
    let mut app = f.app();
    press(&mut app, '?');
    let s = screen(&mut app);
    for k in [
        "Ctrl-S",
        "f then 1-7",
        "move or rename host",
        "rename section",
    ] {
        assert!(s.contains(k), "help lacks {k}");
    }
    app.handle(key(KeyCode::Esc));
    assert!(!screen(&mut app).contains("f then 1-7"));
}

#[test]
fn missing_config_shows_empty_table_and_creates_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("nope").join("config");
    let mut app = App::with_options(&path, options()).unwrap();
    let s = screen(&mut app);
    assert!(s.contains("[Hosts 0/0]"));
    assert!(s.contains("no hosts in"));
    press(&mut app, 'q');
    assert!(!path.exists());
    assert!(!path.parent().unwrap().exists());
}

/// F4: a 103-column banner in a narrow pane scrolls with the cursor instead
/// of wrapping.
#[test]
fn editor_scrolls_horizontally_without_wrapping() {
    let f = Fixture::new(&three_hosts());
    let mut app = f.app();
    app.handle(key(KeyCode::Tab));
    let rule_row = app
        .editor_text()
        .lines()
        .position(|l| l.starts_with("#---"))
        .unwrap();
    for _ in 0..rule_row {
        app.handle(key(KeyCode::Down));
    }
    app.handle(key(KeyCode::End));
    let buf = draw_sized(&mut app, 60, 40);
    let s = lines(&buf).join("\n");
    assert!(s.contains(&format!("ln {} col 104", rule_row + 1)), "{s}");
    let all = lines(&buf);
    let start = all.iter().position(|l| l.contains("Editor ")).unwrap();
    let pane: Vec<&String> = all[start + 1..].iter().collect();
    // The rule line's closing '#' is visible after scrolling, and no line
    // in the pane starts with a wrapped continuation of the banner.
    assert!(pane.iter().any(|l| l.contains("-----#")), "{s}");
    assert_eq!(app.editor_text(), three_hosts());
}
