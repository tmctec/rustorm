//! Initial render, sorting and filtering on ratatui's TestBackend.
//! Catalog cases: ui-1, web-tui-1, sort-*, filter-*.

mod common;
use common::*;
use crossterm::event::KeyCode;

fn sorted(key: char, twice: bool) -> Vec<String> {
    let f = Fixture::new(&six_hosts());
    let mut app = f.app();
    press(&mut app, key);
    if twice {
        press(&mut app, key);
    }
    drawn_order(&mut app, &SIX)
}

fn names(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

/// ui-1 (render half; the pty half is tests/pty.rs) and web-tui-1.
#[test]
fn ui_1_initial_render_of_three_sectioned_hosts() {
    let f = Fixture::new(&three_hosts());
    let mut app = f.app();
    let s = screen(&mut app);
    assert!(s.contains("[Hosts 3/3]"), "{s}");
    for h in [
        "Section", "Host", "User", "HostName", "Port", "Proxy", "Jump",
    ] {
        assert!(s.contains(h), "header {h} missing");
    }
    assert!(s.contains("Sections"));
    for (n, c) in [("All", 3), ("bob", 1), ("other", 2)] {
        assert!(
            s.contains(&format!("{n:<14}{c:>4}")),
            "section row {n}: {s}"
        );
    }
    let buf = draw(&mut app);
    let table = table_lines(&buf).join("\n");
    assert!(table.contains("vps.example.com"));
    assert!(table.contains("ssh -W %h:%p bastion"));
    assert!(table.contains("db.internal"));
    // web-tui-1: file order, sorted inside each section, Host * not a row.
    assert_eq!(
        drawn_order(&mut app, &["vps", "db", "web", "*"]),
        names(&["vps", "db", "web"])
    );
    assert!(!table
        .lines()
        .any(|l| l.contains(" * ") && l.contains("ServerAlive")));
    // editor pane shows the file with its title; status shows the target.
    assert!(s.contains("Editor "));
    assert!(s.contains("# my ssh config"));
    assert!(s.contains("vps -> root@vps.example.com:2222"));
    assert!(s.contains("?:help  q:quit"));
}

#[test]
fn web_tui_1_hosts_sorted_by_name_without_defaults() {
    let f = Fixture::new("Host *\n    User x\n\nHost b\n    HostName b.example.com\n\nHost a\n    HostName a.example.com\n");
    let mut app = f.app();
    assert_eq!(drawn_order(&mut app, &["a", "b", "*"]), names(&["a", "b"]));
    assert!(screen(&mut app).contains("[Hosts 2/2]"));
}

#[test]
fn sort_section() {
    assert_eq!(
        sorted('1', false),
        names(&["ant", "fox", "bee", "cat", "dog", "eel", "gnu"])
    );
    assert_eq!(
        sorted('1', true),
        names(&["dog", "eel", "bee", "cat", "ant", "fox", "gnu"])
    );
}

#[test]
fn sort_host() {
    assert_eq!(
        sorted('2', false),
        names(&["ant", "bee", "cat", "dog", "eel", "fox", "gnu"])
    );
    assert_eq!(
        sorted('2', true),
        names(&["gnu", "fox", "eel", "dog", "cat", "bee", "ant"])
    );
}

#[test]
fn sort_user() {
    assert_eq!(
        sorted('3', false),
        names(&["eel", "gnu", "ant", "bee", "cat", "fox", "dog"])
    );
    assert_eq!(
        sorted('3', true),
        names(&["fox", "cat", "ant", "bee", "gnu", "eel", "dog"])
    );
}

#[test]
fn sort_port_numeric_missing_last() {
    assert_eq!(
        sorted('5', false),
        names(&["bee", "ant", "eel", "cat", "dog", "fox", "gnu"])
    );
    assert_eq!(
        sorted('5', true),
        names(&["eel", "ant", "bee", "cat", "dog", "fox", "gnu"])
    );
}

#[test]
fn sort_proxy() {
    assert_eq!(
        sorted('6', false),
        names(&["fox", "cat", "ant", "bee", "dog", "eel", "gnu"])
    );
    assert_eq!(
        sorted('6', true),
        names(&["ant", "cat", "fox", "bee", "dog", "eel", "gnu"])
    );
}

#[test]
fn sort_jump() {
    assert_eq!(
        sorted('7', false),
        names(&["dog", "fox", "bee", "ant", "cat", "eel", "gnu"])
    );
    assert_eq!(
        sorted('7', true),
        names(&["bee", "fox", "dog", "ant", "cat", "eel", "gnu"])
    );
}

#[test]
fn sort_header_marker_and_file_order_restore() {
    let f = Fixture::new(&six_hosts());
    let mut app = f.app();
    press(&mut app, '2');
    assert!(screen(&mut app).contains("Host▲"));
    press(&mut app, '2');
    assert!(screen(&mut app).contains("Host▼"));
    press(&mut app, '0');
    assert_eq!(
        drawn_order(&mut app, &SIX),
        names(&["gnu", "ant", "fox", "bee", "cat", "dog", "eel"])
    );
}

/// Sets column filter `col` to `text` with `f`, digit, text, Enter.
fn col_filter(app: &mut rustorm_tui::App, col: char, text: &str) {
    press(app, 'f');
    press(app, col);
    typ(app, text);
    app.handle(key(KeyCode::Enter));
}

fn filter_case(col: char, text: &str, header: &str, expect: &[&str]) {
    let f = Fixture::new(&six_hosts());
    let mut app = f.app();
    col_filter(&mut app, col, text);
    assert_eq!(drawn_order(&mut app, &SIX), names(expect));
    let s = screen(&mut app);
    assert!(
        s.contains(&format!("{header}*")),
        "filtered header marker for {header}"
    );
    assert!(s.contains(&format!("[Hosts {}/7]", expect.len())));
    // Clearing the filter restores every row (restore-state).
    press(&mut app, 'f');
    press(&mut app, col);
    backspace(&mut app, text.len());
    app.handle(key(KeyCode::Enter));
    assert_eq!(drawn_order(&mut app, &SIX).len(), 7);
    assert!(screen(&mut app).contains("[Hosts 7/7]"));
}

#[test]
fn filter_section() {
    filter_case('1', "BO", "Section", &["bee", "cat"]);
}

#[test]
fn filter_host() {
    filter_case('2', "e", "Host", &["bee", "eel"]);
}

#[test]
fn filter_user() {
    filter_case('3', "de", "User", &["ant", "bee"]);
}

#[test]
fn filter_proxy() {
    filter_case('6', "gw", "Proxy", &["ant", "cat"]);
}

#[test]
fn filter_jump() {
    filter_case('7', "bastion", "Jump", &["fox", "bee"]);
}

#[test]
fn filter_combo_section_and_user() {
    let f = Fixture::new(&six_hosts());
    let mut app = f.app();
    col_filter(&mut app, '1', "bob");
    col_filter(&mut app, '3', "deploy");
    assert_eq!(drawn_order(&mut app, &SIX), names(&["bee"]));
    let s = screen(&mut app);
    assert!(s.contains("filter: section~bob user~deploy"), "{s}");
    press(&mut app, 'x');
    assert_eq!(drawn_order(&mut app, &SIX).len(), 7);
}

#[test]
fn filter_global_any_column() {
    let f = Fixture::new(&six_hosts());
    let mut app = f.app();
    press(&mut app, '/');
    typ(&mut app, "corkscrew");
    app.handle(key(KeyCode::Enter));
    assert_eq!(drawn_order(&mut app, &SIX), names(&["fox"]));
    // Esc on a filter edit restores the previous text.
    press(&mut app, '/');
    typ(&mut app, "zz");
    app.handle(key(KeyCode::Esc));
    assert_eq!(app.filters().global, "corkscrew");
}

#[test]
fn filter_none_shows_no_hosts_match() {
    let f = Fixture::new(&six_hosts());
    let mut app = f.app();
    press(&mut app, '/');
    typ(&mut app, "zzz");
    app.handle(key(KeyCode::Enter));
    let s = screen(&mut app);
    assert!(s.contains("no hosts match filter: any~zzz"), "{s}");
    assert!(s.contains("[Hosts 0/7]"));
    assert!(drawn_order(&mut app, &SIX).is_empty());
    // Row actions report the empty selection instead of acting (F7).
    press(&mut app, 'd');
    assert_eq!(app.message().as_deref(), Some("Error: No host selected."));
    assert!(screen(&mut app).contains("Error: No host selected."));
}

#[test]
fn section_list_enter_filters_to_section() {
    let f = Fixture::new(&six_hosts());
    let mut app = f.app();
    app.handle(key(KeyCode::BackTab));
    assert_eq!(app.focus(), rustorm_tui::Focus::Sections);
    press(&mut app, 'j');
    press(&mut app, 'j'); // All, alpha, bob
    app.handle(key(KeyCode::Enter));
    assert_eq!(drawn_order(&mut app, &SIX), names(&["bee", "cat"]));
    press(&mut app, 'g');
    app.handle(key(KeyCode::Enter));
    assert_eq!(drawn_order(&mut app, &SIX).len(), 7);
}

#[test]
fn plain_theme_uses_ascii_markers() {
    let f = Fixture::new(&six_hosts());
    let mut opts = options();
    opts.theme = rustorm_tui::Theme::plain();
    let mut app = rustorm_tui::App::with_options(&f.path, opts).unwrap();
    press(&mut app, '2');
    let s = screen(&mut app);
    assert!(s.contains("Host^"));
    assert!(!s.contains('·'));
}
