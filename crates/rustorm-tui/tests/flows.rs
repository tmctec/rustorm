//! Add, edit, delete, clone, move and rename-section flows, each asserting
//! the file equals what the core produces for the same operation (so the
//! TUI changes the file exactly as the CLI would), plus reload-then-apply.
//! Catalog cases: web-tui-2..7.

mod common;
use common::*;
use crossterm::event::KeyCode;
use rustorm_core::{AddSpec, CloneSpec, Config, EditSpec, HostSelector};

fn core(text: &str, f: impl FnOnce(&mut Config)) -> String {
    let mut c = Config::parse(text).unwrap();
    f(&mut c);
    c.render()
}

fn enter(app: &mut rustorm_tui::App) {
    app.handle(key(KeyCode::Enter));
}

fn tab(app: &mut rustorm_tui::App) {
    app.handle(key(KeyCode::Tab));
}

/// web-tui-2
#[test]
fn web_tui_2_add_writes_host_and_backup() {
    let orig = three_hosts();
    let f = Fixture::new(&orig);
    let mut app = f.app();
    press(&mut app, 'a');
    assert!(screen(&mut app).contains("Add host"));
    typ(&mut app, "newbox");
    tab(&mut app);
    typ(&mut app, "root@newbox.example.com:2200");
    tab(&mut app);
    typ(&mut app, "~/.ssh/k.pem");
    enter(&mut app);
    let expected = core(&orig, |c| {
        c.add(
            &AddSpec {
                name: "newbox".into(),
                uri: "root@newbox.example.com:2200".into(),
                identity: Some("~/.ssh/k.pem".into()),
                ..Default::default()
            },
            &env(),
        )
        .unwrap();
    });
    assert_eq!(f.read(), expected);
    assert!(f.read().contains(
        "Host newbox\n    HostName newbox.example.com\n    User root\n    Port 2200\n    IdentityFile ~/.ssh/k.pem\n"
    ));
    assert_eq!(f.backup().as_deref(), Some(orig.as_str()));
    let s = screen(&mut app);
    assert!(
        s.contains("✔ newbox added. Connect with: ssh newbox"),
        "{s}"
    );
    assert!(s.contains("[Hosts 4/4]"));
    assert_eq!(app.selected(), Some("newbox"));
}

#[test]
fn add_into_new_section_on_unsectioned_file_reports_catch_all() {
    let orig = "Host a\n    HostName a.example.com\n";
    let f = Fixture::new(orig);
    let mut app = f.app();
    press(&mut app, 'a');
    typ(&mut app, "b");
    tab(&mut app);
    typ(&mut app, "u@b.example.com");
    tab(&mut app);
    tab(&mut app);
    typ(&mut app, "bob\n");
    assert_eq!(
        app.message().as_deref(),
        Some(
            "✔ b added to section bob. Connect with: ssh b Hosts without a section moved to other."
        )
    );
    assert!(screen(&mut app).contains("Sections"));
}

/// web-tui-3
#[test]
fn web_tui_3_duplicate_add_shows_inline_error_file_unchanged() {
    let orig = three_hosts();
    let f = Fixture::new(&orig);
    let mut app = f.app();
    press(&mut app, 'a');
    typ(&mut app, "vps");
    tab(&mut app);
    typ(&mut app, "root@other.example.com\n");
    let s = screen(&mut app);
    assert!(s.contains("Add host"), "form stays open");
    assert!(
        s.contains("Error: vps already exists. Press e on vps to edit it."),
        "{s}"
    );
    assert_eq!(f.read(), orig);
    assert!(f.backup().is_none());
    app.handle(key(KeyCode::Esc));
    assert!(!screen(&mut app).contains("Add host"));
}

/// web-tui-4
#[test]
fn web_tui_4_edit_with_emptied_identity_removes_identityfile() {
    let orig = three_hosts();
    assert!(orig.contains("IdentityFile ~/.ssh/vps.pem"));
    let f = Fixture::new(&orig);
    let mut app = f.app();
    select(&mut app, "vps");
    press(&mut app, 'e');
    let s = screen(&mut app);
    assert!(s.contains("Edit host vps"));
    assert!(
        s.contains("Connection URI: root@vps.example.com:2222"),
        "{s}"
    );
    backspace(&mut app, "root@vps.example.com:2222".len());
    typ(&mut app, "emre@vps.example.com:2400");
    tab(&mut app);
    backspace(&mut app, "~/.ssh/vps.pem".len());
    enter(&mut app);
    let expected = core(&orig, |c| {
        c.edit(
            &EditSpec {
                name: "vps".into(),
                uri: "emre@vps.example.com:2400".into(),
                ..Default::default()
            },
            &env(),
        )
        .unwrap();
        c.unset(&HostSelector::Name("vps".into()), &["IdentityFile".into()])
            .unwrap();
    });
    let now = f.read();
    assert_eq!(now, expected);
    assert!(!now.contains("IdentityFile"));
    assert!(now.contains("Host vps\n    HostName vps.example.com\n    User emre\n    Port 2400\n"));
    // Other keys and comments untouched.
    assert!(now.contains("# my ssh config\n"));
    assert!(now.contains("# the web box\nHost web\n"));
    assert!(now.contains("    ProxyCommand ssh -W %h:%p bastion\n"));
    assert_eq!(app.message().as_deref(), Some("✔ vps updated."));
}

/// web-tui-5
#[test]
fn web_tui_5_delete_confirmed() {
    let orig = three_hosts();
    let f = Fixture::new(&orig);
    let mut app = f.app();
    select(&mut app, "db");
    press(&mut app, 'd');
    let s = screen(&mut app);
    assert!(
        s.contains(&format!("Delete host db from {}? [y/N]", f.path.display())),
        "{s}"
    );
    press(&mut app, 'y');
    let expected = core(&orig, |c| {
        c.delete(&["db".to_string()]).unwrap();
    });
    let now = f.read();
    assert_eq!(now, expected);
    assert!(!now.contains("Host db"));
    assert!(
        now.contains("# the web box\nHost web\n"),
        "comment above another host survives"
    );
    assert_eq!(f.backup().as_deref(), Some(orig.as_str()));
    let s = screen(&mut app);
    assert!(s.contains("✔ db deleted."));
    assert!(s.contains("[Hosts 2/2]"));
}

/// web-tui-6
#[test]
fn web_tui_6_delete_cancelled_leaves_file_unchanged() {
    let orig = three_hosts();
    let f = Fixture::new(&orig);
    let mut app = f.app();
    select(&mut app, "vps");
    press(&mut app, 'd');
    press(&mut app, 'n');
    assert_eq!(f.read(), orig);
    assert!(f.backup().is_none());
    assert_eq!(app.message().as_deref(), Some("Delete cancelled."));
    // Any key but y cancels, Enter included.
    press(&mut app, 'd');
    enter(&mut app);
    assert_eq!(f.read(), orig);
    assert!(screen(&mut app).contains("[Hosts 3/3]"));
}

#[test]
fn clone_flow_matches_core() {
    let orig = three_hosts();
    let f = Fixture::new(&orig);
    let mut app = f.app();
    select(&mut app, "web");
    press(&mut app, 'c');
    let s = screen(&mut app);
    assert!(s.contains("Clone host web"));
    assert!(s.contains("Section: other"));
    typ(&mut app, "web2\n");
    let expected = core(&orig, |c| {
        c.clone_host(&CloneSpec {
            source: "web".into(),
            new_name: "web2".into(),
            section: Some("other".into()),
            ..Default::default()
        })
        .unwrap();
    });
    assert_eq!(f.read(), expected);
    assert!(f
        .read()
        .contains("Host web2\n    HostName web2.example.com\n"));
    assert_eq!(
        app.message().as_deref(),
        Some("✔ web2 added. Connect with: ssh web2")
    );
    // Cloning onto an existing name keeps the form open with the error.
    select(&mut app, "web");
    press(&mut app, 'c');
    typ(&mut app, "db\n");
    assert!(screen(&mut app).contains("Error: db already exists."));
    assert_eq!(f.read(), expected);
}

#[test]
fn move_to_section_flow_matches_core() {
    let orig = three_hosts();
    let f = Fixture::new(&orig);
    let mut app = f.app();
    select(&mut app, "db");
    press(&mut app, 'm');
    let s = screen(&mut app);
    assert!(s.contains("Move host db"));
    tab(&mut app);
    backspace(&mut app, "other".len());
    typ(&mut app, "bob\n");
    let expected = core(&orig, |c| {
        c.move_host("db", None, Some("bob")).unwrap();
    });
    assert_eq!(f.read(), expected);
    assert_eq!(app.message().as_deref(), Some("✔ db moved to section bob."));
    let s = screen(&mut app);
    assert!(s.contains(&format!("{:<14}{:>4}", "bob", 2)));
    // Rename through the same form.
    select(&mut app, "db");
    press(&mut app, 'm');
    backspace(&mut app, 2);
    typ(&mut app, "pg\n");
    assert_eq!(
        app.message().as_deref(),
        Some("✔ db renamed to pg. Connect with: ssh pg")
    );
    // Neither changed is refused in the form.
    select(&mut app, "pg");
    press(&mut app, 'm');
    enter(&mut app);
    assert!(screen(&mut app).contains("Error: Change the name, the section, or both."));
}

#[test]
fn rename_section_flow_matches_core() {
    let orig = three_hosts();
    let f = Fixture::new(&orig);
    let mut app = f.app();
    app.handle(key(KeyCode::BackTab));
    press(&mut app, 'j'); // bob
    press(&mut app, 'R');
    assert!(screen(&mut app).contains("Rename section bob"));
    backspace(&mut app, 3);
    typ(&mut app, "cypresspt\n");
    let expected = core(&orig, |c| {
        c.rename_section("bob", "cypresspt").unwrap();
    });
    assert_eq!(f.read(), expected);
    assert_eq!(
        app.message().as_deref(),
        Some("✔ section bob renamed to cypresspt.")
    );
    assert!(screen(&mut app).contains("cypresspt"));
}

/// web-tui-7, no conflict: a hand edit to another host survives a TUI write.
#[test]
fn web_tui_7_reload_then_apply_keeps_hand_edit() {
    let orig = three_hosts();
    let f = Fixture::new(&orig);
    let mut app = f.app();
    let hand = orig.replace(
        "    User postgres\n",
        "    User postgres\n    # hand edit\n    Compression yes\n",
    );
    std::fs::write(&f.path, &hand).unwrap();
    select(&mut app, "vps");
    press(&mut app, 'd');
    press(&mut app, 'y');
    let expected = core(&hand, |c| {
        c.delete(&["vps".to_string()]).unwrap();
    });
    let now = f.read();
    assert_eq!(now, expected);
    assert!(now.contains("    # hand edit\n    Compression yes\n"));
    assert_eq!(f.backup().as_deref(), Some(hand.as_str()));
}

/// web-tui-7, conflict: the target host itself changed on disk; the TUI
/// asks, and "no" keeps the disk version untouched.
#[test]
fn web_tui_7_conflict_on_same_host_prompts() {
    let orig = three_hosts();
    let f = Fixture::new(&orig);
    let mut app = f.app();
    select(&mut app, "vps");
    press(&mut app, 'e');
    let hand = orig.replace("    User root\n", "    User handmade\n");
    std::fs::write(&f.path, &hand).unwrap();
    enter(&mut app);
    let s = screen(&mut app);
    assert!(
        s.contains("vps changed on disk since it was loaded. Overwrite it? [y/N]"),
        "{s}"
    );
    press(&mut app, 'n');
    assert_eq!(f.read(), hand);
    assert!(app
        .message()
        .unwrap()
        .contains("Kept the version of vps on disk"));
    // The table shows the reloaded file.
    assert!(table_lines(&draw(&mut app)).join("\n").contains("handmade"));
    // y applies the edit on top of the fresh file.
    std::fs::write(&f.path, orig.replace("    User root\n", "    User again\n")).unwrap();
    select(&mut app, "vps");
    press(&mut app, 'e');
    typ(&mut app, "\n");
    assert!(screen(&mut app).contains("Overwrite it? [y/N]"));
    press(&mut app, 'y');
    assert!(f
        .read()
        .contains("Host vps\n    HostName vps.example.com\n    User handmade\n"));
}

#[test]
fn forms_refuse_while_editor_has_unsaved_edits() {
    let orig = three_hosts();
    let f = Fixture::new(&orig);
    let mut app = f.app();
    tab(&mut app);
    assert_eq!(app.focus(), rustorm_tui::Focus::Editor);
    press(&mut app, 'x');
    app.handle(key(KeyCode::Esc));
    press(&mut app, 'a');
    assert_eq!(
        app.message().as_deref(),
        Some("Error: Save or discard the editor's changes first.")
    );
    assert!(!screen(&mut app).contains("Add host"));
}

/// addsec-tui: `n` creates a section exactly as the core does — from the
/// section list on a sectioned file, and from the table on an unsectioned
/// one, where the catch-all is created too.
#[test]
fn addsec_tui_new_section_key_writes_the_banner_and_catch_all() {
    let orig = three_hosts();
    let f = Fixture::new(&orig);
    let mut app = f.app();
    app.handle(key(KeyCode::BackTab));
    assert_eq!(app.focus(), rustorm_tui::Focus::Sections);
    press(&mut app, 'n');
    assert!(screen(&mut app).contains("New section"));
    typ(&mut app, "lab\n");
    let expected = core(&orig, |c| {
        c.add_section("lab", None).unwrap();
    });
    assert_eq!(f.read(), expected);
    assert_eq!(f.backup().as_deref(), Some(orig.as_str()));
    assert_eq!(app.message().as_deref(), Some("✔ section lab added."));
    let s = screen(&mut app);
    assert!(s.contains("lab"), "{s}");
    let sections: Vec<String> = Config::parse(&f.read())
        .unwrap()
        .sections()
        .into_iter()
        .map(|x| format!("{} {}", x.name, x.hosts))
        .collect();
    assert_eq!(sections, ["bob 1", "lab 0", "other 2"]);

    let orig = "Host a\n    HostName a.example.com\n\nHost b\n    HostName b.example.com\n";
    let f = Fixture::new(orig);
    let mut app = f.app();
    press(&mut app, 'n');
    typ(&mut app, "work\n");
    let expected = core(orig, |c| {
        c.add_section("work", None).unwrap();
    });
    assert_eq!(f.read(), expected);
    assert!(f.read().contains("section: work") && f.read().contains("section: other"));
    assert_eq!(
        app.message().as_deref(),
        Some("✔ section work added; other created with 2 hosts.")
    );
    assert!(
        screen(&mut app).contains("Sections"),
        "the section list appears"
    );

    press(&mut app, 'n');
    typ(&mut app, "WORK\n");
    let s = screen(&mut app);
    assert!(s.contains("Error: section WORK already exists."), "{s}");
    assert_eq!(f.read(), expected, "an existing name writes nothing");
    app.handle(key(KeyCode::Esc));
}

/// addsec-tui (negative): Esc cancels the form and nothing is written.
#[test]
fn addsec_tui_esc_cancels_without_writing() {
    let orig = three_hosts();
    let f = Fixture::new(&orig);
    let mut app = f.app();
    press(&mut app, 'n');
    typ(&mut app, "lab");
    app.handle(key(KeyCode::Esc));
    assert!(!screen(&mut app).contains("New section"));
    assert_eq!(f.read(), orig);
    assert!(f.backup().is_none());
    assert_eq!(app.message().as_deref(), Some("Cancelled."));
    press(&mut app, 'n');
    typ(&mut app, "\n");
    assert!(screen(&mut app).contains("Error: Section name is required."));
    assert_eq!(f.read(), orig);
}
