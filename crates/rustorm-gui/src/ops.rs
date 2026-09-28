//! The writes the GUI makes, each a sequence of `rustorm_core` operations.

use rustorm_core::{AddSpec, CloneSpec, Config, EditSpec, Env, HostSelector};

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

    /// Applies the operation to `config` through the core. On `Err` the
    /// caller discards `config`, so a partial sequence never reaches disk.
    pub fn apply(&self, config: &mut Config, env: &Env) -> rustorm_core::Result<Outcome> {
        match self {
            Op::Add {
                name,
                uri,
                identity,
                section,
            } => {
                let placed = config.add(
                    &AddSpec {
                        name: name.trim().to_string(),
                        uri: uri.trim().to_string(),
                        identity: non_empty(identity),
                        options: Vec::new(),
                        section: non_empty(section),
                    },
                    env,
                )?;
                Ok(Outcome {
                    message: match &placed.section {
                        Some(s) => format!("{} added to section {s}.", placed.name),
                        None => format!("{} added.", placed.name),
                    },
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
                let loc = config
                    .find_host(original)
                    .ok_or_else(|| rustorm_core::Error::EditTargetMissing(original.clone()))?;
                let current = config.section_name(loc).map(str::to_string);
                let mut target = config.host(loc).primary();
                let new_name = name.trim();
                if new_name != target {
                    config.move_host(&target, Some(new_name), None)?;
                    target = new_name.to_string();
                }
                let section = non_empty(section).filter(|s| {
                    current
                        .as_deref()
                        .is_none_or(|c| !c.eq_ignore_ascii_case(s))
                });
                let identity = non_empty(identity);
                let remove_identity = identity.is_none();
                config.edit(
                    &EditSpec {
                        name: target.clone(),
                        uri: uri.trim().to_string(),
                        identity,
                        options: Vec::new(),
                        section,
                    },
                    env,
                )?;
                if remove_identity {
                    config.unset(
                        &HostSelector::Name(target.clone()),
                        &["IdentityFile".to_string()],
                    )?;
                }
                Ok(Outcome {
                    message: format!("{target} updated."),
                    select: Some(target),
                })
            }
            Op::Delete(name) => {
                let deleted = config.delete(std::slice::from_ref(name))?;
                Ok(Outcome {
                    message: format!("{} deleted.", deleted.join(", ")),
                    select: None,
                })
            }
            Op::Clone { source, new_name } => {
                let placed = config.clone_host(&CloneSpec {
                    source: source.clone(),
                    new_name: new_name.trim().to_string(),
                    ..CloneSpec::default()
                })?;
                Ok(Outcome {
                    message: format!("{} added.", placed.name),
                    select: Some(placed.name),
                })
            }
            Op::Move { name, section } => {
                let section = non_empty(section).ok_or_else(|| {
                    rustorm_core::Error::Usage("give a section name.".to_string())
                })?;
                let moved = config.move_host(name, None, Some(&section))?;
                Ok(Outcome {
                    message: format!(
                        "{} moved to section {}.",
                        moved.new_name,
                        moved.section.unwrap_or(section)
                    ),
                    select: Some(moved.new_name),
                })
            }
            Op::AddSection { name } => {
                let name = non_empty(name).ok_or_else(|| {
                    rustorm_core::Error::Usage("a section name is required.".to_string())
                })?;
                let added = config.add_section(&name, None)?;
                Ok(Outcome {
                    message: match added.catch_all {
                        Some((catch_all, n)) => format!(
                            "section {name} added; {catch_all} created with {n} host{}.",
                            if n == 1 { "" } else { "s" }
                        ),
                        None => format!("section {name} added."),
                    },
                    select: None,
                })
            }
        }
    }
}

/// The text of the host `name` answers to in `config`, `None` when absent.
pub fn host_text(config: &Config, name: &str) -> Option<String> {
    config.find_host(name).map(|l| config.host(l).text())
}
