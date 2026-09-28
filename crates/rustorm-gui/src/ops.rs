//! The writes the GUI makes, each a sequence of `rustorm_core` operations.

use rustorm_core::{AddSpec, CloneSpec, EditSpec, Env, HostSelector, Workspace};

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
            Op::Add { name, .. } | Op::Delete(name) | Op::Move { name, .. } => vec![name.clone()],
            Op::Edit { original, name, .. } => vec![original.clone(), name.trim().to_string()],
            Op::Clone { source, new_name } => vec![source.clone(), new_name.trim().to_string()],
            Op::AddSection { .. } => Vec::new(),
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
        }
    }
}

/// The text of the first host `name` answers to in `ws`, `None` when
/// absent.
pub fn host_text(ws: &Workspace, name: &str) -> Option<String> {
    ws.find_host(name).map(|wl| ws.host(wl).text())
}
