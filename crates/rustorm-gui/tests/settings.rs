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

/// Selects `host` and opens All settings.
fn open(h: &mut Harness<'static, App>, host: &str) {
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
