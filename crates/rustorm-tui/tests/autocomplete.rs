//! Keyword completion in the editor: ghost text, Space to accept with a
//! premade value selected, Ctrl-Space to swap a value (docs/tui.md,
//! Editor). Catalog cases: ed-1 .. ed-11 (R-editor-autocomplete).

mod common;
use common::*;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::style::Modifier;
use rustorm_tui::{App, Focus};

const LAB: &str = "\
# lab config

Host lab
    HostName lab.example.com
    User travis
    StrictHostKeyChecking ask

Host other
    HostName other.example.com
";

fn ctrl_space() -> KeyEvent {
    KeyEvent::new(KeyCode::Char(' '), KeyModifiers::CONTROL)
}

/// Opens the editor at `host` and starts a new line after its Host line.
fn new_line_in(app: &mut App, host: &str) {
    select(app, host);
    press(app, 'o');
    assert_eq!(app.focus(), Focus::Editor);
    app.handle(key(KeyCode::End));
    app.handle(key(KeyCode::Enter));
}

/// Puts the editor cursor at the end of the line reading `text`.
fn end_of(app: &mut App, text: &str) {
    select(app, "lab");
    press(app, 'o');
    let row = app.editor_text().lines().position(|l| l == text).unwrap();
    while app.editor_cursor().0 < row {
        app.handle(key(KeyCode::Down));
    }
    app.handle(key(KeyCode::End));
}

fn line(app: &App, row: usize) -> String {
    app.editor_text().lines().nth(row).unwrap_or("").to_string()
}

fn selected(app: &App) -> Option<String> {
    let ((r0, c0), (r1, c1)) = app.editor_selection()?;
    assert_eq!(r0, r1);
    Some(line(app, r0).chars().skip(c0).take(c1 - c0).collect())
}

/// ed-1: `    hostk` shows HostKeyAlias as dim ghost text; Space completes the
/// line to `    HostKeyAlias ` with the cursor ready for the value.
#[test]
fn ed_1_ghost_text_then_space_completes() {
    let f = Fixture::new(LAB);
    let mut app = f.app();
    new_line_in(&mut app, "lab");
    typ(&mut app, "    hostk");
    let buf = draw(&mut app);
    let all = lines(&buf);
    let y = all
        .iter()
        .position(|l| l.contains("    hostkeyAlias"))
        .unwrap_or_else(|| panic!("{}", all.join("\n")));
    let x = all[y].find("hostkeyAlias").unwrap() + "hostk".len();
    let x = all[y][..x].chars().count() as u16;
    for dx in 0.."eyAlias".len() as u16 {
        let cell = &buf[(x + dx, y as u16)];
        assert!(cell.modifier.contains(Modifier::DIM), "{dx}: {cell:?}");
    }
    assert_eq!(line(&app, 3), "    hostk");
    press(&mut app, ' ');
    assert_eq!(line(&app, 3), "    HostKeyAlias ");
    assert_eq!(app.editor_cursor(), (3, 17));
    assert_eq!(selected(&app), None);
    typ(&mut app, "alias1");
    assert_eq!(line(&app, 3), "    HostKeyAlias alias1");
}

/// ed-2: Space after `    por` gives `    Port 22` with 22 selected;
/// typing replaces it.
#[test]
fn ed_2_premade_port_is_selected_and_replaced() {
    let f = Fixture::new(LAB);
    let mut app = f.app();
    new_line_in(&mut app, "lab");
    typ(&mut app, "    por ");
    assert_eq!(line(&app, 3), "    Port 22");
    assert_eq!(selected(&app).as_deref(), Some("22"));
    typ(&mut app, "2222");
    assert_eq!(line(&app, 3), "    Port 2222");
    assert_eq!(selected(&app), None);
}

/// ed-3: `    compr` Space gives `    Compression yes` with yes selected;
/// Ctrl-Space swaps it to no, then back to yes.
#[test]
fn ed_3_ctrl_space_swaps_yes_and_no() {
    let f = Fixture::new(LAB);
    let mut app = f.app();
    new_line_in(&mut app, "lab");
    typ(&mut app, "    compr ");
    assert_eq!(line(&app, 3), "    Compression yes");
    assert_eq!(selected(&app).as_deref(), Some("yes"));
    app.handle(ctrl_space());
    assert_eq!(line(&app, 3), "    Compression no");
    assert_eq!(selected(&app).as_deref(), Some("no"));
    app.handle(ctrl_space());
    assert_eq!(line(&app, 3), "    Compression yes");
}

/// ed-4: Ctrl-Space on `ask` gives the next documented choice.
#[test]
fn ed_4_ctrl_space_next_choice() {
    let f = Fixture::new(LAB);
    let mut app = f.app();
    end_of(&mut app, "    StrictHostKeyChecking ask");
    app.handle(ctrl_space());
    assert_eq!(line(&app, 5), "    StrictHostKeyChecking accept-new");
}

/// ed-5: in a value nothing is suggested and Space is a plain space.
#[test]
fn ed_5_values_are_not_completed() {
    let f = Fixture::new(LAB);
    let mut app = f.app();
    new_line_in(&mut app, "lab");
    typ(&mut app, "    User tra");
    let s = screen(&mut app);
    assert!(
        s.contains("    User tra▏") || s.contains("    User tra "),
        "{s}"
    );
    typ(&mut app, "vis ");
    assert_eq!(line(&app, 3), "    User travis ");
    assert_eq!(selected(&app), None);
}

/// ed-6: a whole keyword typed in lower case is canonicalized and gets its
/// premade value, selected.
#[test]
fn ed_6_exact_keyword_is_canonicalized() {
    let f = Fixture::new(LAB);
    let mut app = f.app();
    new_line_in(&mut app, "lab");
    typ(&mut app, "    port ");
    assert_eq!(line(&app, 3), "    Port 22");
    assert_eq!(selected(&app).as_deref(), Some("22"));
}

/// ed-7: a word matching no keyword gets a plain space.
#[test]
fn ed_7_unknown_word_gets_a_plain_space() {
    let f = Fixture::new(LAB);
    let mut app = f.app();
    new_line_in(&mut app, "lab");
    typ(&mut app, "    zzz ");
    assert_eq!(line(&app, 3), "    zzz ");
    assert_eq!(selected(&app), None);
}

/// ed-8: `ho` at column 0 offers Host; Space gives `Host ` with no value.
#[test]
fn ed_8_host_at_column_zero() {
    let f = Fixture::new(LAB);
    let mut app = f.app();
    select(&mut app, "lab");
    press(&mut app, 'o');
    app.handle(key(KeyCode::Up));
    assert_eq!(app.editor_cursor(), (1, 0));
    typ(&mut app, "ho");
    let s = screen(&mut app);
    assert!(s.lines().any(|l| l.contains("║host ")), "{s}");
    press(&mut app, ' ');
    assert_eq!(line(&app, 1), "Host ");
    assert_eq!(app.editor_cursor(), (1, 5));
    assert_eq!(selected(&app), None);
}

/// ed-9: in a comment Space is a plain space.
#[test]
fn ed_9_comment_gets_a_plain_space() {
    let f = Fixture::new(LAB);
    let mut app = f.app();
    new_line_in(&mut app, "lab");
    typ(&mut app, "    # por ");
    assert_eq!(line(&app, 3), "    # por ");
    assert_eq!(selected(&app), None);
}

/// ed-10: Ctrl-Space on a free-text value changes nothing.
#[test]
fn ed_10_free_text_value_does_not_swap() {
    let f = Fixture::new(LAB);
    let mut app = f.app();
    end_of(&mut app, "    User travis");
    app.handle(ctrl_space());
    assert_eq!(app.editor_text(), LAB);
    assert!(!app.is_editor_modified());
}

/// ed-11: an accepted completion marks the buffer modified; Ctrl-S writes
/// the canonical line.
#[test]
fn ed_11_accepted_completion_saves() {
    let f = Fixture::new(LAB);
    let mut app = f.app();
    new_line_in(&mut app, "lab");
    typ(&mut app, "    compr ");
    assert!(app.is_editor_modified());
    assert!(screen(&mut app).contains("[modified]"));
    app.handle(ctrl('s'));
    assert!(!app.is_editor_modified());
    assert!(
        f.read()
            .contains("Host lab\n    Compression yes\n    HostName lab.example.com\n"),
        "{}",
        f.read()
    );
}
