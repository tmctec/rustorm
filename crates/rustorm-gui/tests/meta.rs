//! Host metadata in the GUI: the Notes & location group of All settings,
//! the tag chips, the detail panel summary and the filter (docs/gui.md;
//! plan anemone step 9; catalog mg-1 .. mg-5).

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

fn open_filled(h: &mut Harness<'static, App>, host: &str) {
    h.run();
    click(h, host);
    click(h, "All settings");
}

fn value(h: &Harness<'static, App>, key: &str) -> String {
    h.state()
        .settings()
        .unwrap()
        .rows
        .iter()
        .find(|r| r.spec.key == key)
        .unwrap()
        .value
        .clone()
}

// mg-1: All settings shows Notes & location first, prefilled; the detail panel shows the labels.
#[test]
fn mg_1_notes_group_and_summary() {
    let f = Fixture::new(LAB);
    let mut h = harness(&f.path);
    open_filled(&mut h, "lab");
    assert!(shown(&h, "Notes & location"), "group header");
    assert_eq!(value(&h, "note"), "Primary build box");
    assert_eq!(value(&h, "location"), "Austin DC, rack 4");
    assert_eq!(value(&h, "tags"), "prod, db");
    assert!(shown(&h, "Austin DC, rack 4"), "location in the summary");
    assert!(shown(&h, "prod"), "tag chip");
    assert!(shown_contains(&h, "Primary build box"), "note in the summary");
    let groups: Vec<usize> = ["Notes & location", "Connection"]
        .iter()
        .map(|g| {
            h.get_all_by_label(g)
                .next()
                .map(|n| n.rect().min.y as usize)
                .unwrap()
        })
        .collect();
    assert!(groups[0] < groups[1], "Notes & location above Connection");
}

// mg-2: editing location and the tags field and saving writes the lines, byte-identical to the core.
#[test]
fn mg_2_save_writes_metadata_lines() {
    let f = Fixture::new(LAB);
    let mut h = harness(&f.path);
    open_filled(&mut h, "lab");
    replace_in(&mut h, "location", "Dallas");
    replace_in(&mut h, "tags", "prod, db, edge");
    click(&mut h, "Save settings");
    let expected = core(
        LAB,
        "lab",
        &[
            SettingChange::set("location", "Dallas"),
            SettingChange::set("tags", "prod, db, edge"),
        ],
    );
    assert_eq!(f.read(), expected);
    assert!(f.read().contains("# location: Dallas\n# tags: prod, db, edge\nHost lab\n"));
    assert_eq!(std::fs::read_to_string(f.backup()).unwrap(), LAB);
}

// mg-3: a tag chip's × removes the tag from the field.
#[test]
fn mg_3_tag_chip_removes_a_tag() {
    let f = Fixture::new(LAB);
    let mut h = harness(&f.path);
    open_filled(&mut h, "lab");
    click(&mut h, "prod ×");
    assert_eq!(value(&h, "tags"), "db");
    click(&mut h, "Save settings");
    assert!(f.read().contains("# tags: db\nHost lab\n"), "{}", f.read());
}

// mg-4: a plain host gets the lines above Host through Add setting.
#[test]
fn mg_4_add_setting_adds_metadata_to_a_plain_host() {
    let f = Fixture::new(LAB);
    let mut h = harness(&f.path);
    open_filled(&mut h, "other");
    type_into(&mut h, "Add setting", "loca");
    h.key_press(egui::Key::Tab);
    h.run();
    replace_in(&mut h, "location", "Oslo");
    click(&mut h, "Save settings");
    let expected = core(LAB, "other", &[SettingChange::set("location", "Oslo")]);
    assert_eq!(f.read(), expected);
    assert!(f.read().ends_with("# location: Oslo\nHost other\n    HostName other.example.com\n"));
}

// mg-5: the filter above the table matches metadata text.
#[test]
fn mg_5_filter_matches_metadata() {
    let f = Fixture::new(LAB);
    let mut h = harness(&f.path);
    h.run();
    assert_eq!(h.state().visible_names(), ["lab", "other"]);
    type_into(&mut h, "filter all", "rack 4");
    assert_eq!(h.state().visible_names(), ["lab"]);
    replace_in(&mut h, "filter all", "db");
    assert_eq!(h.state().visible_names(), ["lab"]);
    replace_in(&mut h, "filter all", "");
    assert_eq!(h.state().visible_names().len(), 2);
}
