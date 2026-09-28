//! Config editor: highlighting, save, unsaved-changes prompt, invalid
//! edits (ed-1, ed-3, ed-4, ed-5).

mod common;

use std::collections::BTreeMap;

use common::*;
use egui::{Color32, FontId};
use egui_kittest::kittest::Queryable;
use rustorm_core::{Config, SpanKind};
use rustorm_gui::highlight::{color, highlight_job, kind_of};
use rustorm_gui::{Dialog, Tab, DISCARD_LABEL};

/// Maps each distinct (trimmed) text piece of the job to its color.
fn colors_by_text(text: &str, dark: bool) -> BTreeMap<String, Color32> {
    let job = highlight_job(text, dark, FontId::monospace(13.0));
    assert_eq!(job.text, text, "the job carries the text unchanged");
    let mut covered = 0;
    let mut out = BTreeMap::new();
    for s in &job.sections {
        let (a, b) = (s.byte_range.start.0, s.byte_range.end.0);
        assert_eq!(a, covered, "sections are contiguous");
        covered = b;
        // Adjacent sections with one format merge, so a value carries its
        // surrounding whitespace; key the map on the trimmed text.
        out.insert(job.text[a..b].trim().to_string(), s.format.color);
    }
    assert_eq!(covered, text.len(), "every byte is in a section");
    out
}

/// ed-1: comment, banner, Host keyword, host name, alias, key, value,
/// ProxyCommand and ProxyJump each get a distinct color, in both palettes.
#[test]
fn ed_1_highlighting_distinct_per_kind() {
    let mut c = Config::parse(
        "# a comment\nHost a b\n    HostName a.example\n    ProxyCommand ssh -W %h:%p bastion\n    ProxyJump jumpbox\n",
    )
    .unwrap();
    c.move_host("a", None, Some("data foundry")).unwrap();
    let text = format!("# top comment\n{}", c.render());
    assert!(text.contains("section: data foundry"));
    for dark in [true, false] {
        let m = colors_by_text(&text, dark);
        let banner_line = text
            .lines()
            .find(|l| l.contains("section: data foundry"))
            .unwrap()
            .trim()
            .to_string();
        let pick = |piece: &str| {
            *m.get(piece)
                .unwrap_or_else(|| panic!("no section for {piece:?} in {m:?}"))
        };
        let got = [
            (SpanKind::Comment, pick("# top comment")),
            (SpanKind::Banner, pick(&banner_line)),
            (SpanKind::HostKeyword, pick("Host")),
            (SpanKind::HostName, pick("a")),
            (SpanKind::Alias, pick("b")),
            (SpanKind::Key, pick("HostName")),
            (SpanKind::Value, pick("a.example")),
            (SpanKind::ProxyCommand, pick("ssh -W %h:%p bastion")),
            (SpanKind::ProxyJump, pick("jumpbox")),
        ];
        for (kind, col) in got {
            assert_eq!(col, color(kind, dark), "{kind:?} color (dark={dark})");
            assert_eq!(kind_of(col, dark), Some(kind));
        }
        let mut distinct: Vec<Color32> = got.iter().map(|(_, c)| *c).collect();
        distinct.sort_by_key(|c| c.to_array());
        distinct.dedup();
        assert_eq!(distinct.len(), got.len(), "every kind has its own color");
        assert_eq!(pick("ProxyCommand"), color(SpanKind::Key, dark));
        assert_ne!(pick("ssh -W %h:%p bastion"), pick("jumpbox"));
    }
}

/// ed-1, rendered: the Editor tab shows the file in the highlighted editor.
#[test]
fn ed_1_editor_tab_renders_the_file() {
    let text = three_hosts_sectioned();
    let f = Fixture::new(&text);
    let mut h = harness(&f.path);
    h.run();
    click(&mut h, "Editor");
    assert_eq!(h.state().tab, Tab::Editor);
    let node = h.get_by_label("config editor");
    assert_eq!(node.value().as_deref(), Some(text.as_str()));
}

/// Cmd/Ctrl+E switches to the Editor tab.
#[test]
fn cmd_e_opens_editor() {
    let f = Fixture::new(&three_hosts_sectioned());
    let mut h = harness(&f.path);
    h.run();
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::E);
    h.run();
    assert_eq!(h.state().tab, Tab::Editor);
}

/// ed-3: an edited value saves through the core: backup written, every
/// untouched line identical, hosts re-sorted inside their section.
#[test]
fn ed_3_editor_save_writes_through_core() {
    let text = three_hosts_sectioned();
    let f = Fixture::new(&text);
    let mut h = harness(&f.path);
    h.run();
    click(&mut h, "Editor");
    // Change a value and add a host out of order to the personal section.
    let edited = text
        .replace(
            "    HostName github.com\n",
            "    HostName github.example.org\n",
        )
        .replace(
            "# the main box\n",
            "Host zed\n    HostName zed.example\n\n# the main box\n",
        );
    assert_ne!(edited, text);
    h.state_mut().set_editor_text(edited.clone());
    h.run();
    assert!(h.state().editor_dirty());
    assert!(h.state().title().ends_with(" •"));
    click(&mut h, "Save");
    let mut expected = Config::parse(&edited).unwrap();
    expected.sort_sections();
    let out = f.read();
    assert_eq!(out, expected.render());
    assert!(out.contains("    HostName github.example.org\n"));
    let vps = out.find("Host vps").unwrap();
    let zed = out.find("Host zed").unwrap();
    assert!(vps < zed, "zed re-sorted after vps in its section");
    for line in text.lines().filter(|l| !l.contains("github.com")) {
        assert!(
            out.lines().any(|o| o == line),
            "untouched line kept: {line:?}"
        );
    }
    assert_eq!(std::fs::read_to_string(f.backup()).unwrap(), text);
    assert!(!h.state().editor_dirty());
    assert!(h.state().rows().iter().any(|r| r.name == "zed"));
}

/// ed-4: closing with unsaved edits asks; Cancel keeps the window and the
/// edit; Don't Save / Discard closes; the file is unchanged throughout.
#[test]
fn ed_4_unsaved_changes_prompt_on_close() {
    let text = three_hosts_sectioned();
    let f = Fixture::new(&text);
    let mut h = harness(&f.path);
    h.run();
    click(&mut h, "Editor");
    h.state_mut().set_editor_text(format!("{text}# unsaved\n"));
    h.state_mut().request_close();
    h.run();
    assert_eq!(h.state().dialog(), Some(&Dialog::UnsavedClose));
    assert!(shown(&h, "Save changes to the config file?"));
    assert!(shown(&h, DISCARD_LABEL));
    assert!(!h.state().is_closing());
    click(&mut h, "Cancel");
    assert!(!h.state().is_closing());
    assert!(h.state().editor_dirty());
    assert_eq!(f.read(), text);

    h.state_mut().request_close();
    h.run();
    click(&mut h, DISCARD_LABEL);
    assert!(h.state().is_closing());
    assert_eq!(f.read(), text);
    assert!(!f.backup().exists());
}

/// ed-5: a Host line without a name refuses the save with its line number.
#[test]
fn ed_5_invalid_edit_refused_with_line_number() {
    let text = three_hosts_sectioned();
    let f = Fixture::new(&text);
    let mut h = harness(&f.path);
    h.run();
    click(&mut h, "Editor");
    let bad = text.replacen("Host github\n", "Host\n", 1);
    let line_no = bad.lines().position(|l| l == "Host").unwrap() + 1;
    h.state_mut().set_editor_text(bad);
    h.run();
    click(&mut h, "Save");
    let err = h.state().editor_error().unwrap().to_string();
    assert_eq!(err, format!("line {line_no}: cannot parse: Host"));
    assert!(shown_contains(&h, &format!("line {line_no}: cannot parse")));
    assert_eq!(f.read(), text);
    assert!(!f.backup().exists());
}

/// Discard Changes restores the buffer from disk.
#[test]
fn discard_restores_buffer() {
    let text = three_hosts_sectioned();
    let f = Fixture::new(&text);
    let mut h = harness(&f.path);
    h.run();
    click(&mut h, "Editor");
    h.state_mut().set_editor_text("garbage\n");
    h.run();
    click(&mut h, "Discard Changes");
    assert_eq!(h.state().editor_text(), text);
    assert!(!h.state().editor_dirty());
}

/// An editor save over a file changed on disk asks first.
#[test]
fn editor_save_over_external_change_prompts() {
    let text = three_hosts_sectioned();
    let f = Fixture::new(&text);
    let mut h = harness(&f.path);
    h.run();
    click(&mut h, "Editor");
    h.state_mut().set_editor_text(format!("{text}# mine\n"));
    h.run();
    let hand = format!("{text}# theirs\n");
    std::fs::write(&f.path, &hand).unwrap();
    click(&mut h, "Save");
    assert_eq!(h.state().dialog(), Some(&Dialog::EditorConflict));
    assert_eq!(f.read(), hand);
    click(&mut h, "Overwrite");
    assert!(f.read().ends_with("# mine\n"));
}
