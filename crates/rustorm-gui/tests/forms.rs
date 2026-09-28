//! Detail form: add, edit, delete, clone, move and reload-then-apply
//! (web-gui-2..7).

mod common;

use common::*;
use rustorm_core::{AddSpec, CloneSpec, Config, EditSpec, HostSelector};
use rustorm_gui::Dialog;

const NO_VPS: &str = "\
# laptop ssh config
Host *
    ServerAliveInterval 60

Host github
    HostName github.com
    User git

# production web
Host web-prod
    HostName webprod.example.com
    User web
";

/// web-gui-2: the add form writes Host vps with HostName, User, Port and
/// IdentityFile, backs up, and shows the row.
#[test]
fn web_gui_2_add_writes_host_and_backup() {
    let f = Fixture::new(NO_VPS);
    let mut h = harness(&f.path);
    h.run();
    click(&mut h, "Add Host");
    type_into(&mut h, "Name", "vps");
    type_into(&mut h, "Connection URI", "root@vps.example.com:2222");
    type_into(&mut h, "Identity file", "~/.ssh/k.pem");
    click(&mut h, "Save");

    let mut expected = Config::parse(NO_VPS).unwrap();
    expected
        .add(
            &AddSpec {
                name: "vps".into(),
                uri: "root@vps.example.com:2222".into(),
                identity: Some("~/.ssh/k.pem".into()),
                ..AddSpec::default()
            },
            &env(),
        )
        .unwrap();
    let text = f.read();
    assert_eq!(text, expected.render());
    assert!(text.ends_with(
        "Host vps\n    HostName vps.example.com\n    User root\n    Port 2222\n    IdentityFile ~/.ssh/k.pem\n"
    ));
    assert!(text.starts_with(NO_VPS), "every existing line is untouched");
    assert_eq!(std::fs::read_to_string(f.backup()).unwrap(), NO_VPS);
    assert!(shown(&h, "vps"));
    assert_eq!(h.state().last_backup(), Some(f.backup().as_path()));
    assert_eq!(h.state().status(), "vps added.");
}

/// web-gui-3: adding a name that exists shows the core's error inline and
/// leaves the file unchanged.
#[test]
fn web_gui_3_duplicate_add_is_refused_inline() {
    let text = three_hosts_sectioned();
    let f = Fixture::new(&text);
    let mut h = harness(&f.path);
    h.run();
    click(&mut h, "Add Host");
    type_into(&mut h, "Name", "vps");
    type_into(&mut h, "Connection URI", "root@elsewhere.example.com");
    click(&mut h, "Save");
    assert!(shown_contains(&h, "vps already exists"));
    assert_eq!(f.read(), text);
    assert!(!f.backup().exists());
    // The form keeps the input.
    assert_eq!(h.state().form().unwrap().uri, "root@elsewhere.example.com");
}

/// web-gui-4: edit replaces HostName/User/Port; an emptied identity file
/// removes IdentityFile; other keys and comments stay.
#[test]
fn web_gui_4_edit_with_emptied_identity() {
    let text = three_hosts_sectioned();
    let f = Fixture::new(&text);
    let mut h = harness(&f.path);
    h.run();
    click(&mut h, "vps");
    assert_eq!(h.state().selected(), Some("vps"));
    let form = h.state().form().unwrap().clone();
    assert_eq!(form.uri, "root@vps.example.com:2222");
    assert_eq!(form.identity, "~/.ssh/vps.pem");
    replace_in(&mut h, "Connection URI", "emre@vps.example.com:2400");
    replace_in(&mut h, "Identity file", "");
    assert_eq!(h.state().form().unwrap().identity, "");
    click(&mut h, "Save");

    let mut expected = Config::parse(&text).unwrap();
    expected
        .edit(
            &EditSpec {
                name: "vps".into(),
                uri: "emre@vps.example.com:2400".into(),
                ..EditSpec::default()
            },
            &env(),
        )
        .unwrap();
    expected
        .unset(&HostSelector::Name("vps".into()), &["IdentityFile".into()])
        .unwrap();
    let out = f.read();
    assert_eq!(out, expected.render());
    assert!(out.contains(
        "# the main box\nHost vps v\n    HostName vps.example.com\n    User emre\n    Port 2400\n"
    ));
    assert!(!out.contains("IdentityFile"));
    assert!(out.contains("# production web\nHost web-prod\n"));
    assert!(out.contains("    ProxyCommand ssh -W %h:%p bastion\n"));
    assert!(f.backup().exists());
}

/// web-gui-5: delete with confirmation removes the block; other comments
/// survive; a backup exists.
#[test]
fn web_gui_5_delete_confirmed() {
    let text = three_hosts_sectioned();
    let f = Fixture::new(&text);
    let mut h = harness(&f.path);
    h.run();
    click(&mut h, "vps");
    click(&mut h, "Delete…");
    assert_eq!(
        h.state().dialog(),
        Some(&Dialog::ConfirmDelete("vps".into()))
    );
    assert!(shown(&h, "Delete host vps?"));
    click(&mut h, "Delete");
    let mut expected = Config::parse(&text).unwrap();
    expected.delete(&["vps".to_string()]).unwrap();
    let out = f.read();
    assert_eq!(out, expected.render());
    assert!(!out.contains("Host vps"));
    assert!(!out.contains("# the main box"));
    assert!(out.contains("# production web\nHost web-prod\n"));
    assert!(out.contains("# laptop ssh config\n"));
    assert_eq!(std::fs::read_to_string(f.backup()).unwrap(), text);
    assert!(!shown(&h, "vps"));
    assert!(h.state().dialog().is_none());
}

/// web-gui-6: cancelling the delete confirmation leaves the file unchanged.
#[test]
fn web_gui_6_delete_cancelled() {
    let text = three_hosts_sectioned();
    let f = Fixture::new(&text);
    let mut h = harness(&f.path);
    h.run();
    click(&mut h, "vps");
    click(&mut h, "Delete…");
    click(&mut h, "Cancel");
    assert!(h.state().dialog().is_none());
    assert_eq!(f.read(), text);
    assert!(!f.backup().exists());
    assert!(shown(&h, "vps"));
}

/// Clone through the dialog: the copy lands in the source's section with
/// HostName rewritten.
#[test]
fn clone_through_dialog() {
    let text = three_hosts_sectioned();
    let f = Fixture::new(&text);
    let mut h = harness(&f.path);
    h.run();
    click(&mut h, "vps");
    click(&mut h, "Clone…");
    type_into(&mut h, "New name", "vps2");
    click(&mut h, "Clone");
    let mut expected = Config::parse(&text).unwrap();
    expected
        .clone_host(&CloneSpec {
            source: "vps".into(),
            new_name: "vps2".into(),
            ..CloneSpec::default()
        })
        .unwrap();
    let out = f.read();
    assert_eq!(out, expected.render());
    assert!(out.contains("Host vps2\n    HostName vps2.example.com\n"));
    assert_eq!(h.state().selected(), Some("vps2"));
    let row = h
        .state()
        .rows()
        .iter()
        .find(|r| r.name == "vps2")
        .unwrap()
        .clone();
    assert_eq!(row.section.as_deref(), Some("personal"));
}

/// A clone onto an existing name shows the error in the dialog.
#[test]
fn clone_onto_existing_name_is_refused() {
    let text = three_hosts_sectioned();
    let f = Fixture::new(&text);
    let mut h = harness(&f.path);
    h.run();
    click(&mut h, "vps");
    click(&mut h, "Clone…");
    type_into(&mut h, "New name", "github");
    click(&mut h, "Clone");
    assert!(shown_contains(&h, "github already exists."));
    assert_eq!(f.read(), text);
}

/// Move to section through the dialog.
#[test]
fn move_to_section_through_dialog() {
    let text = three_hosts_sectioned();
    let f = Fixture::new(&text);
    let mut h = harness(&f.path);
    h.run();
    click(&mut h, "github");
    click(&mut h, "Move to Section…");
    type_into(&mut h, "Section", "work");
    click(&mut h, "Move");
    let mut expected = Config::parse(&text).unwrap();
    expected.move_host("github", None, Some("work")).unwrap();
    assert_eq!(f.read(), expected.render());
    let row = h
        .state()
        .rows()
        .iter()
        .find(|r| r.name == "github")
        .unwrap()
        .clone();
    assert_eq!(row.section.as_deref(), Some("work"));
    assert_eq!(h.state().status(), "github moved to section work.");
}

/// web-gui-7: a hand edit made while the GUI is open survives an
/// unrelated GUI write (reload-then-apply).
#[test]
fn web_gui_7_reload_then_apply_keeps_hand_edit() {
    let text = three_hosts_sectioned();
    let f = Fixture::new(&text);
    let mut h = harness(&f.path);
    h.run();
    let hand = text.replace(
        "    User git\n",
        "    User git\n    # hand edit\n    Compression yes\n",
    );
    assert_ne!(hand, text);
    std::fs::write(&f.path, &hand).unwrap();
    click(&mut h, "vps");
    replace_in(&mut h, "Connection URI", "emre@vps.example.com:2400");
    click(&mut h, "Save");
    let mut expected = Config::parse(&hand).unwrap();
    expected
        .edit(
            &EditSpec {
                name: "vps".into(),
                uri: "emre@vps.example.com:2400".into(),
                identity: Some("~/.ssh/vps.pem".into()),
                ..EditSpec::default()
            },
            &env(),
        )
        .unwrap();
    let out = f.read();
    assert_eq!(out, expected.render());
    assert!(out.contains("    # hand edit\n    Compression yes\n"));
    assert!(out.contains("    User emre\n    Port 2400\n"));
    assert!(h.state().dialog().is_none());
}

/// web-gui-7, conflicting side: the same host changed on disk prompts
/// before overwriting; Cancel keeps the hand edit, Overwrite applies on top.
#[test]
fn web_gui_7_same_host_conflict_prompts() {
    let text = three_hosts_sectioned();
    let f = Fixture::new(&text);
    let mut h = harness(&f.path);
    h.run();
    click(&mut h, "vps");
    let hand = text.replace("    Port 2222\n", "    Port 2223\n");
    std::fs::write(&f.path, &hand).unwrap();
    replace_in(&mut h, "Connection URI", "emre@vps.example.com:2400");
    click(&mut h, "Save");
    assert!(matches!(h.state().dialog(), Some(Dialog::Conflict(_))));
    assert!(shown_contains(&h, "changed on disk"));
    assert_eq!(f.read(), hand, "nothing written before the answer");
    click(&mut h, "Overwrite");
    let out = f.read();
    assert!(out.contains("    User emre\n    Port 2400\n"));
    assert!(!out.contains("2223"));
    assert!(h.state().dialog().is_none());

    // Cancel leaves the reloaded hand edit in place.
    let hand2 = out.replace("    Port 2400\n", "    Port 2401\n");
    std::fs::write(&f.path, &hand2).unwrap();
    click(&mut h, "vps");
    replace_in(&mut h, "Connection URI", "root@vps.example.com:1");
    click(&mut h, "Save");
    assert!(matches!(h.state().dialog(), Some(Dialog::Conflict(_))));
    click(&mut h, "Cancel");
    assert_eq!(f.read(), hand2);
    assert!(h.state().dialog().is_none());
}

/// While the editor holds unsaved text the form cannot write.
#[test]
fn form_writes_blocked_while_editor_dirty() {
    let text = three_hosts_sectioned();
    let f = Fixture::new(&text);
    let mut h = harness(&f.path);
    h.run();
    click(&mut h, "vps");
    h.state_mut().set_editor_text(format!("{text}# pending\n"));
    h.run();
    assert!(shown(&h, "Save or discard the editor first"));
    click(&mut h, "Save");
    assert_eq!(f.read(), text);
}

/// Delete opens the confirmation only while no text field has focus;
/// Cmd/Ctrl+N opens the empty add form.
#[test]
fn delete_key_and_cmd_n() {
    let text = three_hosts_sectioned();
    let f = Fixture::new(&text);
    let mut h = harness(&f.path);
    h.run();
    h.state_mut().select("vps");
    h.run();
    type_into(&mut h, "filter all", "x");
    h.key_press(egui::Key::Delete);
    h.run();
    assert!(
        h.state().dialog().is_none(),
        "Delete while typing does not delete"
    );
    h.state_mut().filters.clear();
    h.state_mut().select("vps");
    h.run();
    // Clicking the row gives focus to no text field.
    click(&mut h, "vps");
    h.key_press(egui::Key::Delete);
    h.run();
    assert_eq!(
        h.state().dialog(),
        Some(&Dialog::ConfirmDelete("vps".into()))
    );
    click(&mut h, "Cancel");
    assert_eq!(f.read(), text);
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::N);
    h.run();
    let form = h.state().form().unwrap();
    assert_eq!(form.mode, rustorm_gui::FormMode::Add);
    assert!(form.name.is_empty());
}

/// addsec-gui: New section… in the sidebar writes the same file the CLI
/// would, on a sectioned and on an unsectioned file.
#[test]
fn addsec_gui_new_section_button_writes_banner_and_catch_all() {
    let text = three_hosts_sectioned();
    let f = Fixture::new(&text);
    let mut h = harness(&f.path);
    h.run();
    click(&mut h, "New section…");
    type_into(&mut h, "Section name", "lab");
    click(&mut h, "Add section");
    let mut expected = Config::parse(&text).unwrap();
    expected.add_section("lab", None).unwrap();
    assert_eq!(f.read(), expected.render());
    assert_eq!(std::fs::read_to_string(f.backup()).unwrap(), text);
    assert_eq!(h.state().status(), "section lab added.");
    let names: Vec<String> = h
        .state()
        .sections()
        .iter()
        .map(|s| s.name.clone())
        .collect();
    assert_eq!(names, ["personal", "work", "lab", "other"]);
    assert!(
        shown_contains(&h, "lab  0"),
        "the sidebar lists the empty section"
    );
    assert!(h.state().dialog().is_none());

    click(&mut h, "New section…");
    type_into(&mut h, "Section name", "Lab");
    click(&mut h, "Add section");
    assert!(shown_contains(&h, "section Lab already exists."));
    assert_eq!(
        f.read(),
        expected.render(),
        "an existing name writes nothing"
    );
    click(&mut h, "Cancel");

    let f = Fixture::new(THREE_HOSTS);
    let mut h = harness(&f.path);
    h.run();
    assert!(h.state().sections().is_empty());
    click(&mut h, "New section…");
    type_into(&mut h, "Section name", "work");
    click(&mut h, "Add section");
    let mut expected = Config::parse(THREE_HOSTS).unwrap();
    expected.add_section("work", None).unwrap();
    assert_eq!(f.read(), expected.render());
    assert_eq!(
        h.state().status(),
        "section work added; other created with 3 hosts."
    );
    let names: Vec<String> = h
        .state()
        .sections()
        .iter()
        .map(|s| s.name.clone())
        .collect();
    assert_eq!(names, ["work", "other"]);
}

/// addsec-gui (negative): Cancel leaves the file unchanged, and the button
/// is disabled while the editor is dirty.
#[test]
fn addsec_gui_cancel_leaves_file_unchanged() {
    let text = three_hosts_sectioned();
    let f = Fixture::new(&text);
    let mut h = harness(&f.path);
    h.run();
    click(&mut h, "New section…");
    type_into(&mut h, "Section name", "lab");
    click(&mut h, "Cancel");
    assert!(h.state().dialog().is_none());
    assert_eq!(f.read(), text);
    assert!(!f.backup().exists());
    click(&mut h, "New section…");
    h.key_press(egui::Key::Escape);
    h.run();
    assert!(h.state().dialog().is_none(), "Esc closes the dialog");
    assert_eq!(f.read(), text);
    click(&mut h, "New section…");
    click(&mut h, "Add section");
    assert!(shown_contains(&h, "a section name is required."));
    assert_eq!(f.read(), text);
    click(&mut h, "Cancel");
    h.state_mut().set_editor_text(format!("{text}# pending\n"));
    h.run();
    click(&mut h, "New section…");
    assert!(
        h.state().dialog().is_none(),
        "disabled while the editor is dirty"
    );
    assert_eq!(f.read(), text);
}
