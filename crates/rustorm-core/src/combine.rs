//! `combine`: folds several parsed config files into the first one.
//!
//! The first input is the base and keeps its order. Every later input is
//! walked part by part (preamble, then each section): its hosts are placed
//! by the rules in docs/cli.md's `combine` section, its `Host *` merges key
//! by key into the base's, and its free lines and `Match` blocks are
//! appended to the part they were in. Nothing is written here; the caller
//! renders the returned [`Config`].

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::include::{glob_match, resolve_include};
use crate::model::{split_patterns, Config, Entry, HostBlock, HostLocation, Line};
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

/// The message of [`Error::CombineConflicts`]: one headline, then one
/// indented `name: first, second` line per conflict.
pub(crate) fn conflicts_message(conflicts: &[Conflict]) -> String {
    let n = conflicts.len();
    let mut msg = format!(
        "{} {} defined more than once; nothing written. Use --on-conflict keep or replace.",
        plural(n, "host"),
        if n == 1 { "is" } else { "are" }
    );
    for c in conflicts {
        msg.push_str(&format!(
            "\n  {}: {}, {}",
            c.name,
            c.first.display(),
            c.second.display()
        ));
    }
    msg
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
    /// Every name in the result so far and the file that defined it.
    owners: HashMap<String, PathBuf>,
    report: CombineReport,
    base_path: &'a Path,
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
            self.report.conflicts.push(Conflict {
                name: name.clone(),
                first: owner,
                second: from.to_path_buf(),
            });
            match self.policy {
                OnConflict::Fail | OnConflict::Keep => {}
                OnConflict::Replace => {
                    let loc = self
                        .result
                        .find_host(&name)
                        .expect("owner is in the result");
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
        let _ = self.base_path;
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
        owners,
        report: CombineReport {
            files,
            includes,
            ..CombineReport::default()
        },
        base_path: &base.path,
    };
    for input in inputs {
        merger.merge_file(input);
    }
    let Merger {
        mut result,
        mut report,
        ..
    } = merger;
    report
        .conflicts
        .sort_by(|a, b| a.name.cmp(&b.name).then(a.second.cmp(&b.second)));
    if policy == OnConflict::Fail && !report.conflicts.is_empty() {
        return Err(Error::CombineConflicts(report.conflicts));
    }
    result.sort_sections();
    report.hosts = result.host_count();
    report.sections = result.sections.len();
    Ok((result, report))
}
