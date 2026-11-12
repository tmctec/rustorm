//! `cleanup`: every duplicate host in the workspace, probed (docs/cli.md,
//! cleanup; D34–D37).
//!
//! [`Workspace::cleanup_report`] finds three kinds of [`Finding`]: the
//! same name in two files (the pairs of
//! [`Workspace::reconcile_report`]), a name defined twice in one file (a
//! dead block), and different names that reach the same
//! `user@hostname:port`. Each [`Definition`] carries the temp config it is
//! probed through: its own block and the workspace's `Host *` blocks in
//! ssh's reading order, `Include` lines left out (D35).
//!
//! Probing goes through a [`Prober`]: [`TcpProber`] (a connect that sends
//! no data) or [`SshProber`] (a `BatchMode` login). [`run_probes`] runs
//! them in parallel and hands each result to a callback as it lands, so a
//! UI can probe on a worker thread and [`CleanupReport::record`] each
//! result; [`CleanupReport::probe`] does both in one call. Every record
//! recomputes the [`Recommendation`]s (D36).
//!
//! [`Workspace::apply_cleanup`] applies recommendations in memory, the
//! same-name ones through [`Workspace::apply_decisions`];
//! [`Workspace::fold_host`] folds a same-target name (D37). Both refuse a
//! block that changed since the report. Nothing is written until
//! [`Workspace::save`], and no file is ever deleted.

use std::io::Read;
use std::net::{TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::keys::is_multi_valued;
use crate::model::HostBlock;
use crate::ops::Env;
use crate::reconcile::{
    key_diffs, same_definition, BlockRef, Decision, PairKind, ReconcileReport,
};
use crate::workspace::{Change, Workspace, WorkspaceLocation};
use crate::{Error, Result};

/// The default bound on one probe.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(5);

/// How many probes run at once by default.
pub const DEFAULT_JOBS: usize = 8;

/// The kind of a finding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FindingKind {
    /// A host defined in two workspace files: live and copy.
    SameName,
    /// A name defined twice in one file: the first block and a dead one.
    Dead,
    /// Different names that reach the same `user@hostname:port`.
    SameTarget,
}

impl FindingKind {
    /// `same name`, `dead` or `same target`, as the report prints it.
    pub fn label(self) -> &'static str {
        match self {
            FindingKind::SameName => "same name",
            FindingKind::Dead => "dead",
            FindingKind::SameTarget => "same target",
        }
    }
}

/// A definition's part in its finding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    /// The definition ssh reads first (same name).
    Live,
    /// A later definition in another file (same name).
    Copy,
    /// The first block of a name in its file (dead), or the first name in
    /// reading order (same target).
    First,
    /// A later block of the same name in the same file.
    Dead,
    /// Another name with the same target (same target).
    Other,
}

impl Role {
    /// `live`, `copy`, `first`, `dead` or `other`.
    pub fn label(self) -> &'static str {
        match self {
            Role::Live => "live",
            Role::Copy => "copy",
            Role::First => "first",
            Role::Dead => "dead",
            Role::Other => "other",
        }
    }
}

/// What one probe found.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProbeStatus {
    /// The connection opened; with `--auth`, ssh logged in.
    Works,
    /// Nothing listens on the port.
    Refused,
    /// The `HostName` does not resolve.
    DnsFailure,
    /// No answer within the timeout.
    Timeout,
    /// The system has no route to the address.
    Unreachable,
    /// ssh reached the host and the login failed.
    AuthFailure,
    /// `ProxyJump` or `ProxyCommand`: a TCP connect cannot follow it.
    NeedsAuth,
}

impl ProbeStatus {
    /// The status as the report prints it: `works`, `dns failure`,
    /// `needs --auth`, ...
    pub fn label(self) -> &'static str {
        match self {
            ProbeStatus::Works => "works",
            ProbeStatus::Refused => "refused",
            ProbeStatus::DnsFailure => "dns failure",
            ProbeStatus::Timeout => "timeout",
            ProbeStatus::Unreachable => "unreachable",
            ProbeStatus::AuthFailure => "auth failure",
            ProbeStatus::NeedsAuth => "needs --auth",
        }
    }

    /// True for [`ProbeStatus::Works`].
    pub fn works(self) -> bool {
        self == ProbeStatus::Works
    }

    /// True for a status that says the definition does not work: anything
    /// but works and needs --auth.
    pub fn fails(self) -> bool {
        !matches!(self, ProbeStatus::Works | ProbeStatus::NeedsAuth)
    }
}

/// A probe's status and the error text behind it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProbeResult {
    /// What the probe found.
    pub status: ProbeStatus,
    /// The error text (`Connection refused`, ssh's last stderr line), if
    /// any.
    pub detail: Option<String>,
}

impl ProbeResult {
    fn new(status: ProbeStatus, detail: Option<String>) -> ProbeResult {
        ProbeResult { status, detail }
    }
}

/// Where a definition connects, resolved the way ssh resolves it through
/// `Host *`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Endpoint {
    /// `User`, else `$USER` (empty when unknown).
    pub user: String,
    /// `HostName` with `%h` expanded, else the name.
    pub hostname: String,
    /// `Port`, else 22.
    pub port: u16,
    /// `ProxyJump <x>` or `ProxyCommand <x>`, unless `none`.
    pub proxy: Option<String>,
}

impl std::fmt::Display for Endpoint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.hostname.contains(':') {
            write!(f, "{}@[{}]:{}", self.user, self.hostname, self.port)
        } else {
            write!(f, "{}@{}:{}", self.user, self.hostname, self.port)
        }
    }
}

/// One definition of a finding.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Definition {
    /// Its part in the finding.
    pub role: Role,
    /// The name it is probed by.
    pub name: String,
    /// The block and where it is.
    #[serde(flatten)]
    pub block: BlockRef,
    /// `user@hostname:port`.
    pub target: String,
    /// The probe's status; `None` until probed.
    pub status: Option<ProbeStatus>,
    /// The probe's error text.
    pub detail: Option<String>,
    /// The resolved endpoint.
    #[serde(skip)]
    pub endpoint: Endpoint,
    /// The temp config it is probed through (D35).
    #[serde(skip)]
    pub probe_config: String,
}

impl Definition {
    /// The probe status as the report prints it, `not probed` when none.
    pub fn status_label(&self) -> &'static str {
        self.status.map_or("not probed", ProbeStatus::label)
    }
}

/// What applying a finding does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    /// The live block takes the copy (D31); the copy's block leaves its
    /// file.
    TakeCopy,
    /// Keep live; the failing copy's block leaves its file.
    DropCopy,
    /// The identical copy's block leaves its file.
    DropIdentical,
    /// The dead block leaves its file.
    DropDead,
    /// The later names join the first name's `Host` line (`--fold`).
    Fold,
}

/// A finding's recommendation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Recommendation {
    /// What applying does; `None` when nothing is recommended.
    pub action: Option<Action>,
    /// The sentence the report prints, without the final period.
    pub text: String,
}

/// One duplicate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Finding {
    /// Same name, dead or same target.
    pub kind: FindingKind,
    /// The names involved: the shared names, or every same-target name.
    pub names: Vec<String>,
    /// The shared `user@hostname:port` of a same-target finding.
    pub target: Option<String>,
    /// Live and copy, first and dead, or one per same-target name.
    pub definitions: Vec<Definition>,
    /// What to do about it, from the kind and the probe statuses.
    pub recommendation: Recommendation,
    /// The index into [`CleanupReport::pairs`] of a same-name finding.
    #[serde(skip)]
    pub pair: Option<usize>,
    /// Same name: the copy matches (D30).
    #[serde(skip)]
    pub identical: bool,
    /// Dead: why the dead block still matters (keys it adds, names only
    /// it carries); empty when it adds nothing. Same target: the keys
    /// that differ from the first name's block.
    #[serde(skip)]
    pub differs: Vec<String>,
}

impl Finding {
    /// The first line of the finding in the report.
    pub fn heading(&self) -> String {
        let mut h = format!("{}: {}", self.kind.label(), self.names.join(", "));
        if let Some(t) = &self.target {
            h.push_str(&format!(" ({t})"));
        }
        h
    }

    fn recompute(&mut self) {
        self.recommendation = match self.kind {
            FindingKind::SameName => self.recommend_pair(),
            FindingKind::Dead => self.recommend_dead(),
            FindingKind::SameTarget => self.recommend_target(),
        };
    }

    fn recommend_pair(&self) -> Recommendation {
        let (live, copy) = (&self.definitions[0], &self.definitions[1]);
        if self.identical {
            return rec(
                Some(Action::DropIdentical),
                format!("drop the identical copy from {}", copy.block.label),
            );
        }
        let (Some(l), Some(c)) = (live.status, copy.status) else {
            return rec(None, "probe to decide".to_string());
        };
        if l == ProbeStatus::NeedsAuth || c == ProbeStatus::NeedsAuth {
            return rec(None, "probe with --auth to decide".to_string());
        }
        match (l.works(), c.works()) {
            (false, true) => rec(
                Some(Action::TakeCopy),
                "take the copy; only the copy works".to_string(),
            ),
            (true, false) => rec(
                Some(Action::DropCopy),
                format!(
                    "keep live and drop the copy from {}; only live works",
                    copy.block.label
                ),
            ),
            (true, true) => rec(
                None,
                "both work and differ; decide with rustorm reconcile".to_string(),
            ),
            (false, false) => rec(None, "both fail; nothing to recommend".to_string()),
        }
    }

    fn recommend_dead(&self) -> Recommendation {
        let (first, dead) = (&self.definitions[0], &self.definitions[1]);
        if !self.differs.is_empty() {
            return rec(
                None,
                format!(
                    "the dead block adds {}; merge it by hand",
                    self.differs.join(", ")
                ),
            );
        }
        if let (Some(f), Some(d)) = (first.status, dead.status) {
            if f.fails() && d.works() {
                return rec(
                    None,
                    "only the dead block works; merge it by hand".to_string(),
                );
            }
        }
        rec(
            Some(Action::DropDead),
            format!(
                "drop the dead block at {}:{}",
                dead.block.label, dead.block.line
            ),
        )
    }

    fn recommend_target(&self) -> Recommendation {
        if !self.differs.is_empty() {
            return rec(
                None,
                format!("settings differ ({}); left as is", self.differs.join(", ")),
            );
        }
        let first = &self.definitions[0].name;
        let rest: Vec<&str> = self.definitions[1..]
            .iter()
            .map(|d| d.name.as_str())
            .collect();
        let flags: Vec<String> = rest.iter().map(|n| format!("--fold {n}")).collect();
        rec(
            Some(Action::Fold),
            format!(
                "fold {} into {first} with {}",
                rest.join(", "),
                flags.join(" ")
            ),
        )
    }
}

fn rec(action: Option<Action>, text: String) -> Recommendation {
    Recommendation { action, text }
}

/// Every finding in scope.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CleanupReport {
    /// Same-name findings, then dead, then same target.
    pub findings: Vec<Finding>,
    /// Hosts across the workspace, `Host *` excluded.
    pub hosts: usize,
    /// The reconcile pairs the same-name findings come from.
    #[serde(skip)]
    pub pairs: ReconcileReport,
}

fn plural(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

impl CleanupReport {
    /// `3 findings in 6 hosts.` or `no duplicates in 6 hosts.`
    pub fn summary(&self) -> String {
        let hosts = plural(self.hosts, "host", "hosts");
        if self.findings.is_empty() {
            format!("no duplicates in {hosts}.")
        } else {
            format!(
                "{} in {hosts}.",
                plural(self.findings.len(), "finding", "findings")
            )
        }
    }

    /// The text report: each finding's heading, one line per definition,
    /// its recommendation, a blank line between findings, then
    /// [`CleanupReport::summary`]. Ends in `\n`.
    pub fn text(&self) -> String {
        let mut out = String::new();
        for f in &self.findings {
            out.push_str(&finding_text(f));
            out.push('\n');
        }
        out.push_str(&self.summary());
        out.push('\n');
        out
    }

    /// Findings with an action `--apply` takes: everything but folds and
    /// findings without a recommendation.
    pub fn applicable(&self) -> Vec<usize> {
        (0..self.findings.len())
            .filter(|&i| {
                self.findings[i]
                    .recommendation
                    .action
                    .is_some_and(|a| a != Action::Fold)
            })
            .collect()
    }

    /// The distinct probes the report needs, one per block and name.
    pub fn probe_targets(&self, timeout: Duration) -> Vec<ProbeTarget> {
        let mut out: Vec<ProbeTarget> = Vec::new();
        for f in &self.findings {
            for d in &f.definitions {
                let dup = out
                    .iter()
                    .any(|t| t.file == d.block.file && t.line == d.block.line && t.name == d.name);
                if !dup {
                    out.push(ProbeTarget {
                        name: d.name.clone(),
                        file: d.block.file.clone(),
                        line: d.block.line,
                        endpoint: d.endpoint.clone(),
                        config: d.probe_config.clone(),
                        timeout,
                    });
                }
            }
        }
        out
    }

    /// Records `result` on every definition `target` probes, then
    /// recomputes the recommendations.
    pub fn record(&mut self, target: &ProbeTarget, result: &ProbeResult) {
        for f in &mut self.findings {
            let mut hit = false;
            for d in &mut f.definitions {
                if d.block.file == target.file && d.block.line == target.line && d.name == target.name
                {
                    d.status = Some(result.status);
                    d.detail = result.detail.clone();
                    hit = true;
                }
            }
            if hit {
                f.recompute();
            }
        }
    }

    /// Probes every definition with `prober`, `jobs` at a time, recording
    /// each result as it lands and passing it to `on_result`. Stops
    /// starting probes once `cancel` is set.
    pub fn probe(
        &mut self,
        prober: &dyn Prober,
        timeout: Duration,
        jobs: usize,
        cancel: Option<&AtomicBool>,
        on_result: &mut dyn FnMut(&ProbeTarget, &ProbeResult),
    ) {
        let targets = self.probe_targets(timeout);
        run_probes(&targets, prober, jobs, cancel, &mut |i, result| {
            self.record(&targets[i], &result);
            on_result(&targets[i], &result);
        });
    }

    /// The index of the same-target finding holding `host`, for
    /// [`Workspace::fold_host`]. [`Error::Usage`] when none does.
    pub fn fold_finding(&self, host: &str) -> Result<usize> {
        self.findings
            .iter()
            .position(|f| {
                f.kind == FindingKind::SameTarget && f.definitions.iter().any(|d| d.name == host)
            })
            .ok_or_else(|| Error::Usage(format!("{host} is in no same-target finding.")))
    }
}

fn finding_text(f: &Finding) -> String {
    let rows: Vec<Vec<String>> = f
        .definitions
        .iter()
        .map(|d| {
            let lead = if f.kind == FindingKind::SameTarget {
                d.name.clone()
            } else {
                d.role.label().to_string()
            };
            let mut row = vec![lead, format!("{}:{}", d.block.label, d.block.line)];
            if f.kind != FindingKind::SameTarget {
                row.push(d.target.clone());
            }
            row.push(d.status_label().to_string());
            row
        })
        .collect();
    let cols = rows.first().map_or(0, Vec::len);
    let widths: Vec<usize> = (0..cols)
        .map(|c| rows.iter().map(|r| r[c].chars().count()).max().unwrap_or(0))
        .collect();
    let mut out = format!("{}\n", f.heading());
    for r in &rows {
        let cells: Vec<String> = r
            .iter()
            .zip(&widths)
            .map(|(cell, w)| format!("{cell:<w$}"))
            .collect();
        out.push_str(&format!("  {}", cells.join("  ")).trim_end().to_string());
        out.push('\n');
    }
    out.push_str(&format!("  recommendation: {}.\n", f.recommendation.text));
    out
}

// ----- probing -----

/// One probe to run: a block, the name it is probed by, and its temp
/// config.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbeTarget {
    /// The name ssh is given.
    pub name: String,
    /// The absolute path of the block's file.
    pub file: PathBuf,
    /// The block's `Host` line.
    pub line: usize,
    /// Where the block connects.
    pub endpoint: Endpoint,
    /// The temp config text: the block and the workspace's `Host *`.
    pub config: String,
    /// The bound on the probe.
    pub timeout: Duration,
}

/// Probes one definition. Implementations must be thread-safe:
/// [`run_probes`] calls them from several threads at once.
pub trait Prober: Send + Sync {
    /// Probes `target`, within `target.timeout` where the implementation
    /// can bound it.
    fn probe(&self, target: &ProbeTarget) -> ProbeResult;
}

/// The default probe: resolve the `HostName` and open a TCP connection to
/// its port, sending no data. A block with `ProxyJump` or `ProxyCommand`
/// is [`ProbeStatus::NeedsAuth`] without a connection.
#[derive(Debug, Clone, Copy, Default)]
pub struct TcpProber;

impl Prober for TcpProber {
    fn probe(&self, target: &ProbeTarget) -> ProbeResult {
        if let Some(proxy) = &target.endpoint.proxy {
            return ProbeResult::new(ProbeStatus::NeedsAuth, Some(proxy.clone()));
        }
        tcp_probe(&target.endpoint.hostname, target.endpoint.port, target.timeout)
    }
}

/// Resolves `host` on a worker thread bounded by `timeout`, then connects
/// to each address in turn within what is left of it.
pub fn tcp_probe(host: &str, port: u16, timeout: Duration) -> ProbeResult {
    let deadline = Instant::now() + timeout;
    let (tx, rx) = mpsc::channel();
    let name = host.to_string();
    std::thread::spawn(move || {
        let r = (name.as_str(), port)
            .to_socket_addrs()
            .map(|a| a.collect::<Vec<_>>());
        let _ = tx.send(r);
    });
    let addrs = match rx.recv_timeout(timeout) {
        Err(_) => {
            return ProbeResult::new(ProbeStatus::Timeout, Some("name lookup timed out".into()))
        }
        Ok(Err(e)) => return ProbeResult::new(ProbeStatus::DnsFailure, Some(e.to_string())),
        Ok(Ok(a)) if a.is_empty() => {
            return ProbeResult::new(ProbeStatus::DnsFailure, Some("no address".into()))
        }
        Ok(Ok(a)) => a,
    };
    let mut last = ProbeResult::new(ProbeStatus::Timeout, None);
    for addr in addrs {
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return ProbeResult::new(ProbeStatus::Timeout, Some("connect timed out".into()));
        }
        match TcpStream::connect_timeout(&addr, left) {
            Ok(stream) => {
                drop(stream);
                return ProbeResult::new(ProbeStatus::Works, None);
            }
            Err(e) => {
                use std::io::ErrorKind as K;
                let status = match e.kind() {
                    K::ConnectionRefused => ProbeStatus::Refused,
                    K::TimedOut | K::WouldBlock => ProbeStatus::Timeout,
                    _ => ProbeStatus::Unreachable,
                };
                last = ProbeResult::new(status, Some(e.to_string()));
            }
        }
    }
    last
}

/// The `--auth` probe: `ssh -F <temp> -o BatchMode=yes -o
/// ConnectTimeout=<secs> <name> true`, with the temp config written mode
/// 0600 in a private temp directory that is removed afterwards.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SshProber {
    /// The ssh program.
    pub program: PathBuf,
}

/// The environment variable naming the ssh program `--auth` runs.
pub const SSH_ENV: &str = "RUSTORM_SSH";

impl SshProber {
    /// `$RUSTORM_SSH`, else `ssh` from `PATH`.
    pub fn from_env() -> SshProber {
        SshProber {
            program: std::env::var_os(SSH_ENV)
                .filter(|v| !v.is_empty())
                .map_or_else(|| PathBuf::from("ssh"), PathBuf::from),
        }
    }
}

impl Default for SshProber {
    fn default() -> SshProber {
        SshProber::from_env()
    }
}

fn write_private(path: &Path, text: &str) -> std::io::Result<()> {
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    use std::io::Write;
    opts.open(path)?.write_all(text.as_bytes())
}

/// Classifies ssh's exit and stderr: 0 works; 255 a connection error by
/// its message, else an auth failure; any other code means ssh logged in
/// and `true` itself failed, so it works.
pub fn classify_ssh(code: Option<i32>, stderr: &str) -> ProbeResult {
    let last = stderr
        .lines()
        .rev()
        .find(|l| !l.trim().is_empty())
        .map(|l| l.trim().to_string());
    match code {
        Some(0) => ProbeResult::new(ProbeStatus::Works, None),
        Some(255) => {
            let e = stderr.to_ascii_lowercase();
            let status = if e.contains("could not resolve")
                || e.contains("name or service not known")
                || e.contains("nodename nor servname")
            {
                ProbeStatus::DnsFailure
            } else if e.contains("connection refused") {
                ProbeStatus::Refused
            } else if e.contains("timed out") {
                ProbeStatus::Timeout
            } else if e.contains("no route to host") || e.contains("network is unreachable") {
                ProbeStatus::Unreachable
            } else {
                ProbeStatus::AuthFailure
            };
            ProbeResult::new(status, last)
        }
        Some(n) => ProbeResult::new(ProbeStatus::Works, Some(format!("true exited {n}"))),
        None => ProbeResult::new(ProbeStatus::Timeout, last),
    }
}

impl Prober for SshProber {
    fn probe(&self, target: &ProbeTarget) -> ProbeResult {
        let failed = |e: std::io::Error| {
            ProbeResult::new(
                ProbeStatus::Unreachable,
                Some(format!("cannot run {}: {e}", self.program.display())),
            )
        };
        let dir = match tempfile::Builder::new().prefix("rustorm-probe-").tempdir() {
            Ok(d) => d,
            Err(e) => return failed(e),
        };
        let config = dir.path().join("config");
        if let Err(e) = write_private(&config, &target.config) {
            return failed(e);
        }
        let secs = target.timeout.as_secs().max(1);
        let child = Command::new(&self.program)
            .arg("-F")
            .arg(&config)
            .args(["-o", "BatchMode=yes", "-o"])
            .arg(format!("ConnectTimeout={secs}"))
            .arg(&target.name)
            .arg("true")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn();
        let mut child = match child {
            Ok(c) => c,
            Err(e) => return failed(e),
        };
        let mut stderr = child.stderr.take().expect("piped stderr");
        let reader = std::thread::spawn(move || {
            let mut s = String::new();
            let _ = stderr.read_to_string(&mut s);
            s
        });
        // ConnectTimeout bounds the connect; the whole login gets three
        // times that plus two seconds before it is killed.
        let deadline = Instant::now() + Duration::from_secs(secs * 3 + 2);
        let code = loop {
            match child.try_wait() {
                Ok(Some(status)) => break status.code(),
                Ok(None) if Instant::now() >= deadline => {
                    let _ = child.kill();
                    let _ = child.wait();
                    let _ = reader.join();
                    return ProbeResult::new(ProbeStatus::Timeout, Some("ssh timed out".into()));
                }
                Ok(None) => std::thread::sleep(Duration::from_millis(10)),
                Err(e) => return failed(e),
            }
        };
        let text = reader.join().unwrap_or_default();
        drop(dir);
        classify_ssh(code, &text)
    }
}

/// Runs `targets` through `prober`, at most `jobs` at once on scoped
/// threads, and calls `on_result(index, result)` on the calling thread as
/// each result lands. Once `cancel` is set no further probe starts; the
/// running ones finish within their timeout.
pub fn run_probes(
    targets: &[ProbeTarget],
    prober: &dyn Prober,
    jobs: usize,
    cancel: Option<&AtomicBool>,
    on_result: &mut dyn FnMut(usize, ProbeResult),
) {
    if targets.is_empty() {
        return;
    }
    let next = AtomicUsize::new(0);
    let (tx, rx) = mpsc::channel::<(usize, ProbeResult)>();
    let workers = jobs.clamp(1, targets.len());
    std::thread::scope(|scope| {
        for _ in 0..workers {
            let tx = tx.clone();
            let next = &next;
            scope.spawn(move || loop {
                if cancel.is_some_and(|c| c.load(Ordering::SeqCst)) {
                    break;
                }
                let i = next.fetch_add(1, Ordering::SeqCst);
                let Some(t) = targets.get(i) else { break };
                if tx.send((i, prober.probe(t))).is_err() {
                    break;
                }
            });
        }
        drop(tx);
        for (i, result) in rx {
            on_result(i, result);
        }
    });
}

// ----- finding detection -----

fn is_pattern(name: &str) -> bool {
    name.contains(['*', '?', '!'])
}

fn concrete(block: &HostBlock) -> Vec<String> {
    block.names().into_iter().filter(|n| !is_pattern(n)).collect()
}

type Rank = Vec<(usize, usize)>;

/// The blocks a definition is resolved and probed through: the `Host *`
/// blocks ssh reads before it, the block, then the `Host *` blocks after.
fn probe_chain<'a>(
    ws: &'a Workspace,
    at: WorkspaceLocation,
    stars: &[(Rank, WorkspaceLocation)],
    rank: &Rank,
) -> Vec<&'a HostBlock> {
    let mut chain: Vec<&HostBlock> = stars
        .iter()
        .filter(|(r, _)| r < rank)
        .map(|(_, l)| ws.host(*l))
        .collect();
    chain.push(ws.host(at));
    chain.extend(stars.iter().filter(|(r, _)| r > rank).map(|(_, l)| ws.host(*l)));
    chain
}

/// The first value of `key` along `chain`, the way ssh takes it.
fn first_value(chain: &[&HostBlock], key: &str) -> Option<String> {
    chain.iter().find_map(|b| {
        b.directives()
            .into_iter()
            .find(|d| d.key.eq_ignore_ascii_case(key))
            .map(|d| d.value.trim().to_string())
    })
}

fn expand_host(value: &str, name: &str) -> String {
    let mut out = String::new();
    let mut chars = value.chars();
    while let Some(c) = chars.next() {
        if c == '%' {
            match chars.next() {
                Some('h') => out.push_str(name),
                Some('%') => out.push('%'),
                Some(o) => {
                    out.push('%');
                    out.push(o);
                }
                None => out.push('%'),
            }
        } else {
            out.push(c);
        }
    }
    out
}

fn resolve(chain: &[&HostBlock], name: &str, env: &Env) -> Endpoint {
    let hostname = first_value(chain, "HostName").map_or_else(|| name.to_string(), |h| expand_host(&h, name));
    let user = first_value(chain, "User")
        .or_else(|| env.user.clone())
        .unwrap_or_default();
    let port = first_value(chain, "Port")
        .and_then(|p| p.parse().ok())
        .unwrap_or(22);
    let proxy = ["ProxyJump", "ProxyCommand"].iter().find_map(|k| {
        first_value(chain, k)
            .filter(|v| !v.eq_ignore_ascii_case("none"))
            .map(|v| format!("{k} {v}"))
    });
    Endpoint {
        user,
        hostname,
        port,
        proxy,
    }
}

/// The temp config text of `chain`: each block, `Include` lines left out,
/// separated by blank lines (D35).
fn chain_text(chain: &[&HostBlock]) -> String {
    let mut parts = Vec::new();
    for b in chain {
        let mut t = String::new();
        for l in b.lines() {
            if l.directive().is_some_and(|d| d.key.eq_ignore_ascii_case("include")) {
                continue;
            }
            t.push_str(l.text());
            t.push('\n');
        }
        parts.push(t);
    }
    parts.join("\n")
}

/// The keys and names a dead block carries that ssh still reads: a name
/// the first block lacks, a multi-valued line the first block lacks, a
/// single-valued key the first block does not set.
fn dead_adds(first: &HostBlock, dead: &HostBlock) -> Vec<String> {
    let mut out = Vec::new();
    let names = first.names();
    for n in dead.names() {
        if !names.contains(&n) {
            out.push(format!("the name {n}"));
        }
    }
    let have = first.directives();
    for d in dead.directives() {
        let same_line = have.iter().any(|h| {
            h.key.eq_ignore_ascii_case(&d.key)
                && h.value.split_whitespace().eq(d.value.split_whitespace())
        });
        let shadowed =
            !is_multi_valued(&d.key) && have.iter().any(|h| h.key.eq_ignore_ascii_case(&d.key));
        let key = crate::keys::canonical_key(&d.key);
        if !same_line && !shadowed && !out.contains(&key) {
            out.push(key);
        }
    }
    out
}

/// One block a definition comes from, before it is placed in a finding.
struct Block {
    at: WorkspaceLocation,
    rank: Rank,
}

impl Workspace {
    fn definition(
        &self,
        role: Role,
        name: &str,
        at: WorkspaceLocation,
        rank: &Rank,
        stars: &[(Rank, WorkspaceLocation)],
        env: &Env,
    ) -> Definition {
        let chain = probe_chain(self, at, stars, rank);
        let endpoint = resolve(&chain, name, env);
        Definition {
            role,
            name: name.to_string(),
            block: self.block_ref(at),
            target: endpoint.to_string(),
            status: None,
            detail: None,
            probe_config: chain_text(&chain),
            endpoint,
        }
    }

    /// The `cleanup` report (D34): same-name pairs as
    /// [`Workspace::reconcile_report`] pairs them, dead blocks, and
    /// same-target names, unprobed. With `scope`, only the copies, dead
    /// blocks and same-target names in those files. `env.user` is the last
    /// fallback for `User`. Reads only.
    pub fn cleanup_report(&self, scope: Option<&[usize]>, env: &Env) -> CleanupReport {
        let in_scope = |file: usize| scope.is_none_or(|s| s.contains(&file));
        let paths = self.read_paths();
        let mut blocks: Vec<Block> = Vec::new();
        let mut stars: Vec<(Rank, WorkspaceLocation)> = Vec::new();
        for (file, f) in self.files.iter().enumerate() {
            for loc in f.config.host_locations() {
                let mut rank = paths[file].clone();
                rank.push((f.config.host_line(loc), usize::MAX));
                let at = WorkspaceLocation { file, loc };
                if f.config.host(loc).is_defaults() {
                    stars.push((rank, at));
                } else {
                    blocks.push(Block { at, rank });
                }
            }
        }
        let rank_of = |at: WorkspaceLocation| -> Rank {
            blocks
                .iter()
                .find(|b| b.at == at)
                .map(|b| b.rank.clone())
                .unwrap_or_default()
        };
        let mut findings = Vec::new();

        // Same name.
        let mut pairs = self.reconcile_report(scope);
        pairs.items.retain(|p| p.kind != PairKind::Orphan);
        for (i, p) in pairs.items.iter().enumerate() {
            let live = p.live.as_ref().expect("a pair has a live side");
            let name = p.names.first().cloned().unwrap_or_else(|| p.name.clone());
            let defs = vec![
                self.definition(Role::Live, &name, live.location(), &rank_of(live.location()), &stars, env),
                self.definition(Role::Copy, &name, p.copy.location(), &rank_of(p.copy.location()), &stars, env),
            ];
            findings.push(Finding {
                kind: FindingKind::SameName,
                names: p.names.clone(),
                target: None,
                definitions: defs,
                recommendation: rec(None, String::new()),
                pair: Some(i),
                identical: p.kind == PairKind::Identical,
                differs: Vec::new(),
            });
        }

        // Dead: a later block of a name in the same file.
        for (file, f) in self.files.iter().enumerate() {
            if !in_scope(file) {
                continue;
            }
            let locs: Vec<_> = f
                .config
                .host_locations()
                .into_iter()
                .filter(|l| !f.config.host(*l).is_defaults())
                .collect();
            let mut seen: Vec<(String, usize)> = Vec::new();
            let mut dead: Vec<(usize, usize, Vec<String>)> = Vec::new();
            for (k, loc) in locs.iter().enumerate() {
                for n in concrete(f.config.host(*loc)) {
                    match seen.iter().find(|(s, _)| *s == n) {
                        Some((_, first)) => {
                            match dead.iter_mut().find(|(a, b, _)| a == first && *b == k) {
                                Some((_, _, names)) => names.push(n),
                                None => dead.push((*first, k, vec![n])),
                            }
                        }
                        None => seen.push((n, k)),
                    }
                }
            }
            for (a, b, names) in dead {
                let (fa, fb) = (
                    WorkspaceLocation { file, loc: locs[a] },
                    WorkspaceLocation { file, loc: locs[b] },
                );
                let name = names[0].clone();
                findings.push(Finding {
                    kind: FindingKind::Dead,
                    names,
                    target: None,
                    definitions: vec![
                        self.definition(Role::First, &name, fa, &rank_of(fa), &stars, env),
                        self.definition(Role::Dead, &name, fb, &rank_of(fb), &stars, env),
                    ],
                    recommendation: rec(None, String::new()),
                    pair: None,
                    identical: false,
                    differs: dead_adds(self.host(fa), self.host(fb)),
                });
            }
        }

        // Same target: one representative block per set of names that
        // share a name, grouped by endpoint (proxy included).
        let mut ordered: Vec<&Block> = blocks.iter().collect();
        ordered.sort_by(|a, b| a.rank.cmp(&b.rank));
        let mut reps: Vec<(&Block, Vec<String>, Endpoint)> = Vec::new();
        for b in ordered {
            let names = concrete(self.host(b.at));
            if names.is_empty() {
                continue;
            }
            if reps.iter().any(|(_, ns, _)| ns.iter().any(|n| names.contains(n))) {
                continue;
            }
            let chain = probe_chain(self, b.at, &stars, &b.rank);
            let endpoint = resolve(&chain, &names[0], env);
            reps.push((b, names, endpoint));
        }
        let key = |e: &Endpoint| (e.user.clone(), e.hostname.to_ascii_lowercase(), e.port, e.proxy.clone());
        let mut grouped: Vec<usize> = Vec::new();
        for i in 0..reps.len() {
            if grouped.contains(&i) {
                continue;
            }
            let group: Vec<usize> = (i..reps.len())
                .filter(|&j| !grouped.contains(&j) && key(&reps[j].2) == key(&reps[i].2))
                .collect();
            grouped.extend(&group);
            if group.len() < 2 || !group.iter().any(|&j| in_scope(reps[j].0.at.file)) {
                continue;
            }
            let first = self.host(reps[group[0]].0.at);
            let mut differs: Vec<String> = Vec::new();
            for &j in &group[1..] {
                let other = self.host(reps[j].0.at);
                if !same_definition(first, other) {
                    for k in key_diffs(first, other) {
                        if !differs.contains(&k.key) {
                            differs.push(k.key);
                        }
                    }
                }
            }
            let defs: Vec<Definition> = group
                .iter()
                .enumerate()
                .map(|(n, &j)| {
                    let (b, names, _) = &reps[j];
                    let role = if n == 0 { Role::First } else { Role::Other };
                    self.definition(role, &names[0], b.at, &b.rank, &stars, env)
                })
                .collect();
            findings.push(Finding {
                kind: FindingKind::SameTarget,
                names: defs.iter().map(|d| d.name.clone()).collect(),
                target: Some(defs[0].target.clone()),
                definitions: defs,
                recommendation: rec(None, String::new()),
                pair: None,
                identical: false,
                differs,
            });
        }
        for f in &mut findings {
            f.recompute();
        }
        CleanupReport {
            findings,
            hosts: self.host_count(),
            pairs,
        }
    }

    fn check_current(&self, f: &Finding) -> Result<()> {
        for d in &f.definitions {
            if d.block.index >= self.files.len() || !self.is_current(&d.block) {
                return Err(Error::CleanupStale(f.names.join(", ")));
            }
        }
        Ok(())
    }

    /// Applies the recommendation of each finding in `findings` (indices
    /// into `report.findings`) in memory: dead blocks leave their files,
    /// then the same-name actions go through
    /// [`Workspace::apply_decisions`] (take the copy and drop it, drop the
    /// copy, drop the identical copy). A block that changed since the
    /// report is [`Error::CleanupStale`]; a finding without an action, or
    /// a fold, is [`Error::Usage`]. Every check runs before anything
    /// changes. Messages name each change. Returns the findings applied.
    pub fn apply_cleanup(
        &mut self,
        report: &CleanupReport,
        findings: &[usize],
    ) -> Result<Change<Vec<usize>>> {
        let mut chosen: Vec<usize> = findings.to_vec();
        chosen.sort_unstable();
        chosen.dedup();
        for &i in &chosen {
            let f = report.findings.get(i).ok_or_else(|| {
                Error::Usage(format!(
                    "no cleanup finding {i}; the report has {}.",
                    report.findings.len()
                ))
            })?;
            match f.recommendation.action {
                None => {
                    return Err(Error::Usage(format!(
                        "{} has nothing to apply: {}.",
                        f.names.join(", "),
                        f.recommendation.text
                    )))
                }
                Some(Action::Fold) => {
                    return Err(Error::Usage(format!(
                        "{} folds with --fold, not --apply.",
                        f.names.join(", ")
                    )))
                }
                Some(_) => {}
            }
            self.check_current(f)?;
        }
        let mut messages = Vec::new();
        let mut files: Vec<usize> = Vec::new();
        // Dead blocks: from the end of each part so earlier indices hold.
        let mut dead: Vec<&Definition> = chosen
            .iter()
            .filter(|&&i| report.findings[i].recommendation.action == Some(Action::DropDead))
            .map(|&i| &report.findings[i].definitions[1])
            .collect();
        dead.sort_by(|a, b| {
            (b.block.index, b.block.loc.section, b.block.loc.index).cmp(&(
                a.block.index,
                a.block.loc.section,
                a.block.loc.index,
            ))
        });
        dead.dedup_by(|a, b| a.block.location() == b.block.location());
        for d in dead {
            self.files[d.block.index].config.remove_host(d.block.loc);
            if !files.contains(&d.block.index) {
                files.push(d.block.index);
            }
        }
        // Same name, against the pairs as they are after the removals.
        let fresh = self.reconcile_report(None);
        let mut decisions: Vec<(usize, Decision)> = Vec::new();
        for &i in &chosen {
            let f = &report.findings[i];
            let Some(p) = f.pair else { continue };
            let old = &report.pairs.items[p];
            let live = old.live.as_ref().expect("live side");
            let j = fresh
                .items
                .iter()
                .position(|q| {
                    q.live.as_ref().is_some_and(|l| l.index == live.index && l.text == live.text)
                        && q.copy.index == old.copy.index
                        && q.copy.text == old.copy.text
                })
                .ok_or_else(|| Error::CleanupStale(f.names.join(", ")))?;
            match f.recommendation.action {
                Some(Action::TakeCopy) => {
                    decisions.push((j, Decision::TakeCopy));
                    decisions.push((j, Decision::DropCopy));
                }
                Some(Action::DropCopy) => decisions.push((j, Decision::DropCopy)),
                Some(Action::DropIdentical) => decisions.push((j, Decision::DropIdentical)),
                _ => {}
            }
        }
        if !decisions.is_empty() {
            let change = self
                .apply_decisions(&fresh, &decisions, None)
                .map_err(|e| match e {
                    Error::ReconcileStale(h) => Error::CleanupStale(h),
                    other => other,
                })?;
            for f in change.files {
                if !files.contains(&f) {
                    files.push(f);
                }
            }
        }
        for &i in &chosen {
            let f = &report.findings[i];
            let name = f.names.join(", ");
            let (a, b) = (&f.definitions[0], &f.definitions[1]);
            let msg = match f.recommendation.action {
                Some(Action::TakeCopy) => format!(
                    "{name}: moved the copy from {} into {}; only the copy works.",
                    b.block.label, a.block.label
                ),
                Some(Action::DropCopy) => format!(
                    "{name}: kept {} and dropped the copy from {}; only live works.",
                    a.block.label, b.block.label
                ),
                Some(Action::DropIdentical) => {
                    format!("{name}: dropped the identical copy from {}.", b.block.label)
                }
                Some(Action::DropDead) => format!(
                    "{name}: dropped the dead block at {}:{}.",
                    b.block.label, b.block.line
                ),
                _ => continue,
            };
            messages.push(msg);
        }
        Ok(self.change(chosen, files, messages, Vec::new()))
    }

    /// Folds `host` (D37): its names join the `Host` line of the first
    /// other name in its same-target finding of `report`, and its block,
    /// leading comments included, leaves its file. [`Error::Usage`] when
    /// no same-target finding holds `host`; [`Error::Undecidable`] (exit 2)
    /// when the directives or the metadata lines differ;
    /// [`Error::CleanupStale`] when either block changed since the report.
    /// Returns the name folded into.
    pub fn fold_host(&mut self, report: &CleanupReport, host: &str) -> Result<Change<String>> {
        let f = &report.findings[report.fold_finding(host)?];
        let from = f
            .definitions
            .iter()
            .find(|d| d.name == host)
            .expect("fold_finding found it");
        let into = f
            .definitions
            .iter()
            .find(|d| d.name != host)
            .expect("a same-target finding has two names");
        for d in [from, into] {
            if d.block.index >= self.files.len() || !self.is_current(&d.block) {
                return Err(Error::CleanupStale(host.to_string()));
            }
        }
        let (a, b) = (self.host(into.block.location()), self.host(from.block.location()));
        if !same_definition(a, b) {
            let keys: Vec<String> = key_diffs(a, b).into_iter().map(|k| k.key).collect();
            return Err(Error::Undecidable {
                host: host.to_string(),
                reason: format!(
                    "differs from {} in {}; not folded",
                    into.name,
                    crate::error::join_and(&keys)
                ),
            });
        }
        let mut names = a.names();
        for n in b.names() {
            if !names.contains(&n) {
                names.push(n);
            }
        }
        self.files[into.block.index]
            .config
            .host_mut(into.block.loc)
            .set_names(&names);
        self.files[from.block.index]
            .config
            .remove_host(from.block.loc);
        let mut files = vec![into.block.index];
        if from.block.index != into.block.index {
            files.push(from.block.index);
        }
        let msg = format!("{host} folded into {} in {}.", into.name, into.block.label);
        Ok(self.change(into.name.clone(), files, vec![msg], Vec::new()))
    }
}

/// `no findings remain.`, `1 finding remains.` or `N findings remain.`
pub fn remaining_findings(n: usize) -> String {
    match n {
        0 => "no findings remain.".to_string(),
        1 => "1 finding remains.".to_string(),
        n => format!("{n} findings remain."),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expand_host_replaces_h_and_percent() {
        assert_eq!(expand_host("%h.example.com", "web"), "web.example.com");
        assert_eq!(expand_host("a%%b", "x"), "a%b");
    }

    #[test]
    fn classify_ssh_reads_exit_and_stderr() {
        assert_eq!(classify_ssh(Some(0), "").status, ProbeStatus::Works);
        assert_eq!(
            classify_ssh(Some(255), "x@y: Permission denied (publickey).\n").status,
            ProbeStatus::AuthFailure
        );
        assert_eq!(
            classify_ssh(Some(255), "ssh: connect to host y port 22: Connection refused\n").status,
            ProbeStatus::Refused
        );
        assert_eq!(
            classify_ssh(Some(255), "ssh: Could not resolve hostname y.invalid\n").status,
            ProbeStatus::DnsFailure
        );
        assert_eq!(
            classify_ssh(Some(255), "ssh: connect to host y port 22: Operation timed out\n").status,
            ProbeStatus::Timeout
        );
        assert_eq!(classify_ssh(Some(1), "").status, ProbeStatus::Works);
    }

    #[test]
    fn dead_adds_names_keys_and_multi_values() {
        let c = crate::Config::parse(
            "Host a\n    HostName x\n    IdentityFile ~/k\n\nHost a\n    HostName y\n    IdentityFile ~/k\n\nHost a b\n    Port 2\n    IdentityFile ~/j\n",
        )
        .unwrap();
        let h = c.hosts();
        assert!(dead_adds(h[0], h[1]).is_empty());
        assert_eq!(dead_adds(h[0], h[2]), ["the name b", "Port", "IdentityFile"]);
    }
}
