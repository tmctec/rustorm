//! The Combine dialog: merge two or more config files into one, deciding
//! each conflict before anything is written (docs/cli.md, combine).

use std::path::{Path, PathBuf};

use egui::{Button, Label, RichText, TextEdit, Ui};
use rustorm_core::{
    CombinePreview, CombineRequest, Error, HostDecision, HostDecisions, OnConflict,
};

use super::{set_label, App};

const DIALOG_W: f32 = 900.0;
const SIDE_W: f32 = 440.0;
const ROW_H: f32 = 20.0;

/// One file the dialog can combine: a loaded workspace file or a typed
/// path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CombineFile {
    /// The path the core reads, absolute for a loaded file.
    pub path: PathBuf,
    /// The path as the dialog shows it (`~/...`).
    pub label: String,
    /// True when the file is merged into the base.
    pub chosen: bool,
    /// True for a path typed into the dialog rather than a loaded file.
    pub typed: bool,
}

/// The open Combine dialog.
#[derive(Debug, Clone)]
pub struct CombineView {
    files: Vec<CombineFile>,
    base: usize,
    /// The Add a file field.
    path_entry: String,
    /// True when the result goes to `output` instead of the base.
    output_other: bool,
    /// The Another file field.
    output: String,
    retire: bool,
    decisions: HostDecisions,
    preview: Option<CombinePreview>,
    /// Why there is no preview, or why the last write was refused.
    error: Option<String>,
}

impl CombineView {
    /// Every file the dialog offers, loaded files first in load order, then
    /// typed paths.
    pub fn files(&self) -> &[CombineFile] {
        &self.files
    }

    /// The index into [`CombineView::files`] of the base.
    pub fn base(&self) -> usize {
        self.base
    }

    /// The current preview; `None` until two files are chosen or while
    /// the inputs cannot be read.
    pub fn preview(&self) -> Option<&CombinePreview> {
        self.preview.as_ref()
    }

    /// The decision for the conflict `name`.
    pub fn decision(&self, name: &str) -> Option<HostDecision> {
        self.decisions.get(name).copied()
    }

    /// True when Retire merged files is checked.
    pub fn retire(&self) -> bool {
        self.retire
    }

    /// Why there is no preview, or the last refused write's message.
    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    /// True when every conflict is decided and the output is named, so
    /// Write is enabled.
    pub fn ready(&self) -> bool {
        self.preview.as_ref().is_some_and(CombinePreview::is_ready)
            && (!self.output_other || !self.output.trim().is_empty())
    }

    /// The inputs in merge order: the base, then each other chosen file in
    /// list order.
    fn inputs(&self) -> Vec<PathBuf> {
        let mut out = vec![self.files[self.base].path.clone()];
        out.extend(
            self.files
                .iter()
                .enumerate()
                .filter(|(i, f)| *i != self.base && f.chosen)
                .map(|(_, f)| f.path.clone()),
        );
        out
    }

    fn request(&self, decisions: HostDecisions, home: Option<&Path>) -> CombineRequest {
        let output = self
            .output_other
            .then(|| expand(self.output.trim(), home))
            .filter(|p| !p.as_os_str().is_empty());
        CombineRequest {
            inputs: self.inputs(),
            output,
            policy: OnConflict::Fail,
            decisions,
            retire: self.retire,
        }
    }

    /// Reads the inputs again and renders the merge with the decisions
    /// that still name a conflict.
    fn recompute(&mut self, home: Option<&Path>) {
        self.preview = None;
        if self.inputs().len() < 2 {
            self.error = None;
            return;
        }
        let plain = match self.request(HostDecisions::new(), home).preview(home) {
            Ok(p) => p,
            Err(e) => {
                self.error = Some(e.to_string());
                return;
            }
        };
        self.decisions
            .retain(|name, _| plain.conflicts.iter().any(|c| c.name == *name));
        if self.decisions.is_empty() {
            self.preview = Some(plain);
            self.error = None;
            return;
        }
        match self.request(self.decisions.clone(), home).preview(home) {
            Ok(p) => {
                self.preview = Some(p);
                self.error = None;
            }
            Err(e) => self.error = Some(e.to_string()),
        }
    }

    /// The output as the preview's summary names it.
    fn output_path(&self, home: Option<&Path>) -> PathBuf {
        if self.output_other && !self.output.trim().is_empty() {
            expand(self.output.trim(), home)
        } else {
            self.files[self.base].path.clone()
        }
    }
}

/// `~/x` against `home`; anything else as typed.
fn expand(text: &str, home: Option<&Path>) -> PathBuf {
    match (text.strip_prefix("~/"), home) {
        (Some(rest), Some(h)) => h.join(rest),
        _ => PathBuf::from(text),
    }
}

/// `path` with the home directory shown as `~`.
pub(super) fn tilde(path: &Path, home: Option<&Path>) -> String {
    match home.and_then(|h| path.strip_prefix(h).ok()) {
        Some(rest) => format!("~/{}", rest.display()),
        None => path.display().to_string(),
    }
}

/// What a click in the dialog asks for, applied after drawing.
enum Action {
    Base(usize),
    Choose(usize, bool),
    AddPath,
    Output(bool),
    Retire(bool),
    Decide(String, HostDecision),
    Write,
    Close,
    /// A text field changed; recompute the preview.
    Edited,
}

impl App {
    /// The open Combine dialog.
    pub fn combine(&self) -> Option<&CombineView> {
        self.combine.as_ref()
    }

    /// Opens the Combine dialog, as the sidebar button does: not while the
    /// editor holds unsaved text. The root is the base; no other file is
    /// chosen yet.
    pub fn open_combine(&mut self) {
        if self.editor_dirty() {
            return;
        }
        let home = self.home();
        let files = self
            .load_indices()
            .into_iter()
            .map(|i| CombineFile {
                path: self.ws.abs(i),
                label: tilde(&self.ws.abs(i), home.as_deref()),
                chosen: false,
                typed: false,
            })
            .collect();
        self.combine = Some(CombineView {
            files,
            base: 0,
            path_entry: String::new(),
            output_other: false,
            output: String::new(),
            retire: false,
            decisions: HostDecisions::new(),
            preview: None,
            error: None,
        });
    }

    /// Makes file `i` of the dialog the base.
    pub fn combine_set_base(&mut self, i: usize) {
        self.combine_action(Action::Base(i));
    }

    /// Checks or unchecks file `i` of the dialog as an input.
    pub fn combine_choose(&mut self, i: usize, chosen: bool) {
        self.combine_action(Action::Choose(i, chosen));
    }

    /// Adds `path` (`~/` expands) as a chosen input, as Add a file does.
    pub fn combine_add_path(&mut self, path: &str) {
        if let Some(v) = &mut self.combine {
            v.path_entry = path.to_string();
        }
        self.combine_action(Action::AddPath);
    }

    /// Sends the result to `path` (`~/` expands), or to the base when
    /// `None`.
    pub fn combine_set_output(&mut self, path: Option<&str>) {
        if let Some(v) = &mut self.combine {
            v.output = path.unwrap_or_default().to_string();
        }
        self.combine_action(Action::Output(path.is_some()));
    }

    /// Checks or unchecks Retire merged files.
    pub fn combine_set_retire(&mut self, retire: bool) {
        self.combine_action(Action::Retire(retire));
    }

    /// Decides the conflict `name`, as its Keep or Replace button does.
    pub fn combine_decide(&mut self, name: &str, decision: HostDecision) {
        self.combine_action(Action::Decide(name.to_string(), decision));
    }

    /// Writes the merge, as Write does. Refused while a conflict is
    /// undecided, while the editor holds unsaved text, and when an input
    /// changed since the preview, which is then recomputed. True when
    /// the file was written.
    pub fn combine_write(&mut self) -> bool {
        self.combine_action(Action::Write);
        self.combine.is_none()
    }

    /// Closes the dialog, as Cancel and Esc do; nothing is written.
    pub fn close_combine(&mut self) {
        self.combine = None;
    }

    pub(super) fn combine_dialog(&mut self, ctx: &egui::Context) {
        if self.combine.is_none() {
            return;
        }
        let modal = egui::Modal::new(egui::Id::new("combine"));
        let mut action = None;
        let response = modal.show(ctx, |ui| {
            action = self.draw_combine(ui);
        });
        if action.is_none() && response.should_close() && self.dialog.is_none() {
            action = Some(Action::Close);
        }
        if let Some(a) = action {
            self.combine_action(a);
        }
    }

    fn draw_combine(&mut self, ui: &mut Ui) -> Option<Action> {
        let blocked = self.editor_dirty();
        let home = self.home();
        let home = home.as_deref();
        let view = self.combine.as_mut()?;
        let mut action = None;
        let weak = ui.visuals().weak_text_color();
        ui.set_width(DIALOG_W);
        ui.heading("Combine files");
        ui.label(
            RichText::new(
                "Merges every chosen file into the base. Nothing is written until Write.",
            )
            .color(weak),
        );
        let max = (ui.ctx().content_rect().height() - 300.0).clamp(220.0, 640.0);
        egui::ScrollArea::vertical()
            .id_salt("combine-body")
            .max_height(max)
            .min_scrolled_height(max)
            .auto_shrink([false, true])
            .show(ui, |ui| {
                ui.set_min_height(max);
                ui.add_space(4.0);
                ui.label(RichText::new("Files").strong());
                for (i, f) in view.files.iter().enumerate() {
                    ui.horizontal(|ui| {
                        let base = ui.radio(view.base == i, "Base");
                        set_label(ui, base.id, &format!("Base {}", f.label));
                        if base.clicked() {
                            action = Some(Action::Base(i));
                        }
                        let mut chosen = f.chosen || view.base == i;
                        let check = ui.add_enabled(
                            view.base != i,
                            egui::Checkbox::new(&mut chosen, "Merge"),
                        );
                        set_label(ui, check.id, &format!("Merge {}", f.label));
                        if check.changed() {
                            action = Some(Action::Choose(i, chosen));
                        }
                        let mut text = RichText::new(&f.label).monospace();
                        if f.typed {
                            text = text.italics();
                        }
                        ui.add(Label::new(text).truncate());
                    });
                }
                ui.horizontal(|ui| {
                    let l = ui.label("Add a file:");
                    let field = ui
                        .add(
                            TextEdit::singleline(&mut view.path_entry)
                                .hint_text("~/.ssh/old/config")
                                .desired_width(420.0),
                        )
                        .labelled_by(l.id);
                    set_label(ui, field.id, "File to add");
                    let enter = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                    if ui
                        .add_enabled(!view.path_entry.trim().is_empty(), Button::new("Add"))
                        .clicked()
                        || enter
                    {
                        action = Some(Action::AddPath);
                    }
                });
                ui.add_space(6.0);
                ui.label(RichText::new("Output").strong());
                ui.horizontal(|ui| {
                    let base_label = format!("Base ({})", view.files[view.base].label);
                    if ui.radio(!view.output_other, base_label).clicked() {
                        action = Some(Action::Output(false));
                    }
                    let other = ui.radio(view.output_other, "Another file:");
                    if other.clicked() {
                        action = Some(Action::Output(true));
                    }
                    let field = ui
                        .add_enabled(
                            view.output_other,
                            TextEdit::singleline(&mut view.output)
                                .hint_text("~/.ssh/config.merged")
                                .desired_width(360.0),
                        )
                        .labelled_by(other.id);
                    set_label(ui, field.id, "Output file");
                    if field.changed() {
                        action = Some(Action::Edited);
                    }
                });
                let mut retire = view.retire;
                let check = ui
                    .checkbox(&mut retire, "Retire merged files")
                    .on_hover_text(
                        "After the write, move each merged file an Include still loads to ~/.ssh/retired/",
                    );
                if check.changed() {
                    action = Some(Action::Retire(retire));
                }
                if view.retire {
                    if let Some(p) = &view.preview {
                        for input in view.inputs().iter().skip(1) {
                            let label = tilde(input, home);
                            let line = if p.retirable.contains(input) {
                                format!("    {label} moves to ~/.ssh/retired/")
                            } else {
                                format!("    {label} stays: no Include loads it")
                            };
                            ui.label(RichText::new(line).color(weak));
                        }
                    }
                }
                ui.separator();
                draw_preview(ui, view, home, blocked, &mut action);
            });
        ui.separator();
        if let Some(e) = &view.error {
            ui.colored_label(ui.visuals().error_fg_color, format!("⚠ {e}"));
        }
        let (_, footer) = egui::Sides::new().show(
            ui,
            |_ui| {},
            |ui| {
                let mut action = None;
                let hint = if blocked {
                    "Save or discard the editor first"
                } else if view.preview.is_none() {
                    "Choose at least two files"
                } else if !view.preview.as_ref().is_some_and(CombinePreview::is_ready) {
                    "Decide every conflict first"
                } else {
                    "Name the output file first"
                };
                if ui
                    .add_enabled(!blocked && view.ready(), Button::new("Write"))
                    .on_hover_text("Write the merged text after a backup")
                    .on_disabled_hover_text(hint)
                    .clicked()
                {
                    action = Some(Action::Write);
                }
                if ui.button("Cancel").clicked() {
                    action = Some(Action::Close);
                }
                if !view.ready() || blocked {
                    ui.label(RichText::new(hint).color(weak));
                }
                action
            },
        );
        action.or(footer)
    }

    fn combine_action(&mut self, action: Action) {
        let home = self.home();
        let home = home.as_deref();
        let Some(view) = &mut self.combine else {
            return;
        };
        match action {
            Action::Base(i) if i < view.files.len() => {
                view.base = i;
                view.recompute(home);
            }
            Action::Choose(i, chosen) if i < view.files.len() => {
                view.files[i].chosen = chosen;
                view.recompute(home);
            }
            Action::AddPath => {
                let text = view.path_entry.trim().to_string();
                if text.is_empty() {
                    return;
                }
                let path = expand(&text, home);
                if !path.is_file() {
                    view.error = Some(format!("{text}: no such file."));
                    return;
                }
                let abs = rustorm_core::absolute_path(&path);
                match view
                    .files
                    .iter()
                    .position(|f| rustorm_core::absolute_path(&f.path) == abs)
                {
                    Some(i) => view.files[i].chosen = true,
                    None => view.files.push(CombineFile {
                        label: tilde(&path, home),
                        path,
                        chosen: true,
                        typed: true,
                    }),
                }
                view.path_entry.clear();
                view.recompute(home);
            }
            Action::Output(other) => {
                view.output_other = other;
                view.recompute(home);
            }
            Action::Retire(retire) => {
                view.retire = retire;
                view.recompute(home);
            }
            Action::Decide(name, decision) => {
                view.decisions.insert(name, decision);
                view.recompute(home);
            }
            Action::Edited => view.recompute(home),
            Action::Write => {
                self.write_combine();
            }
            Action::Close => self.combine = None,
            Action::Base(_) | Action::Choose(..) => {}
        }
    }

    /// Commits the dialog's merge through the core, as `rustorm combine`
    /// does, then reloads the workspace.
    fn write_combine(&mut self) {
        let blocked = self.editor_dirty();
        let home = self.home();
        let home = home.as_deref();
        let Some(view) = &mut self.combine else {
            return;
        };
        if blocked || !view.ready() {
            return;
        }
        let Some(preview) = &view.preview else {
            return;
        };
        let request = view.request(view.decisions.clone(), home);
        let output = view.output_path(home);
        let existed = output.exists();
        match request.commit(Some(&preview.fingerprints), self.write, home) {
            Ok(outcome) => {
                let mut lines: Vec<String> = Vec::new();
                for a in &outcome.report.added {
                    lines.push(format!(
                        "Host *: {} {} added from {}.",
                        a.key,
                        a.value,
                        a.from.display()
                    ));
                }
                for k in &outcome.report.skipped {
                    lines.push(format!(
                        "Host *: {} {} from {} skipped, keeping {}.",
                        k.key,
                        k.value,
                        k.from.display(),
                        k.kept
                    ));
                }
                let moved = |p: &Path| {
                    outcome
                        .retired
                        .iter()
                        .any(|r| r.to.is_some() && r.from == p)
                };
                for w in outcome.report.includes.iter().filter(|w| !moved(&w.loads)) {
                    lines.push(format!(
                        "Warning: Include {} in {} still loads {}.",
                        w.pattern,
                        w.file.display(),
                        w.loads.display()
                    ));
                }
                lines.push(outcome.report.summary(Some(&outcome.output)));
                lines.extend(outcome.retired.iter().map(|r| r.message()));
                self.combine = None;
                if existed && !self.write.no_backup {
                    let backup = rustorm_core::backup_path(&outcome.output);
                    if backup.exists() {
                        self.last_backup = Some(backup);
                    }
                }
                self.reload_after_external_write();
                self.status = lines.join(" ");
            }
            Err(e) => {
                let stale = matches!(e, Error::CombineStale(_));
                let message = e.to_string();
                self.status = format!("error: {message}");
                if let Some(view) = &mut self.combine {
                    if stale {
                        view.recompute(home);
                    }
                    view.error = Some(message);
                }
            }
        }
    }
}

/// The preview: the summary, Host * notes, Include warnings, each conflict
/// with both blocks and Keep / Replace, and the merged text.
fn draw_preview(
    ui: &mut Ui,
    view: &CombineView,
    home: Option<&Path>,
    blocked: bool,
    action: &mut Option<Action>,
) {
    let weak = ui.visuals().weak_text_color();
    ui.label(RichText::new("Preview").strong());
    let Some(p) = &view.preview else {
        let text = if view.error.is_some() {
            "No preview: see the message below."
        } else {
            "Choose a base and at least one file to merge into it."
        };
        ui.label(RichText::new(text).color(weak));
        return;
    };
    ui.label(p.report.summary(Some(&view.output_path(home))));
    for a in &p.report.added {
        ui.label(format!(
            "Host *: {} {} added from {}.",
            a.key,
            a.value,
            tilde(&a.from, home)
        ));
    }
    for k in &p.report.skipped {
        ui.label(format!(
            "Host *: {} {} from {} skipped, keeping {}.",
            k.key,
            k.value,
            tilde(&k.from, home),
            k.kept
        ));
    }
    for w in &p.report.includes {
        ui.colored_label(
            ui.visuals().warn_fg_color,
            format!(
                "Include {} in {} still loads {}.",
                w.pattern,
                tilde(&w.file, home),
                tilde(&w.loads, home)
            ),
        );
    }
    if !p.conflicts.is_empty() {
        ui.add_space(4.0);
        let undecided = p.undecided().len();
        ui.label(
            RichText::new(format!(
                "Conflicts: {} · {undecided} undecided",
                p.conflicts.len()
            ))
            .strong(),
        );
    }
    for c in &p.conflicts {
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.label(RichText::new(&c.name).strong());
            let state = match c.decision {
                None => RichText::new("undecided").color(ui.visuals().warn_fg_color),
                Some(HostDecision::Keep) => RichText::new("keep the earlier block").color(weak),
                Some(HostDecision::Replace) => {
                    RichText::new("replace with the later block").color(weak)
                }
            };
            ui.label(state);
        });
        ui.horizontal_top(|ui| {
            for (side, file, text) in [
                ("earlier", &c.first, &c.first_text),
                ("later", &c.second, &c.second_text),
            ] {
                ui.vertical(|ui| {
                    ui.set_width(SIDE_W);
                    ui.label(RichText::new(format!("{side}  {}", tilde(file, home))).strong());
                    for line in text.lines() {
                        ui.label(RichText::new(line).monospace());
                    }
                });
            }
        });
        ui.horizontal(|ui| {
            let decided = view.decisions.get(&c.name).copied();
            for (word, value, hover) in [
                (
                    "Keep",
                    HostDecision::Keep,
                    format!(
                        "Keep the block from {}; drop the later one",
                        tilde(&c.first, home)
                    ),
                ),
                (
                    "Replace",
                    HostDecision::Replace,
                    format!(
                        "Put the block from {} in the earlier block's place",
                        tilde(&c.second, home)
                    ),
                ),
            ] {
                let resp = ui
                    .add_enabled(!blocked, Button::selectable(decided == Some(value), word))
                    .on_hover_text(hover);
                set_label(ui, resp.id, &format!("{word} {}", c.name));
                if resp.clicked() {
                    *action = Some(Action::Decide(c.name.clone(), value));
                }
            }
        });
    }
    ui.add_space(4.0);
    egui::CollapsingHeader::new("Merged text")
        .id_salt("combine-merged")
        .default_open(true)
        .show(ui, |ui| {
            egui::ScrollArea::vertical()
                .id_salt("combine-text")
                .max_height(260.0)
                .auto_shrink([false, true])
                .show(ui, |ui| {
                    for line in p.text.lines() {
                        ui.add_sized(
                            [DIALOG_W - 40.0, ROW_H - 4.0],
                            Label::new(RichText::new(line).monospace()).truncate(),
                        );
                    }
                });
        });
}
