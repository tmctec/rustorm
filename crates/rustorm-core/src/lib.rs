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
//! - [`include`](mod@include): `Include` resolution the way ssh reads it.
//! - [`keyspec`]: the value type and form group of every keyword.
//! - [`meta`]: the `# key: value` metadata comments above a host.
//! - [`projection`]: `--where`, `--filter`, `--format` and `--just-value`
//!   for the read commands.
//! - [`ops`]: one method on [`Config`] per `rustorm` command.
//! - [`workspace`]: the root and its included files as one [`Workspace`],
//!   with every command routed to the file it changes.
//! - [`userconfig`]: rustorm's own TOML config (command aliases, defaults).
//! - [`Error`]: one error type with CLI messages and exit codes.
#![warn(missing_docs)]

pub mod banner;
pub mod combine;
pub mod error;
pub mod figlet;
pub mod include;
pub mod io;
pub mod keys;
pub mod keyspec;
pub mod lexer;
pub mod meta;
pub mod model;
pub mod ops;
pub mod projection;
pub mod uri;
pub mod userconfig;
pub mod workspace;

pub use combine::{
    combine, CombineInput, CombineReport, Conflict, DefaultsAdded, DefaultsSkipped, IncludeWarning,
    OnConflict,
};
pub use error::{join_and, Error, Result};
pub use include::{
    expand_pattern, glob_match, include_lines, io_reason, looks_like_backup, resolve_include,
    resolve_includes, IncludeLine, IncludeMatch, IncludePlacement, IncludeStatus, IncludedFile,
};
pub use io::{
    absolute_path, backup_path, default_config_path, display_path, dot_backup_path,
    resolve_config_path, write_text, write_text_with_backup, ConfigFile, WriteOptions,
};
pub use keys::{canonical_key, is_known_key, is_multi_valued, KNOWN_KEYS, MULTI_VALUED_KEYS};
pub use keyspec::{
    complete_line, complete_setting, key_spec, key_specs, next_choice, premade_value, swap_value,
    validate_setting, Accepted, KeyGroup, KeySpec, KeyType, LineCompletion, SettingChange,
    SettingRow, SettingsDraft,
};
pub use lexer::{lex, lex_document, lex_line, Lexer, Span, SpanKind};
pub use meta::{is_meta_key, parse_meta_line, split_tags, validate_meta, HostMeta, MetaKey};
pub use model::{
    Banner, Config, Directive, DirectiveParts, Entry, HostBlock, HostLocation, Line, MatchBlock,
    Section,
};
pub use ops::{
    pair_up, parse_option, validate_name, AddSpec, CheckReport, CloneSpec, EditSpec, Env,
    HostSelector, ListRow, Matcher, Moved, Placed, Problem, ProblemKind, SectionAdded,
    SectionRename, SectionSummary, ShownHost, Unaliased,
};
pub use projection::{
    completion_keys, csv_field, missing, parse_filter, project, render, resolve, selected,
    yaml_scalar, Cell, Format, HostView, Projected, Value, Where, WhereOp,
};
pub use uri::ConnectionUri;
pub use userconfig::{ColorMode, UserConfig, UserDefaults};
pub use workspace::{
    Change, FileEntry, FileState, Workspace, WorkspaceLocation, WorkspaceRow, WorkspaceSection,
    WorkspaceShown,
};

/// Returns the crate version, for example `0.1.0`.
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
