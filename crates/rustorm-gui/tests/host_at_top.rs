//! The editor shows a followed host's `Host` line as its first visible
//! row (top-gui-*).
mod common;

use common::*;
use egui_kittest::kittest::{NodeT, Queryable};
use egui_kittest::Harness;
use rustorm_gui::App;

/// Forty hosts, four lines each, with `cypressMelissa` directly above
/// `cypressMBP` in the middle and `host39` last.
fn long_file() -> String {
    let mut s = String::from("Host *\n    ServerAliveInterval 60\n\n");
    for i in 0..40 {
        let name = match i {
            20 => "cypressMelissa".to_string(),
            21 => "cypressMBP".to_string(),
            _ => format!("host{i:02}"),
        };
        s.push_str(&format!(
            "Host {name}\n    HostName {name}.example.com\n    User u{i}\n\n"
        ));
    }
    s
}

/// The screen rect of each editor row, top to bottom, with its text.
fn editor_rows(h: &Harness<'static, App>) -> Vec<(String, egui::Rect)> {
    h.get_by_label("config editor")
        .children()
        .filter_map(|c| {
            let n = c.accesskit_node();
            let b = n.bounding_box()?;
            Some((
                n.value().unwrap_or_default(),
                egui::Rect::from_min_max(
                    egui::pos2(b.x0 as f32, b.y0 as f32),
                    egui::pos2(b.x1 as f32, b.y1 as f32),
                ),
            ))
        })
        .collect()
}

/// The top of the editor's viewport: where the editor's frame starts
/// while it is scrolled to the start of the file.
fn viewport_top(h: &mut Harness<'static, App>) -> f32 {
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Num2);
    h.run();
    let top = h.get_by_label("config editor").rect().top();
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Num1);
    h.run();
    top
}

/// Selects `name` on the Hosts tab, opens the Editor tab and asserts the
/// `Host name` row is the first row wholly inside the viewport and the row
/// above it is scrolled out of view.
fn assert_host_at_top(h: &mut Harness<'static, App>, top: f32, name: &str) {
    replace_in(h, "filter all", name);
    click(h, name);
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Num2);
    h.run();
    let rows = editor_rows(h);
    let i = rows
        .iter()
        .position(|(t, _)| t.trim_end() == format!("Host {name}"))
        .unwrap();
    let host = rows[i].1;
    let above = rows[i - 1].1;
    assert!(
        host.top() >= top - 0.5 && host.top() < top + host.height(),
        "{name}: Host row at {} but the viewport starts at {top}",
        host.top()
    );
    assert!(
        above.bottom() <= top + 0.5,
        "{name}: the row above ends at {}, below the viewport top {top}",
        above.bottom()
    );
}

/// top-gui-1: a host far down a long file, and one at its end, shows its
/// `Host` line as the first visible editor row.
#[test]
fn top_gui_1_followed_host_line_is_the_first_editor_row() {
    let f = Fixture::new(&long_file());
    let mut h = harness(&f.path);
    h.run();
    let top = viewport_top(&mut h);
    assert_host_at_top(&mut h, top, "cypressMBP");
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Num1);
    h.run();
    assert_host_at_top(&mut h, top, "host39");
}
