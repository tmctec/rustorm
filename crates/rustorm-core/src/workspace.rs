//! The workspace: the root config and every file its `Include` lines load.
//!
//! [`Workspace::load`] reads the root, follows its `Include` lines the way
//! ssh does (see [`crate::include`]) and keeps one [`ConfigFile`] per file,
//! the root at index 0, the rest in load order. A root without `Include` is
//! a workspace of one file and every operation behaves exactly as the
//! single-file [`Config`](crate::Config) operation does: the same bytes, the same messages.
//!
//! Every `rustorm` command has a method here that routes the change to one
//! file by the rules of docs/cli.md's Included files (D22) and returns a
//! [`Change`]: the value the [`Config`](crate::Config) operation returns, the files it
//! changed, the success messages and the warnings, worded exactly as the
//! docs print them. Nothing is written until [`Workspace::save`] (every
//! modified file) or [`Workspace::save_file`] (one file, for editors).

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use serde::ser::SerializeMap;
use serde::{Serialize, Serializer};

use crate::error::join_and;
use crate::include::{
    canonical, glob_match, looks_like_backup, IncludeMatch, IncludeStatus, Walked, Walker,
};
use crate::io::{absolute_path, display_path, ConfigFile, WriteOptions};
use crate::model::{HostBlock, HostLocation};
use crate::ops::{
    apply_pairs, check_settable, clone_block, serialize_options, validate_name, AddSpec,
    CheckReport, CloneSpec, EditSpec, Env, HostSelector, ListRow, Matcher, Moved, Placed, Problem,
    ProblemKind, SectionAdded, SectionRename, SectionSummary,
};
use crate::{Error, Result};

/// A host's place in the workspace: the file index and its place there.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorkspaceLocation {
    /// Index into [`Workspace::files`].
    pub file: usize,
    /// The host's place inside that file.
    pub loc: HostLocation,
}

/// Whether a file in load order could be read.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FileState {
    /// Loaded; the index into [`Workspace::files`].
    Loaded(usize),
    /// Matched by an `Include` but unreadable; skipped (D24).
    Unreadable(String),
}

/// One file of the workspace in load order, readable or not.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FileEntry {
    /// The path as matched (absolute for included files, symlinks kept).
    pub path: PathBuf,
    /// 0 for the root, 1 for a file the root includes, and so on.
    pub depth: usize,
    /// Loaded or unreadable.
    pub state: FileState,
}

/// What a workspace operation did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Change<T> {
    /// The single-file operation's result.
    pub value: T,
    /// Indices of the files the operation changed, in write order.
    pub files: Vec<usize>,
    /// Success messages for stdout, exactly as docs/cli.md prints them.
    pub messages: Vec<String>,
    /// Warnings for stderr, without the `warning: ` prefix.
    pub warnings: Vec<String>,
}

/// One `list` or `search` row and the file that holds the host.
///
/// Serializes to the `--json list` row of docs/cli.md: `name`, `file`
/// (absolute path, D23), `section`, `aliases`, `hostname`, `user`, `port`,
/// `options`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceRow {
    /// Index into [`Workspace::files`].
    pub file: usize,
    /// The absolute path of that file.
    pub path: PathBuf,
    /// The row.
    pub row: ListRow,
}

impl Serialize for WorkspaceRow {
    fn serialize<S: Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        struct Options<'a>(&'a [(String, String)]);
        impl Serialize for Options<'_> {
            fn serialize<S: Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
                serialize_options(self.0, s)
            }
        }
        let r = &self.row;
        let mut map = s.serialize_map(Some(8))?;
        map.serialize_entry("name", &r.name)?;
        map.serialize_entry("file", &self.path)?;
        map.serialize_entry("section", &r.section)?;
        map.serialize_entry("aliases", &r.aliases)?;
        map.serialize_entry("hostname", &r.hostname)?;
        map.serialize_entry("user", &r.user)?;
        map.serialize_entry("port", &r.port)?;
        map.serialize_entry("options", &Options(&r.options))?;
        map.end()
    }
}

/// One section and the file that holds it, for `sections`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct WorkspaceSection {
    /// The section name.
    pub name: String,
    /// The absolute path of the file holding it (D23).
    pub file: PathBuf,
    /// Number of hosts in it, `Host *` excluded.
    pub hosts: usize,
    /// True for the file's last section, its catch-all.
    pub catch_all: bool,
    /// Index into [`Workspace::files`].
    #[serde(skip)]
    pub index: usize,
}

impl WorkspaceSection {
    /// The single-file summary of this section.
    pub fn summary(&self) -> SectionSummary {
        SectionSummary {
            name: self.name.clone(),
            hosts: self.hosts,
            catch_all: self.catch_all,
        }
    }
}

/// One host as `show` prints it, with its file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct WorkspaceShown {
    /// The host's primary name.
    pub name: String,
    /// The absolute path of the file holding it (D23).
    pub file: PathBuf,
    /// Its section, if any.
    pub section: Option<String>,
    /// The entry verbatim.
    pub text: String,
    /// Index into [`Workspace::files`].
    #[serde(skip)]
    pub index: usize,
}

/// The root config and every file its `Include` lines load.
#[derive(Debug, Clone)]
pub struct Workspace {
    /// Every readable file: the root at index 0, then the included files in
    /// load order. A file added by [`Workspace::resolve_file`] comes last.
    pub files: Vec<ConfigFile>,
    /// Every file in load order, unreadable ones included.
    pub load_order: Vec<FileEntry>,
    /// The root's `Include` patterns and what they matched, nested.
    pub includes: Vec<IncludeMatch>,
    /// The home directory: resolves `~/` and relative patterns and prints
    /// paths as `~/...`.
    pub home: Option<PathBuf>,
    write_order: Vec<usize>,
}

fn plural(n: usize, word: &str) -> String {
    if n == 1 {
        format!("{n} {word}")
    } else {
        format!("{n} {word}s")
    }
}

fn same_file(a: &Path, b: &Path) -> bool {
    absolute_path(a) == absolute_path(b) || canonical(a) == canonical(b)
}

fn write_error(path: &Path, source: std::io::Error) -> Error {
    Error::Write {
        path: path.to_path_buf(),
        source,
    }
}

/// Fails with [`Error::Write`] when `path` could not be written the way
/// [`crate::write_text`] writes it: the file itself (a read-only file is
/// refused even though a rename would replace it), a temporary file in
/// its directory, and the `~` backup when one is written.
fn check_writable(path: &Path, backup: &Path, options: WriteOptions) -> Result<()> {
    let target = canonical(path);
    if target.exists() {
        fs::OpenOptions::new()
            .append(true)
            .open(&target)
            .map_err(|e| write_error(path, e))?;
        if !options.no_backup && backup.exists() {
            fs::OpenOptions::new()
                .append(true)
                .open(backup)
                .map_err(|e| write_error(backup, e))?;
        }
    }
    if let Some(dir) = target.parent().filter(|d| d.is_dir()) {
        tempfile::Builder::new()
            .prefix(".rustorm-")
            .tempfile_in(dir)
            .map_err(|e| write_error(path, e))?;
    }
    Ok(())
}

impl Workspace {
    /// Loads `root` and every file its `Include` lines load, resolving
    /// `~/` against the process's home directory. A missing root loads as
    /// an empty file; an unreadable root is [`Error::Read`]; an unreadable
    /// included file is recorded in [`Workspace::load_order`] and skipped.
    pub fn load(root: impl Into<PathBuf>) -> Result<Workspace> {
        let home = dirs::home_dir();
        Workspace::load_with_home(root, home.as_deref())
    }

    /// [`Workspace::load`] with an explicit home directory.
    pub fn load_with_home(root: impl Into<PathBuf>, home: Option<&Path>) -> Result<Workspace> {
        let root = ConfigFile::load(root)?;
        let mut walker = Walker {
            home,
            visited: HashSet::from([canonical(&root.path)]),
            loaded: Vec::new(),
        };
        let includes = walker.walk(&root.config, &absolute_path(&root.path), 0);
        let mut ws = Workspace {
            load_order: vec![FileEntry {
                path: root.path.clone(),
                depth: 0,
                state: FileState::Loaded(0),
            }],
            files: vec![root],
            includes,
            home: home.map(Path::to_path_buf),
            write_order: Vec::new(),
        };
        for w in walker.loaded {
            match w {
                Walked::File(file, depth) => {
                    ws.load_order.push(FileEntry {
                        path: file.path.clone(),
                        depth,
                        state: FileState::Loaded(ws.files.len()),
                    });
                    ws.files.push(*file);
                }
                Walked::Unreadable(path, reason, depth) => ws.load_order.push(FileEntry {
                    path,
                    depth,
                    state: FileState::Unreadable(reason),
                }),
            }
        }
        Ok(ws)
    }

    /// A workspace of the one file `file`, without following its `Include`
    /// lines (for callers that already hold a [`ConfigFile`]).
    pub fn single(file: ConfigFile, home: Option<&Path>) -> Workspace {
        Workspace {
            load_order: vec![FileEntry {
                path: file.path.clone(),
                depth: 0,
                state: FileState::Loaded(0),
            }],
            files: vec![file],
            includes: Vec::new(),
            home: home.map(Path::to_path_buf),
            write_order: Vec::new(),
        }
    }

    /// The root file.
    pub fn root(&self) -> &ConfigFile {
        &self.files[0]
    }

    /// True when more than one file is loaded or matched (unreadable ones
    /// count). Messages name files and `list` prints file headings only
    /// then.
    pub fn is_multi(&self) -> bool {
        self.load_order.len() > 1
    }

    /// The path of file `i` as text output prints it (`~/...`).
    pub fn display(&self, i: usize) -> String {
        self.display_path(&self.files[i].path)
    }

    /// `path` as text output prints it (`~/...`).
    pub fn display_path(&self, path: &Path) -> String {
        display_path(path, self.home.as_deref())
    }

    /// The absolute path of file `i`, as `--json` carries it.
    pub fn abs(&self, i: usize) -> PathBuf {
        absolute_path(&self.files[i].path)
    }

    /// The file name of file `i` (`cypress` for `~/.ssh/config.d/cypress`).
    pub fn file_name(&self, i: usize) -> String {
        self.files[i]
            .path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default()
    }

    /// Indices of the files whose model differs from the text on disk.
    pub fn modified(&self) -> Vec<usize> {
        (0..self.files.len())
            .filter(|&i| self.files[i].is_modified())
            .collect()
    }

    /// Hosts across every loaded file, `Host *` excluded.
    pub fn host_count(&self) -> usize {
        self.files.iter().map(|f| f.config.host_count()).sum()
    }

    /// The load warnings read commands print on stderr (D24), without the
    /// `warning: ` prefix: `cannot read ~/.ssh/config.d/private (permission
    /// denied); skipped.`
    pub fn load_warnings(&self) -> Vec<String> {
        self.load_order
            .iter()
            .filter_map(|e| match &e.state {
                FileState::Unreadable(reason) => Some(format!(
                    "cannot read {} ({reason}); skipped.",
                    self.display_path(&e.path)
                )),
                FileState::Loaded(_) => None,
            })
            .collect()
    }

    /// `(files, hosts)` for the `includes` summary line: the root and every
    /// loaded file, and their hosts.
    pub fn include_summary(&self) -> (usize, usize) {
        (self.files.len(), self.host_count())
    }

    /// The `--json includes` document: one object per matched file of each
    /// root `Include` pattern, `{"pattern", "from", "file", "hosts",
    /// "nested"}`; a pattern that matches nothing gives `"file": null`.
    pub fn includes_json(&self) -> serde_json::Value {
        fn one(m: &IncludeMatch) -> Vec<serde_json::Value> {
            if m.files.is_empty() {
                return vec![serde_json::json!({
                    "pattern": m.pattern,
                    "from": m.from,
                    "file": null,
                    "hosts": 0,
                    "nested": [],
                })];
            }
            m.files
                .iter()
                .map(|f| {
                    let hosts = match f.status {
                        IncludeStatus::Loaded { hosts } => hosts,
                        _ => 0,
                    };
                    let nested: Vec<serde_json::Value> = f.nested.iter().flat_map(one).collect();
                    serde_json::json!({
                        "pattern": m.pattern,
                        "from": m.from,
                        "file": f.path,
                        "hosts": hosts,
                        "nested": nested,
                    })
                })
                .collect()
        }
        serde_json::Value::Array(self.includes.iter().flat_map(one).collect())
    }

    /// The first `Host *` block in load order: the defaults `add`, `edit`
    /// and `list` resolve user and port through.
    pub fn defaults(&self) -> Option<&HostBlock> {
        self.files.iter().find_map(|f| f.config.defaults())
    }

    /// The `Host *` directives for `list -l`'s `(*) defaults` block.
    pub fn defaults_options(&self) -> Vec<(String, String)> {
        self.defaults()
            .map(|d| {
                d.directives()
                    .into_iter()
                    .map(|x| (crate::keys::canonical_key(&x.key), x.value))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// The first host answering to `name` in load order.
    pub fn find_host(&self, name: &str) -> Option<WorkspaceLocation> {
        self.find_host_all(name).into_iter().next()
    }

    /// Every host answering to `name`, in load order.
    pub fn find_host_all(&self, name: &str) -> Vec<WorkspaceLocation> {
        let mut out = Vec::new();
        for (file, f) in self.files.iter().enumerate() {
            for loc in f.config.host_locations() {
                if f.config.host(loc).answers_to(name) {
                    out.push(WorkspaceLocation { file, loc });
                }
            }
        }
        out
    }

    /// The host block at `wl`.
    pub fn host(&self, wl: WorkspaceLocation) -> &HostBlock {
        self.files[wl.file].config.host(wl.loc)
    }

    /// The 1-based line of the `Host` line of the host at `wl` in its file.
    pub fn host_line(&self, wl: WorkspaceLocation) -> usize {
        self.files[wl.file].config.host_line(wl.loc)
    }

    /// The host an edit of `name` goes to: in `file` when given, else the
    /// first definition in load order, with a warning naming the other
    /// files that define it (inc-12).
    fn locate(&self, name: &str, file: Option<usize>) -> Option<(WorkspaceLocation, Vec<String>)> {
        if let Some(f) = file {
            return self.files[f]
                .config
                .find_host(name)
                .map(|loc| (WorkspaceLocation { file: f, loc }, Vec::new()));
        }
        let all = self.find_host_all(name);
        let first = *all.first()?;
        let mut others: Vec<usize> = Vec::new();
        for wl in &all[1..] {
            if wl.file != first.file && !others.contains(&wl.file) {
                others.push(wl.file);
            }
        }
        let warnings = if others.is_empty() {
            Vec::new()
        } else {
            let names: Vec<String> = others.iter().map(|&i| self.display(i)).collect();
            vec![format!(
                "{name} is also defined in {}; ssh uses the first.",
                join_and(&names)
            )]
        };
        Some((first, warnings))
    }

    /// The file holding section `name` (matched case-insensitively) and
    /// the section's index there; `Ok(None)` when no file holds it;
    /// [`Error::AmbiguousSection`] when two or more do.
    pub fn find_section(&self, name: &str) -> Result<Option<(usize, usize)>> {
        let hits: Vec<(usize, usize)> = self
            .files
            .iter()
            .enumerate()
            .filter_map(|(i, f)| f.config.find_section(name).map(|s| (i, s)))
            .collect();
        match hits.len() {
            0 => Ok(None),
            1 => Ok(Some(hits[0])),
            _ => Err(Error::AmbiguousSection {
                name: name.to_string(),
                files: hits.iter().map(|(i, _)| self.display(*i)).collect(),
            }),
        }
    }

    fn entry_index(&self, entry: &FileEntry) -> Result<usize> {
        match &entry.state {
            FileState::Loaded(i) => Ok(*i),
            FileState::Unreadable(reason) => Err(Error::UnreadableInclude {
                file: self.display_path(&entry.path),
                reason: reason.clone(),
            }),
        }
    }

    /// Resolves a `--file` argument to a file index.
    ///
    /// A bare name (no `/`, not starting with `~`) matches the file name of
    /// one file in load order; two matches are [`Error::AmbiguousFile`].
    /// Anything else, and a bare name that matches no file, is a path:
    /// `~/` expanded, relative to the current directory. A path in the
    /// workspace is that file; a path outside it is accepted only when an
    /// `Include` pattern of the workspace matches it, and is added as a new
    /// file (created with mode 0600 on save). Otherwise
    /// [`Error::UnknownFile`]. An unreadable file is
    /// [`Error::UnreadableInclude`].
    pub fn resolve_file(&mut self, name: &str) -> Result<usize> {
        let bare = !name.contains('/') && !name.starts_with('~');
        if bare {
            let hits: Vec<&FileEntry> = self
                .load_order
                .iter()
                .filter(|e| e.path.file_name().is_some_and(|n| n == name))
                .collect();
            match hits.len() {
                0 => {}
                1 => return self.entry_index(hits[0]),
                _ => {
                    return Err(Error::AmbiguousFile {
                        name: name.to_string(),
                        files: hits.iter().map(|e| self.display_path(&e.path)).collect(),
                    })
                }
            }
        }
        let path = match name.strip_prefix("~/") {
            Some(rest) => match &self.home {
                Some(h) => h.join(rest),
                None => return Err(Error::UnknownFile(name.to_string())),
            },
            None => absolute_path(Path::new(name)),
        };
        if let Some(e) = self.load_order.iter().find(|e| same_file(&e.path, &path)) {
            return self.entry_index(e);
        }
        let text = path.display().to_string();
        let matched = self
            .includes
            .iter()
            .flat_map(IncludeMatch::walk)
            .filter_map(|m| m.resolved.as_deref())
            .any(|pattern| glob_match(pattern, &text));
        if !matched || path.is_dir() {
            return Err(Error::UnknownFile(name.to_string()));
        }
        let file = ConfigFile::load(&path)?;
        let index = self.files.len();
        self.load_order.push(FileEntry {
            path: file.path.clone(),
            depth: 1,
            state: FileState::Loaded(index),
        });
        self.files.push(file);
        Ok(index)
    }

    fn target(&mut self, file: Option<&str>) -> Result<Option<usize>> {
        file.map(|f| self.resolve_file(f)).transpose()
    }

    /// ` in <file>` on a workspace of several files, else nothing.
    fn suffix(&self, i: usize) -> String {
        if self.is_multi() {
            format!(" in {}", self.display(i))
        } else {
            String::new()
        }
    }

    fn touch(&mut self, i: usize) {
        if !self.write_order.contains(&i) {
            self.write_order.push(i);
        }
    }

    fn change<T>(
        &mut self,
        value: T,
        files: Vec<usize>,
        messages: Vec<String>,
        warnings: Vec<String>,
    ) -> Change<T> {
        for &f in &files {
            self.touch(f);
        }
        Change {
            value,
            files,
            messages,
            warnings,
        }
    }

    /// Where a save backs up file `i`: `<file>~`, or `<dir>/.<name>~` when
    /// an `Include` pattern of the workspace would match `<file>~`, so
    /// neither ssh nor rustorm ever loads rustorm's own backup.
    pub fn backup_path_for(&self, i: usize) -> PathBuf {
        let path = &self.files[i].path;
        let plain = crate::io::backup_path(path);
        let text = absolute_path(&plain).display().to_string();
        let matched = self
            .includes
            .iter()
            .flat_map(IncludeMatch::walk)
            .filter_map(|m| m.resolved.as_deref())
            .any(|pattern| glob_match(pattern, &text));
        if matched {
            crate::io::dot_backup_path(path)
        } else {
            plain
        }
    }

    /// Writes every modified file, each after its own backup (see
    /// [`Workspace::backup_path_for`])
    /// unless `options.no_backup`. Every modified file is checked for
    /// writability first, so a failure writes nothing (inc-18). Files go
    /// in the order the operations touched them (a cross-file `move`
    /// writes the destination first), then any other modified file.
    /// Returns the indices written.
    pub fn save(&mut self, options: WriteOptions) -> Result<Vec<usize>> {
        let modified = self.modified();
        let mut order: Vec<usize> = self
            .write_order
            .iter()
            .copied()
            .filter(|i| modified.contains(i))
            .collect();
        for i in modified {
            if !order.contains(&i) {
                order.push(i);
            }
        }
        for &i in &order {
            check_writable(&self.files[i].path, &self.backup_path_for(i), options)?;
        }
        for &i in &order {
            let backup = self.backup_path_for(i);
            self.files[i].save_with_backup(options, &backup)?;
        }
        self.write_order.clear();
        Ok(order)
    }

    /// Writes file `i` alone when it is modified, after its own backup.
    /// Returns whether it was written. The editors' per-file save.
    pub fn save_file(&mut self, i: usize, options: WriteOptions) -> Result<bool> {
        if !self.files[i].is_modified() {
            return Ok(false);
        }
        let backup = self.backup_path_for(i);
        check_writable(&self.files[i].path, &backup, options)?;
        self.files[i].save_with_backup(options, &backup)?;
        self.write_order.retain(|&w| w != i);
        Ok(true)
    }

    /// Replaces file `i` with `text` (an editor buffer) and writes it after
    /// its own backup.
    pub fn save_text(&mut self, i: usize, text: &str, options: WriteOptions) -> Result<()> {
        let backup = self.backup_path_for(i);
        check_writable(&self.files[i].path, &backup, options)?;
        self.files[i].save_text_with_backup(text, options, &backup)?;
        self.write_order.retain(|&w| w != i);
        Ok(())
    }

    /// Re-reads file `i` from disk, dropping unsaved changes to it.
    pub fn reload_file(&mut self, i: usize) -> Result<()> {
        self.files[i] = ConfigFile::load(self.files[i].path.clone())?;
        self.write_order.retain(|&w| w != i);
        Ok(())
    }

    // ----- read commands -----

    /// `list`: every host of every file, grouped by file in load order and
    /// then as [`Config::list`](crate::Config::list) orders them. User and port fall back to the
    /// workspace's first `Host *`.
    pub fn list(&self, env: &Env) -> Vec<WorkspaceRow> {
        let defaults = self.defaults();
        let mut out = Vec::new();
        for (i, f) in self.files.iter().enumerate() {
            let path = self.abs(i);
            for row in f.config.list_with_defaults(env, defaults) {
                out.push(WorkspaceRow {
                    file: i,
                    path: path.clone(),
                    row,
                });
            }
        }
        out
    }

    /// `search`: the `list` rows whose host matches `pattern`.
    pub fn search(&self, pattern: &str, fixed: bool, env: &Env) -> Result<Vec<WorkspaceRow>> {
        let matcher = Matcher::new(pattern, fixed)?;
        let defaults = self.defaults();
        let mut out = Vec::new();
        for (i, f) in self.files.iter().enumerate() {
            let rows = f.config.list_with_defaults(env, defaults);
            let path = self.abs(i);
            for row in f.config.search_rows(&matcher, rows) {
                out.push(WorkspaceRow {
                    file: i,
                    path: path.clone(),
                    row,
                });
            }
        }
        Ok(out)
    }

    /// `show`: each named host verbatim, the first definition in load
    /// order, with a warning when another file defines it too.
    pub fn show(&self, names: &[String]) -> Result<(Vec<WorkspaceShown>, Vec<String>)> {
        let mut shown = Vec::new();
        let mut warnings = Vec::new();
        for n in names {
            let (wl, w) = self
                .locate(n, None)
                .ok_or_else(|| Error::HostNotFound(n.clone()))?;
            warnings.extend(w);
            let config = &self.files[wl.file].config;
            let h = config.host(wl.loc);
            shown.push(WorkspaceShown {
                name: h.primary(),
                file: self.abs(wl.file),
                section: config.section_name(wl.loc).map(str::to_string),
                text: h.text(),
                index: wl.file,
            });
        }
        Ok((shown, warnings))
    }

    /// `dump`: the text of the root, or of the file `--file` names.
    pub fn dump(&mut self, file: Option<&str>) -> Result<(usize, String)> {
        let i = self.target(file)?.unwrap_or(0);
        Ok((i, self.files[i].config.dump()))
    }

    /// `sections`: every section of every file, in load order.
    pub fn sections(&self) -> Vec<WorkspaceSection> {
        let mut out = Vec::new();
        for (i, f) in self.files.iter().enumerate() {
            for s in f.config.sections() {
                out.push(WorkspaceSection {
                    name: s.name,
                    file: self.abs(i),
                    hosts: s.hosts,
                    catch_all: s.catch_all,
                    index: i,
                });
            }
        }
        out
    }

    /// `check`: every file's problems in load order, each carrying its
    /// file; then, on a workspace of several files, hosts defined in two
    /// files, `Include` patterns that load backups, `Include` lines inside
    /// `Host *` or another block, and unreadable included files.
    pub fn check(&self, env: &Env) -> CheckReport {
        let multi = self.is_multi();
        let mut problems = Vec::new();
        let mut hosts = 0;
        for (i, f) in self.files.iter().enumerate() {
            let report = f.config.check(env);
            hosts += report.hosts;
            for mut p in report.problems {
                p.file = Some(self.abs(i));
                if multi {
                    p.file_label = Some(self.display(i));
                }
                problems.push(p);
            }
        }
        if !multi {
            return CheckReport { problems, hosts };
        }
        let problem = |kind, host: Option<String>, detail: String, file: PathBuf| Problem {
            kind,
            host,
            line: None,
            detail,
            file: Some(file),
            file_label: None,
        };
        let mut names: Vec<(String, Vec<usize>)> = Vec::new();
        for (i, f) in self.files.iter().enumerate() {
            for h in f.config.hosts() {
                if h.is_defaults() {
                    continue;
                }
                for n in h.names() {
                    if n.contains(['*', '?', '!']) {
                        continue;
                    }
                    match names.iter_mut().find(|(k, _)| *k == n) {
                        Some((_, files)) if !files.contains(&i) => files.push(i),
                        Some(_) => {}
                        None => names.push((n, vec![i])),
                    }
                }
            }
        }
        for (n, files) in names.into_iter().filter(|(_, f)| f.len() > 1) {
            let shown: Vec<String> = files.iter().map(|&i| self.display(i)).collect();
            problems.push(problem(
                ProblemKind::DuplicateAcrossFiles,
                Some(n),
                format!("defined in {}; ssh uses the first", join_and(&shown)),
                self.abs(files[0]),
            ));
        }
        let matches: Vec<&IncludeMatch> =
            self.includes.iter().flat_map(IncludeMatch::walk).collect();
        for m in &matches {
            for f in &m.files {
                if looks_like_backup(&f.path) {
                    problems.push(problem(
                        ProblemKind::IncludeLoadsBackup,
                        None,
                        format!(
                            "Include {}: loads {}, which looks like a backup",
                            m.pattern,
                            self.display_path(&f.path)
                        ),
                        m.from.clone(),
                    ));
                }
            }
        }
        let mut seen_lines: HashSet<(PathBuf, usize)> = HashSet::new();
        for m in &matches {
            if !seen_lines.insert((m.from.clone(), m.line)) {
                continue;
            }
            let from = self.display_path(&m.from);
            if m.inside_host_star {
                problems.push(problem(
                    ProblemKind::IncludeInsideHostStar,
                    None,
                    format!(
                        "Include {}: inside Host * in {from}; treated as global",
                        m.directive
                    ),
                    m.from.clone(),
                ));
            } else if let Some(block) = &m.inside_block {
                problems.push(problem(
                    ProblemKind::IncludeInsideBlock,
                    None,
                    format!(
                        "Include {}: inside {block} in {from}; rustorm loads it for every host",
                        m.directive
                    ),
                    m.from.clone(),
                ));
            }
        }
        for m in &matches {
            for f in &m.files {
                if let IncludeStatus::Unreadable { reason } = &f.status {
                    problems.push(problem(
                        ProblemKind::IncludeUnreadable,
                        None,
                        format!("Include {}: cannot read ({reason})", m.pattern),
                        f.path.clone(),
                    ));
                }
            }
        }
        CheckReport { problems, hosts }
    }

    // ----- write commands -----

    fn exists_error(&self, name: &str, file: usize) -> Error {
        if self.is_multi() {
            Error::HostExistsIn {
                name: name.to_string(),
                file: self.display(file),
            }
        } else {
            Error::HostExists(name.to_string())
        }
    }

    /// The file a `--section` write goes to: the file holding the section,
    /// the root when none does.
    fn section_file(&self, section: Option<&str>) -> Result<usize> {
        match section {
            Some(s) => Ok(self.find_section(s)?.map_or(0, |(f, _)| f)),
            None => Ok(0),
        }
    }

    fn added_message(&self, placed: &Placed, explicit: bool, file: usize) -> String {
        let name = &placed.name;
        let suffix = self.suffix(file);
        match &placed.section {
            Some(s) if explicit => {
                format!("{name} added to section {s}{suffix}. Connect with: ssh {name}")
            }
            _ => format!("{name} added{suffix}. Connect with: ssh {name}"),
        }
    }

    /// `add`: into `--file` when given, else the file holding
    /// `spec.section` (the root when none does), else the root.
    pub fn add(&mut self, spec: &AddSpec, file: Option<&str>, env: &Env) -> Result<Change<Placed>> {
        let target = self.target(file)?;
        validate_name(&spec.name)?;
        if let Some(wl) = self.find_host(&spec.name) {
            return Err(self.exists_error(&spec.name, wl.file));
        }
        let dest = match target {
            Some(f) => f,
            None => self.section_file(spec.section.as_deref())?,
        };
        let defaults = self.defaults().cloned();
        let placed = self.files[dest]
            .config
            .add_with_defaults(spec, env, defaults.as_ref())?;
        let msg = self.added_message(&placed, spec.section.is_some(), dest);
        Ok(self.change(placed, vec![dest], vec![msg], Vec::new()))
    }

    /// Takes the host at `from` out of its file and places it in section
    /// `section` of file `dest` (created when missing), renaming it to
    /// `new_name` when given. The destination is touched first.
    fn relocate(
        &mut self,
        from: WorkspaceLocation,
        dest: usize,
        new_name: Option<&str>,
        section: &str,
    ) -> Placed {
        let source = &mut self.files[from.file].config;
        let mut block = source.remove_host(from.loc);
        source.finish();
        if let Some(new) = new_name {
            let mut names = vec![new.to_string()];
            names.extend(block.aliases().into_iter().filter(|a| a != new));
            block.set_names(&names);
        }
        block.last_line_mut().ensure_terminated();
        let target = &mut self.files[dest].config;
        target.ensure_trailing_newline();
        let placed = target.place(block, Some(section), None);
        self.touch(dest);
        self.touch(from.file);
        placed
    }

    /// `edit`: the host in `--file`, else its first definition in load
    /// order. With `spec.section` the host moves to the file holding the
    /// section (the root when none does; `--file` pins it to that file).
    pub fn edit(
        &mut self,
        spec: &EditSpec,
        file: Option<&str>,
        env: &Env,
    ) -> Result<Change<Placed>> {
        let target = self.target(file)?;
        let (wl, warnings) = self
            .locate(&spec.name, target)
            .ok_or_else(|| Error::EditTargetMissing(spec.name.clone()))?;
        let dest = match (&spec.section, target) {
            (None, _) => wl.file,
            (Some(_), Some(f)) => f,
            (Some(s), None) => self.section_file(Some(s))?,
        };
        let defaults = self.defaults().cloned();
        if dest == wl.file {
            let placed =
                self.files[wl.file]
                    .config
                    .edit_with_defaults(spec, env, defaults.as_ref())?;
            let suffix = self.suffix(wl.file);
            let msg = match (&spec.section, &placed.section) {
                (Some(_), Some(s)) => {
                    format!("{} updated and moved to section {s}{suffix}.", placed.name)
                }
                _ => format!("{} updated{suffix}.", placed.name),
            };
            return Ok(self.change(placed, vec![wl.file], vec![msg], warnings));
        }
        let section = spec
            .section
            .clone()
            .expect("dest differs only with a section");
        let in_place = EditSpec {
            section: None,
            ..spec.clone()
        };
        let edited =
            self.files[wl.file]
                .config
                .edit_with_defaults(&in_place, env, defaults.as_ref())?;
        let loc = self.files[wl.file]
            .config
            .find_primary(&edited.name)
            .expect("edited host is in its file");
        let placed = self.relocate(
            WorkspaceLocation { file: wl.file, loc },
            dest,
            None,
            &section,
        );
        let msg = format!(
            "{} updated and moved from {} to section {} in {}.",
            placed.name,
            self.display(wl.file),
            placed.section.as_deref().unwrap_or(&section),
            self.display(dest)
        );
        Ok(self.change(placed, vec![dest, wl.file], vec![msg], warnings))
    }

    /// Applies `apply` to the hosts `selector` picks: one host (in `--file`
    /// or its first definition), or with a regex every matching host of
    /// every file, a name matched in an earlier file skipped in later ones.
    fn update_hosts(
        &mut self,
        selector: &HostSelector,
        target: Option<usize>,
        mut apply: impl FnMut(&mut HostBlock),
    ) -> Result<(Vec<String>, Vec<usize>, Vec<String>)> {
        let (plan, warnings) = match selector {
            HostSelector::Name(name) => {
                let (wl, w) = self
                    .locate(name, target)
                    .ok_or_else(|| Error::HostNotFound(name.clone()))?;
                (vec![(wl.file, vec![wl.loc])], w)
            }
            HostSelector::Regex(pattern) => {
                let files: Vec<usize> = match target {
                    Some(f) => vec![f],
                    None => (0..self.files.len()).collect(),
                };
                let mut seen: HashSet<String> = HashSet::new();
                let mut plan = Vec::new();
                for f in files {
                    let config = &self.files[f].config;
                    let locs = match config.select(selector) {
                        Ok(l) => l,
                        Err(Error::NoMatch(_)) => continue,
                        Err(e) => return Err(e),
                    };
                    let locs: Vec<HostLocation> = locs
                        .into_iter()
                        .filter(|l| !seen.contains(&config.host(*l).primary()))
                        .collect();
                    for l in &locs {
                        seen.insert(config.host(*l).primary());
                    }
                    if !locs.is_empty() {
                        plan.push((f, locs));
                    }
                }
                if plan.is_empty() {
                    return Err(Error::NoMatch(pattern.clone()));
                }
                (plan, Vec::new())
            }
        };
        let mut names = Vec::new();
        let mut files = Vec::new();
        for (f, locs) in plan {
            let config = &mut self.files[f].config;
            for loc in locs {
                let block = config.host_mut(loc);
                apply(block);
                names.push(block.primary());
            }
            config.finish();
            files.push(f);
        }
        Ok((names, files, warnings))
    }

    fn updated_message(
        &self,
        selector: &HostSelector,
        names: &[String],
        files: &[usize],
    ) -> String {
        match selector {
            HostSelector::Name(_) => {
                format!("{} updated{}.", names.join(", "), self.suffix(files[0]))
            }
            HostSelector::Regex(_) => {
                let where_ = match (self.is_multi(), files) {
                    (false, _) => String::new(),
                    (true, [one]) => format!(" in {}", self.display(*one)),
                    (true, many) => format!(" in {}", plural(many.len(), "file")),
                };
                format!(
                    "{} updated{where_}: {}",
                    plural(names.len(), "host"),
                    names.join(", ")
                )
            }
        }
    }

    /// `set`: sets every pair on the selected hosts (see [`Config::set`](crate::Config::set)).
    pub fn set(
        &mut self,
        selector: &HostSelector,
        pairs: &[(String, String)],
        append: bool,
        file: Option<&str>,
    ) -> Result<Change<Vec<String>>> {
        let target = self.target(file)?;
        for (k, _) in pairs {
            check_settable(k)?;
        }
        let (names, files, warnings) =
            self.update_hosts(selector, target, |b| apply_pairs(b, pairs, append))?;
        let msg = self.updated_message(selector, &names, &files);
        Ok(self.change(names, files, vec![msg], warnings))
    }

    /// `unset`: removes every line of each key from the selected hosts.
    pub fn unset(
        &mut self,
        selector: &HostSelector,
        keys: &[String],
        file: Option<&str>,
    ) -> Result<Change<Vec<String>>> {
        let target = self.target(file)?;
        let (names, files, warnings) = self.update_hosts(selector, target, |b| {
            for k in keys {
                b.unset(k);
            }
        })?;
        let msg = self.updated_message(selector, &names, &files);
        Ok(self.change(names, files, vec![msg], warnings))
    }

    /// `clone`: the copy goes to `--file` when given, else the file holding
    /// `spec.section` (the root when none does), else the source's file
    /// and section.
    pub fn clone_host(&mut self, spec: &CloneSpec, file: Option<&str>) -> Result<Change<Placed>> {
        let target = self.target(file)?;
        let (src, warnings) = self
            .locate(&spec.source, None)
            .ok_or_else(|| Error::HostNotFound(spec.source.clone()))?;
        validate_name(&spec.new_name)?;
        if self.find_host(&spec.new_name).is_some() {
            return Err(Error::TargetExists(spec.new_name.clone()));
        }
        for (k, _) in &spec.overrides {
            check_settable(k)?;
        }
        let dest = match (target, &spec.section) {
            (Some(f), _) => f,
            (None, Some(s)) => self.section_file(Some(s))?,
            (None, None) => src.file,
        };
        let placed = if dest == src.file {
            self.files[dest].config.clone_host(spec)?
        } else {
            let block = clone_block(self.host(src), spec);
            let config = &mut self.files[dest].config;
            config.ensure_trailing_newline();
            config.place(block, spec.section.as_deref(), None)
        };
        let msg = self.added_message(&placed, spec.section.is_some(), dest);
        Ok(self.change(placed, vec![dest], vec![msg], warnings))
    }

    /// `move`: a rename edits the host where it is (`--file` picks the
    /// definition); a section move goes to `--file` when given, else the
    /// file holding the section (the root when none does). A move between
    /// files takes the entry, comments included, out of its file and writes
    /// the destination first.
    pub fn move_host(
        &mut self,
        name: &str,
        new_name: Option<&str>,
        section: Option<&str>,
        file: Option<&str>,
    ) -> Result<Change<Moved>> {
        if new_name.is_none() && section.is_none() {
            return Err(Error::MoveNeedsTarget);
        }
        let target = self.target(file)?;
        let lookup = if section.is_some() { None } else { target };
        let (src, warnings) = self
            .locate(name, lookup)
            .ok_or_else(|| Error::HostNotFound(name.to_string()))?;
        let old_name = self.host(src).primary();
        if let Some(new) = new_name {
            validate_name(new)?;
            if self.find_host_all(new).into_iter().any(|wl| wl != src) {
                return Err(Error::TargetExists(new.to_string()));
            }
        }
        let dest = match (section, target) {
            (None, _) => src.file,
            (Some(_), Some(f)) => f,
            (Some(s), None) => self.section_file(Some(s))?,
        };
        let (moved, files) = if dest == src.file {
            let moved = self.files[dest].config.move_host(name, new_name, section)?;
            (moved, vec![dest])
        } else {
            let s = section.expect("dest differs only with a section");
            let placed = self.relocate(src, dest, new_name, s);
            let moved = Moved {
                old_name: old_name.clone(),
                new_name: placed.name,
                section: placed.section,
            };
            (moved, vec![dest, src.file])
        };
        let (old, new) = (&moved.old_name, &moved.new_name);
        let place = match (&moved.section, dest == src.file) {
            (Some(s), true) => format!("to section {s}{}", self.suffix(dest)),
            (Some(s), false) => format!(
                "from {} to section {s} in {}",
                self.display(src.file),
                self.display(dest)
            ),
            (None, _) => String::new(),
        };
        let msg = match (new_name, &moved.section) {
            (Some(_), Some(_)) => {
                format!("{old} renamed to {new} and moved {place}. Connect with: ssh {new}")
            }
            (Some(_), None) => format!(
                "{old} renamed to {new}{}. Connect with: ssh {new}",
                self.suffix(src.file)
            ),
            (None, Some(_)) => format!("{old} moved {place}."),
            (None, None) => format!("{old} unchanged."),
        };
        Ok(self.change(moved, files, vec![msg], warnings))
    }

    /// `delete`: every name must exist (in `--file`, or its first
    /// definition) before anything is removed.
    pub fn delete(&mut self, names: &[String], file: Option<&str>) -> Result<Change<Vec<String>>> {
        let target = self.target(file)?;
        let mut plan: Vec<(usize, String)> = Vec::new();
        let mut warnings = Vec::new();
        for n in names {
            let (wl, w) = self
                .locate(n, target)
                .ok_or_else(|| Error::HostNotFound(n.clone()))?;
            warnings.extend(w);
            let p = self.host(wl).primary();
            if !plan.contains(&(wl.file, p.clone())) {
                plan.push((wl.file, p));
            }
        }
        let mut files: Vec<usize> = Vec::new();
        for (f, _) in &plan {
            if !files.contains(f) {
                files.push(*f);
            }
        }
        for &f in &files {
            let in_file: Vec<String> = plan
                .iter()
                .filter(|(g, _)| *g == f)
                .map(|(_, p)| p.clone())
                .collect();
            self.files[f].config.delete(&in_file)?;
        }
        let messages = plan
            .iter()
            .map(|(f, p)| {
                if self.is_multi() {
                    format!("{p} deleted from {}.", self.display(*f))
                } else {
                    format!("{p} deleted.")
                }
            })
            .collect();
        let value = plan.into_iter().map(|(_, p)| p).collect();
        Ok(self.change(value, files, messages, warnings))
    }

    fn sweep(&mut self, file: Option<&str>) -> Result<Vec<usize>> {
        match self.target(file)? {
            Some(f) => Ok(vec![f]),
            None => {
                if let Some(e) = self
                    .load_order
                    .iter()
                    .find(|e| matches!(e.state, FileState::Unreadable(_)))
                {
                    return Err(self.entry_index(e).expect_err("unreadable"));
                }
                Ok((0..self.files.len()).collect())
            }
        }
    }

    /// `delete-all`'s count before it runs: `(hosts, files holding them)`
    /// over every file, or over the file `--file` names.
    pub fn delete_all_count(&mut self, file: Option<&str>) -> Result<(usize, usize)> {
        let files = self.sweep(file)?;
        let counts: Vec<usize> = files
            .iter()
            .map(|&f| self.files[f].config.host_count())
            .collect();
        Ok((
            counts.iter().sum(),
            counts.iter().filter(|&&c| c > 0).count(),
        ))
    }

    /// The `delete-all` confirmation prompt, trailing space included:
    /// `Delete 14 hosts from /home/me/.ssh/config? [y/N] ` on one file (or
    /// with `--file`), `Delete 8 hosts from 5 files? [y/N] ` on several.
    pub fn delete_all_prompt(&mut self, file: Option<&str>) -> Result<String> {
        let files = self.sweep(file)?;
        let (hosts, holding) = self.delete_all_count(file)?;
        let from = if files.len() == 1 {
            self.files[files[0]].path.display().to_string()
        } else {
            plural(holding, "file")
        };
        Ok(format!(
            "Delete {} from {from}? [y/N] ",
            plural(hosts, "host")
        ))
    }

    /// `delete-all`: removes every host from every file (or from the file
    /// `--file` names); comments, banners, `Include` lines and `Host *`
    /// stay.
    pub fn delete_all(&mut self, file: Option<&str>) -> Result<Change<usize>> {
        let single = file.is_some() || !self.is_multi();
        let files = self.sweep(file)?;
        let mut removed = 0;
        let mut changed = Vec::new();
        for f in files {
            let n = self.files[f].config.delete_all();
            if n > 0 {
                changed.push(f);
            }
            removed += n;
        }
        let msg = if single {
            format!("{} deleted.", plural(removed, "host"))
        } else {
            format!(
                "{} deleted from {}.",
                plural(removed, "host"),
                plural(changed.len(), "file")
            )
        };
        Ok(self.change(removed, changed, vec![msg], Vec::new()))
    }

    fn answers_message(&self, host: &str, names: &[String], file: usize) -> String {
        format!(
            "{host}{} now answers to: {}",
            self.suffix(file),
            names.join(" ")
        )
    }

    /// `alias`: adds names to the host's `Host` line (in `--file`, or its
    /// first definition); a name any file uses for another host is
    /// [`Error::AliasTaken`].
    pub fn alias(
        &mut self,
        name: &str,
        aliases: &[String],
        file: Option<&str>,
    ) -> Result<Change<Vec<String>>> {
        let target = self.target(file)?;
        let (wl, warnings) = self
            .locate(name, target)
            .ok_or_else(|| Error::HostNotFound(name.to_string()))?;
        let own = self.host(wl).names();
        for a in aliases {
            validate_name(a)?;
            if own.contains(a) {
                continue;
            }
            if let Some(other) = self.find_host(a) {
                return Err(Error::AliasTaken {
                    alias: a.clone(),
                    owner: self.host(other).primary(),
                });
            }
        }
        let primary = self.host(wl).primary();
        let names = self.files[wl.file].config.alias(&primary, aliases)?;
        let msg = self.answers_message(&names[0], &names, wl.file);
        Ok(self.change(names, vec![wl.file], vec![msg], warnings))
    }

    /// `unalias`: with `host`, from that host (in `--file`, or its first
    /// definition); without, from the first host in load order carrying
    /// the first alias.
    pub fn unalias(
        &mut self,
        host: Option<&str>,
        aliases: &[String],
        file: Option<&str>,
    ) -> Result<Change<crate::ops::Unaliased>> {
        let target = self.target(file)?;
        let (f, warnings) = match host {
            Some(h) => {
                let (wl, w) = self
                    .locate(h, target)
                    .ok_or_else(|| Error::HostNotFound(h.to_string()))?;
                (wl.file, w)
            }
            None => {
                let first = aliases
                    .first()
                    .ok_or_else(|| Error::Usage("give at least one alias.".to_string()))?;
                let candidates: Vec<usize> = match target {
                    Some(t) => vec![t],
                    None => (0..self.files.len()).collect(),
                };
                let holds = |i: &usize, alias: bool| {
                    self.files[*i].config.hosts().iter().any(|h| {
                        if alias {
                            h.aliases().contains(first)
                        } else {
                            h.primary() == *first
                        }
                    })
                };
                let f = candidates
                    .iter()
                    .find(|i| holds(i, true))
                    .or_else(|| candidates.iter().find(|i| holds(i, false)))
                    .copied()
                    .unwrap_or(candidates[0]);
                (f, Vec::new())
            }
        };
        let result = self.files[f].config.unalias(host, aliases)?;
        let msg = self.answers_message(&result.host, &result.names, f);
        Ok(self.change(result, vec![f], vec![msg], warnings))
    }

    /// `rename-section`: in `--file`, else the file holding the section.
    pub fn rename_section(
        &mut self,
        old: &str,
        new: &str,
        file: Option<&str>,
    ) -> Result<Change<SectionRename>> {
        let f = match self.target(file)? {
            Some(f) => f,
            None => self
                .find_section(old)?
                .map(|(f, _)| f)
                .ok_or_else(|| Error::SectionNotFound(old.to_string()))?,
        };
        let outcome = self.files[f].config.rename_section(old, new)?;
        let suffix = self.suffix(f);
        let msg = match &outcome {
            SectionRename::Renamed { from, to } => {
                format!("section {from} renamed to {to}{suffix}.")
            }
            SectionRename::Merged { from, into } => {
                format!("section {from} merged into {into}{suffix}.")
            }
        };
        Ok(self.change(outcome, vec![f], vec![msg], Vec::new()))
    }

    /// `add-section`: in `--file`, else the root. A name another file
    /// already holds is [`Error::SectionExists`] too, so no write makes a
    /// section ambiguous.
    pub fn add_section(
        &mut self,
        name: &str,
        before: Option<&str>,
        file: Option<&str>,
    ) -> Result<Change<SectionAdded>> {
        let f = self.target(file)?.unwrap_or(0);
        if !name.trim().is_empty()
            && self
                .files
                .iter()
                .enumerate()
                .any(|(i, c)| i != f && c.config.find_section(name).is_some())
        {
            return Err(Error::SectionExists(name.to_string()));
        }
        let added = self.files[f].config.add_section(name, before)?;
        let msg = if self.is_multi() {
            let at = self.display(f);
            match (&added.before, &added.catch_all) {
                (Some(b), _) => format!("section {name} added before {b} in {at}."),
                (None, Some((c, n))) if *n > 0 => format!(
                    "section {name} added to {at}; {c} created with {}.",
                    plural(*n, "host")
                ),
                (None, _) => format!("section {name} added to {at}."),
            }
        } else {
            match (&added.before, &added.catch_all) {
                (Some(b), _) => format!("section {name} added before {b}."),
                (None, Some((c, n))) => {
                    format!(
                        "section {name} added; {c} created with {}.",
                        plural(*n, "host")
                    )
                }
                (None, None) => format!("section {name} added."),
            }
        };
        Ok(self.change(added, vec![f], vec![msg], Vec::new()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(path: &Path, text: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    const CYPRESS: &str = "Host cypressPro\n    HostName 10.10.0.2\n    User travis\n";
    const RANCH: &str = "Host dcevant\n    HostName dcevant.ranch.lan\n";

    fn df_austin() -> String {
        format!(
            "{}\nHost db1\n    HostName db1.example.com\n    User postgres\n",
            crate::banner::banner_text("data foundry")
        )
    }

    /// `~/.ssh/config` including config.d/*: cypress, df-austin, ranch.
    fn home() -> tempfile::TempDir {
        let tmp = tempfile::tempdir().unwrap();
        let ssh = tmp.path().join(".ssh");
        write(
            &ssh.join("config"),
            "Include ~/.ssh/config.d/*\n\nHost github\n    HostName github.com\n    User git\n",
        );
        write(&ssh.join("config.d/cypress"), CYPRESS);
        write(&ssh.join("config.d/df-austin"), &df_austin());
        write(&ssh.join("config.d/ranch"), RANCH);
        tmp
    }

    fn load(tmp: &tempfile::TempDir) -> Workspace {
        Workspace::load_with_home(tmp.path().join(".ssh/config"), Some(tmp.path())).unwrap()
    }

    #[test]
    fn workspace_loads_root_first_then_includes_in_load_order() {
        let tmp = home();
        let ws = load(&tmp);
        assert!(ws.is_multi());
        let names: Vec<String> = (0..ws.files.len()).map(|i| ws.display(i)).collect();
        assert_eq!(
            names,
            vec![
                "~/.ssh/config",
                "~/.ssh/config.d/cypress",
                "~/.ssh/config.d/df-austin",
                "~/.ssh/config.d/ranch"
            ]
        );
        assert_eq!(ws.find_host("dcevant").unwrap().file, 3);
        assert_eq!(ws.find_section("DATA FOUNDRY").unwrap(), Some((2, 0)));
        assert_eq!(ws.find_section("lab").unwrap(), None);
        assert_eq!(ws.host_count(), 4);
        assert_eq!(ws.include_summary(), (4, 4));
        let wl = ws.find_host("db1").unwrap();
        assert_eq!(ws.host_line(wl), 11);
    }

    #[test]
    fn workspace_root_without_include_is_a_workspace_of_one() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join(".ssh/config");
        write(&root, "Host vps\n    HostName vps.example.com\n");
        let ws = Workspace::load_with_home(&root, Some(tmp.path())).unwrap();
        assert!(!ws.is_multi());
        assert_eq!(ws.files.len(), 1);
        assert!(ws.includes.is_empty());
        assert!(ws.load_warnings().is_empty());
    }

    #[test]
    fn workspace_include_matching_nothing_is_still_one_file() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join(".ssh/config");
        write(&root, "Include ~/.ssh/work/*\nHost vps\n");
        let ws = Workspace::load_with_home(&root, Some(tmp.path())).unwrap();
        assert!(!ws.is_multi());
        assert_eq!(ws.files.len(), 1);
        assert!(ws.includes[0].matches_nothing);
    }

    #[test]
    fn workspace_inc_13_file_resolves_bare_names_paths_and_rejects_unknown_names() {
        let tmp = home();
        let mut ws = load(&tmp);
        let ssh = tmp.path().join(".ssh");
        // Bare name against the file names; the root's name too.
        assert_eq!(ws.resolve_file("cypress").unwrap(), 1);
        assert_eq!(ws.resolve_file("config").unwrap(), 0);
        // A path as given, and with ~/ expanded.
        let p = ssh.join("config.d/ranch");
        assert_eq!(ws.resolve_file(p.to_str().unwrap()).unwrap(), 3);
        assert_eq!(ws.resolve_file("~/.ssh/config.d/df-austin").unwrap(), 2);
        // An unknown name is exit 1 with the docs' message.
        let err = ws.resolve_file("nas").unwrap_err();
        assert_eq!(err.to_string(), "no such file nas in the workspace.");
        assert_eq!(err.exit_code(), 1);
        // A path outside every Include pattern is unknown too.
        let err = ws.resolve_file("/etc/hosts").unwrap_err();
        assert_eq!(err.to_string(), "no such file /etc/hosts in the workspace.");
        // A new path an Include pattern matches joins the workspace.
        let n = ws.resolve_file("~/.ssh/config.d/df-evant").unwrap();
        assert_eq!(n, 4);
        assert!(!ws.files[4].existed);
        assert_eq!(ws.resolve_file("df-evant").unwrap(), 4);
        // A bare name two files share is ambiguous.
        write(&ssh.join("config.d/lab"), "Host a\n");
        write(&ssh.join("ranch.d/lab"), "Host b\n");
        fs::write(
            ssh.join("config.d/ranch"),
            format!("Include ranch.d/*\n{RANCH}"),
        )
        .unwrap();
        let mut ws = load(&tmp);
        let err = ws.resolve_file("lab").unwrap_err();
        assert_eq!(
            err.to_string(),
            "file lab matches ~/.ssh/config.d/lab and ~/.ssh/ranch.d/lab. Give a path."
        );
        assert_eq!(err.exit_code(), 1);
    }

    #[cfg(unix)]
    #[test]
    fn workspace_inc_18_unwritable_destination_leaves_both_files_and_no_backups() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = home();
        let ssh = tmp.path().join(".ssh");
        let dest = ssh.join("config.d/df-austin");
        let source = ssh.join("config.d/ranch");
        fs::set_permissions(&dest, fs::Permissions::from_mode(0o444)).unwrap();
        let mut ws = load(&tmp);
        let change = ws
            .move_host("dcevant", None, Some("data foundry"), None)
            .unwrap();
        assert_eq!(change.files, vec![2, 3]);
        let err = ws.save(WriteOptions::default()).unwrap_err();
        assert_eq!(err.exit_code(), 3);
        assert!(err
            .to_string()
            .starts_with(&format!("cannot write {}", dest.display())));
        assert_eq!(fs::read_to_string(&dest).unwrap(), df_austin());
        assert_eq!(fs::read_to_string(&source).unwrap(), RANCH);
        for p in [&dest, &source] {
            assert!(!crate::io::backup_path(p).exists());
            assert!(!crate::io::dot_backup_path(p).exists());
        }
        fs::set_permissions(&dest, fs::Permissions::from_mode(0o600)).unwrap();
    }

    #[test]
    fn workspace_inc_19_included_file_opened_directly_is_a_single_file() {
        let tmp = home();
        let cypress = tmp.path().join(".ssh/config.d/cypress");
        let mut ws = Workspace::load_with_home(&cypress, Some(tmp.path())).unwrap();
        assert!(!ws.is_multi());
        assert_eq!(ws.files.len(), 1);
        let env = Env {
            user: Some("tester".into()),
            home: None,
        };
        let rows = ws.list(&env);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].row.line(), "cypressPro -> travis@10.10.0.2:22");
        let spec = AddSpec {
            name: "vps".into(),
            uri: "root@vps.example.com".into(),
            ..AddSpec::default()
        };
        let change = ws.add(&spec, None, &env).unwrap();
        assert_eq!(change.messages, vec!["vps added. Connect with: ssh vps"]);
        assert_eq!(ws.save(WriteOptions::default()).unwrap(), vec![0]);
        assert_eq!(
            fs::read_to_string(&cypress).unwrap(),
            format!(
                "{CYPRESS}\nHost vps\n    HostName vps.example.com\n    User root\n    Port 22\n"
            )
        );
        // No other file was read into the workspace or written.
        let ssh = tmp.path().join(".ssh");
        assert_eq!(
            fs::read_to_string(ssh.join("config.d/ranch")).unwrap(),
            RANCH
        );
        assert!(!ssh.join("config~").exists());
        assert!(!ssh.join("config.d/ranch~").exists());
    }

    #[test]
    fn workspace_backup_dot_when_glob_matched() {
        let tmp = home();
        let ssh = tmp.path().join(".ssh");
        let mut ws = load(&tmp);
        let before: Vec<PathBuf> = ws.files.iter().map(|f| f.path.clone()).collect();
        assert_eq!(ws.backup_path_for(0), ssh.join("config~"));
        assert_eq!(ws.backup_path_for(1), ssh.join("config.d/.cypress~"));
        ws.set(
            &HostSelector::Name("cypressPro".into()),
            &[("Port".into(), "2222".into())],
            false,
            None,
        )
        .unwrap();
        assert_eq!(ws.save(WriteOptions::default()).unwrap(), vec![1]);
        assert_eq!(
            fs::read_to_string(ssh.join("config.d/.cypress~")).unwrap(),
            CYPRESS
        );
        assert!(!ssh.join("config.d/cypress~").exists());
        let again = load(&tmp);
        let after: Vec<PathBuf> = again.files.iter().map(|f| f.path.clone()).collect();
        assert_eq!(after, before);
        // A root outside every pattern keeps <config>~ (D2), and save_text
        // uses the same rule.
        let mut ws = load(&tmp);
        ws.save_text(0, "Include ~/.ssh/config.d/*\n", WriteOptions::default())
            .unwrap();
        assert!(ssh.join("config~").exists());
        assert!(!ssh.join(".config~").exists());
        ws.save_text(3, RANCH, WriteOptions::default()).unwrap();
        assert!(ssh.join("config.d/.ranch~").exists());
        // A root matched by its own pattern gets the dot backup too.
        write(&ssh.join("config.d/self"), "Include ~/.ssh/config.d/*\n");
        let ws = Workspace::load_with_home(ssh.join("config.d/self"), Some(tmp.path())).unwrap();
        assert_eq!(ws.backup_path_for(0), ssh.join("config.d/.self~"));
    }

    #[test]
    fn workspace_save_file_writes_only_that_file_with_its_backup() {
        let tmp = home();
        let mut ws = load(&tmp);
        ws.set(
            &HostSelector::Name("dcevant".into()),
            &[("User".into(), "x".into())],
            false,
            None,
        )
        .unwrap();
        ws.set(
            &HostSelector::Name("cypressPro".into()),
            &[("User".into(), "y".into())],
            false,
            None,
        )
        .unwrap();
        assert_eq!(ws.modified(), vec![1, 3]);
        assert!(ws.save_file(3, WriteOptions::default()).unwrap());
        let ssh = tmp.path().join(".ssh");
        assert!(ssh.join("config.d/.ranch~").exists());
        assert!(!ssh.join("config.d/ranch~").exists());
        assert!(!ssh.join("config.d/.cypress~").exists());
        assert_eq!(ws.modified(), vec![1]);
        ws.reload_file(1).unwrap();
        assert!(ws.modified().is_empty());
    }
}
