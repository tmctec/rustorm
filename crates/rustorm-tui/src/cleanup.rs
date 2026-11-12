//! The Cleanup view's state: every duplicate host as the core's `cleanup`
//! report finds it, and the probes running off the UI thread
//! (docs/cli.md, cleanup).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::sync::Arc;
use std::time::Duration;

use rustorm_core::{
    run_probes, CleanupReport, Env, ProbeResult, ProbeTarget, Prober, Workspace, DEFAULT_JOBS,
};

/// The Cleanup view.
#[derive(Debug, Clone)]
pub(crate) struct Cleanup {
    pub report: CleanupReport,
    /// The highlighted finding.
    pub sel: usize,
    /// Every probe result this session, by name and temp config text, so
    /// a finding keeps its statuses after a write moves its lines.
    pub results: Vec<(String, String, ProbeResult)>,
    /// The view's key overlay is open.
    pub help: bool,
}

impl Cleanup {
    /// The view over `ws`, read fresh.
    pub fn new(ws: &Workspace, env: &Env) -> Cleanup {
        Cleanup {
            report: ws.cleanup_report(None, env),
            sel: 0,
            results: Vec::new(),
            help: false,
        }
    }

    /// Reads the findings again from `ws`, keeping each probe result whose
    /// definition is unchanged.
    pub fn reread(&mut self, ws: &Workspace, env: &Env) {
        self.report = ws.cleanup_report(None, env);
        for t in self.report.probe_targets(Duration::from_secs(1)) {
            if let Some((_, _, r)) = self
                .results
                .iter()
                .find(|(n, c, _)| *n == t.name && *c == t.config)
            {
                let r = r.clone();
                self.report.record(&t, &r);
            }
        }
        self.sel = self.sel.min(self.report.findings.len().saturating_sub(1));
    }

    /// Records one probe result.
    pub fn record(&mut self, target: &ProbeTarget, result: &ProbeResult) {
        self.report.record(target, result);
        self.results
            .retain(|(n, c, _)| !(*n == target.name && *c == target.config));
        self.results
            .push((target.name.clone(), target.config.clone(), result.clone()));
    }

    /// Each finding's lines as `rustorm cleanup` prints them, then the
    /// summary as the last chunk.
    pub fn chunks(&self) -> Vec<Vec<String>> {
        self.report
            .text()
            .trim_end()
            .split("\n\n")
            .map(|c| c.lines().map(str::to_string).collect())
            .collect()
    }
}

/// Probes running on a worker thread; results arrive over a channel.
pub(crate) struct ProbeRun {
    rx: Receiver<(ProbeTarget, ProbeResult)>,
    cancel: Arc<AtomicBool>,
    /// Probes started.
    pub total: usize,
    /// Results received.
    pub done: usize,
}

impl ProbeRun {
    /// Starts probing every definition of `report` with `prober`.
    pub fn start(report: &CleanupReport, prober: Arc<dyn Prober>, timeout: Duration) -> ProbeRun {
        let targets = report.probe_targets(timeout);
        let total = targets.len();
        let (tx, rx) = mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let stop = Arc::clone(&cancel);
        std::thread::spawn(move || {
            run_probes(
                &targets,
                prober.as_ref(),
                DEFAULT_JOBS,
                Some(&stop),
                &mut |i, result| {
                    if tx.send((targets[i].clone(), result)).is_err() {
                        stop.store(true, Ordering::SeqCst);
                    }
                },
            );
        });
        ProbeRun {
            rx,
            cancel,
            total,
            done: 0,
        }
    }

    /// The results landed so far, and whether every probe has finished.
    pub fn drain(&mut self) -> (Vec<(ProbeTarget, ProbeResult)>, bool) {
        let mut out = Vec::new();
        loop {
            match self.rx.try_recv() {
                Ok(r) => {
                    self.done += 1;
                    out.push(r);
                }
                Err(TryRecvError::Empty) => return (out, false),
                Err(TryRecvError::Disconnected) => return (out, true),
            }
        }
    }

    /// Starts no further probe; the running ones end within their timeout
    /// and their results are dropped.
    pub fn abandon(self) {
        self.cancel.store(true, Ordering::SeqCst);
    }
}
