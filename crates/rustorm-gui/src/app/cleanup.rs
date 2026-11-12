//! The Cleanup dialog: every duplicate host in the workspace, probed on a
//! background thread, applied through the reconcile engine (docs/cli.md,
//! cleanup).

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::sync::Arc;

use egui::{Button, Color32, Label, RichText, Ui};
use rustorm_core::{
    run_probes, Action as Fix, CleanupReport, Definition, FindingKind, ProbeResult, ProbeTarget,
    Prober, SshProber, TcpProber, DEFAULT_JOBS, DEFAULT_TIMEOUT,
};

use super::{set_label, App};
use crate::ops::Op;

const DIALOG_W: f32 = 900.0;
const ROW_H: f32 = 20.0;

/// A probe run in flight: its targets, the channel its worker thread
/// sends results on, and the flag that stops it.
struct ProbeRun {
    targets: Vec<ProbeTarget>,
    pending: Vec<bool>,
    rx: Receiver<(usize, ProbeResult)>,
    cancel: Arc<AtomicBool>,
}

impl std::fmt::Debug for ProbeRun {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProbeRun")
            .field("targets", &self.targets.len())
            .field("pending", &self.pending.iter().filter(|p| **p).count())
            .finish_non_exhaustive()
    }
}

impl Drop for ProbeRun {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::SeqCst);
    }
}

/// A probe result kept across re-reports, keyed by the block it probed.
#[derive(Debug, Clone)]
struct Known {
    file: PathBuf,
    name: String,
    text: String,
    result: ProbeResult,
}

/// The open Cleanup dialog.
#[derive(Debug)]
pub struct CleanupView {
    report: CleanupReport,
    selected: Option<usize>,
    auth: bool,
    run: Option<ProbeRun>,
    known: Vec<Known>,
    /// The messages of the last write.
    messages: Vec<String>,
    error: Option<String>,
}

impl CleanupView {
    fn new(report: CleanupReport) -> CleanupView {
        let selected = (!report.findings.is_empty()).then_some(0);
        CleanupView {
            report,
            selected,
            auth: false,
            run: None,
            known: Vec::new(),
            messages: Vec::new(),
            error: None,
        }
    }

    /// The report the dialog shows, with every probe result recorded so
    /// far.
    pub fn report(&self) -> &CleanupReport {
        &self.report
    }

    /// The selected finding's index.
    pub fn selected(&self) -> Option<usize> {
        self.selected
    }

    /// True while probes are running.
    pub fn probing(&self) -> bool {
        self.run.is_some()
    }

    /// (probes finished, probes started) of the run in flight.
    pub fn progress(&self) -> Option<(usize, usize)> {
        self.run.as_ref().map(|r| {
            let total = r.targets.len();
            (total - r.pending.iter().filter(|p| **p).count(), total)
        })
    }

    /// True when Log in with ssh is checked.
    pub fn auth(&self) -> bool {
        self.auth
    }

    /// The messages of the last write: one per change, then the count of
    /// findings that remain.
    pub fn messages(&self) -> &[String] {
        &self.messages
    }

    /// The last refused action's message.
    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    /// The status `d` shows: `probing…` while its probe runs, else its
    /// status label (`works`, `not probed`, ...).
    pub fn status_text(&self, d: &Definition) -> &'static str {
        let pending = self.run.as_ref().is_some_and(|r| {
            r.targets.iter().zip(&r.pending).any(|(t, p)| {
                *p && t.file == d.block.file && t.line == d.block.line && t.name == d.name
            })
        });
        if pending {
            "probing…"
        } else {
            d.status_label()
        }
    }

    pub(super) fn fail(&mut self, message: String) {
        self.error = Some(message);
    }

    /// Takes the results that landed since the last frame. True when any
    /// did.
    fn drain(&mut self) -> bool {
        let Some(run) = &mut self.run else {
            return false;
        };
        let mut landed = Vec::new();
        while let Ok((i, result)) = run.rx.try_recv() {
            if let Some(p) = run.pending.get_mut(i) {
                *p = false;
            }
            landed.push((run.targets[i].clone(), result));
        }
        let finished = run.pending.iter().all(|p| !p);
        for (target, result) in &landed {
            self.report.record(target, result);
            let text = self
                .report
                .findings
                .iter()
                .flat_map(|f| &f.definitions)
                .find(|d| {
                    d.block.file == target.file
                        && d.block.line == target.line
                        && d.name == target.name
                })
                .map(|d| d.block.text.clone())
                .unwrap_or_default();
            self.known
                .retain(|k| !(k.file == target.file && k.name == target.name && k.text == text));
            self.known.push(Known {
                file: target.file.clone(),
                name: target.name.clone(),
                text,
                result: result.clone(),
            });
        }
        if finished {
            self.run = None;
        }
        !landed.is_empty()
    }

    /// Takes a fresh report and records on it every result already known
    /// for an unchanged block.
    fn update(&mut self, report: CleanupReport) {
        let names = self
            .selected
            .and_then(|i| self.report.findings.get(i))
            .map(|f| (f.kind, f.names.clone()));
        self.report = report;
        let mut hits = Vec::new();
        for f in &self.report.findings {
            for d in &f.definitions {
                if let Some(k) = self
                    .known
                    .iter()
                    .find(|k| k.file == d.block.file && k.name == d.name && k.text == d.block.text)
                {
                    hits.push((target_of(d), k.result.clone()));
                }
            }
        }
        for (t, r) in &hits {
            self.report.record(t, r);
        }
        let n = self.report.findings.len();
        self.selected = names
            .and_then(|(kind, names)| {
                self.report
                    .findings
                    .iter()
                    .position(|f| f.kind == kind && f.names == names)
            })
            .or_else(|| (n > 0).then(|| self.selected.unwrap_or(0).min(n - 1)));
    }
}

/// The probe target a definition is probed through.
fn target_of(d: &Definition) -> ProbeTarget {
    ProbeTarget {
        name: d.name.clone(),
        file: d.block.file.clone(),
        line: d.block.line,
        endpoint: d.endpoint.clone(),
        config: d.probe_config.clone(),
        timeout: DEFAULT_TIMEOUT,
    }
}

/// The core's recommendation in the dialog's words: buttons, not flags.
pub fn recommendation_text(text: &str) -> String {
    let text = match text.find(" with --fold ") {
        Some(i) => &text[..i],
        None => text,
    };
    text.replace(
        "decide with rustorm reconcile",
        "decide in the Conflicts… dialog",
    )
    .replace(
        "probe with --auth to decide",
        "probe with Log in with ssh to decide",
    )
}

fn status_color(ui: &Ui, text: &str) -> Color32 {
    match text {
        "works" => {
            if ui.visuals().dark_mode {
                Color32::from_rgb(0x6c, 0xd0, 0x7a)
            } else {
                Color32::from_rgb(0x1a, 0x7f, 0x37)
            }
        }
        "probing…" | "not probed" | "needs --auth" => ui.visuals().weak_text_color(),
        _ => ui.visuals().error_fg_color,
    }
}

/// What a click in the dialog asks for, applied after drawing.
enum Action {
    Select(usize),
    Auth(bool),
    Probe,
    Apply,
    ApplyAll,
    Fold,
    Close,
}

impl App {
    /// Findings in the cleanup report: the count the sidebar's Cleanup…
    /// button shows.
    pub fn cleanup_count(&self) -> usize {
        self.cleanup_total
    }

    /// The open Cleanup dialog.
    pub fn cleanup(&self) -> Option<&CleanupView> {
        self.cleanup.as_ref()
    }

    /// Uses `prober` for every Cleanup probe, with or without Log in with
    /// ssh, in place of the TCP connect and the ssh login. Tests and the
    /// parity crate inject a deterministic one.
    pub fn set_prober(&mut self, prober: Arc<dyn Prober>) {
        self.prober = Some(prober);
    }

    /// [`App::set_prober`] as a builder.
    pub fn with_prober(mut self, prober: Arc<dyn Prober>) -> App {
        self.set_prober(prober);
        self
    }

    /// Opens the Cleanup dialog, as the sidebar button does: not while the
    /// editor holds unsaved text. With no duplicates it reads `no
    /// duplicates in N hosts.` and every action is disabled.
    pub fn open_cleanup(&mut self) {
        if self.editor_dirty() {
            return;
        }
        let report = self.ws.cleanup_report(None, &self.env);
        self.cleanup = Some(CleanupView::new(report));
    }

    /// Selects finding `i` of the dialog.
    pub fn cleanup_select(&mut self, i: usize) {
        self.cleanup_action(Action::Select(i));
    }

    /// Checks or unchecks Log in with ssh (`--auth`).
    pub fn cleanup_set_auth(&mut self, auth: bool) {
        self.cleanup_action(Action::Auth(auth));
    }

    /// Starts probing every definition on a background thread, as Probe
    /// does. Results land as they finish; [`App::cleanup_poll`] or the
    /// next frame records them.
    pub fn cleanup_probe(&mut self) {
        self.cleanup_action(Action::Probe);
    }

    /// Records the probe results that landed. True when any did.
    pub fn cleanup_poll(&mut self) -> bool {
        self.cleanup.as_mut().is_some_and(CleanupView::drain)
    }

    /// Applies the selected finding's recommendation, as Apply does.
    pub fn cleanup_apply(&mut self) -> bool {
        self.cleanup_action(Action::Apply)
    }

    /// Applies every recommendation except folds, as Apply all does.
    pub fn cleanup_apply_all(&mut self) -> bool {
        self.cleanup_action(Action::ApplyAll)
    }

    /// Folds the selected same-target finding, as Fold does.
    pub fn cleanup_fold(&mut self) -> bool {
        self.cleanup_action(Action::Fold)
    }

    /// Closes the dialog, as Close and Esc do: running probes are
    /// abandoned and nothing is written.
    pub fn close_cleanup(&mut self) {
        self.cleanup = None;
    }

    pub(super) fn cleanup_dialog(&mut self, ctx: &egui::Context) {
        let Some(view) = &mut self.cleanup else {
            return;
        };
        view.drain();
        let modal = egui::Modal::new(egui::Id::new("cleanup"));
        let mut action = None;
        let response = modal.show(ctx, |ui| {
            if let Some(view) = &self.cleanup {
                action = self.draw_cleanup(ui, view);
            }
        });
        if action.is_none() && response.should_close() && self.dialog.is_none() {
            action = Some(Action::Close);
        }
        if let Some(a) = action {
            self.cleanup_action(a);
        }
    }

    fn draw_cleanup(&self, ui: &mut Ui, view: &CleanupView) -> Option<Action> {
        let mut action = None;
        let blocked = self.editor_dirty();
        let weak = ui.visuals().weak_text_color();
        ui.set_width(DIALOG_W);
        ui.heading("Clean up duplicates");
        ui.label(view.report.summary());
        for m in &view.messages {
            ui.label(m);
        }
        if let Some(e) = &view.error {
            ui.colored_label(ui.visuals().error_fg_color, format!("⚠ {e}"));
        }
        ui.add_space(4.0);
        let widths = [110.0, 260.0, 500.0];
        if !view.report.findings.is_empty() {
            ui.horizontal(|ui| {
                for (w, title) in widths.iter().zip(["Kind", "Hosts", "Recommendation"]) {
                    ui.add_sized([*w, ROW_H], Label::new(RichText::new(title).strong()));
                }
            });
        }
        egui::ScrollArea::vertical()
            .id_salt("cleanup-list")
            .max_height(180.0)
            .min_scrolled_height(180.0)
            .auto_shrink([false, true])
            .show(ui, |ui| {
                ui.set_min_height(180.0);
                for (i, f) in view.report.findings.iter().enumerate() {
                    ui.horizontal(|ui| {
                        ui.add_sized([widths[0], ROW_H], Label::new(f.kind.label()));
                        let resp = ui.add_sized(
                            [widths[1], ROW_H],
                            Button::selectable(view.selected == Some(i), f.names.join(", ")),
                        );
                        set_label(
                            ui,
                            resp.id,
                            &format!("{}: {}", f.kind.label(), f.names.join(", ")),
                        );
                        if resp.clicked() {
                            action = Some(Action::Select(i));
                        }
                        let text = recommendation_text(&f.recommendation.text);
                        let color = if f.recommendation.action.is_some() {
                            ui.visuals().text_color()
                        } else {
                            weak
                        };
                        ui.add_sized(
                            [widths[2], ROW_H],
                            Label::new(RichText::new(text).color(color)).truncate(),
                        );
                    });
                }
            });
        ui.separator();
        let selected = view.selected.and_then(|i| view.report.findings.get(i));
        egui::ScrollArea::vertical()
            .id_salt("cleanup-detail")
            .max_height(150.0)
            .min_scrolled_height(150.0)
            .auto_shrink([false, true])
            .show(ui, |ui| {
                ui.set_min_height(150.0);
                if let Some(f) = selected {
                    ui.label(RichText::new(f.heading()).strong());
                    egui::Grid::new("cleanup-defs")
                        .num_columns(4)
                        .spacing([16.0, 4.0])
                        .show(ui, |ui| {
                            for d in &f.definitions {
                                let lead = if f.kind == FindingKind::SameTarget {
                                    d.name.clone()
                                } else {
                                    d.role.label().to_string()
                                };
                                ui.label(RichText::new(&lead).monospace());
                                ui.label(
                                    RichText::new(format!("{}:{}", d.block.label, d.block.line))
                                        .monospace(),
                                );
                                ui.label(RichText::new(&d.target).monospace());
                                let status = view.status_text(d);
                                let resp = ui.label(
                                    RichText::new(format!("● {status}"))
                                        .color(status_color(ui, status)),
                                );

                                if let Some(detail) = &d.detail {
                                    resp.on_hover_text(detail);
                                }
                                ui.end_row();
                            }
                        });
                    ui.add_space(4.0);
                    ui.label(format!(
                        "Recommendation: {}.",
                        recommendation_text(&f.recommendation.text)
                    ));
                }
            });
        ui.separator();
        let probing = view.run.is_some();
        let none = view.report.findings.is_empty();
        let applicable = view.report.applicable().len();
        let action_of = selected.and_then(|f| f.recommendation.action);
        let (left, right) = egui::Sides::new().show(
            ui,
            |ui| {
                let mut action = None;
                if ui
                    .add_enabled(!probing && !none, Button::new("Probe"))
                    .on_hover_text(if view.auth {
                        "Log in to each definition with ssh in BatchMode"
                    } else {
                        "Open a TCP connection to each definition's port; no data is sent"
                    })
                    .on_disabled_hover_text(if none {
                        "Nothing to probe"
                    } else {
                        "Probes are running"
                    })
                    .clicked()
                {
                    action = Some(Action::Probe);
                }
                let mut auth = view.auth;
                if ui
                    .add_enabled(!probing, egui::Checkbox::new(&mut auth, "Log in with ssh"))
                    .on_hover_text("Probe with ssh -o BatchMode=yes <name> true, as --auth does")
                    .changed()
                {
                    action = Some(Action::Auth(auth));
                }
                if let Some((done, total)) = view.progress() {
                    ui.add(
                        egui::ProgressBar::new(done as f32 / total.max(1) as f32)
                            .desired_width(160.0)
                            .text(format!("{done} of {total} probed")),
                    );
                }
                action
            },
            |ui| {
                let mut action = None;
                let busy = if blocked {
                    Some("Save or discard the editor first")
                } else if probing {
                    Some("Wait for the probes, or close")
                } else {
                    None
                };
                let can_apply = busy.is_none() && action_of.is_some_and(|a| a != Fix::Fold);
                if ui
                    .add_enabled(can_apply, Button::new("Apply"))
                    .on_hover_text("Apply the selected recommendation after a backup")
                    .on_disabled_hover_text(busy.unwrap_or(match action_of {
                        Some(Fix::Fold) => "A same-target finding folds with Fold",
                        _ => "The selected finding has nothing to apply",
                    }))
                    .clicked()
                {
                    action = Some(Action::Apply);
                }
                if ui
                    .add_enabled(
                        busy.is_none() && applicable > 0,
                        Button::new(format!("Apply all ({applicable})")),
                    )
                    .on_hover_text("Apply every recommendation except folds")
                    .on_disabled_hover_text(
                        busy.unwrap_or("No finding has a recommendation to apply"),
                    )
                    .clicked()
                {
                    action = Some(Action::ApplyAll);
                }
                if ui
                    .add_enabled(
                        busy.is_none() && action_of == Some(Fix::Fold),
                        Button::new("Fold"),
                    )
                    .on_hover_text(
                        "Add the other names to the first name's Host line and remove their blocks",
                    )
                    .on_disabled_hover_text(
                        busy.unwrap_or("Select a same-target finding whose settings match"),
                    )
                    .clicked()
                {
                    action = Some(Action::Fold);
                }
                if ui.button("Close").clicked() {
                    action = Some(Action::Close);
                }
                action
            },
        );
        action.or(left).or(right)
    }

    fn prober_for(&self, auth: bool) -> Arc<dyn Prober> {
        match &self.prober {
            Some(p) => Arc::clone(p),
            None if auth => Arc::new(SshProber::from_env()),
            None => Arc::new(TcpProber),
        }
    }

    /// Runs one cleanup write through [`App::run`] and re-reports.
    fn cleanup_write(&mut self, op: Op) -> bool {
        let ok = self.run(op);
        let report = self.ws.cleanup_report(None, &self.env);
        if let Some(view) = &mut self.cleanup {
            if ok {
                view.messages = vec![self.status.clone()];
                view.error = None;
            }
            view.update(report);
        }
        ok
    }

    fn cleanup_action(&mut self, action: Action) -> bool {
        let blocked = self.editor_dirty();
        let Some(view) = &mut self.cleanup else {
            return false;
        };
        let writes = !blocked && view.run.is_none();
        match action {
            Action::Select(i) => {
                if i < view.report.findings.len() {
                    view.selected = Some(i);
                }
                false
            }
            Action::Auth(auth) => {
                if view.run.is_none() {
                    view.auth = auth;
                }
                false
            }
            Action::Probe => {
                if view.run.is_some() || view.report.findings.is_empty() {
                    return false;
                }
                let targets = view.report.probe_targets(DEFAULT_TIMEOUT);
                let auth = view.auth;
                let prober = self.prober_for(auth);
                let Some(view) = &mut self.cleanup else {
                    return false;
                };
                let (tx, rx) = mpsc::channel();
                let cancel = Arc::new(AtomicBool::new(false));
                let work = targets.clone();
                let stop = Arc::clone(&cancel);
                let ctx = self.ctx.clone();
                std::thread::spawn(move || {
                    run_probes(
                        &work,
                        prober.as_ref(),
                        DEFAULT_JOBS,
                        Some(&stop),
                        &mut |i, r| {
                            if tx.send((i, r)).is_ok() {
                                if let Some(ctx) = &ctx {
                                    ctx.request_repaint();
                                }
                            }
                        },
                    );
                });
                view.error = None;
                view.messages.clear();
                view.run = Some(ProbeRun {
                    pending: vec![true; targets.len()],
                    targets,
                    rx,
                    cancel,
                });
                true
            }
            Action::Apply => {
                let Some(i) = view.selected else {
                    return false;
                };
                let f = &view.report.findings[i];
                if !writes || f.recommendation.action.is_none_or(|a| a == Fix::Fold) {
                    return false;
                }
                let op = Op::Cleanup {
                    report: Box::new(view.report.clone()),
                    findings: vec![i],
                };
                self.cleanup_write(op)
            }
            Action::ApplyAll => {
                let chosen = view.report.applicable();
                if !writes || chosen.is_empty() {
                    return false;
                }
                let op = Op::Cleanup {
                    report: Box::new(view.report.clone()),
                    findings: chosen,
                };
                self.cleanup_write(op)
            }
            Action::Fold => {
                let Some(f) = view.selected.and_then(|i| view.report.findings.get(i)) else {
                    return false;
                };
                if !writes || f.recommendation.action != Some(Fix::Fold) {
                    return false;
                }
                let op = Op::Fold {
                    finding: Box::new(f.clone()),
                    hosts: f.definitions[1..].iter().map(|d| d.name.clone()).collect(),
                };
                self.cleanup_write(op)
            }
            Action::Close => {
                self.cleanup = None;
                false
            }
        }
    }
}
