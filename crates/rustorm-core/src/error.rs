//! The error type shared by every rustorm operation.
//!
//! `Display` gives the message the CLI prints after `error: `. The strings
//! for the errors in docs/cli.md's examples match those examples exactly.

use std::path::PathBuf;

/// Result alias used across rustorm-core.
pub type Result<T, E = Error> = std::result::Result<T, E>;

/// Every way a rustorm operation fails.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// `add` on a name that exists.
    #[error("{0} already exists. Use rustorm edit or rustorm set to modify it.")]
    HostExists(String),

    /// `edit` on a name that does not exist.
    #[error("{0} does not exist. Use rustorm add to create it.")]
    EditTargetMissing(String),

    /// A host named on the command line does not exist.
    #[error("{0} does not exist.")]
    HostNotFound(String),

    /// `clone` or `move` onto a name that exists.
    #[error("{0} already exists.")]
    TargetExists(String),

    /// `set -r` / `unset -r` matched nothing.
    #[error("no host matches {0}")]
    NoMatch(String),

    /// `move` without a new name or a section.
    #[error("give a new name, a --section, or both.")]
    MoveNeedsTarget,

    /// `delete-all` off a terminal without `--yes`.
    #[error("refusing to delete {0} hosts without --yes on a non-interactive terminal.")]
    RefuseDeleteAll(usize),

    /// `delete-all` confirmation answered no.
    #[error("nothing deleted.")]
    Declined,

    /// A section named on the command line does not exist.
    #[error("section {0} does not exist.")]
    SectionNotFound(String),

    /// `add-section` on a name that exists.
    #[error("section {0} already exists.")]
    SectionExists(String),

    /// `combine` under `--on-conflict fail` found duplicate names.
    #[error("{}", crate::combine::conflicts_message(.0))]
    CombineConflicts(Vec<crate::combine::Conflict>),

    /// A connection URI that does not parse.
    #[error("{uri} is not a valid connection URI: {reason}")]
    InvalidUri {
        /// The URI as given.
        uri: String,
        /// What is wrong with it.
        reason: String,
    },

    /// A host name with whitespace or `@`, an empty name, or `*` alone.
    #[error("{0} is not a valid host name.")]
    InvalidName(String),

    /// A regular expression that does not compile.
    #[error("invalid pattern {pattern}: {reason}")]
    InvalidPattern {
        /// The pattern as given.
        pattern: String,
        /// The regex engine's message.
        reason: String,
    },

    /// An alias that is already another host's name or alias.
    #[error("{alias} is already a name of {owner}.")]
    AliasTaken {
        /// The requested alias.
        alias: String,
        /// Primary name of the host that has it.
        owner: String,
    },

    /// `unalias` on a name that is no host's alias.
    #[error("{0} is not an alias of any host.")]
    AliasNotFound(String),

    /// `unalias NAME ALIAS` where the host lacks that alias.
    #[error("{host} has no alias {alias}.")]
    NotAnAliasOf {
        /// Primary name of the host.
        host: String,
        /// The alias it does not have.
        alias: String,
    },

    /// `unalias` naming a host's primary name.
    #[error("{0} is the primary name; use rustorm move to rename it.")]
    PrimaryName(String),

    /// Key/value arguments that do not pair up.
    #[error("keys and values must come in pairs.")]
    OddKeyValues,

    /// Any other usage error, message given.
    #[error("{0}")]
    Usage(String),

    /// An `-o` argument without `=`.
    #[error("{0} is not KEY=VALUE.")]
    InvalidOption(String),

    /// A key that rustorm-core refuses to set on a host (`Host`, `Match`).
    #[error("{0} cannot be set on a host.")]
    ForbiddenKey(String),

    /// A settings-form value that does not fit its keyword.
    #[error("{key} {reason}.")]
    InvalidSetting {
        /// The keyword, canonical case.
        key: String,
        /// Why, e.g. "must be a port from 1 to 65535".
        reason: String,
    },

    /// The config file could not be read.
    #[error("cannot read {path}: {source}")]
    Read {
        /// The file.
        path: PathBuf,
        /// The underlying I/O error.
        source: std::io::Error,
    },

    /// The config file, its backup or a named copy could not be written.
    #[error("cannot write {path}: {source}")]
    Write {
        /// The file.
        path: PathBuf,
        /// The underlying I/O error.
        source: std::io::Error,
    },

    /// `add` on a name that exists in a workspace of several files.
    #[error("{name} already exists in {file}. Use rustorm edit or rustorm set to modify it.")]
    HostExistsIn {
        /// The host name.
        name: String,
        /// The file holding it, as text output prints paths.
        file: String,
    },

    /// A section name held by two or more workspace files, named by a write
    /// without `--file`.
    #[error("section {name} exists in {}. Say which with --file.", join_and(.files))]
    AmbiguousSection {
        /// The section name as given.
        name: String,
        /// The files holding it, in load order, as text output prints them.
        files: Vec<String>,
    },

    /// A `--file` bare name matching two or more loaded files.
    #[error("file {name} matches {}. Give a path.", join_and(.files))]
    AmbiguousFile {
        /// The name as given.
        name: String,
        /// The matching files, in load order, as text output prints them.
        files: Vec<String>,
    },

    /// A `--file` that names no workspace file and that no `Include`
    /// pattern matches.
    #[error("no such file {0} in the workspace.")]
    UnknownFile(String),

    /// A write routed to an included file that cannot be read (D24).
    #[error("cannot read {file} ({reason}).")]
    UnreadableInclude {
        /// The file, as text output prints paths.
        file: String,
        /// Why, for example `permission denied`.
        reason: String,
    },

    /// rustorm's own TOML config does not parse.
    #[error("invalid rustorm config {path}: {reason}")]
    UserConfig {
        /// The TOML file.
        path: PathBuf,
        /// The parser's message.
        reason: String,
    },
}

impl Error {
    /// The process exit status for this error: 1 when the operation was
    /// refused, 2 for usage errors, 3 for config-file I/O.
    pub fn exit_code(&self) -> i32 {
        match self {
            Error::HostExists(_)
            | Error::EditTargetMissing(_)
            | Error::HostNotFound(_)
            | Error::TargetExists(_)
            | Error::NoMatch(_)
            | Error::RefuseDeleteAll(_)
            | Error::Declined
            | Error::SectionNotFound(_)
            | Error::SectionExists(_)
            | Error::CombineConflicts(_)
            | Error::InvalidUri { .. }
            | Error::InvalidName(_)
            | Error::AliasTaken { .. }
            | Error::AliasNotFound(_)
            | Error::NotAnAliasOf { .. }
            | Error::PrimaryName(_)
            | Error::ForbiddenKey(_)
            | Error::InvalidSetting { .. }
            | Error::HostExistsIn { .. }
            | Error::AmbiguousSection { .. }
            | Error::AmbiguousFile { .. }
            | Error::UnknownFile(_) => 1,
            Error::MoveNeedsTarget
            | Error::InvalidPattern { .. }
            | Error::OddKeyValues
            | Error::InvalidOption(_)
            | Error::Usage(_) => 2,
            Error::Read { .. }
            | Error::Write { .. }
            | Error::UserConfig { .. }
            | Error::UnreadableInclude { .. } => 3,
        }
    }
}

/// Joins `a`, `a and b`, `a, b and c`.
pub fn join_and(items: &[String]) -> String {
    match items {
        [] => String::new(),
        [one] => one.clone(),
        [init @ .., last] => format!("{} and {last}", init.join(", ")),
    }
}
