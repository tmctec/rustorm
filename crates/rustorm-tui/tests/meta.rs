//! Host metadata in the TUI: the Notes & location group of the settings
//! form, the status line, the `/` filter and `# key:` completion in the
//! editor (docs/tui.md; plan anemone step 8; catalog mt-1 .. mt-6).

mod common;
use common::*;
use crossterm::event::KeyCode;
use rustorm_core::{Config, SettingChange};

const LAB: &str = "\
Host *
    User fallback

# the lab box
# note: Primary build box
# location: Austin DC, rack 4
# tags: prod, db
Host lab
    HostName lab.example.com
    Compression yes

Host other
    HostName other.example.com
";

fn core(text: &str, host: &str, changes: &[SettingChange]) -> String {
    let mut c = Config::parse(text).unwrap();
    c.apply_settings(host, changes).unwrap();
    c.render()
}

fn open_filled(app: &mut rustorm_tui::App, host: &str) {
    select(app, host);
    app.handle(key(KeyCode::Enter));
    assert!(app.settings_row().is_some(), "form did not open");
}

fn goto(app: &mut rustorm_tui::App, key_name: &str) {
    app.handle(key(KeyCode::Home));
    for _ in 0..400 {
        if app.settings_row().unwrap().0 == key_name {
            return;
        }
        app.handle(key(KeyCode::Down));
    }
    panic!("no row {key_name}");
}

// mt-1: the filled view opens on the Notes & location group, first, with the host's labels.
#[test]
fn mt_1_notes_group_comes_first_and_is_prefilled() {
    let f = Fixture::new(LAB);
    let mut app = f.app();
    open_filled(&mut app, "lab");
    let s = screen(&mut app);
    let notes = s.find("── Notes & location").expect("Notes group");
    let conn = s.find("── Connection").expect("Connection group");
    assert!(notes < conn, "Notes & location comes first:\n{s}");
    assert_eq!(app.settings_row().unwrap(), ("note", "Primary build box"));
    goto(&mut app, "location");
    assert_eq!(app.settings_row().unwrap().1, "Austin DC, rack 4");
    goto(&mut app, "tags");
    assert_eq!(app.settings_row().unwrap().1, "prod, db");
    let s = screen(&mut app);
    assert!(s.contains("tags in use: db, prod"), "{s}");
    goto(&mut app, "Compression");
    assert_eq!(app.settings_row().unwrap().1, "yes");
}

// mt-2: editing location and tags in the form writes the comment lines, byte-identical to the core.
#[test]
fn mt_2_form_writes_metadata_lines() {
    let f = Fixture::new(LAB);
    let mut app = f.app();
    open_filled(&mut app, "lab");
    goto(&mut app, "location");
    app.handle(ctrl('u'));
    typ(&mut app, "Dallas");
    goto(&mut app, "tags");
    typ(&mut app, ", edge");
    app.handle(key(KeyCode::Enter));
    assert!(app.settings_row().is_none(), "form closed on save");
    let expected = core(
        LAB,
        "lab",
        &[
            SettingChange::set("location", "Dallas"),
            SettingChange::set("tags", "prod, db, edge"),
        ],
    );
    assert_eq!(f.read(), expected);
    assert!(f.read().contains("# the lab box\n# note: Primary build box\n# location: Dallas\n# tags: prod, db, edge\nHost lab\n"));
    assert_eq!(f.backup().as_deref(), Some(LAB));
}

// mt-3: a host without metadata gets the lines from the form, above Host, in write order.
#[test]
fn mt_3_form_adds_metadata_to_a_plain_host() {
    let f = Fixture::new(LAB);
    let mut app = f.app();
    open_filled(&mut app, "other");
    // Filled view: no label rows yet; add through Add setting.
    app.handle(key(KeyCode::End));
    assert_eq!(app.settings_row().unwrap().0, "Add setting");
    typ(&mut app, "loca");
    app.handle(key(KeyCode::Tab));
    assert_eq!(app.settings_row().unwrap().0, "location");
    typ(&mut app, "Oslo");
    app.handle(key(KeyCode::End));
    typ(&mut app, "tags");
    app.handle(key(KeyCode::Tab));
    typ(&mut app, "lab");
    app.handle(key(KeyCode::Enter));
    let expected = core(
        LAB,
        "other",
        &[
            SettingChange::set("location", "Oslo"),
            SettingChange::set("tags", "lab"),
        ],
    );
    assert_eq!(f.read(), expected);
    assert!(f.read().ends_with("# location: Oslo\n# tags: lab\nHost other\n    HostName other.example.com\n"));
}

// mt-4: the status line shows location, tags and the first note line for the selected host.
#[test]
fn mt_4_status_line_shows_metadata() {
    let f = Fixture::new(LAB);
    let mut app = f.app();
    select(&mut app, "lab");
    let s = screen(&mut app);
    assert!(s.contains("Austin DC, rack 4 · prod, db · Primary build box"), "{s}");
    select(&mut app, "other");
    let s = screen(&mut app);
    assert!(!s.contains("Austin DC"), "{s}");
}

// mt-5: the / filter matches metadata text.
#[test]
fn mt_5_filter_matches_metadata() {
    let f = Fixture::new(LAB);
    let mut app = f.app();
    press(&mut app, '/');
    typ(&mut app, "rack 4\n");
    assert_eq!(app.visible_rows().len(), 1);
    assert_eq!(app.visible_rows()[0].name, "lab");
    press(&mut app, 'x');
    press(&mut app, '/');
    typ(&mut app, "db\n");
    let names: Vec<&str> = app.visible_rows().iter().map(|r| r.name.as_str()).collect();
    assert_eq!(names, ["lab"], "tag match");
    press(&mut app, 'x');
    assert_eq!(app.visible_rows().len(), 2);
}

// mt-6: typing `# lo` in the editor offers `location:`; Space accepts into `# location: `.
#[test]
fn mt_6_editor_completes_metadata_keys() {
    let f = Fixture::new(LAB);
    let mut app = f.app();
    select(&mut app, "other");
    press(&mut app, 'o');
    // The cursor is on `Host other`; open a line above it and type a comment.
    app.handle(key(KeyCode::Home));
    app.handle(key(KeyCode::Enter));
    app.handle(key(KeyCode::Up));
    typ(&mut app, "# lo");
    let s = screen(&mut app);
    assert!(s.contains("# location:"), "ghost shows the key:\n{s}");
    press(&mut app, ' ');
    let text = app.editor_text();
    assert!(text.contains("# location: \nHost other\n"), "{text}");
    typ(&mut app, "Oslo");
    app.handle(ctrl('s'));
    assert!(f.read().contains("# location: Oslo\nHost other\n"), "{}", f.read());
    let c = Config::parse(&f.read()).unwrap();
    let h = c.host(c.find_host("other").unwrap());
    assert_eq!(h.meta().location.as_deref(), Some("Oslo"));
}
