//! `combine`: folds several parsed config files into the first one.
//!
//! The first input is the base and keeps its order. Every later input is
//! walked part by part (preamble, then each section): its hosts are placed
//! by the rules in docs/cli.md's `combine` section, its `Host *` merges key
//! by key into the base's, and its free lines and `Match` blocks are
//! appended to the part they were in. [`combine`] and [`combine_with`]
//! write nothing; the caller renders the returned [`Config`].
//!
//! The UIs merge in three steps: [`CombineRequest::preview`] renders the
//! result and every conflict with both blocks, the user decides each
//! conflict into [`CombineRequest::decisions`], and
//! [`CombineRequest::commit`] reads the inputs again, refuses any that
//! changed since the preview, writes the output after its backup and, with
//! [`CombineRequest::retire`], moves each merged input an `Include` still
//! loads to `~/.ssh/retired/` (D32, D33).

use std::collections::{BTreeMap, HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::include::{glob_match, resolve_include};
use crate::io::{absolute_path, write_text, ConfigFile, WriteOptions};
use crate::model::{split_patterns, Config, Entry, HostBlock, HostLocation, Line};
use crate::reconcile::{move_to_retired, retired_destination};
use crate::{keys, Error, Result};

/// What `combine` does with a host name that two inputs define.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum OnConflict {
    /// Report every duplicate and write nothing.
    #[default]
    Fail,
    /// Keep the earlier file's block, drop the later one.
    Keep,
    /// Put the later file's block in the earlier block's place.
    Replace,
}

impl std::str::FromStr for OnConflict {
    type Err = Error;

    fn from_str(s: &str) -> Result<OnConflict> {
        match s {
            "fail" => Ok(OnConflict::Fail),
            "keep" => Ok(OnConflict::Keep),
            "replace" => Ok(OnConflict::Replace),
            other => Err(Error::Usage(format!(
                "{other} is not a conflict policy; use fail, keep or replace."
            ))),
        }
    }
}

/// The decision for one conflict, by `--keep HOST` or `--replace HOST`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HostDecision {
    /// Keep the earlier block byte for byte; drop the later one.
    Keep,
    /// Put the later block in the earlier block's place and section.
    Replace,
}

/// Per-host decisions: a conflict's name (or any name on either block's
/// `Host` line) and what to do with it. Consulted before the policy.
pub type HostDecisions = BTreeMap<String, HostDecision>;

/// Builds [`HostDecisions`] from `--keep` and `--replace` lists. A host
/// named twice is [`Error::Usage`] (`nas is decided twice.`).
pub fn host_decisions(keep: &[String], replace: &[String]) -> Result<HostDecisions> {
    let mut out = HostDecisions::new();
    let named = keep
        .iter()
        .map(|h| (h, HostDecision::Keep))
        .chain(replace.iter().map(|h| (h, HostDecision::Replace)));
    for (host, decision) in named {
        if out.insert(host.clone(), decision).is_some() {
            return Err(Error::Usage(format!("{host} is decided twice.")));
        }
    }
    Ok(out)
}

/// One input file: the path it was named by and its parsed text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CombineInput {
    /// The path as given on the command line.
    pub path: PathBuf,
    /// The parsed file.
    pub config: Config,
}

/// A host name that two inputs define.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Conflict {
    /// The duplicated name (primary or alias).
    pub name: String,
    /// The earlier file.
    pub first: PathBuf,
    /// The later file.
    pub second: PathBuf,
}

/// A `Host *` key the base lacked and gained from a later file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DefaultsAdded {
    /// The key, in canonical case.
    pub key: String,
    /// The value added.
    pub value: String,
    /// The file it came from.
    pub from: PathBuf,
}

/// A `Host *` key the base already had; the later value was skipped.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DefaultsSkipped {
    /// The key, in canonical case.
    pub key: String,
    /// The value skipped.
    pub value: String,
    /// The file it came from.
    pub from: PathBuf,
    /// The base's value(s), comma-joined when the key is multi-valued.
    pub kept: String,
}

/// An `Include` in an input that still matches another input.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct IncludeWarning {
    /// The pattern as written on the `Include` line.
    pub pattern: String,
    /// The file the line is in.
    pub file: PathBuf,
    /// The input the pattern matches.
    pub loads: PathBuf,
}

/// Everything `combine` has to say about a merge.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct CombineReport {
    /// Number of input files.
    pub files: usize,
    /// Hosts in the result, `Host *` excluded.
    pub hosts: usize,
    /// Sections in the result.
    pub sections: usize,
    /// Conflicts resolved by `keep` or `replace`, sorted by name.
    pub conflicts: Vec<Conflict>,
    /// `Host *` keys added to the base.
    pub added: Vec<DefaultsAdded>,
    /// `Host *` keys skipped because the base had them.
    pub skipped: Vec<DefaultsSkipped>,
    /// `Include` lines that still load an input.
    pub includes: Vec<IncludeWarning>,
}

impl CombineReport {
    /// `combined N files into OUTPUT: H hosts, S sections, C conflicts.`, or
    /// without `into OUTPUT` when `output` is `None` (`--stdout`).
    pub fn summary(&self, output: Option<&Path>) -> String {
        let into = output.map_or(String::new(), |p| format!(" into {}", p.display()));
        format!(
            "combined {}{into}: {}, {}, {}.",
            plural(self.files, "file"),
            plural(self.hosts, "host"),
            plural(self.sections, "section"),
            plural(self.conflicts.len(), "conflict")
        )
    }
}

fn plural(n: usize, word: &str) -> String {
    if n == 1 {
        format!("{n} {word}")
    } else {
        format!("{n} {word}s")
    }
}

fn conflict_lines(conflicts: &[Conflict]) -> String {
    conflicts
        .iter()
        .map(|c| {
            format!(
                "\n  {}: {}, {}",
                c.name,
                c.first.display(),
                c.second.display()
            )
        })
        .collect()
}

/// The message of [`Error::CombineConflicts`]: one headline, then one
/// indented `name: first, second` line per conflict.
pub(crate) fn conflicts_message(conflicts: &[Conflict]) -> String {
    let n = conflicts.len();
    format!(
        "{} {} defined more than once; nothing written. Use --on-conflict keep or replace.{}",
        plural(n, "host"),
        if n == 1 { "is" } else { "are" },
        conflict_lines(conflicts)
    )
}

/// The message of [`Error::CombineUndecided`]: the conflicts no
/// `--keep`/`--replace` names, one indented line each.
pub(crate) fn undecided_message(conflicts: &[Conflict]) -> String {
    let n = conflicts.len();
    format!(
        "{} {} left undecided; nothing written. Name {} with --keep or --replace, or use --on-conflict keep or replace.{}",
        plural(n, "conflict"),
        if n == 1 { "is" } else { "are" },
        if n == 1 { "it" } else { "them" },
        conflict_lines(conflicts)
    )
}

fn absolute(path: &Path) -> String {
    if path.is_absolute() {
        path.display().to_string()
    } else {
        std::env::current_dir()
            .map(|d| d.join(path))
            .unwrap_or_else(|_| path.to_path_buf())
            .display()
            .to_string()
    }
}

/// Every `Include` pattern in `config`, as written. ssh honors `Include`
/// at top level and inside `Host`/`Match` blocks, so every line counts.
fn include_patterns(config: &Config) -> Vec<String> {
    config
        .lines()
        .filter_map(Line::directive)
        .filter(|d| d.key.eq_ignore_ascii_case("include"))
        .flat_map(|d| split_patterns(&d.value))
        .collect()
}

fn include_warnings(inputs: &[CombineInput], home: Option<&Path>) -> Vec<IncludeWarning> {
    let mut out = Vec::new();
    for (i, input) in inputs.iter().enumerate() {
        for pattern in include_patterns(&input.config) {
            let Some(resolved) = resolve_include(&pattern, home) else {
                continue;
            };
            for (j, other) in inputs.iter().enumerate() {
                if i != j && glob_match(&resolved, &absolute(&other.path)) {
                    out.push(IncludeWarning {
                        pattern: pattern.clone(),
                        file: input.path.clone(),
                        loads: other.path.clone(),
                    });
                }
            }
        }
    }
    out
}

/// Appends free lines (comments, `Include`, global directives) to a part,
/// separated from what precedes them by one blank line and from a following
/// banner by another.
fn append_lines(result: &mut Config, part: Option<usize>, lines: Vec<Line>) {
    if lines.is_empty() {
        return;
    }
    result.ensure_trailing_newline();
    let followed_by_banner = match part {
        None => result.has_sections(),
        Some(i) => i + 1 < result.sections.len(),
    };
    let entries = result.entries_mut(part);
    if entries.last().is_some_and(|e| !e.is_blank_line()) {
        entries.push(Entry::Line(Line::blank()));
    }
    for mut line in lines {
        line.ensure_terminated();
        entries.push(Entry::Line(line));
    }
    if followed_by_banner {
        entries.push(Entry::Line(Line::blank()));
    }
}

struct Merger<'a> {
    result: Config,
    policy: OnConflict,
    decisions: &'a HostDecisions,
    /// Decision keys that named a conflict.
    used: HashSet<String>,
    /// Every name in the result so far and the file that defined it.
    owners: HashMap<String, PathBuf>,
    report: CombineReport,
    /// Every conflict with both blocks and its decision, in merge order.
    previews: Vec<PreviewConflict>,
}

impl Merger<'_> {
    fn defaults_location(&self) -> Option<HostLocation> {
        self.result
            .host_locations()
            .into_iter()
            .find(|l| self.result.host(*l).is_defaults())
    }

    fn merge_defaults(&mut self, block: HostBlock, part: Option<usize>, from: &Path) {
        let Some(loc) = self.defaults_location() else {
            let mut block = block;
            block.last_line_mut().ensure_terminated();
            self.result.insert_host(part, block);
            return;
        };
        let had: Vec<String> = self
            .result
            .host(loc)
            .directives()
            .into_iter()
            .map(|d| d.key.to_ascii_lowercase())
            .collect();
        for d in block.directives() {
            let key = keys::canonical_key(&d.key);
            if had.contains(&d.key.to_ascii_lowercase()) {
                let kept = self.result.host(loc).get_all(&d.key).join(", ");
                self.report.skipped.push(DefaultsSkipped {
                    key,
                    value: d.value,
                    from: from.to_path_buf(),
                    kept,
                });
            } else {
                self.result.host_mut(loc).append(&key, &d.value);
                self.report.added.push(DefaultsAdded {
                    key,
                    value: d.value,
                    from: from.to_path_buf(),
                });
            }
        }
    }

    fn merge_host(&mut self, mut block: HostBlock, part: Option<usize>, from: &Path) {
        block.last_line_mut().ensure_terminated();
        let names = block.names();
        let taken = names
            .iter()
            .find_map(|n| self.owners.get(n).map(|owner| (n.clone(), owner.clone())));
        if let Some((name, owner)) = taken {
            let loc = self
                .result
                .find_host(&name)
                .expect("owner is in the result");
            let earlier = self.result.host(loc);
            let explicit = names
                .iter()
                .chain(earlier.names().iter())
                .find_map(|n| self.decisions.get_key_value(n))
                .map(|(k, d)| (k.clone(), *d));
            if let Some((key, _)) = &explicit {
                self.used.insert(key.clone());
            }
            let decision = explicit.as_ref().map(|(_, d)| *d).or(match self.policy {
                OnConflict::Fail => None,
                OnConflict::Keep => Some(HostDecision::Keep),
                OnConflict::Replace => Some(HostDecision::Replace),
            });
            self.previews.push(PreviewConflict {
                name: name.clone(),
                first: owner.clone(),
                second: from.to_path_buf(),
                first_text: earlier.text(),
                second_text: block.text(),
                decision,
                explicit: explicit.is_some(),
            });
            self.report.conflicts.push(Conflict {
                name: name.clone(),
                first: owner,
                second: from.to_path_buf(),
            });
            match decision {
                None | Some(HostDecision::Keep) => {}
                Some(HostDecision::Replace) => {
                    for old in self.result.host(loc).names() {
                        self.owners.remove(&old);
                    }
                    for n in &names {
                        self.owners.insert(n.clone(), from.to_path_buf());
                    }
                    *self.result.host_mut(loc) = block;
                }
            }
            return;
        }
        let target = if self.result.has_sections() {
            part.or_else(|| self.result.catch_all())
        } else {
            part
        };
        self.result.insert_host(target, block);
        for n in names {
            self.owners.insert(n, from.to_path_buf());
        }
    }

    /// Folds the entries of one part of a later file into `part` of the
    /// result (`None` = preamble).
    fn merge_part(&mut self, entries: Vec<Entry>, part: Option<usize>, from: &Path) {
        let mut lines: Vec<Line> = Vec::new();
        for entry in entries {
            match entry {
                Entry::Host(h) if h.is_defaults() => self.merge_defaults(h, part, from),
                Entry::Host(h) => self.merge_host(h, part, from),
                Entry::Match(mut m) => {
                    append_lines(&mut self.result, part, std::mem::take(&mut lines));
                    self.result.ensure_trailing_newline();
                    m.body
                        .last_mut()
                        .unwrap_or(&mut m.header)
                        .ensure_terminated();
                    let followed = match part {
                        None => self.result.has_sections(),
                        Some(i) => i + 1 < self.result.sections.len(),
                    };
                    let target = self.result.entries_mut(part);
                    if target.last().is_some_and(|e| !e.is_blank_line()) {
                        target.push(Entry::Line(Line::blank()));
                    }
                    target.push(Entry::Match(m));
                    if followed {
                        target.push(Entry::Line(Line::blank()));
                    }
                }
                Entry::Line(l) if !l.is_blank() => lines.push(l),
                Entry::Line(_) => {}
            }
        }
        append_lines(&mut self.result, part, lines);
    }

    fn merge_file(&mut self, input: CombineInput) {
        let CombineInput { path, config } = input;
        self.merge_part(config.preamble, None, &path);
        for section in config.sections {
            let idx = self.result.ensure_section(section.name());
            self.merge_part(section.entries, Some(idx), &path);
        }
    }
}

/// Merges `inputs` (two or more) into a copy of the first one.
///
/// `home` resolves `~/` and relative `Include` patterns for the warnings.
/// Under [`OnConflict::Fail`] any duplicate name is
/// [`Error::CombineConflicts`], listing every duplicate; under `keep` and
/// `replace` the conflicts are resolved and listed in the report.
pub fn combine(
    inputs: Vec<CombineInput>,
    policy: OnConflict,
    home: Option<&Path>,
) -> Result<(Config, CombineReport)> {
    combine_with(inputs, policy, &HostDecisions::new(), home)
}

/// [`combine`] with per-host decisions consulted before `policy` (D32).
///
/// A decision key that names no conflict is [`Error::Usage`], checked
/// first. Under [`OnConflict::Fail`] the conflicts no decision names are
/// [`Error::CombineConflicts`] when `decisions` is empty, else
/// [`Error::CombineUndecided`].
pub fn combine_with(
    inputs: Vec<CombineInput>,
    policy: OnConflict,
    decisions: &HostDecisions,
    home: Option<&Path>,
) -> Result<(Config, CombineReport)> {
    let merged = merge(inputs, policy, decisions, home)?;
    let undecided: Vec<Conflict> = merged
        .previews
        .iter()
        .filter(|p| p.decision.is_none())
        .map(PreviewConflict::conflict)
        .collect();
    if !undecided.is_empty() {
        let undecided = sorted(undecided);
        return Err(if decisions.is_empty() {
            Error::CombineConflicts(undecided)
        } else {
            Error::CombineUndecided(undecided)
        });
    }
    Ok((merged.result, merged.report))
}

fn sorted(mut conflicts: Vec<Conflict>) -> Vec<Conflict> {
    conflicts.sort_by(|a, b| a.name.cmp(&b.name).then(a.second.cmp(&b.second)));
    conflicts
}

struct Merged {
    result: Config,
    report: CombineReport,
    previews: Vec<PreviewConflict>,
}

/// The merge itself: never fails on a conflict. An undecided conflict
/// keeps the earlier block, as `keep` would.
fn merge(
    inputs: Vec<CombineInput>,
    policy: OnConflict,
    decisions: &HostDecisions,
    home: Option<&Path>,
) -> Result<Merged> {
    if inputs.len() < 2 {
        return Err(Error::Usage(
            "combine needs at least two files.".to_string(),
        ));
    }
    let includes = include_warnings(&inputs, home);
    let files = inputs.len();
    let mut inputs = inputs.into_iter();
    let base = inputs.next().expect("two or more inputs");
    let mut owners = HashMap::new();
    for h in base.config.hosts() {
        if !h.is_defaults() {
            for n in h.names() {
                owners.entry(n).or_insert_with(|| base.path.clone());
            }
        }
    }
    let mut merger = Merger {
        result: base.config,
        policy,
        decisions,
        used: HashSet::new(),
        owners,
        report: CombineReport {
            files,
            includes,
            ..CombineReport::default()
        },
        previews: Vec::new(),
    };
    for input in inputs {
        merger.merge_file(input);
    }
    let Merger {
        mut result,
        mut report,
        used,
        previews,
        ..
    } = merger;
    if let Some(unused) = decisions.keys().find(|k| !used.contains(*k)) {
        return Err(Error::Usage(format!(
            "{unused} is not a conflict; --keep and --replace name a host two files define."
        )));
    }
    report.conflicts = sorted(report.conflicts);
    result.sort_sections();
    report.hosts = result.host_count();
    report.sections = result.sections.len();
    Ok(Merged {
        result,
        report,
        previews,
    })
}

/// A conflict as a preview shows it: both blocks and the decision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PreviewConflict {
    /// The duplicated name (primary or alias).
    pub name: String,
    /// The file the earlier block came from.
    pub first: PathBuf,
    /// The later file.
    pub second: PathBuf,
    /// The earlier block verbatim, leading comments included.
    pub first_text: String,
    /// The later block verbatim.
    pub second_text: String,
    /// The decision in effect: a per-host decision, else the policy's;
    /// `None` when undecided under `fail`.
    pub decision: Option<HostDecision>,
    /// True when a per-host decision made it.
    pub explicit: bool,
}

impl PreviewConflict {
    /// The conflict without its blocks.
    pub fn conflict(&self) -> Conflict {
        Conflict {
            name: self.name.clone(),
            first: self.first.clone(),
            second: self.second.clone(),
        }
    }
}

/// An input as read at preview time, to refuse a commit after it changed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Fingerprint {
    /// The input's path as given.
    pub path: PathBuf,
    /// The length of its text in bytes.
    pub len: usize,
    /// A hash of its text.
    pub hash: u64,
}

impl Fingerprint {
    fn of(path: &Path, text: &str) -> Fingerprint {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        text.hash(&mut h);
        Fingerprint {
            path: path.to_path_buf(),
            len: text.len(),
            hash: h.finish(),
        }
    }
}

/// What a combine would write, before anything is written.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CombinePreview {
    /// The merged text; an undecided conflict keeps the earlier block.
    pub text: String,
    /// Counts, `Host *` keys added and skipped, `Include` warnings and
    /// every conflict.
    pub report: CombineReport,
    /// Every conflict with both blocks, in merge order.
    pub conflicts: Vec<PreviewConflict>,
    /// One per input, in order, for [`CombineRequest::commit`].
    pub fingerprints: Vec<Fingerprint>,
    /// The later inputs `--retire` would move (an `Include` still loads
    /// them), in input order.
    pub retirable: Vec<PathBuf>,
}

impl CombinePreview {
    /// Conflicts without a decision.
    pub fn undecided(&self) -> Vec<&PreviewConflict> {
        self.conflicts
            .iter()
            .filter(|c| c.decision.is_none())
            .collect()
    }

    /// True when every conflict has a decision, so a commit can write.
    pub fn is_ready(&self) -> bool {
        self.conflicts.iter().all(|c| c.decision.is_some())
    }
}

/// What [`CombineRequest::commit`] did with one later input under
/// `--retire`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CombineRetired {
    /// The input as given.
    pub from: PathBuf,
    /// Where it moved; `None` when no `Include` loads it, or it holds the
    /// result, and it stayed.
    pub to: Option<PathBuf>,
}

impl CombineRetired {
    /// `<from> retired to <to>.` or `<from>: no Include loads it; not
    /// retired.`
    pub fn message(&self) -> String {
        match &self.to {
            Some(to) => format!("{} retired to {}.", self.from.display(), to.display()),
            None => format!("{}: no Include loads it; not retired.", self.from.display()),
        }
    }
}

/// What [`CombineRequest::commit`] wrote.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CombineOutcome {
    /// The text written.
    pub text: String,
    /// The merge report.
    pub report: CombineReport,
    /// The file written.
    pub output: PathBuf,
    /// One entry per later input with [`CombineRequest::retire`], else
    /// empty.
    pub retired: Vec<CombineRetired>,
}

/// Reads each input. A missing input is [`Error::Read`] (not found); an
/// unreadable one is [`Error::Read`] too.
pub fn load_inputs(paths: &[PathBuf]) -> Result<Vec<(CombineInput, Fingerprint)>> {
    let mut out = Vec::with_capacity(paths.len());
    for path in paths {
        let file = ConfigFile::load(path)?;
        if !file.existed {
            return Err(Error::Read {
                path: path.clone(),
                source: std::io::Error::from(std::io::ErrorKind::NotFound),
            });
        }
        let fp = Fingerprint::of(path, &file.original);
        out.push((
            CombineInput {
                path: path.clone(),
                config: file.config,
            },
            fp,
        ));
    }
    Ok(out)
}

/// One `combine` run: what the CLI flags or a UI's choices say.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CombineRequest {
    /// The files, base first; two or more.
    pub inputs: Vec<PathBuf>,
    /// Where the result goes; `None` writes the base.
    pub output: Option<PathBuf>,
    /// What an undecided conflict does.
    pub policy: OnConflict,
    /// Per-host decisions, consulted before the policy.
    pub decisions: HostDecisions,
    /// Move each later input an `Include` still loads to
    /// `~/.ssh/retired/` after the write.
    pub retire: bool,
}

impl CombineRequest {
    /// The file the result is written to.
    pub fn output_path(&self) -> Option<&Path> {
        self.output
            .as_deref()
            .or(self.inputs.first().map(PathBuf::as_path))
    }

    /// Reads the inputs and renders the merge without writing. Undecided
    /// conflicts are listed, never an error; a decision naming no conflict
    /// is [`Error::Usage`].
    pub fn preview(&self, home: Option<&Path>) -> Result<CombinePreview> {
        if self.inputs.len() < 2 {
            return Err(Error::Usage(
                "combine needs at least two files.".to_string(),
            ));
        }
        let (inputs, fingerprints): (Vec<_>, Vec<_>) =
            load_inputs(&self.inputs)?.into_iter().unzip();
        let merged = merge(inputs, self.policy, &self.decisions, home)?;
        let retirable = retirable(&self.inputs, &merged.report, self.output_path());
        Ok(CombinePreview {
            text: merged.result.render(),
            report: merged.report,
            conflicts: merged.previews,
            fingerprints,
            retirable,
        })
    }

    /// Reads the inputs and merges them, failing on undecided conflicts
    /// as [`combine_with`] does. Writes nothing (`--stdout`); `retire` is a
    /// usage error here.
    pub fn render(&self, home: Option<&Path>) -> Result<(String, CombineReport)> {
        if self.inputs.len() < 2 {
            return Err(Error::Usage(
                "combine needs at least two files.".to_string(),
            ));
        }
        if self.retire {
            return Err(Error::Usage(
                "--retire moves files after a write; drop --stdout.".to_string(),
            ));
        }
        let inputs = load_inputs(&self.inputs)?
            .into_iter()
            .map(|(i, _)| i)
            .collect();
        let (result, report) = combine_with(inputs, self.policy, &self.decisions, home)?;
        Ok((result.render(), report))
    }

    /// Reads the inputs again, refuses one that differs from `expected`
    /// ([`Error::CombineStale`]), merges as [`combine_with`] does, writes
    /// the output after its backup, then retires (D33). A failed write
    /// retires nothing; destinations are checked before the write.
    pub fn commit(
        &self,
        expected: Option<&[Fingerprint]>,
        options: WriteOptions,
        home: Option<&Path>,
    ) -> Result<CombineOutcome> {
        if self.inputs.len() < 2 {
            return Err(Error::Usage(
                "combine needs at least two files.".to_string(),
            ));
        }
        let (inputs, fingerprints): (Vec<_>, Vec<_>) =
            load_inputs(&self.inputs)?.into_iter().unzip();
        let patterns: Vec<String> = inputs
            .iter()
            .flat_map(|i| include_patterns(&i.config))
            .collect();
        if let Some(expected) = expected {
            for (now, then) in fingerprints.iter().zip(expected) {
                if now != then {
                    return Err(Error::CombineStale(now.path.display().to_string()));
                }
            }
            if expected.len() != fingerprints.len() {
                return Err(Error::Usage(
                    "the preview names other files; preview again.".to_string(),
                ));
            }
        }
        let output = self
            .output_path()
            .expect("two or more inputs")
            .to_path_buf();
        let (result, report) = combine_with(inputs, self.policy, &self.decisions, home)?;
        let moves = if self.retire {
            let home =
                home.ok_or_else(|| Error::Usage("no home directory to retire into.".to_string()))?;
            plan_retire(&self.inputs, &report, &output, home, &patterns)?
        } else {
            Vec::new()
        };
        let text = result.render();
        write_text(&output, &text, options)?;
        let mut retired = Vec::new();
        for (from, to) in moves {
            if let Some(to) = &to {
                move_to_retired(&absolute_path(&from), to)?;
            }
            retired.push(CombineRetired { from, to });
        }
        Ok(CombineOutcome {
            text,
            report,
            output,
            retired,
        })
    }
}

/// The later inputs an `Include` still loads, except the output.
fn retirable(inputs: &[PathBuf], report: &CombineReport, output: Option<&Path>) -> Vec<PathBuf> {
    let out = output.map(absolute_path);
    inputs
        .iter()
        .skip(1)
        .filter(|p| {
            let abs = absolute_path(p);
            out.as_ref() != Some(&abs)
                && report
                    .includes
                    .iter()
                    .any(|w| absolute_path(&w.loads) == abs)
        })
        .cloned()
        .collect()
}

/// Each later input and where it retires to (`None`: it stays). A
/// destination an `Include` pattern would load is [`Error::Usage`].
fn plan_retire(
    inputs: &[PathBuf],
    report: &CombineReport,
    output: &Path,
    home: &Path,
    patterns: &[String],
) -> Result<Vec<(PathBuf, Option<PathBuf>)>> {
    let movable = retirable(inputs, report, Some(output));
    let mut taken: Vec<PathBuf> = Vec::new();
    let mut out = Vec::new();
    for input in inputs.iter().skip(1) {
        if !movable.contains(input) {
            out.push((input.clone(), None));
            continue;
        }
        let name = input
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        let to = retired_destination(home, &name, &taken);
        let text = to.display().to_string();
        let loaded = patterns
            .iter()
            .any(|p| resolve_include(p, Some(home)).is_some_and(|r| glob_match(&r, &text)));
        if loaded {
            return Err(Error::Usage(format!(
                "{} is loaded by an Include; not retired.",
                to.display()
            )));
        }
        taken.push(to.clone());
        out.push((input.clone(), Some(to)));
    }
    Ok(out)
}
