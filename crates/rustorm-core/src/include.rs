//! `Include` resolution, the way ssh reads it.
//!
//! An `Include` line holds one or more whitespace-separated patterns. Each
//! pattern is an absolute path, a `~/` path under the home directory, or a
//! path relative to `~/.ssh` (also when the root config lives elsewhere).
//! Patterns may hold `*`, `?` and `[...]`; their matches load in lexical
//! order, only regular files load, and a pattern that matches nothing loads
//! nothing. An included file's own `Include` lines are followed depth first
//! at the position of the line, and a file already loaded is not loaded
//! again, so an include cycle ends at the first repeat.
//!
//! [`resolve_includes`] returns the tree of `Include` lines and their
//! matches; [`crate::Workspace::load`] walks the same tree and keeps the
//! parsed files. `combine` shares [`resolve_include`] and [`glob_match`].

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::io::ConfigFile;
use crate::model::{split_patterns, Config, Entry, Line};
use crate::Error;

/// Glob match with `*` (any run of non-`/` characters), `?` (one non-`/`
/// character) and `[...]` (one character from a set or range, `!` or `^`
/// negating it); everything else is literal.
pub fn glob_match(pattern: &str, text: &str) -> bool {
    let p: Vec<char> = pattern.chars().collect();
    let t: Vec<char> = text.chars().collect();
    glob_chars(&p, &t)
}

fn glob_chars(p: &[char], t: &[char]) -> bool {
    match (p.first(), t.first()) {
        (None, None) => true,
        (None, Some(_)) => false,
        (Some('*'), _) => {
            let mut i = 0;
            loop {
                if glob_chars(&p[1..], &t[i..]) {
                    return true;
                }
                if i < t.len() && t[i] != '/' {
                    i += 1;
                } else {
                    return false;
                }
            }
        }
        (Some('?'), Some(c)) => *c != '/' && glob_chars(&p[1..], &t[1..]),
        (Some('['), Some(c)) => match class_match(&p[1..], *c) {
            Some((matched, used)) => matched && *c != '/' && glob_chars(&p[1 + used..], &t[1..]),
            None => *c == '[' && glob_chars(&p[1..], &t[1..]),
        },
        (Some(a), Some(b)) => a == b && glob_chars(&p[1..], &t[1..]),
        (Some(_), None) => false,
    }
}

/// Matches `c` against the class whose body starts at `p` (just after `[`).
/// Returns whether it matched and how many pattern characters the class
/// used, closing `]` included; `None` when the class is not closed.
fn class_match(p: &[char], c: char) -> Option<(bool, usize)> {
    let mut i = 0;
    let negate = matches!(p.first(), Some('!' | '^'));
    if negate {
        i += 1;
    }
    let mut matched = false;
    let mut first = true;
    while i < p.len() {
        if p[i] == ']' && !first {
            return Some((matched != negate, i + 1));
        }
        first = false;
        if i + 2 < p.len() && p[i + 1] == '-' && p[i + 2] != ']' {
            if p[i] <= c && c <= p[i + 2] {
                matched = true;
            }
            i += 3;
        } else {
            if p[i] == c {
                matched = true;
            }
            i += 1;
        }
    }
    None
}

fn has_glob(s: &str) -> bool {
    s.contains(['*', '?', '['])
}

/// An `Include` pattern as an absolute path string: absolute as is, `~/`
/// under `home`, anything else under `~/.ssh`, as ssh resolves them.
/// `None` when the pattern needs the home directory and it is unknown.
pub fn resolve_include(pattern: &str, home: Option<&Path>) -> Option<String> {
    let p = pattern.trim_matches('"');
    if p.starts_with('/') {
        return Some(p.to_string());
    }
    if let Some(rest) = p.strip_prefix("~/") {
        return Some(home?.join(rest).display().to_string());
    }
    Some(home?.join(".ssh").join(p).display().to_string())
}

/// Every regular file an absolute pattern matches, in lexical order of
/// their paths. Components with glob characters are matched against the
/// directory listing; a leading `.` in a name must be matched literally.
/// Directories and anything unreadable along the way are skipped.
pub fn expand_pattern(resolved: &str) -> Vec<PathBuf> {
    if !has_glob(resolved) {
        let p = PathBuf::from(resolved);
        return if p.is_file() { vec![p] } else { Vec::new() };
    }
    let mut candidates = vec![PathBuf::from("/")];
    for comp in resolved.split('/').filter(|c| !c.is_empty()) {
        let mut next = Vec::new();
        for base in &candidates {
            if !has_glob(comp) {
                let p = base.join(comp);
                if p.exists() {
                    next.push(p);
                }
                continue;
            }
            let Ok(dir) = fs::read_dir(base) else {
                continue;
            };
            for entry in dir.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                if name.starts_with('.') && !comp.starts_with('.') {
                    continue;
                }
                if glob_match(comp, &name) {
                    next.push(base.join(&name));
                }
            }
        }
        candidates = next;
    }
    let mut files: Vec<PathBuf> = candidates.into_iter().filter(|p| p.is_file()).collect();
    files.sort();
    files
}

/// True when a file name looks like a backup: it ends in `~`, `.bak`,
/// `.orig` or `.old`, or contains `.bak.`.
pub fn looks_like_backup(path: &Path) -> bool {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    name.ends_with('~')
        || name.ends_with(".bak")
        || name.ends_with(".orig")
        || name.ends_with(".old")
        || name.contains(".bak.")
}

/// The reason text for an unreadable file, as `check` and the warnings
/// print it: `permission denied`, `no such file or directory`, ...
pub fn io_reason(e: &std::io::Error) -> String {
    use std::io::ErrorKind;
    match e.kind() {
        ErrorKind::PermissionDenied => "permission denied".to_string(),
        ErrorKind::NotFound => "no such file or directory".to_string(),
        ErrorKind::InvalidData => "not valid UTF-8".to_string(),
        _ => {
            let s = e.to_string();
            let s = match s.find(" (os error") {
                Some(i) => s[..i].to_string(),
                None => s,
            };
            let mut chars = s.chars();
            match chars.next() {
                Some(c) => c.to_lowercase().chain(chars).collect(),
                None => s,
            }
        }
    }
}

/// Where an `Include` line stands in its file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum IncludePlacement {
    /// Outside any block (preamble or section).
    TopLevel,
    /// In the body of `Host *`; read as global.
    HostStar,
    /// In the body of another `Host` or a `Match` block; still loaded.
    Block(String),
}

/// One `Include` line of a file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IncludeLine {
    /// The line's value as written: every pattern, quotes kept.
    pub value: String,
    /// The patterns, split on whitespace, quotes removed.
    pub patterns: Vec<String>,
    /// 1-based line number in the file.
    pub line: usize,
    /// Where the line stands.
    pub placement: IncludePlacement,
}

fn include_of(line: &Line) -> Option<String> {
    line.directive()
        .filter(|d| d.key.eq_ignore_ascii_case("include"))
        .map(|d| d.value)
}

/// Every `Include` line of `config` in file order, with its placement.
pub fn include_lines(config: &Config) -> Vec<IncludeLine> {
    let mut out = Vec::new();
    let mut n = 0usize;
    let mut visit = |line: &Line, placement: &IncludePlacement, out: &mut Vec<IncludeLine>| {
        n += 1;
        if let Some(value) = include_of(line) {
            out.push(IncludeLine {
                patterns: split_patterns(&value),
                value,
                line: n,
                placement: placement.clone(),
            });
        }
    };
    let top = IncludePlacement::TopLevel;
    let mut parts: Vec<(Option<&[Line]>, &[Entry])> = vec![(None, &config.preamble)];
    for s in &config.sections {
        parts.push((Some(&s.banner.lines), &s.entries));
    }
    for (banner, entries) in parts {
        for l in banner.unwrap_or(&[]) {
            visit(l, &top, &mut out);
        }
        for e in entries {
            match e {
                Entry::Line(l) => visit(l, &top, &mut out),
                Entry::Host(h) => {
                    let inner = if h.is_defaults() {
                        IncludePlacement::HostStar
                    } else {
                        IncludePlacement::Block(h.header.text().trim().to_string())
                    };
                    for l in &h.leading {
                        visit(l, &top, &mut out);
                    }
                    visit(&h.header, &top, &mut out);
                    for l in &h.body {
                        visit(l, &inner, &mut out);
                    }
                }
                Entry::Match(m) => {
                    let inner = IncludePlacement::Block(m.header.text().trim().to_string());
                    visit(&m.header, &top, &mut out);
                    for l in &m.body {
                        visit(l, &inner, &mut out);
                    }
                }
            }
        }
    }
    out
}

/// What became of one file an `Include` pattern matched.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum IncludeStatus {
    /// Read and parsed; `hosts` excludes `Host *`.
    Loaded {
        /// Number of hosts in the file.
        hosts: usize,
    },
    /// Already in the workspace (an earlier match or an include cycle).
    AlreadyLoaded,
    /// Could not be read; skipped (D24).
    Unreadable {
        /// Why, for example `permission denied`.
        reason: String,
    },
}

/// One file matched by an `Include` pattern.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct IncludedFile {
    /// The matched path (absolute, symlinks not resolved).
    pub path: PathBuf,
    /// Loaded, already loaded, or unreadable.
    pub status: IncludeStatus,
    /// The file's own `Include` patterns, in file order (empty unless
    /// loaded).
    pub nested: Vec<IncludeMatch>,
}

/// One pattern of one `Include` line and what it matched.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct IncludeMatch {
    /// The pattern as written (quotes removed).
    pub pattern: String,
    /// The pattern as an absolute path; `None` when it needs the home
    /// directory and none is known.
    pub resolved: Option<String>,
    /// The file holding the `Include` line.
    pub from: PathBuf,
    /// 1-based line number of the `Include` line in `from`.
    pub line: usize,
    /// The whole value of the `Include` line as written, every pattern.
    pub directive: String,
    /// Matched regular files in lexical order.
    pub files: Vec<IncludedFile>,
    /// True when the pattern matched no file (not an error).
    pub matches_nothing: bool,
    /// True when the line sits in `Host *`; its files load as global.
    pub inside_host_star: bool,
    /// The header of another `Host` or `Match` block holding the line.
    pub inside_block: Option<String>,
    /// The included file holding the line; `None` for the root's lines.
    pub nested_from: Option<PathBuf>,
    /// Nesting depth: 0 for the root's lines, 1 for a file it loads, ...
    pub depth: usize,
}

impl IncludeMatch {
    /// This match and every nested match below it, depth first.
    pub fn walk(&self) -> Vec<&IncludeMatch> {
        let mut out = vec![self];
        for f in &self.files {
            for n in &f.nested {
                out.extend(n.walk());
            }
        }
        out
    }
}

/// One file in load order, as the walk found it.
#[derive(Debug)]
pub(crate) enum Walked {
    /// Read and parsed.
    File(Box<ConfigFile>, usize),
    /// Matched but unreadable.
    Unreadable(PathBuf, String, usize),
}

pub(crate) fn canonical(path: &Path) -> PathBuf {
    fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

pub(crate) struct Walker<'a> {
    pub home: Option<&'a Path>,
    pub visited: HashSet<PathBuf>,
    pub loaded: Vec<Walked>,
}

impl Walker<'_> {
    pub(crate) fn walk(&mut self, config: &Config, from: &Path, depth: usize) -> Vec<IncludeMatch> {
        let mut out = Vec::new();
        for line in include_lines(config) {
            for pattern in &line.patterns {
                let resolved = resolve_include(pattern, self.home);
                let paths = resolved.as_deref().map(expand_pattern).unwrap_or_default();
                let mut files = Vec::new();
                for path in paths {
                    let key = canonical(&path);
                    if !self.visited.insert(key) {
                        files.push(IncludedFile {
                            path,
                            status: IncludeStatus::AlreadyLoaded,
                            nested: Vec::new(),
                        });
                        continue;
                    }
                    match ConfigFile::load(&path) {
                        Ok(file) => {
                            let hosts = file.config.host_count();
                            let config = file.config.clone();
                            self.loaded.push(Walked::File(Box::new(file), depth + 1));
                            let nested = self.walk(&config, &path, depth + 1);
                            files.push(IncludedFile {
                                path,
                                status: IncludeStatus::Loaded { hosts },
                                nested,
                            });
                        }
                        Err(e) => {
                            let reason = match &e {
                                Error::Read { source, .. } => io_reason(source),
                                other => other.to_string(),
                            };
                            self.loaded.push(Walked::Unreadable(
                                path.clone(),
                                reason.clone(),
                                depth + 1,
                            ));
                            files.push(IncludedFile {
                                path,
                                status: IncludeStatus::Unreadable { reason },
                                nested: Vec::new(),
                            });
                        }
                    }
                }
                out.push(IncludeMatch {
                    pattern: pattern.clone(),
                    resolved,
                    from: from.to_path_buf(),
                    line: line.line,
                    directive: line.value.clone(),
                    matches_nothing: files.is_empty(),
                    files,
                    inside_host_star: line.placement == IncludePlacement::HostStar,
                    inside_block: match &line.placement {
                        IncludePlacement::Block(h) => Some(h.clone()),
                        _ => None,
                    },
                    nested_from: (depth > 0).then(|| from.to_path_buf()),
                    depth,
                });
            }
        }
        out
    }
}

/// Resolves every `Include` of `config`, the parsed file at `root`, and of
/// every file they load, depth first. `home` resolves `~/` and relative
/// patterns (relative ones under `home/.ssh`). The root and each loaded
/// file are loaded once; a repeat is [`IncludeStatus::AlreadyLoaded`].
pub fn resolve_includes(config: &Config, root: &Path, home: Option<&Path>) -> Vec<IncludeMatch> {
    let mut walker = Walker {
        home,
        visited: HashSet::from([canonical(root)]),
        loaded: Vec::new(),
    };
    walker.walk(config, root, 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(path: &Path, text: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    fn cfg(text: &str) -> Config {
        Config::parse(text).unwrap()
    }

    fn paths(m: &IncludeMatch) -> Vec<PathBuf> {
        m.files.iter().map(|f| f.path.clone()).collect()
    }

    #[test]
    fn glob_star_does_not_cross_slash() {
        assert!(glob_match("/h/.ssh/config.d/*", "/h/.ssh/config.d/cypress"));
        assert!(!glob_match("/h/.ssh/config.d/*", "/h/.ssh/config.d/x/y"));
        assert!(glob_match("/h/.ssh/c?nfig", "/h/.ssh/config"));
        assert!(!glob_match("/h/.ssh/c?nfig", "/h/.ssh/cnfig"));
        assert!(glob_match("/a/*.conf", "/a/.conf"));
    }

    #[test]
    fn glob_classes() {
        assert!(glob_match("/a/[bc]x", "/a/cx"));
        assert!(!glob_match("/a/[bc]x", "/a/dx"));
        assert!(glob_match("/a/[a-c]1", "/a/b1"));
        assert!(glob_match("/a/[!a-c]1", "/a/d1"));
        assert!(!glob_match("/a/[!a-c]1", "/a/b1"));
        assert!(glob_match("/a/[x", "/a/[x"));
    }

    #[test]
    fn include_patterns_resolve_like_ssh() {
        let h = Path::new("/h");
        assert_eq!(resolve_include("/abs/x", Some(h)).unwrap(), "/abs/x");
        assert_eq!(resolve_include("~/x/*", Some(h)).unwrap(), "/h/x/*");
        assert_eq!(
            resolve_include("config.d/*", Some(h)).unwrap(),
            "/h/.ssh/config.d/*"
        );
        assert_eq!(resolve_include("config.d/*", None), None);
    }

    #[test]
    fn backup_names() {
        for n in [
            "a~",
            "a.bak",
            "a.orig",
            "a.old",
            "df-austin.bak.20260628232757",
        ] {
            assert!(looks_like_backup(Path::new(n)), "{n}");
        }
        for n in ["cypress", "bakery", "a.older"] {
            assert!(!looks_like_backup(Path::new(n)), "{n}");
        }
    }

    #[test]
    fn inc_3_each_pattern_form_resolves_like_ssh_and_globs_sort_lexically() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path();
        let ssh = home.join(".ssh");
        let abs = home.join("elsewhere/abs.conf");
        write(&abs, "Host abs\n");
        write(&home.join("tilde.conf"), "Host tilde\n");
        for name in ["zeta", "alpha", "mid", ".hidden"] {
            write(&ssh.join("config.d").join(name), &format!("Host {name}\n"));
        }
        fs::create_dir_all(ssh.join("config.d/subdir")).unwrap();
        write(&ssh.join("rel.conf"), "Host rel\n");
        let root = ssh.join("config");
        let text = format!(
            "Include {} ~/tilde.conf\nInclude rel.conf\nInclude config.d/*\nHost github\n    HostName github.com\n",
            abs.display()
        );
        write(&root, &text);
        let m = resolve_includes(&cfg(&text), &root, Some(home));
        assert_eq!(m.len(), 4);
        assert_eq!(m[0].pattern, abs.display().to_string());
        assert_eq!(paths(&m[0]), vec![abs.clone()]);
        assert_eq!(m[1].pattern, "~/tilde.conf");
        assert_eq!(paths(&m[1]), vec![home.join("tilde.conf")]);
        assert_eq!(m[0].directive, format!("{} ~/tilde.conf", abs.display()));
        assert_eq!(
            m[2].resolved.as_deref(),
            Some(ssh.join("rel.conf").to_str().unwrap())
        );
        assert_eq!(paths(&m[2]), vec![ssh.join("rel.conf")]);
        // Lexical order; the dotfile and the directory are skipped.
        assert_eq!(
            paths(&m[3]),
            vec![
                ssh.join("config.d/alpha"),
                ssh.join("config.d/mid"),
                ssh.join("config.d/zeta")
            ]
        );
        assert!(m.iter().all(|x| x.depth == 0 && x.nested_from.is_none()));
        assert!(m.iter().all(|x| !x.inside_host_star && !x.matches_nothing));
        assert_eq!(m[3].files[0].status, IncludeStatus::Loaded { hosts: 1 });
        // Relative patterns stay under ~/.ssh even when the root is elsewhere.
        let other_root = home.join("elsewhere/root");
        let m = resolve_includes(&cfg("Include rel.conf\n"), &other_root, Some(home));
        assert_eq!(paths(&m[0]), vec![ssh.join("rel.conf")]);
    }

    #[test]
    fn inc_4_include_inside_host_star_is_global_and_flagged() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path();
        let ssh = home.join(".ssh");
        write(
            &ssh.join("config.d/cypress"),
            "Host cypressPro\n    HostName 10.10.0.2\n",
        );
        write(&ssh.join("in-block"), "Host blocky\n");
        let text = "Host *\n    ServerAliveInterval 60\n    include ~/.ssh/config.d/*\n\nHost vps\n    HostName v\n    Include in-block\n";
        let root = ssh.join("config");
        write(&root, text);
        let m = resolve_includes(&cfg(text), &root, Some(home));
        assert_eq!(m.len(), 2);
        assert!(m[0].inside_host_star);
        assert_eq!(m[0].inside_block, None);
        assert_eq!(m[0].line, 3);
        assert_eq!(paths(&m[0]), vec![ssh.join("config.d/cypress")]);
        assert!(!m[1].inside_host_star);
        assert_eq!(m[1].inside_block.as_deref(), Some("Host vps"));
        assert_eq!(paths(&m[1]), vec![ssh.join("in-block")]);
    }

    #[test]
    fn inc_21_nested_includes_are_followed_depth_first_and_cycles_stop() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path();
        let ssh = home.join(".ssh");
        let root = ssh.join("config");
        let root_text = "Include config.d/*\nHost github\n    HostName github.com\n";
        write(&root, root_text);
        write(&ssh.join("config.d/a"), "Host a1\n");
        write(
            &ssh.join("config.d/ranch"),
            "Include ranch.d/*\nInclude ~/.ssh/config\nHost dcevant\n",
        );
        write(&ssh.join("config.d/zz"), "Host zz\n");
        write(
            &ssh.join("ranch.d/lab"),
            "Include ~/.ssh/config.d/zz\nHost lab-1\n",
        );
        let mut walker = Walker {
            home: Some(home),
            visited: HashSet::from([canonical(&root)]),
            loaded: Vec::new(),
        };
        let m = walker.walk(&cfg(root_text), &root, 0);
        let order: Vec<PathBuf> = walker
            .loaded
            .iter()
            .map(|w| match w {
                Walked::File(f, _) => f.path.clone(),
                Walked::Unreadable(p, _, _) => p.clone(),
            })
            .collect();
        // Depth first: zz loads through lab before the root's glob reaches it.
        assert_eq!(
            order,
            vec![
                ssh.join("config.d/a"),
                ssh.join("config.d/ranch"),
                ssh.join("ranch.d/lab"),
                ssh.join("config.d/zz"),
            ]
        );
        let ranch = &m[0].files[1];
        assert_eq!(ranch.nested.len(), 2);
        assert_eq!(
            ranch.nested[0].nested_from.as_deref(),
            Some(ssh.join("config.d/ranch").as_path())
        );
        assert_eq!(ranch.nested[0].depth, 1);
        let lab = &ranch.nested[0].files[0];
        assert_eq!(lab.nested[0].depth, 2);
        assert_eq!(
            lab.nested[0].files[0].status,
            IncludeStatus::Loaded { hosts: 1 }
        );
        // The include back to the root and the second sight of zz are repeats.
        assert_eq!(
            ranch.nested[1].files[0].status,
            IncludeStatus::AlreadyLoaded
        );
        assert_eq!(m[0].files[2].status, IncludeStatus::AlreadyLoaded);
        assert_eq!(m[0].walk().len(), 4);
    }

    #[test]
    fn inc_22_pattern_matching_nothing_is_not_an_error() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path();
        let root = home.join(".ssh/config");
        let text = "Include ~/.ssh/work/* missing.conf\nHost vps\n    HostName vps.example.com\n";
        write(&root, text);
        let m = resolve_includes(&cfg(text), &root, Some(home));
        assert_eq!(m.len(), 2);
        assert!(m.iter().all(|x| x.matches_nothing && x.files.is_empty()));
        assert_eq!(m[0].pattern, "~/.ssh/work/*");
        assert_eq!(m[1].pattern, "missing.conf");
        // Without a home directory the relative pattern cannot resolve.
        let m = resolve_includes(&cfg(text), &root, None);
        assert!(m.iter().all(|x| x.resolved.is_none() && x.matches_nothing));
    }
}
