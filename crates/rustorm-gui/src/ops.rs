//! The writes the GUI makes, each a sequence of `rustorm_core` operations.

use std::path::{Path, PathBuf};

use rustorm_core::{
    remaining_message, AddSpec, CloneSpec, Decision, EditSpec, Env, HostSelector, PairKind,
    SectionRename, SettingChange, Workspace, WriteOptions,
};

/// One reconcile decision, naming its pair as the Conflicts dialog showed
/// it, so a pair that changed since is refused instead of decided blind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PairDecision {
    /// A name the pair answers to.
    pub name: String,
    /// The copy's file, absolute.
    pub copy: PathBuf,
    /// The live block's text as shown; `None` for an orphan.
    pub live_text: Option<String>,
    /// The copy's text as shown.
    pub copy_text: String,
    /// What to do.
    pub decision: Decision,
}

/// One write the user asked for from the Hosts tab.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Op {
    /// The add form: the web version's name, URI and identity file, plus
    /// a section (empty: the catch-all).
    Add {
        /// Host name.
        name: String,
        /// `[user@]host[:port]`.
        uri: String,
        /// `IdentityFile`; empty writes none.
        identity: String,
        /// Destination section; empty for the default.
        section: String,
    },
    /// The edit form. An emptied identity removes every `IdentityFile`.
    Edit {
        /// The host's name when the form opened.
        original: String,
        /// The name in the form; a different one renames the host.
        name: String,
        /// `[user@]host[:port]`.
        uri: String,
        /// `IdentityFile`; empty removes it.
        identity: String,
        /// Section; a different one moves the host there.
        section: String,
    },
    /// Delete one host.
    Delete(String),
    /// Clone a host under a new name into the source's section.
    Clone {
        /// The host to copy.
        source: String,
        /// The copy's name.
        new_name: String,
    },
    /// Move a host to a section, creating it when missing.
    Move {
        /// The host.
        name: String,
        /// The destination section.
        section: String,
    },
    /// Create an empty section before the catch-all.
    AddSection {
        /// The section name.
        name: String,
    },
    /// Rename a section; a name another section already has merges into it.
    RenameSection {
        /// The section's current name.
        old: String,
        /// The new name.
        new: String,
        /// The file holding it, as a `--file` argument, when the name is in
        /// two files.
        file: Option<String>,
    },
    /// The All settings section: sets and unsets several keys of one host
    /// in one write.
    Settings {
        /// The host.
        name: String,
        /// One entry per keyword whose values changed.
        changes: Vec<SettingChange>,
    },
    /// The Conflicts dialog: decisions on pairs of the reconcile report.
    Reconcile {
        /// The decisions, applied together.
        decisions: Vec<PairDecision>,
        /// The copy file whose orphans are in scope; `None` reports every
        /// pair and no orphans.
        scope: Option<PathBuf>,
        /// Pairs kept live this session, as (name, copy file); they do not
        /// count as remaining.
        kept: Vec<(String, PathBuf)>,
    },
    /// Moves a fully resolved copy to `~/.ssh/retired/`.
    Retire {
        /// The file, absolute.
        file: PathBuf,
        /// Names of the conflicts and orphans decided without a write.
        decided: Vec<String>,
    },
}

/// The index of the loaded file at absolute path `path`.
fn file_index(ws: &Workspace, path: &Path) -> rustorm_core::Result<usize> {
    (0..ws.files.len())
        .find(|&i| ws.abs(i) == path)
        .ok_or_else(|| rustorm_core::Error::Usage(format!("{} is not loaded.", path.display())))
}

/// Conflicts of `ws` not in `kept`.
pub fn remaining_conflicts(ws: &Workspace, kept: &[(String, PathBuf)]) -> usize {
    ws.reconcile_report(None)
        .items
        .iter()
        .filter(|p| {
            p.kind == PairKind::Conflict
                && !kept
                    .iter()
                    .any(|(n, c)| p.answers_to(n) && p.copy.file == *c)
        })
        .count()
}

/// What a successful [`Op`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome {
    /// The status-bar message.
    pub message: String,
    /// The host to select afterwards.
    pub select: Option<String>,
}

fn non_empty(s: &str) -> Option<String> {
    let t = s.trim();
    (!t.is_empty()).then(|| t.to_string())
}

impl Op {
    /// Every host name the operation reads or writes, for conflict checks
    /// against a file changed on disk.
    pub fn hosts(&self) -> Vec<String> {
        match self {
            Op::Add { name, .. }
            | Op::Delete(name)
            | Op::Move { name, .. }
            | Op::Settings { name, .. } => vec![name.clone()],
            Op::Edit { original, name, .. } => vec![original.clone(), name.trim().to_string()],
            Op::Clone { source, new_name } => vec![source.clone(), new_name.trim().to_string()],
            // A reconcile decision checks its pair's text itself.
            Op::AddSection { .. }
            | Op::RenameSection { .. }
            | Op::Reconcile { .. }
            | Op::Retire { .. } => Vec::new(),
        }
    }

    /// Applies the operation to `ws` through the core, which routes each
    /// write to its file. On `Err` the caller discards `ws`, so a partial
    /// sequence never reaches disk. On a workspace of one file the status
    /// messages are the GUI's own; on several they are the core's, which
    /// name the file written.
    pub fn apply(&self, ws: &mut Workspace, env: &Env) -> rustorm_core::Result<Outcome> {
        let multi = ws.is_multi();
        let said = |messages: Vec<String>, single: String| {
            if multi {
                messages.join(" ")
            } else {
                single
            }
        };
        match self {
            Op::Add {
                name,
                uri,
                identity,
                section,
            } => {
                let change = ws.add(
                    &AddSpec {
                        name: name.trim().to_string(),
                        uri: uri.trim().to_string(),
                        identity: non_empty(identity),
                        options: Vec::new(),
                        section: non_empty(section),
                    },
                    None,
                    env,
                )?;
                let placed = change.value;
                let single = match &placed.section {
                    Some(s) => format!("{} added to section {s}.", placed.name),
                    None => format!("{} added.", placed.name),
                };
                Ok(Outcome {
                    message: said(change.messages, single),
                    select: Some(placed.name),
                })
            }
            Op::Edit {
                original,
                name,
                uri,
                identity,
                section,
            } => {
                let wl = ws
                    .find_host(original)
                    .ok_or_else(|| rustorm_core::Error::EditTargetMissing(original.clone()))?;
                let current = ws.files[wl.file]
                    .config
                    .section_name(wl.loc)
                    .map(str::to_string);
                let mut target = ws.host(wl).primary();
                let new_name = name.trim();
                if new_name != target {
                    ws.move_host(&target, Some(new_name), None, None)?;
                    target = new_name.to_string();
                }
                let section = non_empty(section).filter(|s| {
                    current
                        .as_deref()
                        .is_none_or(|c| !c.eq_ignore_ascii_case(s))
                });
                let identity = non_empty(identity);
                let remove_identity = identity.is_none();
                let change = ws.edit(
                    &EditSpec {
                        name: target.clone(),
                        uri: uri.trim().to_string(),
                        identity,
                        options: Vec::new(),
                        section,
                    },
                    None,
                    env,
                )?;
                if remove_identity {
                    ws.unset(
                        &HostSelector::Name(target.clone()),
                        &["IdentityFile".to_string()],
                        None,
                    )?;
                }
                Ok(Outcome {
                    message: said(change.messages, format!("{target} updated.")),
                    select: Some(target),
                })
            }
            Op::Delete(name) => {
                let change = ws.delete(std::slice::from_ref(name), None)?;
                let single = format!("{} deleted.", change.value.join(", "));
                Ok(Outcome {
                    message: said(change.messages, single),
                    select: None,
                })
            }
            Op::Clone { source, new_name } => {
                let change = ws.clone_host(
                    &CloneSpec {
                        source: source.clone(),
                        new_name: new_name.trim().to_string(),
                        ..CloneSpec::default()
                    },
                    None,
                )?;
                let placed = change.value;
                Ok(Outcome {
                    message: said(change.messages, format!("{} added.", placed.name)),
                    select: Some(placed.name),
                })
            }
            Op::Move { name, section } => {
                let section = non_empty(section).ok_or_else(|| {
                    rustorm_core::Error::Usage("give a section name.".to_string())
                })?;
                let change = ws.move_host(name, None, Some(&section), None)?;
                let moved = change.value;
                let single = format!(
                    "{} moved to section {}.",
                    moved.new_name,
                    moved.section.clone().unwrap_or(section)
                );
                Ok(Outcome {
                    message: said(change.messages, single),
                    select: Some(moved.new_name),
                })
            }
            Op::Settings { name, changes } => {
                let change = ws.apply_settings(name, changes, None)?;
                Ok(Outcome {
                    message: said(change.messages, format!("{name} updated.")),
                    select: Some(name.clone()),
                })
            }
            Op::RenameSection { old, new, file } => {
                let new = non_empty(new).ok_or_else(|| {
                    rustorm_core::Error::Usage("a section name is required.".to_string())
                })?;
                let change = ws.rename_section(old, &new, file.as_deref())?;
                let single = match &change.value {
                    SectionRename::Renamed { from, to } => {
                        format!("section {from} renamed to {to}.")
                    }
                    SectionRename::Merged { from, into } => {
                        format!("section {from} merged into {into}.")
                    }
                };
                Ok(Outcome {
                    message: said(change.messages, single),
                    select: None,
                })
            }
            Op::AddSection { name } => {
                let name = non_empty(name).ok_or_else(|| {
                    rustorm_core::Error::Usage("a section name is required.".to_string())
                })?;
                let change = ws.add_section(&name, None, None)?;
                let single = match &change.value.catch_all {
                    Some((catch_all, n)) => format!(
                        "section {name} added; {catch_all} created with {n} host{}.",
                        if *n == 1 { "" } else { "s" }
                    ),
                    None => format!("section {name} added."),
                };
                Ok(Outcome {
                    message: said(change.messages, single),
                    select: None,
                })
            }
            Op::Reconcile {
                decisions,
                scope,
                kept,
            } => {
                let scope = match scope {
                    Some(p) => Some(vec![file_index(ws, p)?]),
                    None => None,
                };
                let report = ws.reconcile_report(scope.as_deref());
                let mut picked = Vec::new();
                for d in decisions {
                    let stale = || rustorm_core::Error::ReconcileStale(d.name.clone());
                    let i = report
                        .items
                        .iter()
                        .position(|p| p.answers_to(&d.name) && p.copy.file == d.copy)
                        .ok_or_else(stale)?;
                    let p = &report.items[i];
                    if p.copy.text != d.copy_text
                        || p.live.as_ref().map(|l| &l.text) != d.live_text.as_ref()
                    {
                        return Err(stale());
                    }
                    picked.push((i, d.decision.clone()));
                }
                let change = ws.apply_decisions(&report, &picked, None)?;
                // The core counts this run's decisions; the dialog also
                // counts the pairs kept live earlier.
                let mut messages = change.messages;
                messages.pop();
                messages.push(remaining_message(remaining_conflicts(ws, kept)));
                Ok(Outcome {
                    message: messages.join(" "),
                    select: None,
                })
            }
            Op::Retire { file, decided } => {
                let i = file_index(ws, file)?;
                let change = ws.retire(i, decided, WriteOptions::default())?;
                Ok(Outcome {
                    message: change.messages.join(" "),
                    select: None,
                })
            }
        }
    }
}

/// The text of the first host `name` answers to in `ws`, `None` when
/// absent.
pub fn host_text(ws: &Workspace, name: &str) -> Option<String> {
    ws.find_host(name).map(|wl| ws.host(wl).text())
}
