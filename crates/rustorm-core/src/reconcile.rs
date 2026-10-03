//! `reconcile`: hosts defined in two or more workspace files (docs/cli.md,
//! reconcile; D28–D31).
//!
//! [`Workspace::reconcile_report`] pairs every copy with the live
//! definition, the one ssh reads first, and classifies the pair as
//! identical, a conflict, or (for a host only a named copy holds) an
//! orphan. [`Workspace::apply_decisions`] applies a [`Decision`] per pair in
//! memory; [`Workspace::save`] writes each touched file once, after its
//! backup. [`Workspace::retire`] moves a fully resolved copy to
//! `~/.ssh/retired/`. No file is ever deleted.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::include::{glob_match, looks_like_backup, IncludeMatch};
use crate::io::{absolute_path, WriteOptions};
use crate::keys::canonical_key;
use crate::meta::MetaKey;
use crate::model::{HostBlock, HostLocation, Line};
use crate::workspace::{same_file, Change, Workspace, WorkspaceLocation};
use crate::{Error, Result};

/// What a pair is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PairKind {
    /// Directives and metadata lines match (D30).
    Identical,
    /// The copy differs from the live definition.
    Conflict,
    /// A host in a named copy that no other file defines.
    Orphan,
}

/// One side of a pair: a host block and where it is.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BlockRef {
    /// Index into [`Workspace::files`].
    #[serde(skip)]
    pub index: usize,
    /// The block's place in that file.
    #[serde(skip)]
    pub loc: HostLocation,
    /// The absolute path of the file (D23).
    pub file: PathBuf,
    /// The file as text output prints it (`~/...`).
    #[serde(skip)]
    pub label: String,
    /// The 1-based line of the `Host` line.
    pub line: usize,
    /// The block's section, if any.
    pub section: Option<String>,
    /// The block verbatim, leading comments included.
    pub text: String,
}

impl BlockRef {
    /// The [`WorkspaceLocation`] of the block.
    pub fn location(&self) -> WorkspaceLocation {
        WorkspaceLocation {
            file: self.index,
            loc: self.loc,
        }
    }
}

/// A key whose values differ between the live definition and the copy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct KeyDiff {
    /// The ssh keyword in canonical case, or a metadata key (`location`).
    pub key: String,
    /// The live values, whitespace collapsed; empty when unset.
    pub live: Vec<String>,
    /// The copy's values, whitespace collapsed; empty when unset.
    pub copy: Vec<String>,
}

/// A live definition and one copy of it, or an orphan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Pair {
    /// The live host's primary name (the orphan's for an orphan).
    pub name: String,
    /// Every name the two blocks share, as `check` reports them.
    pub names: Vec<String>,
    /// Identical, conflict or orphan.
    pub kind: PairKind,
    /// The live definition; `None` for an orphan.
    pub live: Option<BlockRef>,
    /// The copy, or the orphan.
    pub copy: BlockRef,
    /// True when the live definition sits in a file that looks like a
    /// backup.
    pub live_is_backup: bool,
    /// `github: ssh reads ~/.ssh/config.d/cypress.bak first, so its
    /// definition is the live one.` when `live_is_backup`.
    pub note: Option<String>,
    /// The differing keys of a conflict, metadata keys first.
    pub keys: Vec<KeyDiff>,
    /// A conflict's unified diff, live first, every line ending in `\n`;
    /// empty otherwise.
    pub diff: String,
}

impl Pair {
    /// True when `host` is the pair's name or one of its shared names.
    pub fn answers_to(&self, host: &str) -> bool {
        self.name == host || self.names.iter().any(|n| n == host)
    }
}

/// Every pair in scope.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ReconcileReport {
    /// Pairs whose copy differs.
    pub conflicts: usize,
    /// Pairs whose copy matches.
    pub identical: usize,
    /// Hosts only a named copy holds.
    pub orphans: usize,
    /// Files holding a pair or an orphan.
    pub files: usize,
    /// Conflicts and identical pairs in `check`'s order, then orphans.
    pub items: Vec<Pair>,
}

/// One side of a per-key pick.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Pick {
    /// Keep the live values.
    Live,
    /// Take the copy's values.
    Copy,
}

/// The pick for one differing key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeyPick {
    /// A key of [`Pair::keys`].
    pub key: String,
    /// Which side's values win.
    pub pick: Pick,
}

/// What to do with one pair.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Decision {
    /// Keep the live definition; writes nothing. On an orphan, leave it out
    /// of the live config.
    KeepLive,
    /// The live block takes the copy's body and metadata lines, keeping its
    /// `Host` line, section and place (D31).
    TakeCopy,
    /// Per-key picks applied to the live block.
    Keys(Vec<KeyPick>),
    /// Move an orphan into the root or the `add_to` file.
    Add,
    /// Remove an identical copy's block from the copy's file.
    DropIdentical,
    /// Undecided.
    Skip,
}

/// What [`Workspace::apply_decisions`] did.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Applied {
    /// Conflicts that took the copy.
    pub taken: Vec<String>,
    /// Conflicts that kept the live definition.
    pub kept: Vec<String>,
    /// Conflicts decided key by key.
    pub keyed: Vec<String>,
    /// Orphans added.
    pub added: Vec<String>,
    /// Orphans left out.
    pub left_out: Vec<String>,
    /// Identical copies dropped.
    pub dropped: Vec<String>,
    /// The names of every conflict and orphan given a decision other than
    /// [`Decision::Skip`]; [`Workspace::retire`] takes them.
    pub decided: Vec<String>,
    /// Conflicts in the report left without a decision.
    pub remaining: usize,
}

/// A file [`Workspace::retire`] moved.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Retired {
    /// Where it was.
    pub from: PathBuf,
    /// Where it is now, under `~/.ssh/retired/`.
    pub to: PathBuf,
}

fn plural(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

fn is_pattern(name: &str) -> bool {
    name.contains(['*', '?', '!'])
}

fn collapse(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The block's directives as compared (D30): `Key value`, canonical key
/// case, whitespace collapsed, in file order.
fn directive_lines(block: &HostBlock) -> Vec<String> {
    block
        .directives()
        .iter()
        .map(|d| format!("{} {}", canonical_key(&d.key), collapse(&d.value)))
        .collect()
}

/// The lines a pair is compared and diffed on: metadata lines, the `Host`
/// line, then one indented line per directive.
pub fn normalized_lines(block: &HostBlock) -> Vec<String> {
    let mut out = block.meta().lines();
    out.push(format!("Host {}", block.names().join(" ")));
    out.extend(
        directive_lines(block)
            .into_iter()
            .map(|l| format!("    {l}")),
    );
    out
}

/// True when `a` and `b` have the same directives and metadata lines (D30).
pub fn same_definition(a: &HostBlock, b: &HostBlock) -> bool {
    directive_lines(a) == directive_lines(b) && a.meta().lines() == b.meta().lines()
}

/// Every key whose values differ between `live` and `copy`: metadata keys
/// in table order, then ssh keywords in order of first appearance.
pub fn key_diffs(live: &HostBlock, copy: &HostBlock) -> Vec<KeyDiff> {
    let mut out = Vec::new();
    let (lm, cm) = (live.meta(), copy.meta());
    for key in MetaKey::ALL {
        let (l, c) = (lm.get(key), cm.get(key));
        if l != c {
            out.push(KeyDiff {
                key: key.name().to_string(),
                live: l,
                copy: c,
            });
        }
    }
    let mut keys: Vec<String> = Vec::new();
    for d in live.directives().iter().chain(copy.directives().iter()) {
        let k = canonical_key(&d.key);
        if !keys.iter().any(|x| x.eq_ignore_ascii_case(&k)) {
            keys.push(k);
        }
    }
    for k in keys {
        let values = |b: &HostBlock| -> Vec<String> {
            b.directives()
                .iter()
                .filter(|d| d.key.eq_ignore_ascii_case(&k))
                .map(|d| collapse(&d.value))
                .collect()
        };
        let (l, c) = (values(live), values(copy));
        if l != c {
            out.push(KeyDiff {
                key: k,
                live: l,
                copy: c,
            });
        }
    }
    out
}

/// A unified diff of `a` (the live lines) against `b` (the copy's), one
/// hunk with full context, every line ending in `\n`:
///
/// ```text
/// --- ~/.ssh/config.d/cypress (live)
/// +++ ~/.ssh/config.d/cypress.bak (copy)
/// @@ -1,3 +1,3 @@ cypressPro
/// ```
pub fn unified_diff(a: &[String], b: &[String], live: &str, copy: &str, name: &str) -> String {
    let (n, m) = (a.len(), b.len());
    // lcs[i][j]: length of the longest common subsequence of a[i..], b[j..].
    let mut lcs = vec![vec![0usize; m + 1]; n + 1];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            lcs[i][j] = if a[i] == b[j] {
                lcs[i + 1][j + 1] + 1
            } else {
                lcs[i + 1][j].max(lcs[i][j + 1])
            };
        }
    }
    let range = |len: usize| {
        if len == 0 {
            "0,0".to_string()
        } else {
            format!("1,{len}")
        }
    };
    let mut out = format!(
        "--- {live} (live)\n+++ {copy} (copy)\n@@ -{} +{} @@ {name}\n",
        range(n),
        range(m)
    );
    let (mut i, mut j) = (0, 0);
    while i < n || j < m {
        if i < n && j < m && a[i] == b[j] {
            out.push_str(&format!(" {}\n", a[i]));
            i += 1;
            j += 1;
        } else if i < n && (j == m || lcs[i + 1][j] >= lcs[i][j + 1]) {
            out.push_str(&format!("-{}\n", a[i]));
            i += 1;
        } else {
            out.push_str(&format!("+{}\n", b[j]));
            j += 1;
        }
    }
    out
}

impl ReconcileReport {
    /// `2 conflicts, 1 identical, 0 orphans across 2 files`.
    pub fn summary(&self) -> String {
        format!(
            "{}, {} identical, {} across {}",
            plural(self.conflicts, "conflict", "conflicts"),
            self.identical,
            plural(self.orphans, "orphan", "orphans"),
            plural(self.files, "file", "files")
        )
    }

    /// The `--list` text: the summary, a `note: ...` line per live backup,
    /// each conflict's diff, then one `orphan: <host> in <file>` line per
    /// orphan; a blank line between diffs and before the orphans. Ends in
    /// `\n`.
    pub fn list_text(&self) -> String {
        let mut out = format!("{}\n", self.summary());
        for p in &self.items {
            if let Some(note) = &p.note {
                out.push_str(&format!("note: {note}\n"));
            }
        }
        let mut parts: Vec<String> = self
            .items
            .iter()
            .filter(|p| p.kind == PairKind::Conflict)
            .map(|p| p.diff.clone())
            .collect();
        let orphans: String = self
            .items
            .iter()
            .filter(|p| p.kind == PairKind::Orphan)
            .map(|p| format!("orphan: {} in {}\n", p.name, p.copy.label))
            .collect();
        if !orphans.is_empty() {
            parts.push(orphans);
        }
        out.push_str(&parts.join("\n"));
        out
    }

    /// The index of the one pair answering to `host`.
    /// [`Error::NotDuplicated`] when none does; [`Error::AmbiguousCopy`]
    /// when copies in two files do.
    pub fn lookup(&self, host: &str) -> Result<usize> {
        let hits: Vec<usize> = (0..self.items.len())
            .filter(|&i| self.items[i].answers_to(host))
            .collect();
        match hits.as_slice() {
            [] => Err(Error::NotDuplicated(host.to_string())),
            [one] => Ok(*one),
            many => Err(Error::AmbiguousCopy {
                host: host.to_string(),
                files: many
                    .iter()
                    .map(|&i| self.items[i].copy.label.clone())
                    .collect(),
            }),
        }
    }

    /// `decision` for every pair of `kind` not already in `decided`
    /// (`--all-live`, `--all-copy`, `--drop-identical`).
    pub fn bulk(
        &self,
        kind: PairKind,
        decision: &Decision,
        decided: &[(usize, Decision)],
    ) -> Vec<(usize, Decision)> {
        (0..self.items.len())
            .filter(|&i| self.items[i].kind == kind && !decided.iter().any(|(d, _)| *d == i))
            .map(|i| (i, decision.clone()))
            .collect()
    }

    /// Conflicts without a decision other than [`Decision::Skip`].
    pub fn remaining(&self, decisions: &[(usize, Decision)]) -> usize {
        (0..self.items.len())
            .filter(|&i| {
                self.items[i].kind == PairKind::Conflict
                    && !decisions
                        .iter()
                        .any(|(d, dec)| *d == i && *dec != Decision::Skip)
            })
            .count()
    }
}

/// `2 conflicts remain.`, `1 conflict remains.` or `no conflicts remain.`
pub fn remaining_message(n: usize) -> String {
    match n {
        0 => "no conflicts remain.".to_string(),
        1 => "1 conflict remains.".to_string(),
        n => format!("{n} conflicts remain."),
    }
}

/// A host block of the workspace with its names and reading position.
struct Def {
    at: WorkspaceLocation,
    names: Vec<String>,
    rank: Vec<(usize, usize)>,
}

impl Workspace {
    /// Each loaded file's place in ssh's reading order: the root `[]`, a
    /// file an `Include` on line `L` loads `parent + [(L, n)]`, where `n`
    /// counts matched files in load order. A host on line `l` of a file
    /// ranks at `path + [(l, MAX)]`, so it sorts before an `Include` below
    /// it and after one above it (D29). Files no `Include` loaded rank last.
    fn read_paths(&self) -> Vec<Vec<(usize, usize)>> {
        fn walk(
            ws: &Workspace,
            matches: &[IncludeMatch],
            parent: &[(usize, usize)],
            out: &mut [Option<Vec<(usize, usize)>>],
            counter: &mut usize,
        ) {
            for m in matches {
                for f in &m.files {
                    *counter += 1;
                    let mut path = parent.to_vec();
                    path.push((m.line, *counter));
                    if let Some(i) = ws.files.iter().position(|c| same_file(&c.path, &f.path)) {
                        if out[i].is_none() {
                            out[i] = Some(path.clone());
                        }
                    }
                    walk(ws, &f.nested, &path, out, counter);
                }
            }
        }
        let mut out: Vec<Option<Vec<(usize, usize)>>> = vec![None; self.files.len()];
        out[0] = Some(Vec::new());
        let mut counter = 0;
        walk(self, &self.includes, &[], &mut out, &mut counter);
        out.into_iter()
            .enumerate()
            .map(|(i, p)| p.unwrap_or_else(|| vec![(usize::MAX, i)]))
            .collect()
    }

    fn defs(&self) -> Vec<Def> {
        let paths = self.read_paths();
        let mut out = Vec::new();
        for (file, f) in self.files.iter().enumerate() {
            for loc in f.config.host_locations() {
                let h = f.config.host(loc);
                if h.is_defaults() {
                    continue;
                }
                let mut rank = paths[file].clone();
                rank.push((f.config.host_line(loc), usize::MAX));
                out.push(Def {
                    at: WorkspaceLocation { file, loc },
                    names: h.names().into_iter().filter(|n| !is_pattern(n)).collect(),
                    rank,
                });
            }
        }
        out
    }

    fn block_ref(&self, at: WorkspaceLocation) -> BlockRef {
        let config = &self.files[at.file].config;
        BlockRef {
            index: at.file,
            loc: at.loc,
            file: self.abs(at.file),
            label: self.display(at.file),
            line: config.host_line(at.loc),
            section: config.section_name(at.loc).map(str::to_string),
            text: config.host(at.loc).text(),
        }
    }

    /// The `reconcile` report: every host defined in two or more files as
    /// pairs of the live definition (the one ssh reads first, D29) and each
    /// later file's first definition. With `scope`, only the copies in
    /// those files, plus their orphans: hosts no other file defines. Glob
    /// patterns and `Host *` are never paired. Reads only.
    pub fn reconcile_report(&self, scope: Option<&[usize]>) -> ReconcileReport {
        let defs = self.defs();
        // Every concrete name and the defs answering to it, in file order.
        let mut names: Vec<(String, Vec<usize>)> = Vec::new();
        for (d, def) in defs.iter().enumerate() {
            for n in &def.names {
                match names.iter_mut().find(|(k, _)| k == n) {
                    Some((_, ds)) => ds.push(d),
                    None => names.push((n.clone(), vec![d])),
                }
            }
        }
        // (live def, copy def, shared names) in check's order.
        let mut pairs: Vec<(usize, usize, Vec<String>)> = Vec::new();
        for (n, mut ds) in names.iter().cloned() {
            let mut files: Vec<usize> = ds.iter().map(|&d| defs[d].at.file).collect();
            files.dedup();
            files.sort_unstable();
            files.dedup();
            if files.len() < 2 {
                continue;
            }
            ds.sort_by(|&a, &b| defs[a].rank.cmp(&defs[b].rank));
            let live = ds[0];
            let mut seen = vec![defs[live].at.file];
            for &d in &ds[1..] {
                let f = defs[d].at.file;
                if seen.contains(&f) {
                    continue;
                }
                seen.push(f);
                match pairs.iter_mut().find(|(l, c, _)| *l == live && *c == d) {
                    Some((_, _, shared)) => shared.push(n.clone()),
                    None => pairs.push((live, d, vec![n.clone()])),
                }
            }
        }
        let in_scope = |file: usize| scope.is_none_or(|s| s.contains(&file));
        let mut items = Vec::new();
        for (live, copy, shared) in pairs {
            if !in_scope(defs[copy].at.file) {
                continue;
            }
            let (lb, cb) = (self.host(defs[live].at), self.host(defs[copy].at));
            let live_ref = self.block_ref(defs[live].at);
            let copy_ref = self.block_ref(defs[copy].at);
            let name = lb.primary();
            let identical = same_definition(lb, cb);
            let live_is_backup = looks_like_backup(&self.files[live_ref.index].path);
            let note = live_is_backup.then(|| {
                format!(
                    "{name}: ssh reads {} first, so its definition is the live one.",
                    live_ref.label
                )
            });
            let (keys, diff) = if identical {
                (Vec::new(), String::new())
            } else {
                (
                    key_diffs(lb, cb),
                    unified_diff(
                        &normalized_lines(lb),
                        &normalized_lines(cb),
                        &live_ref.label,
                        &copy_ref.label,
                        &name,
                    ),
                )
            };
            items.push(Pair {
                name,
                names: shared,
                kind: if identical {
                    PairKind::Identical
                } else {
                    PairKind::Conflict
                },
                live: Some(live_ref),
                copy: copy_ref,
                live_is_backup,
                note,
                keys,
                diff,
            });
        }
        if let Some(scope) = scope {
            for def in &defs {
                if !scope.contains(&def.at.file) || def.names.is_empty() {
                    continue;
                }
                let elsewhere = def.names.iter().any(|n| {
                    names
                        .iter()
                        .find(|(k, _)| k == n)
                        .is_some_and(|(_, ds)| ds.iter().any(|&d| defs[d].at.file != def.at.file))
                });
                if elsewhere {
                    continue;
                }
                let block = self.host(def.at);
                items.push(Pair {
                    name: block.primary(),
                    names: def.names.clone(),
                    kind: PairKind::Orphan,
                    live: None,
                    copy: self.block_ref(def.at),
                    live_is_backup: false,
                    note: None,
                    keys: Vec::new(),
                    diff: String::new(),
                });
            }
        }
        let count = |k: PairKind| items.iter().filter(|p| p.kind == k).count();
        let mut files: Vec<usize> = items
            .iter()
            .flat_map(|p| p.live.iter().map(|l| l.index).chain([p.copy.index]))
            .collect();
        files.sort_unstable();
        files.dedup();
        ReconcileReport {
            conflicts: count(PairKind::Conflict),
            identical: count(PairKind::Identical),
            orphans: count(PairKind::Orphan),
            files: files.len(),
            items,
        }
    }

    /// Resolves `reconcile FILE...` to file indices, each as `--file`
    /// resolves it.
    pub fn reconcile_scope(&mut self, files: &[String]) -> Result<Vec<usize>> {
        let mut out = Vec::new();
        for f in files {
            let i = self.resolve_file(f)?;
            if !out.contains(&i) {
                out.push(i);
            }
        }
        Ok(out)
    }

    fn check_decision(&self, pair: &Pair, decision: &Decision) -> Result<()> {
        let refuse = |reason: &str| {
            Err(Error::Undecidable {
                host: pair.name.clone(),
                reason: reason.to_string(),
            })
        };
        match (pair.kind, decision) {
            (_, Decision::Skip) => Ok(()),
            (PairKind::Orphan, Decision::Add | Decision::KeepLive) => Ok(()),
            (PairKind::Orphan, _) => refuse("is an orphan; add it or leave it out"),
            (_, Decision::Add) => {
                refuse("is not an orphan; only a host no other file defines is added")
            }
            (PairKind::Conflict, Decision::DropIdentical) => {
                refuse("differs from its live definition; take the copy or keep the live one")
            }
            (PairKind::Conflict, Decision::Keys(picks)) => {
                for p in picks {
                    if !pair.keys.iter().any(|k| k.key.eq_ignore_ascii_case(&p.key)) {
                        return refuse(&format!("has no differing key {}", p.key));
                    }
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }

    fn is_current(&self, side: &BlockRef) -> bool {
        let config = &self.files[side.index].config;
        config.host_locations().contains(&side.loc) && config.host(side.loc).text() == side.text
    }

    /// Applies one decision per pair of `report` in memory: take-copy and
    /// per-key picks rewrite the live block in place; drop and add take the
    /// copy's block (leading comments included) out of its file; add places
    /// the orphan in `add_to` (the root when `None`), in its section when
    /// that file has one of the same name. Every decision is checked before
    /// anything changes. Messages name each decision, then the drop count,
    /// then [`remaining_message`]. Nothing is written until
    /// [`Workspace::save`].
    pub fn apply_decisions(
        &mut self,
        report: &ReconcileReport,
        decisions: &[(usize, Decision)],
        add_to: Option<usize>,
    ) -> Result<Change<Applied>> {
        let mut live_edits: Vec<WorkspaceLocation> = Vec::new();
        for (i, decision) in decisions {
            let pair = report.items.get(*i).ok_or_else(|| {
                Error::Usage(format!(
                    "no reconcile item {i}; the report has {}.",
                    report.items.len()
                ))
            })?;
            self.check_decision(pair, decision)?;
            let current =
                pair.live.iter().all(|l| self.is_current(l)) && self.is_current(&pair.copy);
            if !current {
                return Err(Error::ReconcileStale(pair.name.clone()));
            }
            if pair.kind == PairKind::Conflict
                && matches!(decision, Decision::TakeCopy | Decision::Keys(_))
            {
                let at = pair
                    .live
                    .as_ref()
                    .expect("a conflict has a live side")
                    .location();
                if live_edits.contains(&at) {
                    return Err(Error::Undecidable {
                        host: pair.name.clone(),
                        reason: "has copies in two files; take one of them".to_string(),
                    });
                }
                live_edits.push(at);
            }
        }
        let target = add_to.unwrap_or(0);
        let mut applied = Applied::default();
        let mut messages = Vec::new();
        let mut touched: Vec<usize> = Vec::new();
        let mut removals: Vec<(WorkspaceLocation, Option<usize>)> = Vec::new();
        let mut drop_files: Vec<usize> = Vec::new();
        for (i, decision) in decisions {
            let pair = &report.items[*i];
            let copy_label = pair.copy.label.clone();
            let decided = !matches!(decision, Decision::Skip) && pair.kind != PairKind::Identical;
            if decided {
                applied.decided.push(pair.name.clone());
            }
            match (pair.kind, decision) {
                (_, Decision::Skip) => {}
                (PairKind::Orphan, Decision::Add) => {
                    removals.push((pair.copy.location(), Some(*i)));
                    applied.added.push(pair.name.clone());
                    messages.push(format!(
                        "{} added to {} from {copy_label}.",
                        pair.name,
                        self.display(target)
                    ));
                }
                (PairKind::Orphan, _) => {
                    applied.left_out.push(pair.name.clone());
                    messages.push(format!(
                        "{}: left out; it stays in {copy_label}.",
                        pair.name
                    ));
                }
                (PairKind::Identical, Decision::DropIdentical) => {
                    removals.push((pair.copy.location(), None));
                    applied.dropped.push(pair.name.clone());
                    if !drop_files.contains(&pair.copy.index) {
                        drop_files.push(pair.copy.index);
                    }
                }
                (PairKind::Identical, _) => {}
                (PairKind::Conflict, Decision::KeepLive) => {
                    let live = pair.live.as_ref().expect("live side");
                    applied.kept.push(pair.name.clone());
                    messages.push(format!("{}: kept {}.", pair.name, live.label));
                }
                (PairKind::Conflict, Decision::TakeCopy) => {
                    let live = pair.live.as_ref().expect("live side");
                    let copy = self.host(pair.copy.location()).clone();
                    take_copy(self.files[live.index].config.host_mut(live.loc), &copy);
                    if !touched.contains(&live.index) {
                        touched.push(live.index);
                    }
                    applied.taken.push(pair.name.clone());
                    messages.push(format!(
                        "{}: took the copy from {copy_label} into {}.",
                        pair.name, live.label
                    ));
                }
                (PairKind::Conflict, Decision::Keys(picks)) => {
                    let live = pair.live.as_ref().expect("live side");
                    let copy = self.host(pair.copy.location()).clone();
                    let taken: Vec<String> = picks
                        .iter()
                        .filter(|p| p.pick == Pick::Copy)
                        .map(|p| {
                            pair.keys
                                .iter()
                                .find(|k| k.key.eq_ignore_ascii_case(&p.key))
                                .map_or_else(|| p.key.clone(), |k| k.key.clone())
                        })
                        .collect();
                    if taken.is_empty() {
                        applied.kept.push(pair.name.clone());
                        messages.push(format!("{}: kept {}.", pair.name, live.label));
                    } else {
                        let block = self.files[live.index].config.host_mut(live.loc);
                        for k in &taken {
                            take_key(block, &copy, k);
                        }
                        if !touched.contains(&live.index) {
                            touched.push(live.index);
                        }
                        applied.keyed.push(pair.name.clone());
                        messages.push(format!(
                            "{}: took {} from {copy_label} into {}.",
                            pair.name,
                            taken.join(", "),
                            live.label
                        ));
                    }
                }
                (PairKind::Conflict, _) => {}
            }
        }
        // Remove from the end of each part first so earlier indices hold.
        removals.sort_by(|(a, _), (b, _)| {
            (b.file, b.loc.section, b.loc.index).cmp(&(a.file, a.loc.section, a.loc.index))
        });
        removals.dedup_by(|(a, _), (b, _)| a == b);
        let mut adds: Vec<(usize, HostBlock)> = Vec::new();
        for (at, add) in removals {
            let block = self.files[at.file].config.remove_host(at.loc);
            if !touched.contains(&at.file) {
                touched.push(at.file);
            }
            if let Some(i) = add {
                adds.push((i, block));
            }
        }
        adds.sort_by_key(|(i, _)| *i);
        for (i, mut block) in adds {
            let section = report.items[i]
                .copy
                .section
                .clone()
                .filter(|s| self.files[target].config.find_section(s).is_some());
            block.last_line_mut().ensure_terminated();
            let config = &mut self.files[target].config;
            config.ensure_trailing_newline();
            config.place(block, section.as_deref(), None);
            if !touched.contains(&target) {
                touched.push(target);
            }
        }
        if !applied.dropped.is_empty() {
            let from = match drop_files.as_slice() {
                [one] => self.display(*one),
                many => plural(many.len(), "file", "files"),
            };
            messages.push(format!(
                "{} dropped from {from}.",
                plural(applied.dropped.len(), "identical copy", "identical copies")
            ));
        }
        applied.remaining = report.remaining(decisions);
        messages.push(remaining_message(applied.remaining));
        Ok(self.change(applied, touched, messages, Vec::new()))
    }

    /// Why file `file` cannot retire yet: each conflict and orphan in it
    /// not in `decided` (`lab-1 (conflict)`, `printer (orphan)`), and each
    /// host it holds the live definition of while a differing copy sits in
    /// another file (`github (read here first)`). Reads the workspace as it
    /// is in memory, so decisions already applied count.
    pub fn retire_blockers(&self, file: usize, decided: &[String]) -> Vec<String> {
        let is_decided = |p: &Pair| decided.iter().any(|d| p.answers_to(d));
        let mut out = Vec::new();
        for p in self.reconcile_report(Some(&[file])).items {
            match p.kind {
                PairKind::Conflict if !is_decided(&p) => out.push(format!("{} (conflict)", p.name)),
                PairKind::Orphan if !is_decided(&p) => out.push(format!("{} (orphan)", p.name)),
                _ => {}
            }
        }
        for p in self.reconcile_report(None).items {
            let live_here = p.live.as_ref().is_some_and(|l| l.index == file);
            if live_here && p.kind == PairKind::Conflict && p.copy.index != file {
                let entry = format!("{} (read here first)", p.name);
                if !out.contains(&entry) {
                    out.push(entry);
                }
            }
        }
        out
    }

    /// Moves file `file` to `~/.ssh/retired/<name>` (`<name>.1`, `.2`, ...
    /// when taken), creating the directory with mode 0700, once
    /// [`Workspace::retire_blockers`] is empty. Unsaved changes to the file
    /// are written first. The root never retires ([`Error::RetireRoot`]);
    /// blockers give [`Error::RetireUnresolved`]. The workspace still lists
    /// the file afterwards; load it again to drop it.
    pub fn retire(
        &mut self,
        file: usize,
        decided: &[String],
        options: WriteOptions,
    ) -> Result<Change<Retired>> {
        if file == 0 {
            return Err(Error::RetireRoot(self.display(0)));
        }
        let remaining = self.retire_blockers(file, decided);
        if !remaining.is_empty() {
            return Err(Error::RetireUnresolved {
                file: self.display(file),
                remaining,
            });
        }
        let home = self
            .home
            .clone()
            .ok_or_else(|| Error::Usage("no home directory to retire into.".to_string()))?;
        let dir = home.join(".ssh").join("retired");
        let name = self.file_name(file);
        let mut to = dir.join(&name);
        let mut n = 1;
        while to.exists() {
            to = dir.join(format!("{name}.{n}"));
            n += 1;
        }
        let text = to.display().to_string();
        let loaded = self
            .includes
            .iter()
            .flat_map(IncludeMatch::walk)
            .filter_map(|m| m.resolved.as_deref())
            .any(|pattern| glob_match(pattern, &text));
        if loaded {
            return Err(Error::Usage(format!(
                "{} is loaded by an Include; not retired.",
                self.display_path(&to)
            )));
        }
        self.save_file(file, options)?;
        create_private_dir(&dir)?;
        let from = absolute_path(&self.files[file].path);
        fs::rename(&from, &to).map_err(|source| Error::Write {
            path: to.clone(),
            source,
        })?;
        let msg = format!(
            "{} retired to {}.",
            self.display(file),
            self.display_path(&to)
        );
        Ok(Change {
            value: Retired { from, to },
            files: vec![file],
            messages: vec![msg],
            warnings: Vec::new(),
        })
    }
}

fn create_private_dir(dir: &Path) -> Result<()> {
    let err = |source| Error::Write {
        path: dir.to_path_buf(),
        source,
    };
    if !dir.is_dir() {
        fs::create_dir_all(dir).map_err(err)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(dir, fs::Permissions::from_mode(0o700)).map_err(err)?;
        }
    }
    Ok(())
}

/// Replaces `live`'s body with `copy`'s and its metadata lines with
/// `copy`'s, keeping `live`'s `Host` line and plain leading comments (D31).
/// The block keeps whether its last line ended the file without a newline.
fn take_copy(live: &mut HostBlock, copy: &HostBlock) {
    let was_terminated = !live.last_line_mut().eol().is_empty();
    live.leading
        .retain(|l| crate::meta::parse_meta_line(l.text()).is_none());
    for l in &copy.leading {
        if crate::meta::parse_meta_line(l.text()).is_some() {
            let mut l = l.clone();
            l.ensure_terminated();
            live.leading.push(l);
        }
    }
    live.header.ensure_terminated();
    live.body = copy
        .body
        .iter()
        .map(|l| {
            let mut l = l.clone();
            l.ensure_terminated();
            l
        })
        .collect();
    if !was_terminated {
        let last = live.last_line_mut();
        *last = Line::from_raw(last.text().to_string());
    }
}

/// Gives `live` `copy`'s values of `key` (an ssh keyword or a metadata
/// key); a key the copy lacks is removed.
fn take_key(live: &mut HostBlock, copy: &HostBlock, key: &str) {
    if let Some(m) = MetaKey::parse(key) {
        live.set_meta(m, &copy.meta_values(m));
        return;
    }
    let values: Vec<String> = copy
        .directives()
        .into_iter()
        .filter(|d| d.key.eq_ignore_ascii_case(key))
        .map(|d| d.value)
        .collect();
    match values.split_first() {
        None => {
            live.unset(key);
        }
        Some((first, rest)) => {
            live.set(key, first);
            for v in rest {
                live.append(key, v);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn unified_diff_puts_removals_before_additions() {
        let a = lines(&["Host x", "    HostName a", "    User u"]);
        let b = lines(&["Host x", "    HostName b", "    User u", "    Port 22"]);
        assert_eq!(
            unified_diff(&a, &b, "A", "B", "x"),
            "--- A (live)\n+++ B (copy)\n@@ -1,3 +1,4 @@ x\n Host x\n-    HostName a\n+    HostName b\n     User u\n+    Port 22\n"
        );
    }

    #[test]
    fn normalized_lines_collapse_whitespace_and_key_case() {
        let c = crate::Config::parse("# note: n\n# plain\nHost x y\n\thostname   a   b\n").unwrap();
        let h = c.hosts()[0];
        assert_eq!(
            normalized_lines(h),
            lines(&["# note: n", "Host x y", "    HostName a b"])
        );
    }

    #[test]
    fn remaining_message_counts() {
        assert_eq!(remaining_message(0), "no conflicts remain.");
        assert_eq!(remaining_message(1), "1 conflict remains.");
        assert_eq!(remaining_message(19), "19 conflicts remain.");
    }
}
