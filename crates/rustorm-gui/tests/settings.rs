//! The detail panel's All settings section: every keyword a host can set,
//! grouped, one write per Save (catalog tf-gui-1 .. tf-gui-8).

mod common;

use common::*;
use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;
use rustorm_core::{Config, SettingChange};
use rustorm_gui::App;

const LAB: &str = "\
Host *
    User fallback

# the lab box
Host lab
    # keep this comment
    HostName lab.example.com
    Compression yes
    LocalForward 8080 localhost:80

Host other
    HostName other.example.com
";

fn core(text: &str, host: &str, changes: &[SettingChange]) -> String {
    let mut c = Config::parse(text).unwrap();
    c.apply_settings(host, changes).unwrap();
    c.render()
}

/// Selects `host` and opens All settings showing every keyword.
fn open(h: &mut Harness<'static, App>, host: &str) {
    open_filled(h, host);
    click(h, "All");
}

/// Selects `host` and opens All settings in its default filled view.
fn open_filled(h: &mut Harness<'static, App>, host: &str) {
    h.run();
    click(h, host);
    click(h, "All settings");
}

fn value(h: &Harness<'static, App>, key: &str, nth: usize) -> String {
    h.state()
        .settings()
        .unwrap()
        .rows
        .iter()
        .filter(|r| r.spec.key == key)
        .nth(nth)
        .unwrap()
        .value
        .clone()
}

/// Picks `word` in the drop-down labelled `key`.
fn pick(h: &mut Harness<'static, App>, key: &str, word: &str) {
    click(h, key);
    click(h, word);
}

/// tf-gui-1: All settings shows grouped controls prefilled from the host,
/// inherited values marked as coming from Host *.
#[test]
fn tf_gui_1_all_settings_is_grouped_and_prefilled() {
    let f = Fixture::new(LAB);
    let mut h = harness(&f.path);
    open(&mut h, "lab");
    for g in [
        "Connection",
        "Authentication",
        "Forwarding",
        "Proxy",
        "Multiplexing",
        "Advanced",
    ] {
        assert!(h.query_all_by_label(g).next().is_some(), "{g}");
    }
    click(&mut h, "Connection");
    assert_eq!(value(&h, "HostName", 0), "lab.example.com");
    assert_eq!(value(&h, "Compression", 0), "yes");
    assert!(h.query_all_by_label("Compression").next().is_some());
    let user = h
        .state()
        .settings()
        .unwrap()
        .rows
        .iter()
        .find(|r| r.spec.key == "User")
        .unwrap()
        .clone();
    assert_eq!(user.value, "");
    assert_eq!(user.inherited.as_deref(), Some("fallback"));
    click(&mut h, "Connection");
    click(&mut h, "Forwarding");
    assert!(h.query_all_by_label("LocalForward").next().is_some());
    assert_eq!(value(&h, "LocalForward", 0), "8080 localhost:80");
    assert_eq!(value(&h, "LocalForward", 1), "");
}

/// tf-gui-2: a drop-down and a text field saved together make one write
/// and one backup.
#[test]
fn tf_gui_2_two_changes_one_write() {
    let f = Fixture::new(LAB);
    let mut h = harness(&f.path);
    open(&mut h, "lab");
    click(&mut h, "Connection");
    pick(&mut h, "Compression", "no");
    assert_eq!(value(&h, "Compression", 0), "no");
    click(&mut h, "Connection");
    click(&mut h, "Forwarding");
    type_into(&mut h, "ForwardAgent", "no");
    click(&mut h, "Save settings");
    assert_eq!(
        f.read(),
        core(
            LAB,
            "lab",
            &[
                SettingChange::set("Compression", "no"),
                SettingChange::set("ForwardAgent", "no"),
            ]
        )
    );
    assert!(f.read().contains("    # keep this comment\n"));
    assert_eq!(std::fs::read_to_string(f.backup()).unwrap(), LAB);
    assert_eq!(h.state().status(), "lab updated.");
}

/// tf-gui-3: a bad value shows its problem inline and Save writes nothing.
#[test]
fn tf_gui_3_bad_port_is_refused() {
    let f = Fixture::new(LAB);
    let mut h = harness(&f.path);
    open(&mut h, "lab");
    click(&mut h, "Connection");
    type_into(&mut h, "Port", "abc");
    assert!(shown(&h, "Port must be a port from 1 to 65535"));
    click(&mut h, "Save settings");
    assert!(!h.state_mut().save_settings());
    assert_eq!(
        h.state().settings_error(),
        Some("Port must be a port from 1 to 65535.")
    );
    assert_eq!(f.read(), LAB);
    assert!(!f.backup().exists());
}

/// tf-gui-4: LocalForward rows are added and removed.
#[test]
fn tf_gui_4_multi_valued_rows_add_and_remove() {
    let f = Fixture::new(LAB);
    let mut h = harness(&f.path);
    open(&mut h, "lab");
    click(&mut h, "Forwarding");
    type_into(&mut h, "LocalForward", "8443 localhost:443");
    assert_eq!(value(&h, "LocalForward", 1), "8443 localhost:443");
    assert_eq!(value(&h, "LocalForward", 2), "");
    // Remove the first one.
    h.get_all_by_label("−").next().unwrap().click();
    h.run();
    assert_eq!(value(&h, "LocalForward", 0), "");
    click(&mut h, "Save settings");
    let text = f.read();
    assert!(
        text.contains("    LocalForward 8443 localhost:443\n"),
        "{text}"
    );
    assert!(!text.contains("8080"), "{text}");
}

/// tf-gui-5: edits are dropped when another host is selected.
#[test]
fn tf_gui_5_selecting_another_host_discards() {
    let f = Fixture::new(LAB);
    let mut h = harness(&f.path);
    open(&mut h, "lab");
    click(&mut h, "Connection");
    pick(&mut h, "Compression", "no");
    click(&mut h, "other");
    click(&mut h, "lab");
    assert_eq!(value(&h, "Compression", 0), "yes");
    assert_eq!(f.read(), LAB);
    assert!(!f.backup().exists());
}

/// tf-gui-6: with unsaved editor text the section writes nothing.
#[test]
fn tf_gui_6_editor_guard() {
    let f = Fixture::new(LAB);
    let mut h = harness(&f.path);
    h.run();
    h.state_mut().set_editor_text(format!("{LAB}# edit\n"));
    open(&mut h, "lab");
    assert!(shown(&h, "Save or discard the editor first"));
    assert!(!h.state_mut().save_settings());
    assert_eq!(f.read(), LAB);
}

/// tf-gui-7: a host in an included file is written there.
#[test]
fn tf_gui_7_included_host_writes_its_file() {
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path().join("config.d");
    std::fs::create_dir(&d).unwrap();
    let root = dir.path().join("config");
    let root_text = format!("Include {}/*\n\nHost github\n    User git\n", d.display());
    std::fs::write(&root, &root_text).unwrap();
    let ranch = d.join("ranch");
    let ranch_text = "Host ranch-nas\n    HostName nas.ranch.lan\n";
    std::fs::write(&ranch, ranch_text).unwrap();
    let mut h = harness(&root);
    open(&mut h, "ranch-nas");
    click(&mut h, "Connection");
    pick(&mut h, "Compression", "yes");
    click(&mut h, "Save settings");
    assert_eq!(
        std::fs::read_to_string(&ranch).unwrap(),
        core(
            ranch_text,
            "ranch-nas",
            &[SettingChange::set("Compression", "yes")]
        )
    );
    assert_eq!(std::fs::read_to_string(&root).unwrap(), root_text);
}

/// tf-gui-8: Save with nothing changed writes nothing.
#[test]
fn tf_gui_8_unchanged_save_writes_nothing() {
    let f = Fixture::new(LAB);
    let mut h = harness(&f.path);
    open(&mut h, "lab");
    click(&mut h, "Save settings");
    assert!(!h.state_mut().save_settings());
    assert_eq!(h.state().status(), "no changes.");
    assert_eq!(f.read(), LAB);
    assert!(!f.backup().exists());
}

/// Drives the follow and All settings flows on a three-file workspace
/// through the real widgets and writes what they show to the file named by
/// `RUSTORM_GUI_EVIDENCE`. Run with `--ignored`.
#[test]
#[ignore]
fn gui_walkthrough_evidence() {
    use std::fmt::Write as _;
    let out_path = std::env::var("RUSTORM_GUI_EVIDENCE").expect("RUSTORM_GUI_EVIDENCE");
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path().join("config.d");
    std::fs::create_dir(&d).unwrap();
    let root = dir.path().join("config");
    std::fs::write(
        &root,
        format!(
            "# root\nInclude {}/*\n\nHost github\n    User git\n",
            d.display()
        ),
    )
    .unwrap();
    let ranch = d.join("ranch");
    let before = "# ranch machines\nHost dcevant\n    # the old box\n    HostName dcevant.ranch.lan\n\nHost ranch-nas\n    HostName nas.ranch.lan\n    Compression yes\n";
    std::fs::write(&ranch, before).unwrap();
    std::fs::write(
        d.join("cypress"),
        "Host cypressPro\n    HostName 10.0.0.2\n",
    )
    .unwrap();
    let mut out = String::new();
    let mut h = harness(&root);
    h.run();
    for host in ["cypressPro", "ranch-nas"] {
        click(&mut h, host);
        let s = h.state();
        let ws = s.workspace();
        writeln!(
            out,
            "click {host}: tab {:?}, editor file {}, cursor queued for line {:?}",
            s.tab,
            ws.file_name(s.current_file()),
            s.pending_editor_line()
        )
        .unwrap();
    }
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Num2);
    h.run();
    let s = h.state();
    let line = s.editor_line().unwrap();
    writeln!(
        out,
        "Cmd+2: tab {:?}, editor line {line}: {:?}",
        s.tab,
        s.buffer(s.current_file()).lines().nth(line - 1).unwrap()
    )
    .unwrap();
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Num1);
    h.run();
    click(&mut h, "All settings");
    click(&mut h, "All");
    click(&mut h, "Forwarding");
    let labels: Vec<String> = ["ForwardAgent", "LocalForward", "GatewayPorts", "Tunnel"]
        .iter()
        .filter(|k| h.query_all_by_label(k).next().is_some())
        .map(|k| k.to_string())
        .collect();
    writeln!(
        out,
        "All settings > Forwarding shows: {}",
        labels.join(", ")
    )
    .unwrap();
    type_into(&mut h, "ForwardAgent", "yes");
    type_into(&mut h, "LocalForward", "8080 localhost:80");
    click(&mut h, "Forwarding");
    click(&mut h, "Multiplexing");
    pick(&mut h, "ControlMaster", "auto");
    click(&mut h, "Save settings");
    writeln!(out, "Save settings: status {:?}", h.state().status()).unwrap();
    let after = std::fs::read_to_string(&ranch).unwrap();
    writeln!(out, "--- config.d/ranch before\n{before}--- after\n{after}").unwrap();
    let backups: Vec<String> = std::fs::read_dir(&d)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().to_string())
        .filter(|n| n.ends_with('~'))
        .collect();
    writeln!(out, "backups: {backups:?}").unwrap();

    // Filled view and Add setting on a cypressMelissa-shaped host.
    let melissa = "Host cypressMelissa\n    HostName 192.168.144.130\n    User home\n    Port 22\n    IdentityFile ~/.ssh/to_cypressMelissa\n    IdentitiesOnly yes\n";
    std::fs::write(d.join("cypress"), melissa).unwrap();
    let mut h = harness(&root);
    h.run();
    click(&mut h, "cypressMelissa");
    click(&mut h, "All settings");
    let keys = |h: &Harness<'static, App>| {
        let s = h.state();
        let rows = &s.settings().unwrap().rows;
        s.settings_shown_rows()
            .into_iter()
            .map(|i| rows[i].spec.key)
            .collect::<Vec<_>>()
            .join(", ")
    };
    writeln!(out, "\n--- filled view, cypressMelissa: {}", keys(&h)).unwrap();
    click(&mut h, "All");
    writeln!(
        out,
        "All: {} rows shown",
        h.state().settings_shown_rows().len()
    )
    .unwrap();
    click(&mut h, "Filled");
    writeln!(out, "Filled again: {}", keys(&h)).unwrap();
    type_into(&mut h, "Add setting", "hostk");
    writeln!(
        out,
        "typed hostk, suggestion shown: {}",
        h.query_all_by_label("→ HostKeyAlias").next().is_some()
    )
    .unwrap();
    h.key_press(egui::Key::Tab);
    h.run();
    h.get_all_by_label("HostKeyAlias")
        .last()
        .unwrap()
        .type_text("alias1");
    h.run();
    click(&mut h, "Save settings");
    writeln!(
        out,
        "Add setting + Save: status {:?}\n--- config.d/cypress after\n{}",
        h.state().status(),
        std::fs::read_to_string(d.join("cypress")).unwrap()
    )
    .unwrap();

    // Editor completion on the same file.
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Num2);
    h.run();
    click(&mut h, "config editor");
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    h.run();
    h.key_press(egui::Key::Backspace);
    h.run();
    let text = |h: &mut Harness<'static, App>, t: &str| {
        h.event(egui::Event::Text(t.into()));
        h.run();
    };
    text(&mut h, "Host cypressMelissa\n    por");
    writeln!(
        out,
        "\n--- editor: typed `    por`, ghost {:?}",
        h.state().editor_suggestion()
    )
    .unwrap();
    text(&mut h, " ");
    writeln!(
        out,
        "Space -> {:?}, selection {:?}",
        h.state().editor_text().lines().last().unwrap(),
        h.state().editor_selection()
    )
    .unwrap();
    text(&mut h, "2222\n    compr");
    text(&mut h, " ");
    writeln!(
        out,
        "`    compr` Space -> {:?}",
        h.state().editor_text().lines().last().unwrap()
    )
    .unwrap();
    h.key_press_modifiers(egui::Modifiers::CTRL, egui::Key::Space);
    h.run();
    writeln!(
        out,
        "Ctrl+Space -> {:?}",
        h.state().editor_text().lines().last().unwrap()
    )
    .unwrap();
    writeln!(out, "--- editor buffer\n{}", h.state().editor_text()).unwrap();

    // Host metadata: a tagged host's Notes & location group, chips and the
    // summary in the detail panel; a plain host gains a location through
    // Add setting.
    std::fs::write(
        d.join("cypress"),
        "# note: Primary build box\n# location: Austin DC, rack 4\n# tags: prod, db\nHost cypressMelissa\n    HostName 192.168.144.130\n\nHost plainbox\n    HostName 10.0.0.9\n",
    )
    .unwrap();
    let mut h = harness(&root);
    h.run();
    click(&mut h, "cypressMelissa");
    click(&mut h, "All settings");
    writeln!(out, "\n--- metadata, cypressMelissa: filled view {}", keys(&h)).unwrap();
    writeln!(
        out,
        "Notes & location group shown: {}; summary shows location {}, chip prod {}, note {}",
        h.query_all_by_label("Notes & location").next().is_some(),
        h.query_all_by_label("Austin DC, rack 4").next().is_some(),
        h.query_all_by_label("prod").next().is_some(),
        h.query_all_by_label_contains("Primary build box").next().is_some()
    )
    .unwrap();
    click(&mut h, "prod ×");
    writeln!(
        out,
        "clicked `prod ×`: tags field now {:?}",
        h.state()
            .settings()
            .unwrap()
            .rows
            .iter()
            .find(|r| r.spec.key == "tags")
            .map(|r| r.value.clone())
    )
    .unwrap();
    click(&mut h, "Save settings");
    writeln!(out, "Save settings: status {:?}", h.state().status()).unwrap();
    click(&mut h, "plainbox");
    // All settings keeps its open state across hosts.
    type_into(&mut h, "Add setting", "loca");
    h.key_press(egui::Key::Tab);
    h.run();
    h.get_all_by_label("location")
        .last()
        .unwrap()
        .type_text("Dallas");
    h.run();
    click(&mut h, "Save settings");
    writeln!(
        out,
        "plainbox: Add setting loca, Tab, Dallas, Save: status {:?}\n--- config.d/cypress after\n{}",
        h.state().status(),
        std::fs::read_to_string(d.join("cypress")).unwrap()
    )
    .unwrap();

    std::fs::write(out_path, out).unwrap();
    assert!(after.contains("ControlMaster auto"));
}

// ----- filled view and Add setting (R-settings-filled-view) -----

fn shown_keys(h: &Harness<'static, App>) -> Vec<&'static str> {
    let s = h.state();
    let rows = &s.settings().unwrap().rows;
    s.settings_shown_rows()
        .into_iter()
        .map(|i| rows[i].spec.key)
        .collect()
}

/// fv-gui-1: All settings opens on the host's set keys only, with the
/// Filled / All toggle and an Add setting field.
#[test]
fn fv_gui_1_opens_filled() {
    let f = Fixture::new(LAB);
    let mut h = harness(&f.path);
    open_filled(&mut h, "lab");
    assert!(!h.state().settings_show_all());
    assert_eq!(shown_keys(&h), ["Compression", "HostName", "LocalForward"]);
    assert!(h.query_all_by_label("Add setting").next().is_some());
    assert!(h.query_all_by_label("Filled").next().is_some());
    assert!(h.query_all_by_label("Proxy").next().is_none());
    assert!(h.query_all_by_label("Connection").next().is_some());
}

/// fv-gui-2: All shows every keyword; Filled returns to the set keys.
#[test]
fn fv_gui_2_toggle_all_and_back() {
    let f = Fixture::new(LAB);
    let mut h = harness(&f.path);
    open_filled(&mut h, "lab");
    click(&mut h, "All");
    assert!(h.state().settings_show_all());
    assert_eq!(
        h.state().settings_shown_rows().len(),
        h.state().settings().unwrap().rows.len()
    );
    assert!(h.query_all_by_label("Proxy").next().is_some());
    click(&mut h, "Filled");
    assert_eq!(shown_keys(&h), ["Compression", "HostName", "LocalForward"]);
}

/// fv-gui-3: typing hostk suggests HostKeyAlias; Tab adds it with its field
/// focused; Save writes the value.
#[test]
fn fv_gui_3_add_setting_by_typing() {
    let f = Fixture::new(LAB);
    let mut h = harness(&f.path);
    open_filled(&mut h, "lab");
    type_into(&mut h, "Add setting", "hostk");
    assert!(h.query_all_by_label("→ HostKeyAlias").next().is_some());
    h.key_press(egui::Key::Tab);
    h.run();
    assert!(shown_keys(&h).contains(&"HostKeyAlias"));
    h.get_all_by_label("HostKeyAlias")
        .last()
        .unwrap()
        .type_text("alias1");
    h.run();
    assert_eq!(value(&h, "HostKeyAlias", 0), "alias1");
    click(&mut h, "Save settings");
    assert_eq!(
        f.read(),
        core(LAB, "lab", &[SettingChange::set("HostKeyAlias", "alias1")])
    );
}

/// fv-gui-4: an added choice keyword gets its drop-down, prefilled.
#[test]
fn fv_gui_4_added_choice_has_drop_down() {
    let f = Fixture::new(LAB);
    let mut h = harness(&f.path);
    open_filled(&mut h, "lab");
    type_into(&mut h, "Add setting", "stricth");
    h.key_press(egui::Key::Enter);
    h.run();
    assert_eq!(value(&h, "StrictHostKeyChecking", 0), "yes");
    pick(&mut h, "StrictHostKeyChecking", "accept-new");
    assert_eq!(value(&h, "StrictHostKeyChecking", 0), "accept-new");
}

/// fv-gui-5: text matching no keyword adds nothing and says so.
#[test]
fn fv_gui_5_no_match() {
    let f = Fixture::new(LAB);
    let mut h = harness(&f.path);
    open_filled(&mut h, "lab");
    let before = shown_keys(&h);
    type_into(&mut h, "Add setting", "zzz");
    h.key_press(egui::Key::Tab);
    h.run();
    assert_eq!(h.state().settings_add_error(), Some("no matching keyword"));
    assert_eq!(shown_keys(&h), before);
    assert!(h.query_all_by_label("no matching keyword").next().is_some());
}

/// fv-14 (GUI): an added Port arrives as 22, selected, so typing replaces
/// it.
#[test]
fn fv_14_gui_premade_value_is_selected() {
    let f = Fixture::new(LAB);
    let mut h = harness(&f.path);
    open_filled(&mut h, "lab");
    type_into(&mut h, "Add setting", "por");
    h.key_press(egui::Key::Tab);
    h.run();
    assert_eq!(value(&h, "Port", 0), "22");
    h.event(egui::Event::Text("2222".into()));
    h.run();
    assert_eq!(value(&h, "Port", 0), "2222");
}
