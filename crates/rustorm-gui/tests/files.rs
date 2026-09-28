//! Included files: the Files list, the editor's file selector, the file
//! column, per-file Save and the multi-file Quit alert (inc-gui-1..4).

mod common;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use common::*;
use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;
use rustorm_core::Config;
use rustorm_gui::{App, Dialog, Tab, DISCARD_ALL_LABEL};

const CYPRESS: &str = "\
Host cypressPro
    HostName 10.10.0.2
    User travis

Host cypressPro-ext
    HostName cypress.example.com
";

const RANCH: &str = "\
# ranch machines
Host dcevant
    HostName dcevant.ranch.lan

Host ranch-nas
    HostName nas.ranch.lan
    User admin
";

/// A root that includes `config.d/*` by absolute path, plus two included
/// files: the three-file workspace every test here uses.
struct Multi {
    dir: tempfile::TempDir,
    root: PathBuf,
    root_text: String,
}

impl Multi {
    fn new() -> Multi {
        Multi::with(CYPRESS, RANCH)
    }

    fn with(cypress: &str, ranch: &str) -> Multi {
        let dir = tempfile::tempdir().unwrap();
        let d = dir.path().join("config.d");
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("cypress"), cypress).unwrap();
        std::fs::write(d.join("ranch"), ranch).unwrap();
        let root_text = format!(
            "Include {}/*\n\nHost github\n    HostName github.com\n    User git\n",
            d.display()
        );
        let root = dir.path().join("config");
        std::fs::write(&root, &root_text).unwrap();
        Multi {
            dir,
            root,
            root_text,
        }
    }

    fn file(&self, name: &str) -> PathBuf {
        self.dir.path().join("config.d").join(name)
    }

    /// Every file under the temp dir and its bytes.
    fn snapshot(&self) -> BTreeMap<PathBuf, Vec<u8>> {
        fn walk(dir: &Path, out: &mut BTreeMap<PathBuf, Vec<u8>>) {
            for e in std::fs::read_dir(dir).unwrap() {
                let p = e.unwrap().path();
                if p.is_dir() {
                    walk(&p, out);
                } else {
                    out.insert(p.clone(), std::fs::read(&p).unwrap());
                }
            }
        }
        let mut out = BTreeMap::new();
        walk(self.dir.path(), &mut out);
        out
    }
}

/// The paths whose bytes differ between two snapshots, new ones included.
fn changed(
    before: &BTreeMap<PathBuf, Vec<u8>>,
    after: &BTreeMap<PathBuf, Vec<u8>>,
) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = after
        .iter()
        .filter(|(p, b)| before.get(*p) != Some(*b))
        .map(|(p, _)| p.clone())
        .collect();
    out.extend(before.keys().filter(|p| !after.contains_key(*p)).cloned());
    out.sort();
    out
}

fn index_of(h: &Harness<'static, App>, name: &str) -> usize {
    let ws = h.state().workspace();
    (0..ws.files.len())
        .find(|&i| ws.file_name(i) == name)
        .unwrap_or_else(|| panic!("no file {name}"))
}

fn display(h: &Harness<'static, App>, i: usize) -> String {
    h.state().workspace().display(i)
}

fn editor_value(h: &Harness<'static, App>) -> String {
    h.get_by_label("config editor").value().unwrap_or_default()
}

/// The workspace loads in load order and shows the Files list, the file
/// column and every host of every file.
#[test]
fn files_list_and_column_show_on_a_workspace() {
    let m = Multi::new();
    let mut h = harness(&m.root);
    h.run();
    let s = h.state();
    assert!(s.is_multi());
    let names: Vec<String> = (0..s.workspace().files.len())
        .map(|i| s.workspace().file_name(i))
        .collect();
    assert_eq!(names, ["config", "cypress", "ranch"]);
    let mut hosts = s.visible_names();
    hosts.sort();
    assert_eq!(
        hosts,
        [
            "cypressPro",
            "cypressPro-ext",
            "dcevant",
            "github",
            "ranch-nas"
        ]
    );
    assert!(shown(&h, "Files"));
    assert!(shown(&h, "config  1"));
    assert!(shown(&h, "cypress  2"));
    assert!(shown(&h, "ranch  2"));
    assert!(shown(&h, "file"), "the file column header shows");
}

/// Selecting a file in the sidebar shows only its hosts and selects it in
/// the editor; a second click clears the filter.
#[test]
fn sidebar_file_row_filters_and_selects_the_editor_file() {
    let m = Multi::new();
    let mut h = harness(&m.root);
    h.run();
    click(&mut h, "ranch  2");
    let ranch = index_of(&h, "ranch");
    assert_eq!(h.state().visible_names(), ["dcevant", "ranch-nas"]);
    assert_eq!(h.state().current_file(), ranch);
    click(&mut h, "ranch  2");
    assert_eq!(h.state().visible_names().len(), 5);
    assert_eq!(h.state().current_file(), ranch);
}

/// The file column filters like the other columns.
#[test]
fn file_column_filters() {
    let m = Multi::new();
    let mut h = harness(&m.root);
    h.run();
    type_into(&mut h, "filter file", "cypress");
    assert_eq!(h.state().visible_names(), ["cypressPro", "cypressPro-ext"]);
}

/// inc-gui-1: choosing a file in the editor's file selector shows that
/// file; Save writes only it, after its own backup; the status bar and
/// title name it.
#[test]
fn inc_gui_1_file_selector_and_save_writes_only_that_file() {
    let m = Multi::new();
    let mut h = harness(&m.root);
    h.run();
    click(&mut h, "Editor");
    assert_eq!(editor_value(&h), m.root_text, "the root shows first");
    let ranch = index_of(&h, "ranch");
    let ranch_label = display(&h, ranch);
    click(&mut h, "editor file");
    click(&mut h, &ranch_label);
    assert_eq!(h.state().current_file(), ranch);
    assert_eq!(editor_value(&h), RANCH);
    assert!(shown(&h, &ranch_label), "the status bar names the file");
    assert!(h.state().title().ends_with(&format!(" — {ranch_label}")));

    let edited = RANCH.replace("nas.ranch.lan", "nas2.ranch.lan");
    h.state_mut().set_editor_text(edited.clone());
    h.run();
    assert!(h.state().file_dirty(ranch));
    assert!(h.state().title().ends_with(" •"));
    assert_eq!(
        h.get_by_label("editor file").value().as_deref(),
        Some(format!("{ranch_label} •").as_str()),
        "the selector marks it"
    );
    assert!(shown(&h, "ranch  2  •"), "the Files list marks it");

    let before = m.snapshot();
    click(&mut h, "Save");
    let after = m.snapshot();

    let backup = h.state().workspace().backup_path_for(ranch);
    // config.d/ranch~ would match the Include pattern config.d/*, so the
    // backup goes to config.d/.ranch~ (the core's backup rule).
    assert_eq!(backup, m.file(".ranch~"));
    let mut expected = vec![m.file("ranch"), backup.clone()];
    expected.sort();
    assert_eq!(
        changed(&before, &after),
        expected,
        "only ranch and its backup"
    );
    let mut sorted = Config::parse(&edited).unwrap();
    sorted.sort_sections();
    assert_eq!(
        std::fs::read_to_string(m.file("ranch")).unwrap(),
        sorted.render()
    );
    assert_eq!(std::fs::read_to_string(&backup).unwrap(), RANCH);
    assert_eq!(h.state().last_backup(), Some(backup.as_path()));
    assert!(!h.state().editor_dirty());
    let backup_label = format!("backup: {}", h.state().workspace().display_path(&backup));
    assert!(shown(&h, &backup_label), "the status bar shows its backup");
    assert!(shown_contains(&h, &format!("saved {ranch_label}.")));
    assert!(
        !h.state().workspace().files.iter().any(|f| f.path == backup),
        "the backup is not loaded as a workspace file"
    );
}

/// Each file keeps its own buffer when the selector switches files.
#[test]
fn switching_files_keeps_each_buffer() {
    let m = Multi::new();
    let mut h = harness(&m.root);
    h.run();
    click(&mut h, "Editor");
    let cypress = index_of(&h, "cypress");
    let ranch = index_of(&h, "ranch");
    h.state_mut().select_file(cypress);
    h.state_mut().set_editor_text(format!("{CYPRESS}# c\n"));
    h.state_mut().select_file(ranch);
    h.run();
    assert_eq!(editor_value(&h), RANCH);
    h.state_mut().select_file(cypress);
    h.run();
    assert_eq!(editor_value(&h), format!("{CYPRESS}# c\n"));
    assert_eq!(h.state().dirty_files(), [cypress]);
}

/// inc-gui-2: a host of an included file shows its file in the table's
/// file column; Show in Editor loads that file at the host's line.
#[test]
fn inc_gui_2_open_host_in_editor_selects_file_and_line() {
    let m = Multi::new();
    let mut h = harness(&m.root);
    h.run();
    let ranch = index_of(&h, "ranch");
    let row = h
        .state()
        .rows()
        .iter()
        .find(|r| r.name == "ranch-nas")
        .unwrap()
        .clone();
    assert_eq!(row.file, ranch);
    assert_eq!(row.file_name, "ranch");
    assert_eq!(
        h.query_all_by_label("ranch").count(),
        2,
        "the file column names the file on both ranch rows"
    );
    click(&mut h, "ranch-nas");
    assert_eq!(h.state().selected(), Some("ranch-nas"));
    click(&mut h, "Show in Editor");
    let s = h.state();
    assert_eq!(s.tab, Tab::Editor);
    assert_eq!(s.current_file(), ranch);
    let ws = s.workspace();
    let line = ws.host_line(ws.find_host("ranch-nas").unwrap());
    assert_eq!(s.editor_line(), Some(line));
    assert_eq!(
        RANCH.lines().nth(line - 1),
        Some("Host ranch-nas"),
        "line {line} is the Host line"
    );
    h.run();
    assert_eq!(editor_value(&h), RANCH);
}

/// inc-gui-3: Quit with two files unsaved shows one alert listing both;
/// Cancel stays, Don't Save / Discard All writes neither.
#[test]
fn inc_gui_3_quit_lists_every_dirty_file_cancel_and_discard() {
    let m = Multi::new();
    let mut h = harness(&m.root);
    h.run();
    let cypress = index_of(&h, "cypress");
    let ranch = index_of(&h, "ranch");
    h.state_mut().select_file(cypress);
    h.state_mut().set_editor_text(format!("{CYPRESS}# c\n"));
    h.state_mut().select_file(ranch);
    h.state_mut().set_editor_text(format!("{RANCH}# r\n"));
    let before = m.snapshot();

    h.state_mut().request_close();
    h.run();
    let names = vec![display(&h, cypress), display(&h, ranch)];
    assert_eq!(
        h.state().dialog(),
        Some(&Dialog::UnsavedCloseAll(names.clone()))
    );
    assert!(shown(&h, "Save changes to 2 files?"));
    for n in &names {
        assert!(shown_contains(&h, n), "the alert lists {n}");
    }
    assert!(shown(&h, "Save All"));
    click(&mut h, "Cancel");
    assert!(!h.state().is_closing());
    assert_eq!(h.state().dirty_files(), [cypress, ranch]);
    assert_eq!(m.snapshot(), before);

    h.state_mut().request_close();
    h.run();
    click(&mut h, DISCARD_ALL_LABEL);
    assert!(h.state().is_closing());
    assert_eq!(m.snapshot(), before, "discard writes neither file");
}

/// inc-gui-3: Save All in the Quit alert writes both files, each with its
/// own backup, and closes.
#[test]
fn inc_gui_3_quit_save_all_writes_both() {
    let m = Multi::new();
    let mut h = harness(&m.root);
    h.run();
    let cypress = index_of(&h, "cypress");
    let ranch = index_of(&h, "ranch");
    h.state_mut().select_file(cypress);
    h.state_mut().set_editor_text(format!("{CYPRESS}# c\n"));
    h.state_mut().select_file(ranch);
    h.state_mut().set_editor_text(format!("{RANCH}# r\n"));
    let before = m.snapshot();
    h.state_mut().request_close();
    h.run();
    click(&mut h, "Save All");
    assert!(h.state().is_closing());
    assert!(std::fs::read_to_string(m.file("cypress"))
        .unwrap()
        .ends_with("# c\n"));
    assert!(std::fs::read_to_string(m.file("ranch"))
        .unwrap()
        .ends_with("# r\n"));
    let mut expected = vec![
        m.file("cypress"),
        m.file(".cypress~"),
        m.file("ranch"),
        m.file(".ranch~"),
    ];
    expected.sort();
    assert_eq!(changed(&before, &m.snapshot()), expected);
    assert_eq!(std::fs::read_to_string(&m.root).unwrap(), m.root_text);
}

/// A refused save in Save All keeps the window open on that file.
#[test]
fn quit_save_all_stops_at_a_refused_file() {
    let m = Multi::new();
    let mut h = harness(&m.root);
    h.run();
    let cypress = index_of(&h, "cypress");
    let ranch = index_of(&h, "ranch");
    h.state_mut().select_file(cypress);
    h.state_mut().set_editor_text("Host\n");
    h.state_mut().select_file(ranch);
    h.state_mut().set_editor_text(format!("{RANCH}# r\n"));
    let before = m.snapshot();
    h.state_mut().request_close();
    h.run();
    click(&mut h, "Save All");
    assert!(!h.state().is_closing());
    assert_eq!(h.state().current_file(), cypress);
    assert_eq!(h.state().tab, Tab::Editor);
    assert_eq!(h.state().editor_error(), Some("line 1: cannot parse: Host"));
    assert_eq!(m.snapshot(), before);
}

/// The form writes to the file that holds the host, and the status bar
/// names it.
#[test]
fn form_edit_writes_the_file_that_holds_the_host() {
    let m = Multi::new();
    let mut h = harness(&m.root);
    h.run();
    let before = m.snapshot();
    click(&mut h, "dcevant");
    replace_in(&mut h, "Connection URI", "ops@dcevant.ranch.lan");
    click(&mut h, "Save");
    let out = std::fs::read_to_string(m.file("ranch")).unwrap();
    assert!(out.contains("Host dcevant\n    HostName dcevant.ranch.lan\n    User ops\n"));
    let mut expected = vec![m.file("ranch"), m.file(".ranch~")];
    expected.sort();
    assert_eq!(changed(&before, &m.snapshot()), expected);
    let ranch = index_of(&h, "ranch");
    assert_eq!(
        h.state().status(),
        format!("dcevant updated in {}.", display(&h, ranch))
    );
}

/// A section held by two files is refused inline with the core's message.
#[test]
fn form_section_in_two_files_is_refused_inline() {
    let sectioned = |text: &str, host: &str| {
        let mut c = Config::parse(text).unwrap();
        c.move_host(host, None, Some("lab")).unwrap();
        c.render()
    };
    let m = Multi::with(
        &sectioned(CYPRESS, "cypressPro"),
        &sectioned(RANCH, "dcevant"),
    );
    let mut h = harness(&m.root);
    h.run();
    // The sidebar shows lab once per file, with the file name.
    assert!(shown(&h, "lab  1  cypress"));
    assert!(shown(&h, "lab  1  ranch"));
    click(&mut h, "lab  1  ranch");
    assert_eq!(h.state().visible_names(), ["dcevant"]);
    click(&mut h, "All hosts  5");
    let before = m.snapshot();
    click(&mut h, "Add Host");
    type_into(&mut h, "Name", "lab-2");
    type_into(&mut h, "Connection URI", "lab-2.example.com");
    type_into(&mut h, "Section", "lab");
    click(&mut h, "Save");
    let err = h.state().form().unwrap().error.clone().unwrap();
    assert!(err.starts_with("section lab exists in "), "{err}");
    assert!(shown_contains(&h, "section lab exists in"));
    assert_eq!(m.snapshot(), before);
}

/// inc-gui-4: a config without Include opens exactly as before: no Files
/// list, no file column, no file selector, no Show in Editor, and the
/// title and status bar name only the config.
#[test]
fn inc_gui_4_no_include_shows_no_file_ui() {
    let text = three_hosts_sectioned();
    let f = Fixture::new(&text);
    let mut h = harness(&f.path);
    h.run();
    assert!(!h.state().is_multi());
    assert_eq!(h.state().title(), format!("rustorm — {}", f.path.display()));
    assert!(shown(&h, "section"), "the table shows");
    assert!(!shown(&h, "file"), "no file column");
    assert!(!shown(&h, "filter file"));
    assert!(!shown(&h, "Files"), "no Files list");
    assert!(shown(&h, &f.path.display().to_string()));
    assert!(shown(&h, "no backup yet"));
    assert!(h.state().rows().iter().all(|r| r.file_name.is_empty()));
    click(&mut h, "vps");
    assert!(h.state().form().is_some());
    assert!(!shown(&h, "Show in Editor"));
    click(&mut h, "Editor");
    assert_eq!(editor_value(&h), text);
    assert!(!shown(&h, "editor file"), "no file selector");
}

// ----- the editor follows the selection (R-editor-follows) -----

fn line_of(text: &str, name: &str) -> usize {
    text.lines()
        .position(|l| l.trim() == format!("Host {name}"))
        .unwrap()
        + 1
}

/// follow-gui-1: a Files row click selects that file in the editor and
/// stays on the Hosts tab.
#[test]
fn follow_gui_1_files_click_selects_the_editor_file() {
    let m = Multi::new();
    let mut h = harness(&m.root);
    h.run();
    click(&mut h, "cypress  2");
    assert_eq!(h.state().current_file(), index_of(&h, "cypress"));
    assert_eq!(h.state().tab, Tab::Hosts);
}

/// follow-gui-2: clearing the Files filter leaves the editor on the file.
#[test]
fn follow_gui_2_clearing_the_files_filter_keeps_the_editor() {
    let m = Multi::new();
    let mut h = harness(&m.root);
    h.run();
    click(&mut h, "ranch  2");
    click(&mut h, "ranch  2");
    assert_eq!(h.state().visible_names().len(), 5);
    assert_eq!(h.state().current_file(), index_of(&h, "ranch"));
}

/// follow-gui-3: a host row click points the editor at the host's file and
/// Host line while the Hosts tab stays.
#[test]
fn follow_gui_3_host_click_points_the_editor_at_the_host() {
    let m = Multi::new();
    let mut h = harness(&m.root);
    h.run();
    click(&mut h, "ranch-nas");
    let s = h.state();
    assert_eq!(s.tab, Tab::Hosts);
    assert_eq!(s.current_file(), index_of(&h, "ranch"));
    assert_eq!(s.pending_editor_line(), Some(line_of(RANCH, "ranch-nas")));
    click(&mut h, "github");
    assert_eq!(h.state().current_file(), 0);
    assert_eq!(
        h.state().pending_editor_line(),
        Some(line_of(&m.root_text, "github"))
    );
}

/// follow-gui-4: the Editor tab then opens on that host's Host line.
#[test]
fn follow_gui_4_editor_tab_opens_on_the_selected_host() {
    let m = Multi::new();
    let mut h = harness(&m.root);
    h.run();
    click(&mut h, "ranch-nas");
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Num2);
    h.run();
    let s = h.state();
    assert_eq!(s.tab, Tab::Editor);
    assert_eq!(s.pending_editor_line(), None);
    assert_eq!(s.editor_line(), Some(line_of(RANCH, "ranch-nas")));
    assert_eq!(editor_value(&h), RANCH);
}

/// follow-gui-5: switching tabs without a new selection queues nothing, so
/// a cursor the user placed stays.
#[test]
fn follow_gui_5_tab_round_trip_does_not_rejump() {
    let m = Multi::new();
    let mut h = harness(&m.root);
    h.run();
    click(&mut h, "ranch-nas");
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Num2);
    h.run();
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Num1);
    h.run();
    assert_eq!(h.state().tab, Tab::Hosts);
    assert_eq!(h.state().pending_editor_line(), None);
    // Re-clicking the selected host is not a new selection either.
    click(&mut h, "ranch-nas");
    assert_eq!(h.state().pending_editor_line(), None);
}

/// follow-gui-6: Esc clears the selection and leaves the editor alone.
#[test]
fn follow_gui_6_escape_leaves_the_editor() {
    let m = Multi::new();
    let mut h = harness(&m.root);
    h.run();
    click(&mut h, "dcevant");
    let pending = h.state().pending_editor_line();
    h.key_press(egui::Key::Escape);
    h.run();
    assert_eq!(h.state().selected(), None);
    assert_eq!(h.state().current_file(), index_of(&h, "ranch"));
    assert_eq!(h.state().pending_editor_line(), pending);
}

/// follow-gui-7: on a single file the editor follows to each Host line.
#[test]
fn follow_gui_7_single_file_follows_the_host_line() {
    let f = Fixture::new(THREE_HOSTS);
    let mut h = harness(&f.path);
    h.run();
    click(&mut h, "web-prod");
    assert_eq!(
        h.state().pending_editor_line(),
        Some(line_of(THREE_HOSTS, "web-prod"))
    );
}

/// follow-gui-8: Show in Editor still opens the Editor tab at the host.
#[test]
fn follow_gui_8_show_in_editor_still_switches_tab() {
    let m = Multi::new();
    let mut h = harness(&m.root);
    h.run();
    click(&mut h, "dcevant");
    click(&mut h, "Show in Editor");
    assert_eq!(h.state().tab, Tab::Editor);
    assert_eq!(h.state().editor_line(), Some(line_of(RANCH, "dcevant")));
}

/// Up and Down move the host selection, and the editor follows.
#[test]
fn follow_gui_arrow_keys_move_the_selection() {
    let m = Multi::new();
    let mut h = harness(&m.root);
    h.run();
    let names = h.state().visible_names();
    h.key_press(egui::Key::ArrowDown);
    h.run();
    assert_eq!(h.state().selected(), Some(names[0].as_str()));
    h.key_press(egui::Key::ArrowDown);
    h.run();
    let second = names[1].clone();
    assert_eq!(h.state().selected(), Some(second.as_str()));
    let ws = h.state().workspace();
    let wl = ws.find_host(&second).unwrap();
    assert_eq!(h.state().current_file(), wl.file);
    assert_eq!(h.state().pending_editor_line(), Some(ws.host_line(wl)));
    h.key_press(egui::Key::ArrowUp);
    h.run();
    assert_eq!(h.state().selected(), Some(names[0].as_str()));
}
