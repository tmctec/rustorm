//! rustorm-core: the ssh_config model shared by the rustorm binaries.
//!
//! The crate parses `~/.ssh/config` into a line-preserving [`Config`], runs
//! every operation of the `rustorm` command line against it, and writes it
//! back atomically through [`ConfigFile`]. Lines an operation does not touch
//! render byte for byte as they were read.
//!
//! - [`Config`] and [`model`]: parse, render, sections and host blocks.
//! - [`banner`] and [`figlet`]: section banners in the FIGlet standard font.
//! - [`lexer`]: the syntax highlighter for the TUI and GUI editors.
//! - [`io`]: loading, backups and atomic writes.
//! - [`ops`]: one method on [`Config`] per `rustorm` command.
//! - [`userconfig`]: rustorm's own TOML config (command aliases, defaults).
//! - [`Error`]: one error type with CLI messages and exit codes.
#![warn(missing_docs)]

pub mod banner;
pub mod combine;
pub mod error;
pub mod figlet;
pub mod io;
pub mod keys;
pub mod lexer;
pub mod model;
pub mod ops;
pub mod uri;
pub mod userconfig;

pub use combine::{
    combine, glob_match, CombineInput, CombineReport, Conflict, DefaultsAdded, DefaultsSkipped,
    IncludeWarning, OnConflict,
};
pub use error::{Error, Result};
pub use io::{
    backup_path, default_config_path, resolve_config_path, write_text, ConfigFile, WriteOptions,
};
pub use keys::{canonical_key, is_known_key, is_multi_valued, KNOWN_KEYS, MULTI_VALUED_KEYS};
pub use lexer::{lex, lex_document, lex_line, Lexer, Span, SpanKind};
pub use model::{
    Banner, Config, Directive, DirectiveParts, Entry, HostBlock, HostLocation, Line, MatchBlock,
    Section,
};
pub use ops::{
    pair_up, parse_option, validate_name, AddSpec, CheckReport, CloneSpec, EditSpec, Env,
    HostSelector, ListRow, Matcher, Moved, Placed, Problem, ProblemKind, SectionAdded,
    SectionRename, SectionSummary, ShownHost, Unaliased,
};
pub use uri::ConnectionUri;
pub use userconfig::{ColorMode, UserConfig, UserDefaults};

/// Returns the crate version, for example `0.1.0`.
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
