//! The Conflicts dialog: every host defined in two workspace files, paired
//! live and copy by the core's reconcile report, decided one pair at a time
//! (docs/cli.md, reconcile).

use std::path::PathBuf;

use egui::{Button, Color32, Label, RichText, Ui};
use rustorm_core::{
    canonical_key, is_meta_key, parse_meta_line, Decision, KeyPick, Pair, PairKind, Pick,
    ReconcileReport, Workspace,
};

use super::{set_label, App};
use crate::ops::{Op, PairDecision};

const DIALOG_W: f32 = 900.0;
const SIDE_W: f32 = 440.0;
const ROW_H: f32 = 20.0;

/// The open Conflicts dialog.
#[derive(Debug, Clone)]
pub struct ConflictsView {
    report: ReconcileReport,
    /// Index into `report.items`.
    selected: Option<usize>,
    /// Pairs decided without a write this session (kept live, keys that
    /// kept a difference, orphans left out), as (name, copy file).
    kept: Vec<(String, PathBuf)>,
    /// The per-key chooser, one pick per key of the selected pair.
    picks: Vec<Pick>,
    retire: Option<RetireView>,
    error: Option<String>,
}

/// The Retire confirmation inside the Conflicts dialog.
#[derive(Debug, Clone)]
pub struct RetireView {
    /// The file, absolute.
    pub file: PathBuf,
    /// The file as messages print it (`~/...`).
    pub label: String,
    /// True once a retire was refused; `blockers` and `orphans` are then
    /// kept current.
    pub refused: bool,
    /// What blocks it: `lab-1 (conflict)`, `printer (orphan)`, ...
    pub blockers: Vec<String>,
    /// The file's undecided orphans, each offered Add and Leave out.
    pub orphans: Vec<Pair>,
}

impl ConflictsView {
    fn new(report: ReconcileReport) -> ConflictsView {
        let mut view = ConflictsView {
            report,
            selected: None,
            kept: Vec::new(),
            picks: Vec::new(),
            retire: None,
            error: None,
        };
        view.select(view.order().first().copied());
        view
    }

    /// The report the dialog shows.
    pub fn report(&self) -> &ReconcileReport {
        &self.report
    }

    /// True when `p` was kept live (or its orphan left out) this session.
    pub fn is_kept(&self, p: &Pair) -> bool {
        self.kept
            .iter()
            .any(|(n, c)| p.answers_to(n) && p.copy.file == *c)
    }

    /// The names decided without a write, which retire counts as decided.
    pub fn decided(&self) -> Vec<String> {
        self.kept.iter().map(|(n, _)| n.clone()).collect()
    }

    /// Item indices in list order: undecided conflicts, identical pairs,
    /// then conflicts kept live.
    pub fn order(&self) -> Vec<usize> {
        let items = &self.report.items;
        let open = |i: &usize| items[*i].kind == PairKind::Conflict && !self.is_kept(&items[*i]);
        let all: Vec<usize> = (0..items.len()).collect();
        let mut out: Vec<usize> = all.iter().copied().filter(open).collect();
        out.extend(
            all.iter()
                .filter(|&&i| items[i].kind == PairKind::Identical),
        );
        out.extend(
            all.iter()
                .filter(|&&i| items[i].kind == PairKind::Conflict && self.is_kept(&items[i])),
        );
        out
    }

    /// The list's kind column for `p`: `conflict`, `conflict · labels
    /// only` when only metadata differs, `identical`, or `kept live`.
    pub fn kind_text(&self, p: &Pair) -> String {
        match p.kind {
            PairKind::Conflict if self.is_kept(p) => "kept live".to_string(),
            PairKind::Conflict if p.keys.iter().all(|k| is_meta_key(&k.key)) => {
                "conflict · labels only".to_string()
            }
            PairKind::Conflict => "conflict".to_string(),
            PairKind::Identical => "identical".to_string(),
            PairKind::Orphan => "orphan".to_string(),
        }
    }

    /// The selected pair.
    pub fn selected(&self) -> Option<&Pair> {
        self.selected.map(|i| &self.report.items[i])
    }

    /// The lines of the selected pair highlighted as differing: (live,
    /// copy).
    pub fn marked_lines(&self) -> (Vec<String>, Vec<String>) {
        let Some(p) = self.selected() else {
            return (Vec::new(), Vec::new());
        };
        let marked = |text: &str| {
            text.lines()
                .filter(|l| differs(p, l))
                .map(str::to_string)
                .collect()
        };
        (
            p.live.as_ref().map(|l| marked(&l.text)).unwrap_or_default(),
            marked(&p.copy.text),
        )
    }

    /// The per-key chooser's picks for the selected pair.
    pub fn picks(&self) -> &[Pick] {
        &self.picks
    }

    /// The Retire confirmation, when open.
    pub fn retire(&self) -> Option<&RetireView> {
        self.retire.as_ref()
    }

    /// The last refused decision's message.
    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    pub(super) fn fail(&mut self, message: String) {
        self.error = Some(message);
    }

    fn select(&mut self, i: Option<usize>) {
        self.selected = i;
        self.picks = vec![Pick::Live; self.selected().map_or(0, |p| p.keys.len())];
    }

    fn key_of(&self, i: usize) -> (String, PathBuf) {
        let p = &self.report.items[i];
        (p.name.clone(), p.copy.file.clone())
    }

    /// Takes a fresh report, keeping the selection on the same pair when it
    /// is still undecided, else on the pair now at its place in the list.
    fn update(&mut self, report: ReconcileReport) {
        let before = self
            .selected
            .map(|i| (self.key_of(i), self.report.items[i].kind));
        let at = self
            .selected
            .and_then(|s| self.order().iter().position(|&i| i == s))
            .unwrap_or(0);
        self.report = report;
        let order = self.order();
        let same = before.as_ref().and_then(|((n, c), kind)| {
            order.iter().copied().find(|&i| {
                let p = &self.report.items[i];
                p.answers_to(n) && p.copy.file == *c && p.kind == *kind && !self.is_kept(p)
            })
        });
        match same {
            Some(i) if self.report.items[i].keys.len() == self.picks.len() => {
                self.selected = Some(i);
            }
            Some(i) => self.select(Some(i)),
            None => self.select(order.get(at.min(order.len().saturating_sub(1))).copied()),
        }
    }
}

/// True when `line` of one of `p`'s blocks sets a key whose values differ.
fn differs(p: &Pair, line: &str) -> bool {
    let t = line.trim();
    let key = if t.starts_with('#') {
        parse_meta_line(t).map(|(k, _)| k.name().to_string())
    } else {
        t.split(|c: char| c.is_whitespace() || c == '=')
            .next()
            .filter(|w| !w.is_empty() && !w.eq_ignore_ascii_case("host"))
            .map(canonical_key)
    };
    key.is_some_and(|k| p.keys.iter().any(|d| d.key.eq_ignore_ascii_case(&k)))
}

fn decision(p: &Pair, decision: Decision) -> PairDecision {
    PairDecision {
        name: p.name.clone(),
        copy: p.copy.file.clone(),
        live_text: p.live.as_ref().map(|l| l.text.clone()),
        copy_text: p.copy.text.clone(),
        decision,
    }
}

/// The undecided orphans of file `i`.
fn orphans_in(ws: &Workspace, i: usize, view: &ConflictsView) -> Vec<Pair> {
    ws.reconcile_report(Some(&[i]))
        .items
        .into_iter()
        .filter(|p| p.kind == PairKind::Orphan && !view.is_kept(p))
        .collect()
}

/// What a click in the dialog asks for, applied after drawing.
enum Action {
    Select(usize),
    Pick(usize, Pick),
    KeepLive,
    TakeCopy,
    ApplyKeys,
    Skip,
    DropIdentical,
    AskRetire,
    CancelRetire,
    Retire,
    AddOrphan(usize),
    LeaveOut(usize),
    Close,
}

impl App {
    /// Pairs in the reconcile report: the count the sidebar's Conflicts…
    /// button shows.
    pub fn conflict_count(&self) -> usize {
        self.conflict_total
    }

    /// The open Conflicts dialog.
    pub fn conflicts(&self) -> Option<&ConflictsView> {
        self.conflicts.as_ref()
    }

    /// Opens the Conflicts dialog, as the sidebar button does: not while
    /// the editor holds unsaved text, nor when no host is duplicated.
    pub fn open_conflicts(&mut self) {
        if self.editor_dirty() {
            return;
        }
        let report = self.ws.reconcile_report(None);
        if !report.items.is_empty() {
            self.conflicts = Some(ConflictsView::new(report));
        }
    }

    /// Re-reports after a write or a refusal: the sidebar count, the
    /// dialog's list and an open Retire's blockers. The dialog closes when
    /// no pair remains.
    pub(super) fn rereport(&mut self) {
        let report = self.ws.reconcile_report(None);
        self.conflict_total = report.items.len();
        self.conflict_summary = report.summary();
        if report.items.is_empty() {
            self.conflicts = None;
        }
        let ws = &self.ws;
        let Some(view) = &mut self.conflicts else {
            return;
        };
        view.update(report);
        let Some(r) = &view.retire else {
            return;
        };
        match (0..ws.files.len()).find(|&i| ws.abs(i) == r.file) {
            None => view.retire = None,
            Some(i) if r.refused => {
                let blockers = ws.retire_blockers(i, &view.decided());
                let orphans = orphans_in(ws, i, view);
                if let Some(r) = &mut view.retire {
                    r.blockers = blockers;
                    r.orphans = orphans;
                }
            }
            Some(_) => {}
        }
    }

    /// Runs one reconcile write; `keep` joins the pairs kept live when it
    /// succeeds. A refusal (a pair changed since the report) re-reports.
    fn decide(
        &mut self,
        decisions: Vec<PairDecision>,
        scope: Option<PathBuf>,
        keep: Vec<(String, PathBuf)>,
    ) -> bool {
        let Some(view) = &self.conflicts else {
            return false;
        };
        let mut kept = view.kept.clone();
        kept.extend(keep);
        let ok = self.run(Op::Reconcile {
            decisions,
            scope,
            kept: kept.clone(),
        });
        if let Some(view) = &mut self.conflicts {
            if ok {
                view.kept = kept;
                view.error = None;
            }
        }
        self.rereport();
        ok
    }

    pub(super) fn conflicts_dialog(&mut self, ctx: &egui::Context) {
        if self.conflicts.is_none() {
            return;
        }
        let modal = egui::Modal::new(egui::Id::new("conflicts"));
        let mut action = None;
        let response = modal.show(ctx, |ui| {
            if let Some(view) = &self.conflicts {
                action = self.draw_conflicts(ui, view);
            }
        });
        if action.is_none() && response.should_close() && self.dialog.is_none() {
            action = Some(Action::Close);
        }
        if let Some(a) = action {
            self.conflict_action(a);
        }
    }

    fn draw_conflicts(&self, ui: &mut Ui, view: &ConflictsView) -> Option<Action> {
        let mut action = None;
        let blocked = self.editor_dirty();
        let weak = ui.visuals().weak_text_color();
        ui.set_width(DIALOG_W);
        ui.heading("Conflicts");
        let kept = view
            .report
            .items
            .iter()
            .filter(|p| p.kind == PairKind::Conflict && view.is_kept(p))
            .count();
        let mut summary = view.report.summary();
        if kept > 0 {
            summary.push_str(&format!("; {kept} kept live"));
        }
        ui.label(summary);
        if let Some(e) = &view.error {
            ui.colored_label(ui.visuals().error_fg_color, format!("⚠ {e}"));
        }
        ui.add_space(4.0);
        let widths = [200.0, 250.0, 250.0, 170.0];
        ui.horizontal(|ui| {
            for (w, title) in widths.iter().zip(["Host", "Live", "Copy", "Kind"]) {
                ui.add_sized([*w, ROW_H], Label::new(RichText::new(title).strong()));
            }
        });
        egui::ScrollArea::vertical()
            .id_salt("conflicts-list")
            .max_height(200.0)
            .auto_shrink([false, true])
            .show(ui, |ui| {
                for i in view.order() {
                    let p = &view.report.items[i];
                    ui.horizontal(|ui| {
                        let resp = ui.add_sized(
                            [widths[0], ROW_H],
                            Button::selectable(view.selected == Some(i), &p.name),
                        );
                        set_label(ui, resp.id, &format!("{} in {}", p.name, p.copy.label));
                        if resp.clicked() {
                            action = Some(Action::Select(i));
                        }
                        let live = p.live.as_ref().map_or("—".to_string(), |l| l.label.clone());
                        ui.add_sized([widths[1], ROW_H], Label::new(live).truncate());
                        ui.add_sized([widths[2], ROW_H], Label::new(&p.copy.label).truncate());
                        let kind = view.kind_text(p);
                        let text = if p.kind == PairKind::Conflict && !view.is_kept(p) {
                            RichText::new(kind).color(ui.visuals().warn_fg_color)
                        } else {
                            RichText::new(kind).color(weak)
                        };
                        ui.add_sized([widths[3], ROW_H], Label::new(text));
                    });
                }
            });
        ui.separator();
        if let Some(p) = view.selected() {
            self.draw_pair(ui, view, p, blocked, &mut action);
        }
        ui.separator();
        match &view.retire {
            Some(r) => draw_retire(ui, r, blocked, &mut action),
            None => {
                ui.horizontal(|ui| {
                    let selected = view.selected();
                    let file_identical = selected.is_some_and(|s| {
                        view.report
                            .items
                            .iter()
                            .any(|p| p.kind == PairKind::Identical && p.copy.file == s.copy.file)
                    });
                    let hover = selected.map_or(String::new(), |s| {
                        format!("Remove every identical copy from {}", s.copy.label)
                    });
                    if ui
                        .add_enabled(!blocked && file_identical, Button::new("Drop identical"))
                        .on_hover_text(hover)
                        .clicked()
                    {
                        action = Some(Action::DropIdentical);
                    }
                    if let Some(s) = selected {
                        let name = s
                            .copy
                            .file
                            .file_name()
                            .map_or(String::new(), |n| n.to_string_lossy().to_string());
                        if ui
                            .add_enabled(
                                !blocked && s.copy.index != 0,
                                Button::new(format!("Retire {name}")),
                            )
                            .on_hover_text(format!(
                                "Move {} to ~/.ssh/retired/ once every host in it is resolved",
                                s.copy.label
                            ))
                            .clicked()
                        {
                            action = Some(Action::AskRetire);
                        }
                    }
                    if ui.button("Close").clicked() {
                        action = Some(Action::Close);
                    }
                });
            }
        }
        action
    }

    /// The selected pair: both blocks side by side, live on the left, the
    /// differing lines highlighted; the decision buttons; and for a
    /// conflict the per-key chooser.
    fn draw_pair(
        &self,
        ui: &mut Ui,
        view: &ConflictsView,
        p: &Pair,
        blocked: bool,
        action: &mut Option<Action>,
    ) {
        let weak = ui.visuals().weak_text_color();
        let mark = Color32::from_rgba_unmultiplied(230, 160, 0, 60);
        if let Some(note) = &p.note {
            ui.label(RichText::new(format!("note: {note}")).color(weak));
        }
        ui.horizontal_top(|ui| {
            let sides = [("live", p.live.as_ref()), ("copy", Some(&p.copy))];
            for (side, block) in sides {
                ui.vertical(|ui| {
                    ui.set_width(SIDE_W);
                    let Some(b) = block else {
                        ui.label(RichText::new("live  (no other file defines it)").strong());
                        return;
                    };
                    ui.label(RichText::new(format!("{side}  {}:{}", b.label, b.line)).strong());
                    egui::ScrollArea::vertical()
                        .id_salt(("conflicts-side", side))
                        .max_height(200.0)
                        .auto_shrink([false, true])
                        .show(ui, |ui| {
                            for line in b.text.lines() {
                                let mut text = RichText::new(line).monospace();
                                if differs(p, line) {
                                    text = text.background_color(mark);
                                }
                                ui.label(text);
                            }
                        });
                });
            }
        });
        ui.add_space(4.0);
        if p.kind == PairKind::Conflict {
            ui.label(RichText::new("Per key").strong());
            for (k, (diff, pick)) in p.keys.iter().zip(&view.picks).enumerate() {
                ui.horizontal(|ui| {
                    let values = |v: &[String]| {
                        if v.is_empty() {
                            "(unset)".to_string()
                        } else {
                            v.join("; ")
                        }
                    };
                    ui.add_sized([150.0, ROW_H], Label::new(&diff.key).truncate());
                    ui.add_sized(
                        [250.0, ROW_H],
                        Label::new(RichText::new(values(&diff.live)).monospace()).truncate(),
                    );
                    ui.add_sized(
                        [250.0, ROW_H],
                        Label::new(RichText::new(values(&diff.copy)).monospace()).truncate(),
                    );
                    for (word, value) in [("live", Pick::Live), ("copy", Pick::Copy)] {
                        let resp =
                            ui.add_enabled(!blocked, Button::selectable(*pick == value, word));
                        set_label(ui, resp.id, &format!("{} {word}", diff.key));
                        if resp.clicked() {
                            *action = Some(Action::Pick(k, value));
                        }
                    }
                });
            }
        } else {
            ui.label(
                RichText::new("identical: Drop identical removes the copy from its file")
                    .color(weak),
            );
        }
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            let conflict = p.kind == PairKind::Conflict;
            if conflict {
                if ui
                    .add_enabled(!blocked, Button::new("Keep live"))
                    .on_hover_text(format!(
                        "Keep {}; write nothing",
                        p.live.as_ref().map_or("", |l| l.label.as_str())
                    ))
                    .clicked()
                {
                    *action = Some(Action::KeepLive);
                }
                if ui
                    .add_enabled(!blocked, Button::new("Take copy"))
                    .on_hover_text(
                        "The live block takes the copy's settings; its Host line and place stay",
                    )
                    .clicked()
                {
                    *action = Some(Action::TakeCopy);
                }
                if ui
                    .add_enabled(!blocked, Button::new("Apply keys"))
                    .on_hover_text("Take the keys marked copy; keep the rest")
                    .clicked()
                {
                    *action = Some(Action::ApplyKeys);
                }
            }
            if ui.button("Skip").clicked() {
                *action = Some(Action::Skip);
            }
        });
    }

    fn conflict_action(&mut self, action: Action) {
        let Some(view) = &mut self.conflicts else {
            return;
        };
        let pair = view.selected().cloned();
        let one = |d: Decision| pair.as_ref().map(|p| vec![decision(p, d)]);
        let key = pair.as_ref().map(|p| (p.name.clone(), p.copy.file.clone()));
        match action {
            Action::Select(i) => {
                view.select(Some(i));
                view.error = None;
            }
            Action::Pick(k, pick) => {
                if let Some(p) = view.picks.get_mut(k) {
                    *p = pick;
                }
            }
            Action::Skip => {
                let order = view.order();
                let at = view
                    .selected
                    .and_then(|s| order.iter().position(|&i| i == s))
                    .map_or(0, |a| (a + 1) % order.len());
                view.select(order.get(at).copied());
                view.error = None;
            }
            Action::KeepLive => {
                if let (Some(d), Some(k)) = (one(Decision::KeepLive), key) {
                    self.decide(d, None, vec![k]);
                }
            }
            Action::TakeCopy => {
                if let Some(d) = one(Decision::TakeCopy) {
                    self.decide(d, None, Vec::new());
                }
            }
            Action::ApplyKeys => {
                let Some(p) = &pair else {
                    return;
                };
                let picks: Vec<KeyPick> = p
                    .keys
                    .iter()
                    .zip(&view.picks)
                    .map(|(k, pick)| KeyPick {
                        key: k.key.clone(),
                        pick: *pick,
                    })
                    .collect();
                let keep = if view.picks.contains(&Pick::Live) {
                    key.into_iter().collect()
                } else {
                    Vec::new()
                };
                let d = vec![decision(p, Decision::Keys(picks))];
                self.decide(d, None, keep);
            }
            Action::DropIdentical => {
                let Some(p) = &pair else {
                    return;
                };
                let d: Vec<PairDecision> = view
                    .report
                    .items
                    .iter()
                    .filter(|q| q.kind == PairKind::Identical && q.copy.file == p.copy.file)
                    .map(|q| decision(q, Decision::DropIdentical))
                    .collect();
                self.decide(d, None, Vec::new());
            }
            Action::AskRetire => {
                if let Some(p) = &pair {
                    view.retire = Some(RetireView {
                        file: p.copy.file.clone(),
                        label: p.copy.label.clone(),
                        refused: false,
                        blockers: Vec::new(),
                        orphans: Vec::new(),
                    });
                }
            }
            Action::CancelRetire => view.retire = None,
            Action::Retire => {
                let Some(r) = &mut view.retire else {
                    return;
                };
                let file = r.file.clone();
                let decided = view.decided();
                if !self.run(Op::Retire { file, decided }) {
                    if let Some(r) = self.conflicts.as_mut().and_then(|v| v.retire.as_mut()) {
                        r.refused = true;
                    }
                }
                self.rereport();
            }
            Action::AddOrphan(i) | Action::LeaveOut(i) => {
                let add = matches!(action, Action::AddOrphan(_));
                let Some(r) = &view.retire else {
                    return;
                };
                let Some(o) = r.orphans.get(i).cloned() else {
                    return;
                };
                let o = &o;
                let scope = Some(r.file.clone());
                if add {
                    self.decide(vec![decision(o, Decision::Add)], scope, Vec::new());
                } else {
                    let keep = vec![(o.name.clone(), o.copy.file.clone())];
                    self.decide(vec![decision(o, Decision::KeepLive)], scope, keep);
                }
            }
            Action::Close => self.conflicts = None,
        }
    }
}

/// The Retire confirmation; once refused, what blocks it, with Add and
/// Leave out on each orphan.
fn draw_retire(ui: &mut Ui, r: &RetireView, blocked: bool, action: &mut Option<Action>) {
    ui.heading(format!("Retire {}?", r.label));
    ui.label("It moves to ~/.ssh/retired/ and leaves the Include glob. No file is deleted.");
    if r.refused {
        if r.blockers.is_empty() {
            ui.label("Nothing blocks it now.");
        } else {
            ui.colored_label(
                ui.visuals().error_fg_color,
                format!("Not retired; undecided: {}.", r.blockers.join(", ")),
            );
            for b in &r.blockers {
                ui.label(RichText::new(format!("• {b}")).monospace());
            }
        }
        for (i, o) in r.orphans.iter().enumerate() {
            ui.horizontal(|ui| {
                ui.add_sized([200.0, ROW_H], Label::new(format!("orphan {}", o.name)));
                let add = ui.add_enabled(!blocked, Button::new("Add"));
                set_label(ui, add.id, &format!("Add {}", o.name));
                if add.clicked() {
                    *action = Some(Action::AddOrphan(i));
                }
                let out = ui.add_enabled(!blocked, Button::new("Leave out"));
                set_label(ui, out.id, &format!("Leave out {}", o.name));
                if out.clicked() {
                    *action = Some(Action::LeaveOut(i));
                }
            });
        }
    }
    ui.horizontal(|ui| {
        if ui.button("Cancel").clicked() {
            *action = Some(Action::CancelRetire);
        }
        if ui.add_enabled(!blocked, Button::new("Retire")).clicked() {
            *action = Some(Action::Retire);
        }
    });
}
