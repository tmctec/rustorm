//! The command-line grammar (docs/cli.md), declared with clap derive.

use std::path::PathBuf;

use clap::{Args, Parser, Subcommand, ValueEnum};

/// rustorm manages the hosts in your ~/.ssh/config.
#[derive(Debug, Parser)]
#[command(
    name = "rustorm",
    about = "Manage the hosts in your ~/.ssh/config",
    disable_version_flag = true,
    subcommand_required = false,
    arg_required_else_help = false
)]
pub struct Cli {
    /// Operate on FILE instead of ~/.ssh/config
    #[arg(short = 'c', long, global = true, value_name = "FILE")]
    pub config: Option<PathBuf>,

    /// Do not write <config>~ before changing the file
    #[arg(long, global = true)]
    pub no_backup: bool,

    /// Emit JSON on list, show, dump, search, check, includes and reconcile
    #[arg(long, global = true)]
    pub json: bool,

    /// Disable ANSI color (NO_COLOR does the same)
    #[arg(long, global = true)]
    pub no_color: bool,

    /// Suppress success messages
    #[arg(short, long, global = true)]
    pub quiet: bool,

    /// Section for add, edit, clone, move, list, show and search
    #[arg(short, long, global = true, value_name = "NAME")]
    pub section: Option<String>,

    /// The workspace file to write (or dump), overriding routing: a loaded
    /// file's name such as cypress, or a path
    #[arg(
        id = "workspace_file",
        short = 'f',
        long = "file",
        global = true,
        value_name = "NAME|FILE"
    )]
    pub file: Option<String>,

    /// Print the version and exit
    #[arg(short = 'V', long, global = true)]
    pub version: bool,

    #[command(subcommand)]
    pub command: Option<Cmd>,
}

/// Shells `completion` generates scripts for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum Shell {
    /// GNU bash
    Bash,
    /// Z shell
    Zsh,
    /// fish
    Fish,
    /// PowerShell
    Powershell,
}

/// `--format` names (docs/cli.md, Reading output).
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum FormatName {
    /// Lines as the file spells them
    Txt,
    /// One JSON document
    Json,
    /// RFC 4180 rows
    Csv,
    /// A YAML sequence
    Yaml,
    /// Same as yaml
    Yml,
}

/// The read options `list`, `show` and `search` share (docs/cli.md,
/// Reading output).
#[derive(Debug, Clone, Default, Args)]
pub struct ReadArgs {
    /// Keep hosts where KEY=VALUE, KEY!=VALUE or KEY~PATTERN holds (repeatable; all must hold)
    #[arg(long = "where", value_name = "KEY=VALUE")]
    pub where_: Vec<String>,
    /// Print only these keys, comma-separated, in this order
    #[arg(long, value_name = "KEYS")]
    pub filter: Option<String>,
    /// Output format
    #[arg(long, value_name = "FMT", value_enum)]
    pub format: Option<FormatName>,
    /// Print values only, without keys
    #[arg(long)]
    pub just_value: bool,
    /// Exit 0 when a filtered key is not set on a host
    #[arg(long)]
    pub allow_missing: bool,
}

/// Every rustorm command.
#[derive(Debug, Subcommand)]
pub enum Cmd {
    /// Add a host from a connection URI
    Add {
        /// New host name
        #[arg(value_name = "NAME")]
        name: String,
        /// [user@]host[:port]
        #[arg(value_name = "URI")]
        uri: String,
        /// Write IdentityFile FILE
        #[arg(short, long, value_name = "FILE")]
        identity: Option<String>,
        /// Any ssh_config directive as KEY=VALUE (repeatable)
        #[arg(short, long = "option", value_name = "KEY=VALUE")]
        option: Vec<String>,
    },
    /// Replace HostName, User and Port of a host from a URI
    Edit {
        /// Existing host name
        #[arg(value_name = "NAME")]
        name: String,
        /// [user@]host[:port]
        #[arg(value_name = "URI")]
        uri: String,
        /// Write IdentityFile FILE
        #[arg(short, long, value_name = "FILE")]
        identity: Option<String>,
        /// Any ssh_config directive as KEY=VALUE (repeatable)
        #[arg(short, long = "option", value_name = "KEY=VALUE")]
        option: Vec<String>,
    },
    /// Set one or more keys on a host
    #[command(visible_alias = "update")]
    Set {
        /// Treat NAME as a regular expression over host names
        #[arg(short, long)]
        regex: bool,
        /// Add a value to a multi-valued key instead of replacing
        #[arg(short, long)]
        append: bool,
        /// Add a tag to the host's # tags: line (repeatable)
        #[arg(long, value_name = "TAG")]
        tag: Vec<String>,
        /// Remove a tag from the host's # tags: line (repeatable)
        #[arg(long, value_name = "TAG")]
        untag: Vec<String>,
        /// Existing host name, or pattern with --regex
        #[arg(value_name = "NAME")]
        name: String,
        /// KEY VALUE pairs (a value starting with - goes after --)
        #[arg(value_name = "KEY VALUE")]
        pairs: Vec<String>,
    },
    /// Remove keys from a host
    Unset {
        /// Treat NAME as a regular expression over host names
        #[arg(short, long)]
        regex: bool,
        /// Existing host name, or pattern with --regex
        #[arg(value_name = "NAME")]
        name: String,
        /// Keys to remove
        #[arg(value_name = "KEY", required = true)]
        keys: Vec<String>,
    },
    /// Copy a host to a new name
    #[command(visible_aliases = ["copy", "cp"])]
    Clone {
        /// Copy HostName verbatim
        #[arg(long)]
        keep_hostname: bool,
        /// Existing host name to copy
        #[arg(value_name = "NAME")]
        name: String,
        /// Name of the copy
        #[arg(value_name = "NEW-NAME")]
        new_name: String,
        /// KEY VALUE overrides for the copy
        #[arg(value_name = "KEY VALUE", allow_hyphen_values = true)]
        pairs: Vec<String>,
    },
    /// Rename a host, move it to a section, or both
    #[command(name = "move", visible_aliases = ["rename", "mv"])]
    Move {
        /// Existing host name
        #[arg(value_name = "NAME")]
        name: String,
        /// New name
        #[arg(value_name = "NEW-NAME")]
        new_name: Option<String>,
    },
    /// Remove hosts
    #[command(visible_aliases = ["rm", "del"])]
    Delete {
        /// Existing host names to delete
        #[arg(value_name = "NAME", required = true)]
        names: Vec<String>,
    },
    /// Remove every host
    #[command(visible_alias = "delete_all")]
    DeleteAll {
        /// Do not ask for confirmation
        #[arg(short, long)]
        yes: bool,
    },
    /// List hosts
    #[command(visible_alias = "ls")]
    List {
        /// Also print every other key and the Host * defaults
        #[arg(short, long)]
        long: bool,
        /// Print only names
        #[arg(short, long)]
        names: bool,
        #[command(flatten)]
        read: ReadArgs,
    },
    /// Print entries verbatim
    Show {
        /// Existing host names or aliases (optional with --where or --section)
        #[arg(value_name = "NAME")]
        names: Vec<String>,
        #[command(flatten)]
        read: ReadArgs,
    },
    /// Print the whole config file as parsed
    #[command(visible_alias = "cat")]
    Dump,
    /// Find hosts by regular expression
    #[command(visible_aliases = ["find", "grep"])]
    Search {
        /// Search the literal text
        #[arg(short = 'F', long)]
        fixed_strings: bool,
        /// Regular expression
        #[arg(value_name = "PATTERN", allow_hyphen_values = true)]
        pattern: String,
        #[command(flatten)]
        read: ReadArgs,
    },
    /// Add names to a host's Host line
    Alias {
        /// Existing host name
        #[arg(value_name = "NAME")]
        name: String,
        /// Aliases to add
        #[arg(value_name = "ALIAS", required = true)]
        aliases: Vec<String>,
    },
    /// Remove names from a host's Host line
    Unalias {
        /// Existing host name, then aliases; or one alias
        #[arg(value_name = "NAME|ALIAS", required = true)]
        args: Vec<String>,
    },
    /// List sections with host counts
    Sections,
    /// Create an empty section
    AddSection {
        /// Section name
        #[arg(value_name = "NAME")]
        name: String,
        /// Insert in front of this section instead of before the catch-all
        #[arg(long, value_name = "SECTION")]
        before: Option<String>,
    },
    /// Merge two or more config files into the first
    #[command(visible_alias = "merge")]
    Combine {
        /// Files to merge; the first is the base and the default output
        #[arg(value_name = "FILE", required = true)]
        files: Vec<PathBuf>,
        /// Write the result here instead of the first FILE
        #[arg(short, long, value_name = "FILE")]
        output: Option<PathBuf>,
        /// What to do with a name two files define: fail, keep or replace
        #[arg(long, value_name = "POLICY", default_value = "fail")]
        on_conflict: String,
        /// Print the result instead of writing a file
        #[arg(long)]
        stdout: bool,
    },
    /// Rename a section, or merge it into another
    RenameSection {
        /// Current name
        #[arg(value_name = "OLD")]
        old: String,
        /// New name
        #[arg(value_name = "NEW")]
        new: String,
    },
    /// Copy the config file
    Backup {
        /// Destination (default <config>~)
        #[arg(value_name = "FILE")]
        file: Option<PathBuf>,
    },
    /// Report problems in the config file
    #[command(visible_alias = "lint")]
    Check,
    /// List the Include lines, the files they load and their host counts
    Includes,
    /// Resolve hosts defined in two or more workspace files
    #[command(visible_alias = "resolve")]
    Reconcile {
        /// Only the copies in these workspace files (a loaded file's name or a path)
        #[arg(value_name = "FILE")]
        files: Vec<String>,
        /// Print the report; decide nothing
        #[arg(long)]
        list: bool,
        /// Take the copy for HOST (repeatable)
        #[arg(long, value_name = "HOST")]
        take_copy: Vec<String>,
        /// Keep the live definition of HOST, or leave the orphan HOST out (repeatable)
        #[arg(long, value_name = "HOST")]
        keep_live: Vec<String>,
        /// Move the orphan HOST into the root or the --file target (repeatable)
        #[arg(long, value_name = "HOST")]
        add: Vec<String>,
        /// Keep the live definition of every other conflict
        #[arg(long)]
        all_live: bool,
        /// Take the copy of every other conflict
        #[arg(long)]
        all_copy: bool,
        /// Remove every identical copy from its file
        #[arg(long)]
        drop_identical: bool,
        /// Move each named FILE to ~/.ssh/retired/ once fully resolved
        #[arg(long)]
        retire: bool,
    },
    /// Print a shell completion script
    Completion {
        /// Shell
        #[arg(value_enum)]
        shell: Shell,
    },
    /// Print the version
    Version,
}

impl Cmd {
    /// The canonical command name.
    pub fn name(&self) -> &'static str {
        match self {
            Cmd::Add { .. } => "add",
            Cmd::Edit { .. } => "edit",
            Cmd::Set { .. } => "set",
            Cmd::Unset { .. } => "unset",
            Cmd::Clone { .. } => "clone",
            Cmd::Move { .. } => "move",
            Cmd::Delete { .. } => "delete",
            Cmd::DeleteAll { .. } => "delete-all",
            Cmd::List { .. } => "list",
            Cmd::Show { .. } => "show",
            Cmd::Dump => "dump",
            Cmd::Search { .. } => "search",
            Cmd::Alias { .. } => "alias",
            Cmd::Unalias { .. } => "unalias",
            Cmd::Sections => "sections",
            Cmd::AddSection { .. } => "add-section",
            Cmd::Combine { .. } => "combine",
            Cmd::RenameSection { .. } => "rename-section",
            Cmd::Backup { .. } => "backup",
            Cmd::Check => "check",
            Cmd::Includes => "includes",
            Cmd::Reconcile { .. } => "reconcile",
            Cmd::Completion { .. } => "completion",
            Cmd::Version => "version",
        }
    }

    /// True when the command takes `-s, --section`.
    pub fn takes_section(&self) -> bool {
        matches!(
            self,
            Cmd::Add { .. }
                | Cmd::Edit { .. }
                | Cmd::Clone { .. }
                | Cmd::Move { .. }
                | Cmd::List { .. }
                | Cmd::Show { .. }
                | Cmd::Search { .. }
        )
    }

    /// True when the command takes `-f, --file`: the writes (the host
    /// edits pick which definition to change with it), `dump`, and
    /// `reconcile` (the `--add` destination).
    pub fn takes_file(&self) -> bool {
        matches!(
            self,
            Cmd::Add { .. }
                | Cmd::Edit { .. }
                | Cmd::Set { .. }
                | Cmd::Unset { .. }
                | Cmd::Clone { .. }
                | Cmd::Move { .. }
                | Cmd::Delete { .. }
                | Cmd::DeleteAll { .. }
                | Cmd::Alias { .. }
                | Cmd::Unalias { .. }
                | Cmd::AddSection { .. }
                | Cmd::RenameSection { .. }
                | Cmd::Dump
                | Cmd::Reconcile { .. }
        )
    }
}
