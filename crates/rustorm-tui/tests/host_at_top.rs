//! Following a host puts its `Host` line on the editor pane's first row
//! (docs/tui.md, Editor). Catalog cases: top-1 .. top-8.

mod common;
use common::*;
use crossterm::event::KeyCode;
use rustorm_tui::{App, Focus};

/// Hosts in file order: alphabetical, as the table lists them, except
/// that cypressMelissa sits directly above cypressMBP.
const NAMES: [&str; 40] = [
    "acacia",
    "alder",
    "ash",
    "aspen",
    "balsa",
    "banyan",
    "beech",
    "birch",
    "boxwood",
    "buckeye",
    "butternut",
    "catalpa",
    "cedar",
    "cherry",
    "chestnut",
    "cottonwood",
    "cypressMelissa",
    "cypressMBP",
    "cypressMini",
    "cypressNAS",
    "deodar",
    "dogwood",
    "ebony",
    "elm",
    "fig",
    "fir",
    "ginkgo",
    "gum",
    "hazel",
    "hemlock",
    "hickory",
    "holly",
    "ilex",
    "ironwood",
    "jacaranda",
    "juniper",
    "katsura",
    "larch",
    "linden",
    "maple",
];

/// About 40 host blocks of 3 to 6 lines; cypressMelissa's is short so
/// cypressMBP shows a few lines below it, and the last block is short so
/// the file ends well inside one pane height of it.
fn fixture() -> String {
    let mut s = String::from("# lab hosts\n\n");
    for (i, n) in NAMES.iter().enumerate() {
        s.push_str(&format!("Host {n}\n    HostName {n}.example.com\n"));
        if *n == "cypressMelissa" || i == NAMES.len() - 1 {
            s.push('\n');
            continue;
        }
        s.push_str("    User admin\n");
        for k in 0..i % 4 {
            s.push_str(&format!("    LocalForward {} localhost:80\n", 8000 + k));
        }
        s.push('\n');
    }
    s
}

/// Handles `code` and draws, as the real loop does after every key.
fn step(app: &mut App, code: KeyCode) {
    app.handle(key(code));
    draw(app);
}

/// Moves the table selection to `name` one row at a time, drawing each.
fn walk_to(app: &mut App, name: &str) {
    let order: Vec<String> = app.visible_rows().iter().map(|r| r.name.clone()).collect();
    let to = order.iter().position(|n| n == name).unwrap();
    for _ in 0..order.len() {
        let at = order
            .iter()
            .position(|n| Some(n.as_str()) == app.selected())
            .unwrap();
        if at == to {
            return;
        }
        step(
            app,
            if at < to {
                KeyCode::Char('j')
            } else {
                KeyCode::Char('k')
            },
        );
    }
    assert_eq!(app.selected(), Some(name));
}

/// The rows inside the editor pane's border, trimmed.
fn pane(app: &mut App) -> Vec<String> {
    let all = lines(&draw(app));
    let title = all.iter().position(|l| l.contains("Editor ")).unwrap();
    all[title + 1..all.len() - 3]
        .iter()
        .map(|l| {
            l.trim_matches(|c: char| "│║".contains(c))
                .trim_end()
                .to_string()
        })
        .collect()
}

fn first_row(app: &mut App) -> String {
    pane(app)[0].clone()
}

/// Asserts the pane's first row is `Host <name>`; the failure names the
/// row it is on instead.
fn assert_first(app: &mut App, name: &str) {
    let rows = pane(app);
    let want = format!("Host {name}");
    let at = rows.iter().position(|l| *l == want);
    assert_eq!(
        at,
        Some(0),
        "{want} is on pane row {at:?} of {} (0 = first); pane: {rows:#?}",
        rows.len()
    );
}

fn host_line_row(name: &str) -> usize {
    fixture()
        .lines()
        .position(|l| l == format!("Host {name}"))
        .unwrap()
}

/// top-1: moving the selection down to a host below the visible lines puts
/// its Host line on the first row.
#[test]
fn top_1_selection_down_puts_host_line_first() {
    let f = Fixture::new(&fixture());
    let mut app = f.app();
    draw(&mut app);
    assert!(host_line_row("fir") >= pane(&mut app).len());
    walk_to(&mut app, "fir");
    assert_first(&mut app, "fir");
}

/// top-2: moving the selection up to a host above the visible lines puts
/// its Host line on the first row.
#[test]
fn top_2_selection_up_puts_host_line_first() {
    let f = Fixture::new(&fixture());
    let mut app = f.app();
    walk_to(&mut app, "maple");
    walk_to(&mut app, "hazel");
    assert_first(&mut app, "hazel");
}

/// top-3: cypressMBP's Host line already shows below cypressMelissa's;
/// selecting it still scrolls it to the first row.
#[test]
fn top_3_visible_next_host_still_scrolls_to_first_row() {
    let f = Fixture::new(&fixture());
    let mut app = f.app();
    draw(&mut app);
    walk_to(&mut app, "cypressMelissa");
    let before = pane(&mut app);
    assert!(before.iter().any(|l| l == "Host cypressMBP"), "{before:#?}");
    // The table lists cypressMBP above cypressMelissa.
    step(&mut app, KeyCode::Char('k'));
    assert_eq!(app.selected(), Some("cypressMBP"));
    assert_first(&mut app, "cypressMBP");
}

/// top-4: a host near the end of the file still lands on the first row;
/// the pane below the file's last line stays empty.
#[test]
fn top_4_host_near_end_of_file_is_first_row() {
    let f = Fixture::new(&fixture());
    let mut app = f.app();
    draw(&mut app);
    walk_to(&mut app, "maple");
    assert_first(&mut app, "maple");
    let rows = pane(&mut app);
    assert!(rows[4..].iter().all(|l| l.is_empty()), "{rows:#?}");
}

/// top-5: `o` on a host far down the file opens the editor with its Host
/// line first, after the editor was scrolled elsewhere.
#[test]
fn top_5_o_opens_editor_with_host_line_first() {
    let f = Fixture::new(&fixture());
    let mut app = f.app();
    draw(&mut app);
    walk_to(&mut app, "hickory");
    step(&mut app, KeyCode::Tab);
    assert_eq!(app.focus(), Focus::Editor);
    for _ in 0..host_line_row("hickory") {
        step(&mut app, KeyCode::Up);
    }
    assert_eq!(first_row(&mut app), "# lab hosts");
    step(&mut app, KeyCode::Esc);
    step(&mut app, KeyCode::Char('o'));
    assert_eq!(app.focus(), Focus::Editor);
    assert_first(&mut app, "hickory");
}

/// top-6: arrow keys past the pane's bottom scroll one line at a time
/// (ordinary movement does not jump).
#[test]
fn top_6_arrow_keys_scroll_one_line_at_a_time() {
    let text = fixture();
    let all: Vec<&str> = text.lines().collect();
    let f = Fixture::new(&text);
    let mut app = f.app();
    step(&mut app, KeyCode::Tab);
    assert_eq!(app.focus(), Focus::Editor);
    assert_eq!(app.editor_cursor(), (0, 0));
    let h = pane(&mut app).len();
    for _ in 0..h - 1 {
        step(&mut app, KeyCode::Down);
    }
    assert_eq!(first_row(&mut app), all[0]);
    for line in &all[1..=3] {
        step(&mut app, KeyCode::Down);
        assert_eq!(first_row(&mut app), *line);
    }
}

/// top-7: after a quick-form write the editor shows the host's Host line
/// first.
#[test]
fn top_7_write_puts_host_line_first() {
    let f = Fixture::new(&fixture());
    let mut app = f.app();
    draw(&mut app);
    walk_to(&mut app, "ebony");
    step(&mut app, KeyCode::Char('e'));
    typ(&mut app, "1");
    step(&mut app, KeyCode::Enter);
    assert!(f.read().contains("HostName ebony.example.com1"));
    assert_eq!(app.selected(), Some("ebony"));
    assert_first(&mut app, "ebony");
}

/// top-8: with a host shown first, Tab to the editor and back without
/// changing the selection leaves the view where it was.
#[test]
fn top_8_unchanged_selection_never_rescrolls() {
    let text = fixture();
    let f = Fixture::new(&text);
    let mut app = f.app();
    draw(&mut app);
    assert_eq!(app.selected(), Some("acacia"));
    // Scroll the editor to the end of the file, then select alder, the
    // second host in the file, from the table.
    step(&mut app, KeyCode::Tab);
    for _ in 0..text.lines().count() {
        step(&mut app, KeyCode::Down);
    }
    step(&mut app, KeyCode::Esc);
    walk_to(&mut app, "alder");
    assert_first(&mut app, "alder");
    let before = pane(&mut app);
    step(&mut app, KeyCode::Tab);
    assert_eq!(app.focus(), Focus::Editor);
    step(&mut app, KeyCode::Tab);
    assert_eq!(app.focus(), Focus::Table);
    assert_eq!(pane(&mut app), before);
    assert_eq!(app.selected(), Some("alder"));
}
