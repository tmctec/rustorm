//! Every `rustorm` command as a method on [`Config`].
//!
//! Operations change the in-memory model only; [`crate::ConfigFile::save`]
//! writes it. Each writing operation validates all of its input before it
//! changes anything, so an `Err` leaves the model untouched, and it re-sorts
//! the hosts of every section when it succeeds.

use std::path::PathBuf;

use serde::Serialize;

pub use crate::combine::{combine, CombineInput, CombineReport, OnConflict};
use crate::keys;
use crate::keyspec::SettingChange;
use crate::model::{Banner, Config, Entry, HostBlock, HostLocation, Line, Section};
use crate::uri::ConnectionUri;
use crate::{Error, Result};

/// Process context that operations read: the login user and home directory.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Env {
    /// `$USER`, the last fallback for a host's user.
    pub user: Option<String>,
    /// The home directory, for `~` in `IdentityFile` paths.
    pub home: Option<PathBuf>,
}

impl Env {
    /// Reads `$USER` and the home directory of the running process.
    pub fn from_process() -> Env {
        Env {
            user: std::env::var("USER").ok().filter(|u| !u.is_empty()),
            home: dirs::home_dir(),
        }
    }
}

/// Checks that `name` can be a host name or alias: not empty, no whitespace,
/// no `@`, and not `*` alone.
pub fn validate_name(name: &str) -> Result<()> {
    if name.is_empty() || name == "*" || name.contains('@') || name.chars().any(char::is_whitespace)
    {
        return Err(Error::InvalidName(name.to_string()));
    }
    Ok(())
}

/// Splits an `-o KEY=VALUE` argument on the first `=`.
///
/// ```
/// let (k, v) = rustorm_core::parse_option("ProxyCommand=ssh -o A=B bastion").unwrap();
/// assert_eq!((k.as_str(), v.as_str()), ("ProxyCommand", "ssh -o A=B bastion"));
/// ```
pub fn parse_option(arg: &str) -> Result<(String, String)> {
    match arg.split_once('=') {
        Some((k, v)) if !k.trim().is_empty() => Ok((k.trim().to_string(), v.to_string())),
        _ => Err(Error::InvalidOption(arg.to_string())),
    }
}

/// Pairs up `KEY VALUE KEY VALUE …` arguments. An odd count is
/// [`Error::OddKeyValues`].
pub fn pair_up(args: &[String]) -> Result<Vec<(String, String)>> {
    if !args.len().is_multiple_of(2) {
        return Err(Error::OddKeyValues);
    }
    Ok(args
        .chunks(2)
        .map(|c| (c[0].clone(), c[1].clone()))
        .collect())
}

pub(crate) fn check_settable(key: &str) -> Result<()> {
    if key.eq_ignore_ascii_case("host") || key.eq_ignore_ascii_case("match") {
        return Err(Error::ForbiddenKey(key.to_string()));
    }
    Ok(())
}

pub(crate) fn check_settings(changes: &[SettingChange]) -> Result<()> {
    for c in changes {
        check_settable(&c.key)?;
        let key = crate::canonical_key(&c.key);
        if c.values.len() > 1 && !keys::is_multi_valued(&key) {
            return Err(Error::InvalidSetting {
                key,
                reason: "takes one value".into(),
            });
        }
        for v in &c.values {
            crate::validate_setting(&key, v).map_err(|reason| Error::InvalidSetting {
                key: key.clone(),
                reason,
            })?;
        }
    }
    Ok(())
}

pub(crate) fn apply_settings(block: &mut HostBlock, changes: &[SettingChange]) {
    for c in changes {
        let mut values = c.values.iter();
        match values.next() {
            None => {
                block.unset(&c.key);
            }
            Some(first) => {
                block.set(&c.key, first.trim());
                for v in values {
                    block.append(&c.key, v.trim());
                }
            }
        }
    }
}

pub(crate) fn apply_pairs(block: &mut HostBlock, pairs: &[(String, String)], append: bool) {
    for (k, v) in pairs {
        if append && keys::is_multi_valued(k) {
            block.append(k, v);
        } else {
            block.set(k, v);
        }
    }
}

/// Selects the hosts a `set` or `unset` applies to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostSelector {
    /// One host by primary name or alias.
    Name(String),
    /// Every host whose primary name matches the pattern, anchored to the
    /// whole name (`^(?:pattern)$`). `Host *` is never selected.
    Regex(String),
}

/// Input for [`Config::add`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AddSpec {
    /// The new host's name.
    pub name: String,
    /// `[user@]host[:port]`.
    pub uri: String,
    /// `-i`: written as `IdentityFile`.
    pub identity: Option<String>,
    /// `-o KEY=VALUE` pairs, in order.
    pub options: Vec<(String, String)>,
    /// `-s`: the destination section, created when missing.
    pub section: Option<String>,
}

/// Input for [`Config::edit`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EditSpec {
    /// The host to edit (primary name or alias).
    pub name: String,
    /// `[user@]host[:port]`.
    pub uri: String,
    /// `-i`: replaces `IdentityFile`.
    pub identity: Option<String>,
    /// `-o KEY=VALUE` pairs, each replacing the key.
    pub options: Vec<(String, String)>,
    /// `-s`: move the host to this section, created when missing.
    pub section: Option<String>,
}

/// Input for [`Config::clone_host`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CloneSpec {
    /// The host to copy (primary name or alias).
    pub source: String,
    /// The copy's name.
    pub new_name: String,
    /// Copy `HostName` verbatim instead of substituting the new name.
    pub keep_hostname: bool,
    /// Key/value pairs that override or add keys on the copy.
    pub overrides: Vec<(String, String)>,
    /// The copy's section; the source's section when `None`.
    pub section: Option<String>,
}

/// Where a new or moved host landed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Placed {
    /// The host's primary name.
    pub name: String,
    /// Its section, `None` in an unsectioned file or the preamble.
    pub section: Option<String>,
}

/// The result of [`Config::move_host`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Moved {
    /// The primary name before the move.
    pub old_name: String,
    /// The primary name after the move.
    pub new_name: String,
    /// The section after the move, when a section was requested.
    pub section: Option<String>,
}

/// The result of [`Config::unalias`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Unaliased {
    /// The host's primary name.
    pub host: String,
    /// Every name the host answers to afterwards.
    pub names: Vec<String>,
}

/// One row of [`Config::sections`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SectionSummary {
    /// The section name.
    pub name: String,
    /// Number of hosts in it, `Host *` excluded.
    pub hosts: usize,
    /// True for the last section, which receives hosts added without
    /// `--section`.
    pub catch_all: bool,
}

/// The result of [`Config::add_section`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SectionAdded {
    /// The new section's name as given.
    pub name: String,
    /// The section it was inserted in front of, when `--before` was given.
    pub before: Option<String>,
    /// The catch-all created alongside on a file that had no sections, with
    /// the number of hosts it received.
    pub catch_all: Option<(String, usize)>,
}

/// The result of [`Config::rename_section`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum SectionRename {
    /// The section got a new name and a regenerated banner.
    Renamed {
        /// The old name.
        from: String,
        /// The new name.
        to: String,
    },
    /// The new name belonged to another section; the hosts moved there and
    /// the old banner is gone.
    Merged {
        /// The removed section.
        from: String,
        /// The section that received its hosts.
        into: String,
    },
}

/// One host as `show` prints it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ShownHost {
    /// The host's primary name.
    pub name: String,
    /// Its section, if any.
    pub section: Option<String>,
    /// The entry verbatim: comments directly above, `Host` line and body.
    pub text: String,
}

/// One row of `list`, with user and port resolved.
///
/// Serializes to the `--json list` shape of docs/cli.md: `name`, `section`,
/// `aliases`, `hostname`, `user`, `port`, `options`, plus `proxy_command`
/// and `proxy_jump`. `options` is an object in file order; a multi-valued
/// key maps to an array of its values.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ListRow {
    /// Primary name.
    pub name: String,
    /// Section name, `None` in an unsectioned file or the preamble.
    pub section: Option<String>,
    /// Names after the first.
    pub aliases: Vec<String>,
    /// `HostName`, `None` when absent.
    pub hostname: Option<String>,
    /// `User`, else `Host *`'s `User`, else `$USER` (empty when unknown).
    pub user: String,
    /// `Port`, else `Host *`'s `Port`, else 22.
    pub port: u16,
    /// Every other directive in file order, keys in canonical case.
    #[serde(serialize_with = "serialize_options")]
    pub options: Vec<(String, String)>,
    /// `ProxyCommand`, when set.
    pub proxy_command: Option<String>,
    /// `ProxyJump`, when set.
    pub proxy_jump: Option<String>,
}

pub(crate) fn serialize_options<S: serde::Serializer>(
    options: &[(String, String)],
    serializer: S,
) -> std::result::Result<S::Ok, S::Error> {
    use serde::ser::SerializeMap;
    let mut keys_in_order: Vec<&str> = Vec::new();
    for (k, _) in options {
        if !keys_in_order.contains(&k.as_str()) {
            keys_in_order.push(k);
        }
    }
    let mut map = serializer.serialize_map(Some(keys_in_order.len()))?;
    for key in keys_in_order {
        let values: Vec<&str> = options
            .iter()
            .filter(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
            .collect();
        if keys::is_multi_valued(key) {
            map.serialize_entry(key, &values)?;
        } else {
            map.serialize_entry(key, values[0])?;
        }
    }
    map.end()
}

impl ListRow {
    /// `user@hostname:port`, or `[no hostname]` when `HostName` is absent.
    /// An IPv6 hostname is bracketed.
    pub fn target(&self) -> String {
        match &self.hostname {
            None => "[no hostname]".to_string(),
            Some(h) if h.contains(':') => format!("{}@[{}]:{}", self.user, h, self.port),
            Some(h) => format!("{}@{}:{}", self.user, h, self.port),
        }
    }

    /// The row as `list` prints it without alignment: `name -> target`.
    pub fn line(&self) -> String {
        format!("{} -> {}", self.name, self.target())
    }
}

/// A compiled `search` pattern.
#[derive(Debug, Clone)]
pub struct Matcher {
    regex: regex::Regex,
}

impl Matcher {
    /// Compiles `pattern` as a regular expression, or as literal text when
    /// `fixed`. An invalid regex is [`Error::InvalidPattern`].
    pub fn new(pattern: &str, fixed: bool) -> Result<Matcher> {
        let source = if fixed {
            regex::escape(pattern)
        } else {
            pattern.to_string()
        };
        regex::Regex::new(&source)
            .map(|regex| Matcher { regex })
            .map_err(|e| Error::InvalidPattern {
                pattern: pattern.to_string(),
                reason: e.to_string(),
            })
    }

    /// True when the pattern occurs in `text`.
    pub fn is_match(&self, text: &str) -> bool {
        self.regex.is_match(text)
    }

    /// Byte ranges of every match in `text`, for highlighting.
    pub fn find_ranges(&self, text: &str) -> Vec<std::ops::Range<usize>> {
        self.regex.find_iter(text).map(|m| m.range()).collect()
    }
}

fn anchored(pattern: &str) -> Result<regex::Regex> {
    regex::Regex::new(&format!("^(?:{pattern})$")).map_err(|e| Error::InvalidPattern {
        pattern: pattern.to_string(),
        reason: e.to_string(),
    })
}

/// What `check` found wrong.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProblemKind {
    /// A key that is not an ssh_config(5) keyword.
    UnknownKey,
    /// A single-valued key set more than once in one entry.
    DuplicateKey,
    /// A host without `HostName` (wildcard patterns are exempt).
    MissingHostName,
    /// An `IdentityFile` path that does not exist.
    MissingIdentityFile,
    /// A name used by more than one entry.
    DuplicateName,
    /// A line that is neither blank, a comment nor a directive.
    UnparsableLine,
    /// A host name defined in two or more workspace files.
    DuplicateAcrossFiles,
    /// An `Include` that loads a file whose name looks like a backup.
    IncludeLoadsBackup,
    /// An `Include` inside `Host *`, read as global.
    IncludeInsideHostStar,
    /// An `Include` inside another `Host` or a `Match` block; rustorm loads
    /// it for every host.
    IncludeInsideBlock,
    /// An included file that cannot be read (D24).
    IncludeUnreadable,
}

/// One problem `check` reports.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Problem {
    /// What is wrong.
    pub kind: ProblemKind,
    /// The host it concerns, when there is one.
    pub host: Option<String>,
    /// The 1-based line number, for unparsable lines.
    pub line: Option<usize>,
    /// The description without the host prefix.
    pub detail: String,
    /// The absolute path of the file the problem is in (D23). Set by
    /// [`crate::Workspace::check`]; `None` from [`Config::check`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file: Option<PathBuf>,
    /// On a workspace of several files, the file as text output prints it,
    /// shown before `line N` of an unparsable line.
    #[serde(skip)]
    pub file_label: Option<String>,
}

impl std::fmt::Display for Problem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match (&self.host, self.line) {
            (Some(h), _) => write!(f, "{h}: {}", self.detail),
            (None, Some(n)) => match &self.file_label {
                Some(file) => write!(f, "{file} line {n}: {}", self.detail),
                None => write!(f, "line {n}: {}", self.detail),
            },
            (None, None) => write!(f, "{}", self.detail),
        }
    }
}

/// The result of [`Config::check`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CheckReport {
    /// Every problem, in file order per kind of check.
    pub problems: Vec<Problem>,
    /// Number of hosts checked, `Host *` excluded.
    pub hosts: usize,
}

fn plural(n: usize, word: &str) -> String {
    if n == 1 {
        format!("{n} {word}")
    } else {
        format!("{n} {word}s")
    }
}

impl CheckReport {
    /// True when no problem was found.
    pub fn is_clean(&self) -> bool {
        self.problems.is_empty()
    }

    /// The closing line: `2 problems in 14 hosts.` or
    /// `no problems in 14 hosts.`
    pub fn summary(&self) -> String {
        let hosts = plural(self.hosts, "host");
        if self.problems.is_empty() {
            format!("no problems in {hosts}.")
        } else {
            format!("{} in {hosts}.", plural(self.problems.len(), "problem"))
        }
    }
}

fn expand_identity(value: &str, home: Option<&std::path::Path>) -> Option<PathBuf> {
    let value = value.trim_matches('"');
    if value.eq_ignore_ascii_case("none") || value.contains('$') {
        return None;
    }
    let (base, rest) = if let Some(rest) = value.strip_prefix("~/") {
        (home?.to_path_buf(), rest)
    } else if let Some(rest) = value.strip_prefix("%d/") {
        (home?.to_path_buf(), rest)
    } else if let Some(rest) = value.strip_prefix('/') {
        (PathBuf::from("/"), rest)
    } else {
        (home?.to_path_buf(), value)
    };
    if rest.contains('%') || value.starts_with('~') && !value.starts_with("~/") {
        return None;
    }
    Some(base.join(rest))
}

/// The copy `clone` makes of `source`: its body under `Host NEW-NAME`,
/// `HostName` rewritten unless `keep_hostname`, then the overrides.
pub(crate) fn clone_block(source: &HostBlock, spec: &CloneSpec) -> HostBlock {
    let mut block = HostBlock::new(std::slice::from_ref(&spec.new_name));
    block.body = source.body.clone();
    if !spec.keep_hostname {
        if let Some(hn) = block.get("HostName") {
            if hn.contains(&spec.source) {
                block.set("HostName", &hn.replace(&spec.source, &spec.new_name));
            }
        }
    }
    apply_pairs(&mut block, &spec.overrides, false);
    block.last_line_mut().ensure_terminated();
    block
}

impl Config {
    pub(crate) fn finish(&mut self) {
        self.sort_sections();
    }

    fn resolve_user_port(
        uri: &ConnectionUri,
        env: &Env,
        defaults: Option<&HostBlock>,
    ) -> (String, u16) {
        let user = uri
            .user
            .clone()
            .or_else(|| defaults.and_then(|d| d.get("User")))
            .or_else(|| env.user.clone())
            .unwrap_or_default();
        let port = uri
            .port
            .or_else(|| {
                defaults
                    .and_then(|d| d.get("Port"))
                    .and_then(|p| p.parse().ok())
            })
            .unwrap_or(22);
        (user, port)
    }

    fn default_user_port(env: &Env, defaults: Option<&HostBlock>) -> (String, u16) {
        let user = defaults
            .and_then(|d| d.get("User"))
            .or_else(|| env.user.clone())
            .unwrap_or_default();
        let port = defaults
            .and_then(|d| d.get("Port"))
            .and_then(|p| p.parse().ok())
            .unwrap_or(22);
        (user, port)
    }

    /// Number of hosts, `Host *` excluded.
    pub fn host_count(&self) -> usize {
        self.hosts().iter().filter(|h| !h.is_defaults()).count()
    }

    pub(crate) fn place(
        &mut self,
        block: HostBlock,
        section: Option<&str>,
        fallback: Option<usize>,
    ) -> Placed {
        let name = block.primary();
        let target = match section {
            Some(s) => Some(self.ensure_section(s)),
            None => fallback.or_else(|| self.catch_all()),
        };
        self.insert_host(target, block);
        self.finish();
        Placed {
            name,
            section: target.map(|i| self.sections[i].name().to_string()),
        }
    }

    /// `add`: appends `Host NAME` with `HostName`, `User` and `Port` from the
    /// URI (user and port resolved through `Host *` and `env`), then
    /// `IdentityFile` and the `-o` options. The host goes to `spec.section`
    /// (created when missing), else the catch-all, else the end of an
    /// unsectioned file.
    pub fn add(&mut self, spec: &AddSpec, env: &Env) -> Result<Placed> {
        let defaults = self.defaults().cloned();
        self.add_with_defaults(spec, env, defaults.as_ref())
    }

    /// [`Config::add`] with user and port resolved through `defaults`
    /// instead of this file's own `Host *` (a workspace passes the first
    /// `Host *` in load order).
    pub(crate) fn add_with_defaults(
        &mut self,
        spec: &AddSpec,
        env: &Env,
        defaults: Option<&HostBlock>,
    ) -> Result<Placed> {
        validate_name(&spec.name)?;
        if self.find_host(&spec.name).is_some() {
            return Err(Error::HostExists(spec.name.clone()));
        }
        let uri = ConnectionUri::parse(&spec.uri)?;
        for (k, _) in &spec.options {
            check_settable(k)?;
        }
        let (user, port) = Config::resolve_user_port(&uri, env, defaults);
        let mut block = HostBlock::new(std::slice::from_ref(&spec.name));
        block.set("HostName", &uri.host);
        block.set("User", &user);
        block.set("Port", &port.to_string());
        if let Some(id) = &spec.identity {
            block.set("IdentityFile", id);
        }
        apply_pairs(&mut block, &spec.options, true);
        self.ensure_trailing_newline();
        Ok(self.place(block, spec.section.as_deref(), None))
    }

    /// `edit`: replaces `HostName`, `User` and `Port` of an existing host
    /// from the URI, sets `IdentityFile` and the `-o` options, and moves the
    /// host to `spec.section` when given. Other keys stay.
    pub fn edit(&mut self, spec: &EditSpec, env: &Env) -> Result<Placed> {
        let defaults = self.defaults().cloned();
        self.edit_with_defaults(spec, env, defaults.as_ref())
    }

    /// [`Config::edit`] with user and port resolved through `defaults`.
    pub(crate) fn edit_with_defaults(
        &mut self,
        spec: &EditSpec,
        env: &Env,
        defaults: Option<&HostBlock>,
    ) -> Result<Placed> {
        let loc = self
            .find_host(&spec.name)
            .ok_or_else(|| Error::EditTargetMissing(spec.name.clone()))?;
        let uri = ConnectionUri::parse(&spec.uri)?;
        for (k, _) in &spec.options {
            check_settable(k)?;
        }
        let (user, port) = Config::resolve_user_port(&uri, env, defaults);
        let block = self.host_mut(loc);
        block.set("HostName", &uri.host);
        block.set("User", &user);
        block.set("Port", &port.to_string());
        if let Some(id) = &spec.identity {
            block.set("IdentityFile", id);
        }
        apply_pairs(block, &spec.options, false);
        let name = block.primary();
        if let Some(section) = &spec.section {
            let block = self.remove_host(loc);
            return Ok(self.place(block, Some(section), None));
        }
        self.finish();
        let section = self
            .find_primary(&name)
            .and_then(|l| self.section_name(l).map(str::to_string));
        Ok(Placed { name, section })
    }

    pub(crate) fn select(&self, selector: &HostSelector) -> Result<Vec<HostLocation>> {
        match selector {
            HostSelector::Name(name) => self
                .find_host(name)
                .map(|l| vec![l])
                .ok_or_else(|| Error::HostNotFound(name.clone())),
            HostSelector::Regex(pattern) => {
                let re = anchored(pattern)?;
                let found: Vec<HostLocation> = self
                    .host_locations()
                    .into_iter()
                    .filter(|l| {
                        let h = self.host(*l);
                        !h.is_defaults() && re.is_match(&h.primary())
                    })
                    .collect();
                if found.is_empty() {
                    Err(Error::NoMatch(pattern.clone()))
                } else {
                    Ok(found)
                }
            }
        }
    }

    /// `set`: sets every pair on the selected hosts. A multi-valued key is
    /// replaced (all its lines), or gains one line with `append`. Returns
    /// the primary names of the updated hosts in file order.
    pub fn set(
        &mut self,
        selector: &HostSelector,
        pairs: &[(String, String)],
        append: bool,
    ) -> Result<Vec<String>> {
        for (k, _) in pairs {
            check_settable(k)?;
        }
        let locs = self.select(selector)?;
        let mut names = Vec::new();
        for loc in locs {
            let block = self.host_mut(loc);
            apply_pairs(block, pairs, append);
            names.push(block.primary());
        }
        self.finish();
        Ok(names)
    }

    /// The settings form: applies every change to host `name` in one go,
    /// after checking each value against its keyword (see
    /// [`validate_setting`](crate::validate_setting)). Returns the host's
    /// primary name.
    pub fn apply_settings(&mut self, name: &str, changes: &[SettingChange]) -> Result<String> {
        check_settings(changes)?;
        let loc = self
            .find_host(name)
            .ok_or_else(|| Error::HostNotFound(name.to_string()))?;
        let block = self.host_mut(loc);
        apply_settings(block, changes);
        let primary = block.primary();
        self.finish();
        Ok(primary)
    }

    /// `unset`: removes every line of each key from the selected hosts. A
    /// key the host lacks is not an error. Returns the updated primary names.
    pub fn unset(&mut self, selector: &HostSelector, keys: &[String]) -> Result<Vec<String>> {
        let locs = self.select(selector)?;
        let mut names = Vec::new();
        for loc in locs {
            let block = self.host_mut(loc);
            for k in keys {
                block.unset(k);
            }
            names.push(block.primary());
        }
        self.finish();
        Ok(names)
    }

    /// `clone`: copies the source's body under `Host NEW-NAME`. Unless
    /// `keep_hostname`, every occurrence of the source name in `HostName`
    /// becomes the new name. Overrides then replace or add keys. The copy
    /// goes to `spec.section`, else the source's section, else the end of an
    /// unsectioned file.
    pub fn clone_host(&mut self, spec: &CloneSpec) -> Result<Placed> {
        let loc = self
            .find_host(&spec.source)
            .ok_or_else(|| Error::HostNotFound(spec.source.clone()))?;
        validate_name(&spec.new_name)?;
        if self.find_host(&spec.new_name).is_some() {
            return Err(Error::TargetExists(spec.new_name.clone()));
        }
        for (k, _) in &spec.overrides {
            check_settable(k)?;
        }
        let block = clone_block(self.host(loc), spec);
        self.ensure_trailing_newline();
        let fallback = loc.section;
        Ok(self.place(block, spec.section.as_deref(), fallback))
    }

    /// `move`: renames a host, moves it to a section, or both. A rename
    /// keeps aliases and keys and leaves `HostName` alone.
    pub fn move_host(
        &mut self,
        name: &str,
        new_name: Option<&str>,
        section: Option<&str>,
    ) -> Result<Moved> {
        if new_name.is_none() && section.is_none() {
            return Err(Error::MoveNeedsTarget);
        }
        let loc = self
            .find_host(name)
            .ok_or_else(|| Error::HostNotFound(name.to_string()))?;
        let old_name = self.host(loc).primary();
        if let Some(new) = new_name {
            validate_name(new)?;
            if let Some(other) = self.find_host(new) {
                if other != loc {
                    return Err(Error::TargetExists(new.to_string()));
                }
            }
        }
        let block = self.host_mut(loc);
        if let Some(new) = new_name {
            let mut names = vec![new.to_string()];
            names.extend(block.aliases().into_iter().filter(|a| a != new));
            block.set_names(&names);
        }
        let new_name = block.primary();
        let section = match section {
            Some(s) => {
                let block = self.remove_host(loc);
                self.place(block, Some(s), None).section
            }
            None => {
                self.finish();
                None
            }
        };
        Ok(Moved {
            old_name,
            new_name,
            section,
        })
    }

    /// `delete`: removes whole entries with the comments directly above
    /// them. Every name must exist; otherwise nothing changes and the first
    /// missing name is reported. Returns the deleted primary names in
    /// argument order.
    pub fn delete(&mut self, names: &[String]) -> Result<Vec<String>> {
        let mut primaries = Vec::new();
        for n in names {
            let loc = self
                .find_host(n)
                .ok_or_else(|| Error::HostNotFound(n.clone()))?;
            let p = self.host(loc).primary();
            if !primaries.contains(&p) {
                primaries.push(p);
            }
        }
        for p in &primaries {
            let loc = self.find_primary(p).expect("checked above");
            self.remove_host(loc);
        }
        self.finish();
        Ok(primaries)
    }

    /// `delete-all`: removes every host except `Host *`. Comments (including
    /// those above removed hosts), blank lines, banners and `Match` blocks
    /// stay. Returns the number of hosts removed.
    pub fn delete_all(&mut self) -> usize {
        let mut removed = 0;
        let mut locs = self.host_locations();
        locs.reverse();
        for loc in locs {
            if self.host(loc).is_defaults() {
                continue;
            }
            let leading = std::mem::take(&mut self.host_mut(loc).leading);
            self.remove_host(loc);
            let entries = self.entries_mut(loc.section);
            let at = loc.index.min(entries.len());
            for (offset, line) in leading.into_iter().enumerate() {
                entries.insert(at + offset, Entry::Line(line));
            }
            removed += 1;
        }
        removed
    }

    /// `list`: one row per host (`Host *` excluded), grouped by part in file
    /// order (preamble first, then each section) and sorted by name inside
    /// each part.
    pub fn list(&self, env: &Env) -> Vec<ListRow> {
        self.list_with_defaults(env, self.defaults())
    }

    /// [`Config::list`] with user and port falling back to `defaults`.
    pub(crate) fn list_with_defaults(
        &self,
        env: &Env,
        defaults: Option<&HostBlock>,
    ) -> Vec<ListRow> {
        let (default_user, default_port) = Config::default_user_port(env, defaults);
        let parts = std::iter::once(None).chain((0..self.sections.len()).map(Some));
        let mut rows = Vec::new();
        for part in parts {
            let mut part_rows: Vec<ListRow> = self
                .entries(part)
                .iter()
                .filter_map(|e| match e {
                    Entry::Host(h) if !h.is_defaults() => Some(h),
                    _ => None,
                })
                .map(|h| {
                    let options: Vec<(String, String)> = h
                        .directives()
                        .into_iter()
                        .filter(|d| {
                            !["hostname", "user", "port"]
                                .contains(&d.key.to_ascii_lowercase().as_str())
                        })
                        .map(|d| (keys::canonical_key(&d.key), d.value))
                        .collect();
                    ListRow {
                        name: h.primary(),
                        section: part.map(|i| self.sections[i].name().to_string()),
                        aliases: h.aliases(),
                        hostname: h.get("HostName"),
                        user: h.get("User").unwrap_or_else(|| default_user.clone()),
                        port: h
                            .get("Port")
                            .and_then(|p| p.parse().ok())
                            .unwrap_or(default_port),
                        options,
                        proxy_command: h.get("ProxyCommand"),
                        proxy_jump: h.get("ProxyJump"),
                    }
                })
                .collect();
            part_rows.sort_by_key(|r| (r.name.to_lowercase(), r.name.clone()));
            rows.extend(part_rows);
        }
        rows
    }

    /// The `Host *` directives in file order, keys in canonical case, for
    /// `list -l`'s `(*) defaults` block.
    pub fn defaults_options(&self) -> Vec<(String, String)> {
        self.defaults()
            .map(|d| {
                d.directives()
                    .into_iter()
                    .map(|x| (keys::canonical_key(&x.key), x.value))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// `show`: each named entry verbatim, found by primary name or alias.
    /// The first missing name is an error.
    pub fn show(&self, names: &[String]) -> Result<Vec<ShownHost>> {
        names
            .iter()
            .map(|n| {
                let loc = self
                    .find_host(n)
                    .ok_or_else(|| Error::HostNotFound(n.clone()))?;
                let h = self.host(loc);
                Ok(ShownHost {
                    name: h.primary(),
                    section: self.section_name(loc).map(str::to_string),
                    text: h.text(),
                })
            })
            .collect()
    }

    /// `dump`: the whole file as parsed.
    pub fn dump(&self) -> String {
        self.render()
    }

    /// `search`: the `list` rows of every host whose name, alias, key, value
    /// or `name -> user@hostname:port` line matches `pattern` (a regular
    /// expression, or literal text when `fixed`).
    pub fn search(&self, pattern: &str, fixed: bool, env: &Env) -> Result<Vec<ListRow>> {
        let matcher = Matcher::new(pattern, fixed)?;
        Ok(self.search_rows(&matcher, self.list(env)))
    }

    /// Keeps the rows of `rows` whose host matches `matcher`.
    pub(crate) fn search_rows(&self, matcher: &Matcher, rows: Vec<ListRow>) -> Vec<ListRow> {
        rows.into_iter()
            .filter(|row| {
                let block = self.find_primary(&row.name).map(|l| self.host(l));
                std::iter::once(row.name.clone())
                    .chain(row.aliases.iter().cloned())
                    .chain(std::iter::once(row.line()))
                    .chain(
                        block
                            .map(|b| b.directives())
                            .unwrap_or_default()
                            .into_iter()
                            .flat_map(|d| [d.key, d.value]),
                    )
                    .any(|s| matcher.is_match(&s))
            })
            .collect()
    }

    /// `alias`: adds names to a host's `Host` line. Names already on that
    /// line are skipped; a name used by another host is an error. Returns
    /// every name the host answers to.
    pub fn alias(&mut self, name: &str, aliases: &[String]) -> Result<Vec<String>> {
        let loc = self
            .find_host(name)
            .ok_or_else(|| Error::HostNotFound(name.to_string()))?;
        let mut names = self.host(loc).names();
        for a in aliases {
            validate_name(a)?;
            if names.contains(a) {
                continue;
            }
            if let Some(other) = self.find_host(a) {
                return Err(Error::AliasTaken {
                    alias: a.clone(),
                    owner: self.host(other).primary(),
                });
            }
            names.push(a.clone());
        }
        self.host_mut(loc).set_names(&names);
        self.finish();
        Ok(names)
    }

    /// `unalias`: removes aliases from a host's `Host` line. With `host`,
    /// the aliases must belong to it; without, the host carrying the first
    /// alias is found. The primary name cannot be removed.
    pub fn unalias(&mut self, host: Option<&str>, aliases: &[String]) -> Result<Unaliased> {
        let loc = match host {
            Some(h) => self
                .find_host(h)
                .ok_or_else(|| Error::HostNotFound(h.to_string()))?,
            None => {
                let first = aliases
                    .first()
                    .ok_or_else(|| Error::Usage("give at least one alias.".to_string()))?;
                match self
                    .host_locations()
                    .into_iter()
                    .find(|l| self.host(*l).aliases().contains(first))
                {
                    Some(l) => l,
                    None if self.find_primary(first).is_some() => {
                        return Err(Error::PrimaryName(first.clone()))
                    }
                    None => return Err(Error::AliasNotFound(first.clone())),
                }
            }
        };
        let block = self.host(loc);
        let primary = block.primary();
        let mut names = block.names();
        for a in aliases {
            if *a == primary {
                return Err(Error::PrimaryName(a.clone()));
            }
            if !names.contains(a) {
                return Err(Error::NotAnAliasOf {
                    host: primary,
                    alias: a.clone(),
                });
            }
        }
        names.retain(|n| !aliases.contains(n) || *n == primary);
        self.host_mut(loc).set_names(&names);
        self.finish();
        Ok(Unaliased {
            host: primary,
            names,
        })
    }

    /// `sections`: every section in file order with its host count; the
    /// last is the catch-all.
    pub fn sections(&self) -> Vec<SectionSummary> {
        let last = self.sections.len().saturating_sub(1);
        self.sections
            .iter()
            .enumerate()
            .map(|(i, s)| SectionSummary {
                name: s.name().to_string(),
                hosts: s.hosts().filter(|h| !h.is_defaults()).count(),
                catch_all: i == last,
            })
            .collect()
    }

    /// `add-section`: creates an empty section `name`. It goes before the
    /// catch-all, or before `before` when given. On a file without sections
    /// the catch-all `other` is created too and receives every host.
    pub fn add_section(&mut self, name: &str, before: Option<&str>) -> Result<SectionAdded> {
        if name.trim().is_empty() {
            return Err(Error::Usage("a section name is required.".to_string()));
        }
        if self.find_section(name).is_some() {
            return Err(Error::SectionExists(name.to_string()));
        }
        if let Some(b) = before {
            let at = self
                .find_section(b)
                .ok_or_else(|| Error::SectionNotFound(b.to_string()))?;
            let before_name = self.sections[at].name().to_string();
            self.ensure_trailing_newline();
            let prev = if at == 0 { None } else { Some(at - 1) };
            let entries = self.entries_mut(prev);
            if entries.last().is_some_and(|e| !e.is_blank_line()) {
                entries.push(Entry::Line(Line::blank()));
            }
            self.sections.insert(at, Section::new(name));
            self.finish();
            return Ok(SectionAdded {
                name: name.to_string(),
                before: Some(before_name),
                catch_all: None,
            });
        }
        let had_sections = self.has_sections();
        self.ensure_section(name);
        let catch_all = if had_sections {
            None
        } else {
            let last = self.sections.last().expect("ensure_section created one");
            Some((
                last.name().to_string(),
                last.hosts().filter(|h| !h.is_defaults()).count(),
            ))
        };
        self.finish();
        Ok(SectionAdded {
            name: name.to_string(),
            before: None,
            catch_all: catch_all.filter(|(n, _)| !n.eq_ignore_ascii_case(name)),
        })
    }

    /// `rename-section`: regenerates the banner under the new name. When
    /// another section already has that name (matched case-insensitively),
    /// the hosts and comments move into it and the old section disappears.
    /// The catch-all stays the catch-all because it stays last.
    pub fn rename_section(&mut self, old: &str, new: &str) -> Result<SectionRename> {
        let from = self
            .find_section(old)
            .ok_or_else(|| Error::SectionNotFound(old.to_string()))?;
        let from_name = self.sections[from].name().to_string();
        let target = self.find_section(new).filter(|&t| t != from);
        let Some(target) = target else {
            self.sections[from].banner = Banner::generate(new);
            self.finish();
            return Ok(SectionRename::Renamed {
                from: from_name,
                to: new.to_string(),
            });
        };
        self.ensure_trailing_newline();
        let removed = self.sections.remove(from);
        let into = if target > from { target - 1 } else { target };
        let into_name = self.sections[into].name().to_string();
        let mut comments = Vec::new();
        for entry in removed.entries {
            match entry {
                Entry::Host(h) => {
                    self.insert_host(Some(into), h);
                }
                Entry::Match(m) => {
                    let entries = self.entries_mut(Some(into));
                    entries.push(Entry::Line(Line::blank()));
                    entries.push(Entry::Match(m));
                }
                Entry::Line(l) if !l.is_blank() => comments.push(Entry::Line(l)),
                Entry::Line(_) => {}
            }
        }
        self.entries_mut(Some(into)).extend(comments);
        self.finish();
        Ok(SectionRename::Merged {
            from: from_name,
            into: into_name,
        })
    }

    /// `check`: reports unknown keys, duplicate single-valued keys, hosts
    /// without `HostName`, missing `IdentityFile` paths, names used by more
    /// than one entry, and unparsable lines. The model is not changed.
    pub fn check(&self, env: &Env) -> CheckReport {
        let mut problems = Vec::new();
        let hosts = self.hosts();
        for h in &hosts {
            let name = h.primary();
            let mut seen: Vec<String> = Vec::new();
            let mut reported: Vec<String> = Vec::new();
            for d in h.directives() {
                if !keys::is_known_key(&d.key) {
                    problems.push(Problem {
                        kind: ProblemKind::UnknownKey,
                        host: Some(name.clone()),
                        line: None,
                        file: None,
                        file_label: None,
                        detail: format!("unknown key {}", d.key),
                    });
                    continue;
                }
                let lower = d.key.to_ascii_lowercase();
                if !keys::is_multi_valued(&d.key) {
                    if seen.contains(&lower) && !reported.contains(&lower) {
                        problems.push(Problem {
                            kind: ProblemKind::DuplicateKey,
                            host: Some(name.clone()),
                            line: None,
                            file: None,
                            file_label: None,
                            detail: format!("duplicate key {}", keys::canonical_key(&d.key)),
                        });
                        reported.push(lower.clone());
                    }
                    seen.push(lower.clone());
                }
                if lower == "identityfile" {
                    if let Some(path) = expand_identity(&d.value, env.home.as_deref()) {
                        if !path.exists() {
                            problems.push(Problem {
                                kind: ProblemKind::MissingIdentityFile,
                                host: Some(name.clone()),
                                line: None,
                                file: None,
                                file_label: None,
                                detail: format!("IdentityFile {} does not exist", d.value),
                            });
                        }
                    }
                }
            }
            let wildcard = h.names().iter().any(|n| n.contains(['*', '?', '!']));
            if !wildcard && h.get("HostName").is_none() {
                problems.push(Problem {
                    kind: ProblemKind::MissingHostName,
                    host: Some(name.clone()),
                    line: None,
                    file: None,
                    file_label: None,
                    detail: "no HostName".to_string(),
                });
            }
        }
        let mut counted: Vec<(String, usize)> = Vec::new();
        for h in &hosts {
            for n in h.names() {
                match counted.iter_mut().find(|(k, _)| *k == n) {
                    Some((_, c)) => *c += 1,
                    None => counted.push((n, 1)),
                }
            }
        }
        for (n, c) in counted.into_iter().filter(|(_, c)| *c > 1) {
            problems.push(Problem {
                kind: ProblemKind::DuplicateName,
                host: Some(n),
                line: None,
                file: None,
                file_label: None,
                detail: format!("name used by {c} entries"),
            });
        }
        for (i, line) in self.lines().enumerate() {
            if line.is_unparsable() {
                problems.push(Problem {
                    kind: ProblemKind::UnparsableLine,
                    host: None,
                    line: Some(i + 1),
                    file: None,
                    file_label: None,
                    detail: format!("cannot parse: {}", line.text().trim()),
                });
            }
        }
        CheckReport {
            problems,
            hosts: self.host_count(),
        }
    }
}
