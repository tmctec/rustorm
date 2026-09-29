//! Keyword completion in the Editor tab (catalog ed-gui-1 .. ed-gui-5):
//! Space accepts the suggested keyword and its premade value, selected;
//! Ctrl+Space swaps a yes/no or choice value.

mod common;

use common::*;
use egui_kittest::Harness;
use rustorm_gui::App;

/// An Editor tab with an empty, focused buffer.
fn editor() -> (Fixture, Harness<'static, App>) {
    let f = Fixture::new("Host lab\n    HostName lab.example.com\n");
    let mut h = harness(&f.path);
    h.run();
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Num2);
    h.run();
    click(&mut h, "config editor");
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    h.run();
    h.key_press(egui::Key::Backspace);
    h.run();
    assert_eq!(h.state().editor_text(), "");
    (f, h)
}

fn text(h: &mut Harness<'static, App>, t: &str) {
    h.event(egui::Event::Text(t.into()));
    h.run();
}

/// ed-gui-1: `    hostk` suggests HostKeyAlias; Space completes the keyword.
#[test]
fn ed_gui_1_space_accepts_the_suggestion() {
    let (_f, mut h) = editor();
    text(&mut h, "Host lab\n    hostk");
    assert_eq!(h.state().editor_suggestion(), Some("eyAlias"));
    text(&mut h, " ");
    assert_eq!(h.state().editor_text(), "Host lab\n    HostKeyAlias ");
    let end = h.state().editor_text().chars().count();
    assert_eq!(h.state().editor_selection(), Some((end, end)));
}

/// ed-gui-2: `    por` Space gives `Port 22` with 22 selected; typing replaces it.
#[test]
fn ed_gui_2_premade_value_is_selected() {
    let (_f, mut h) = editor();
    text(&mut h, "Host lab\n    por");
    text(&mut h, " ");
    let t = h.state().editor_text().to_string();
    assert_eq!(t, "Host lab\n    Port 22");
    let end = t.chars().count();
    assert_eq!(h.state().editor_selection(), Some((end - 2, end)));
    text(&mut h, "2222");
    assert_eq!(h.state().editor_text(), "Host lab\n    Port 2222");
}

/// ed-gui-3: Ctrl+Space swaps a yes/no value under the cursor.
#[test]
fn ed_gui_3_ctrl_space_swaps_yes_no() {
    let (_f, mut h) = editor();
    text(&mut h, "Host lab\n    Compression yes");
    h.key_press_modifiers(egui::Modifiers::CTRL, egui::Key::Space);
    h.run();
    assert_eq!(h.state().editor_text(), "Host lab\n    Compression no");
    h.key_press_modifiers(egui::Modifiers::CTRL, egui::Key::Space);
    h.run();
    assert_eq!(h.state().editor_text(), "Host lab\n    Compression yes");
}

/// ed-gui-4: a word matching no keyword gets a plain space.
#[test]
fn ed_gui_4_no_match_is_a_plain_space() {
    let (_f, mut h) = editor();
    text(&mut h, "Host lab\n    zzz");
    assert_eq!(h.state().editor_suggestion(), None);
    text(&mut h, " ");
    assert_eq!(h.state().editor_text(), "Host lab\n    zzz ");
}

/// ed-gui-5: in a value, Space is a plain space.
#[test]
fn ed_gui_5_value_space_is_plain() {
    let (_f, mut h) = editor();
    text(&mut h, "Host lab\n    User tra");
    assert_eq!(h.state().editor_suggestion(), None);
    text(&mut h, " ");
    assert_eq!(h.state().editor_text(), "Host lab\n    User tra ");
}
