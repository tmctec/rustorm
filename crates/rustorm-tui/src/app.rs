//! The TUI state machine and its rendering (docs/tui.md).

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, BorderType, Borders, Cell, Clear, List, ListItem, ListState, Paragraph, Row as TRow,
    Table, TableState, Wrap,
};
use ratatui::Frame;
use rustorm_core::{
    join_and, AddSpec, Change, CloneSpec, Config, EditSpec, Env, Error, FileState, HostSelector,
    ProblemKind, SectionRename, Workspace, WriteOptions,
};

use crate::editor::Editor;
use crate::hosts::{self, Column, Filters, Row, Sort};
use crate::theme::Theme;

/// Startup settings.
#[derive(Debug, Clone)]
pub struct Options {
    /// Skip the `<config>~` backup.
    pub no_backup: bool,
    /// Colors and glyphs.
    pub theme: Theme,
    /// `$USER` and home for URI resolution.
    pub env: Env,
}

impl Options {
    /// Options from the process environment.
    pub fn detect() -> Options {
        Options {
            no_backup: false,
            theme: Theme::detect(),
            env: Env::from_process(),
        }
    }
}

/// The focused pane.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    /// The file list; only on a workspace of several files.
    Files,
    /// The section list.
    Sections,
    /// The host table.
    Table,
    /// The editor.
    Editor,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Msg {
    Success(String),
    Error(String),
    Info(String),
}

#[derive(Debug, Clone)]
enum FormKind {
    Add,
    Edit {
        name: String,
        identity: String,
        section: Option<String>,
    },
    Clone {
        source: String,
    },
    Move {
        name: String,
        section: Option<String>,
    },
    RenameSection {
        name: String,
        /// The file holding it, when another file holds a section of the
        /// same name.
        file: Option<usize>,
    },
    AddSection,
}

#[derive(Debug, Clone)]
struct Form {
    kind: FormKind,
    title: String,
    fields: Vec<(&'static str, String)>,
    focus: usize,
    error: Option<String>,
}

#[derive(Debug, Clone)]
enum Op {
    Add(AddSpec),
    Edit {
        spec: EditSpec,
        remove_identity: bool,
    },
    Delete(String),
    Clone(CloneSpec),
    Move {
        name: String,
        new_name: Option<String>,
        section: Option<String>,
    },
    RenameSection {
        old: String,
        new: String,
        /// The file as a `--file` argument, when the name is in two files.
        file: Option<String>,
    },
    AddSection {
        name: String,
    },
}

impl Op {
    /// The existing host the operation rewrites, for the conflict check.
    fn target(&self) -> Option<&str> {
        match self {
            Op::Edit { spec, .. } => Some(&spec.name),
            Op::Delete(n) => Some(n),
            Op::Move { name, .. } => Some(name),
            _ => None,
        }
    }

    /// The host to keep selected after the write.
    fn focus_name(&self) -> Option<String> {
        match self {
            Op::Add(s) => Some(s.name.clone()),
            Op::Edit { spec, .. } => Some(spec.name.clone()),
            Op::Clone(s) => Some(s.new_name.clone()),
            Op::Move { name, new_name, .. } => Some(new_name.clone().unwrap_or(name.clone())),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
enum Prompt {
    Delete {
        name: String,
        file: usize,
    },
    Overwrite {
        op: Op,
        host: String,
    },
    /// One file has unsaved edits.
    Quit,
    /// Several files have unsaved edits; their indices.
    QuitAll(Vec<usize>),
    EditorConflict {
        file: usize,
    },
    Discard,
}

#[derive(Debug, Clone)]
enum Mode {
    Normal,
    Help,
    Form(Form),
    Prompt(Prompt),
    PickFilterColumn,
    Filter {
        column: Option<Column>,
        previous: String,
    },
}

type Stamp = Option<(SystemTime, u64)>;

fn stamp(path: &Path) -> Stamp {
    let m = std::fs::metadata(path).ok()?;
    Some((m.modified().ok()?, m.len()))
}

fn tilde(path: &Path) -> String {
    if let Some(home) = std::env::var_os("HOME").filter(|h| !h.is_empty()) {
        if let Ok(rest) = path.strip_prefix(&home) {
            return format!("~/{}", rest.display());
        }
    }
    path.display().to_string()
}

/// A core error as the TUI shows it: CLI remedies become TUI keys (F1).
fn tui_error(e: &Error) -> String {
    match e {
        Error::HostExists(n) => format!("{n} already exists. Press e on {n} to edit it."),
        Error::HostExistsIn { name, file } => {
            format!("{name} already exists in {file}. Press e on {name} to edit it.")
        }
        Error::AmbiguousSection { name, files } => format!(
            "section {name} exists in {}. Edit the file you mean in the editor (F).",
            join_and(files)
        ),
        Error::EditTargetMissing(n) => format!("{n} does not exist. Press a to add it."),
        other => other.to_string(),
    }
}

fn apply(cfg: &mut Config, op: &Op, env: &Env) -> Result<String, Error> {
    match op {
        Op::Add(spec) => {
            let had_sections = cfg.has_sections();
            let placed = cfg.add(spec, env)?;
            let n = &placed.name;
            let mut msg = match (&spec.section, &placed.section) {
                (Some(_), Some(s)) => {
                    format!("{n} added to section {s}. Connect with: ssh {n}")
                }
                _ => format!("{n} added. Connect with: ssh {n}"),
            };
            if !had_sections && cfg.has_sections() {
                if let Some(last) = cfg.sections().last() {
                    msg.push_str(&format!(" Hosts without a section moved to {}.", last.name));
                }
            }
            Ok(msg)
        }
        Op::Edit {
            spec,
            remove_identity,
        } => {
            let placed = cfg.edit(spec, env)?;
            if *remove_identity {
                cfg.unset(
                    &HostSelector::Name(placed.name.clone()),
                    &["IdentityFile".to_string()],
                )?;
            }
            Ok(format!("{} updated.", placed.name))
        }
        Op::Delete(name) => {
            let gone = cfg.delete(std::slice::from_ref(name))?;
            Ok(format!("{} deleted.", gone.join(", ")))
        }
        Op::Clone(spec) => {
            let placed = cfg.clone_host(spec)?;
            let n = &placed.name;
            Ok(format!("{n} added. Connect with: ssh {n}"))
        }
        Op::Move {
            name,
            new_name,
            section,
        } => {
            let m = cfg.move_host(name, new_name.as_deref(), section.as_deref())?;
            let (o, n) = (&m.old_name, &m.new_name);
            Ok(match (new_name.is_some() && o != n, &m.section) {
                (true, Some(s)) => {
                    format!("{o} renamed to {n} and moved to section {s}. Connect with: ssh {n}")
                }
                (true, None) => format!("{o} renamed to {n}. Connect with: ssh {n}"),
                (false, Some(s)) => format!("{n} moved to section {s}."),
                (false, None) => format!("{n} updated."),
            })
        }
        Op::RenameSection { old, new, .. } => Ok(match cfg.rename_section(old, new)? {
            SectionRename::Renamed { from, to } => format!("section {from} renamed to {to}."),
            SectionRename::Merged { from, into } => format!("section {from} merged into {into}."),
        }),
        Op::AddSection { name } => {
            let added = cfg.add_section(name, None)?;
            Ok(match added.catch_all {
                Some((catch_all, n)) => format!(
                    "section {name} added; {catch_all} created with {n} host{}.",
                    if n == 1 { "" } else { "s" }
                ),
                None => format!("section {name} added."),
            })
        }
    }
}

/// The status text of a workspace change: its messages, then its
/// warnings.
fn change_text<T>(c: Change<T>) -> String {
    let mut parts = c.messages;
    parts.extend(c.warnings.into_iter().map(|w| format!("Warning: {w}")));
    parts.join(" ")
}

/// `op` through the workspace's routed operations (docs/cli.md, Included
/// files): the messages name the file written.
fn apply_ws(ws: &mut Workspace, op: &Op, env: &Env) -> Result<String, Error> {
    Ok(match op {
        Op::Add(spec) => change_text(ws.add(spec, None, env)?),
        Op::Edit {
            spec,
            remove_identity,
        } => {
            let c = ws.edit(spec, None, env)?;
            if *remove_identity {
                ws.unset(
                    &HostSelector::Name(c.value.name.clone()),
                    &["IdentityFile".to_string()],
                    None,
                )?;
            }
            change_text(c)
        }
        Op::Delete(name) => change_text(ws.delete(std::slice::from_ref(name), None)?),
        Op::Clone(spec) => change_text(ws.clone_host(spec, None)?),
        Op::Move {
            name,
            new_name,
            section,
        } => change_text(ws.move_host(name, new_name.as_deref(), section.as_deref(), None)?),
        Op::RenameSection { old, new, file } => {
            change_text(ws.rename_section(old, new, file.as_deref())?)
        }
        Op::AddSection { name } => change_text(ws.add_section(name, None, None)?),
    })
}

fn host_text(ws: &Workspace, name: &str) -> Option<String> {
    ws.find_host(name).map(|l| ws.host(l).text())
}

/// The home directory for `~/` in Include patterns and displayed paths.
fn home_dir(env: &Env) -> Option<PathBuf> {
    env.home.clone().or_else(|| {
        std::env::var_os("HOME")
            .filter(|h| !h.is_empty())
            .map(PathBuf::from)
    })
}

/// One row of the section list.
#[derive(Debug, Clone)]
struct SectionRow {
    name: String,
    hosts: usize,
    /// Index of the file holding it.
    file: usize,
    /// Another file holds a section of the same name.
    shared: bool,
}

fn section_rows(ws: &Workspace) -> Vec<SectionRow> {
    if !ws.is_multi() {
        return ws
            .root()
            .config
            .sections()
            .into_iter()
            .map(|s| SectionRow {
                name: s.name,
                hosts: s.hosts,
                file: 0,
                shared: false,
            })
            .collect();
    }
    let all = ws.sections();
    all.iter()
        .map(|s| SectionRow {
            name: s.name.clone(),
            hosts: s.hosts,
            file: s.index,
            shared: all
                .iter()
                .any(|o| o.index != s.index && o.name.eq_ignore_ascii_case(&s.name)),
        })
        .collect()
}

/// `text` cut to `width` characters, eliding its start.
fn elide_start(text: &str, width: usize, ellipsis: &str) -> String {
    let n = text.chars().count();
    if n <= width {
        return text.to_string();
    }
    let keep = width.saturating_sub(ellipsis.chars().count());
    let tail: String = text.chars().skip(n - keep).collect();
    format!("{ellipsis}{tail}")
}

fn centered(area: Rect, width: u16, height: u16) -> Rect {
    let w = width.min(area.width);
    let h = height.min(area.height);
    Rect {
        x: area.x + (area.width - w) / 2,
        y: area.y + (area.height - h) / 2,
        width: w,
        height: h,
    }
}

/// The key overlay's rows; on a workspace of several files the digit keys
/// reach 8 and the file list keys join.
fn help_rows(multi: bool) -> Vec<(&'static str, &'static str, &'static str)> {
    if !multi {
        return HELP.to_vec();
    }
    let mut rows = Vec::new();
    for &(k, ctx, act) in HELP {
        match k {
            "1-7" => rows.push(("1-8", ctx, act)),
            "f then 1-7" => rows.push(("f then 1-8", ctx, act)),
            _ => rows.push((k, ctx, act)),
        }
        if k == "Tab / Shift-Tab" {
            rows.push(("F", "table, sections", "file list"));
            rows.push(("Enter", "files", "show file in editor"));
        }
    }
    rows
}

const HELP: &[(&str, &str, &str)] = &[
    ("q", "table, sections", "quit"),
    ("Ctrl-C", "any", "quit"),
    ("?", "table, sections", "toggle this help"),
    ("Tab / Shift-Tab", "panes", "cycle focus"),
    ("Up Down / j k", "table, sections", "move"),
    ("g G / Home End", "table, sections", "first / last"),
    ("1-7", "table", "sort by column; again flips"),
    ("0", "table", "file order"),
    ("/", "table", "global filter"),
    ("f then 1-7", "table", "column filter"),
    ("x", "table", "clear filters"),
    ("Enter", "sections", "filter to section"),
    ("a", "table", "add host"),
    ("e / Enter", "table", "edit host"),
    ("d", "table", "delete host"),
    ("c", "table", "clone host"),
    ("m", "table", "move or rename host"),
    ("R", "sections", "rename section"),
    ("n", "table, sections", "new empty section"),
    ("o", "table", "open editor at host"),
    ("Ctrl-S", "editor", "save"),
    ("Ctrl-R", "editor", "discard edits"),
    ("Esc", "editor, form, prompt", "back / cancel"),
];

/// The whole TUI: model, view state and the mode it is in.
pub struct App {
    ws: Workspace,
    /// Modification stamp per file of `ws.files`, as loaded or last written.
    stamps: Vec<Stamp>,
    opts: Options,
    rows: Vec<Row>,
    visible: Vec<usize>,
    sort: Option<Sort>,
    filters: Filters,
    table: TableState,
    sections: Vec<SectionRow>,
    section_sel: usize,
    /// Selection in the file list, an index into `ws.load_order`.
    files_sel: usize,
    focus: Focus,
    mode: Mode,
    /// One buffer per file of `ws.files`.
    editors: Vec<Editor>,
    /// The file the editor shows.
    cur: usize,
    msg: Option<Msg>,
    quit: bool,
}

impl App {
    /// Loads `config_path` with options from the environment.
    pub fn new(config_path: impl AsRef<Path>) -> rustorm_core::Result<App> {
        App::with_options(config_path, Options::detect())
    }

    /// Loads `config_path`. A missing file loads empty and creates nothing.
    pub fn with_options(config_path: impl AsRef<Path>, opts: Options) -> rustorm_core::Result<App> {
        let path: PathBuf = config_path.as_ref().to_path_buf();
        let ws = Workspace::load_with_home(&path, home_dir(&opts.env).as_deref())?;
        let editors = ws.files.iter().map(|f| Editor::new(&f.original)).collect();
        let stamps = ws.files.iter().map(|f| stamp(&f.path)).collect();
        let mut app = App {
            ws,
            stamps,
            opts,
            rows: Vec::new(),
            visible: Vec::new(),
            sort: None,
            filters: Filters::default(),
            table: TableState::default(),
            sections: Vec::new(),
            section_sel: 0,
            files_sel: 0,
            focus: Focus::Table,
            mode: Mode::Normal,
            editors,
            cur: 0,
            msg: None,
            quit: false,
        };
        app.refresh(None);
        Ok(app)
    }

    // ----- queries for the binary and tests -----

    /// True once the user has quit.
    pub fn should_quit(&self) -> bool {
        self.quit
    }

    /// The focused pane.
    pub fn focus(&self) -> Focus {
        self.focus
    }

    /// The rows the table shows, in display order.
    pub fn visible_rows(&self) -> Vec<&Row> {
        self.visible.iter().map(|i| &self.rows[*i]).collect()
    }

    /// The selected host's name.
    pub fn selected(&self) -> Option<&str> {
        self.selected_row().map(|r| r.name.as_str())
    }

    /// The status message, prefixed as the status line shows it.
    pub fn message(&self) -> Option<String> {
        self.msg.as_ref().map(|m| match m {
            Msg::Success(s) => format!("{} {s}", self.opts.theme.ok()),
            Msg::Error(s) => format!("Error: {s}"),
            Msg::Info(s) => s.clone(),
        })
    }

    /// The buffer of the file on screen.
    pub fn editor_text(&self) -> String {
        self.editors[self.cur].text()
    }

    /// The editor cursor `(row, col)`, zero-based.
    pub fn editor_cursor(&self) -> (usize, usize) {
        self.editors[self.cur].cursor()
    }

    /// True when the buffer on screen differs from its file as loaded.
    pub fn is_editor_modified(&self) -> bool {
        self.is_dirty(self.cur)
    }

    /// True when the workspace has more than one file (docs/tui.md, Files).
    pub fn is_multi(&self) -> bool {
        self.ws.is_multi()
    }

    /// The path of the file the editor shows.
    pub fn shown_file(&self) -> &Path {
        &self.ws.files[self.cur].path
    }

    /// The paths of every file, root first, in load order.
    pub fn files(&self) -> Vec<&Path> {
        self.ws.files.iter().map(|f| f.path.as_path()).collect()
    }

    /// The paths of the files whose buffers have unsaved edits.
    pub fn dirty_files(&self) -> Vec<&Path> {
        self.dirty()
            .into_iter()
            .map(|i| self.ws.files[i].path.as_path())
            .collect()
    }

    /// The active filters.
    pub fn filters(&self) -> &Filters {
        &self.filters
    }

    // ----- state helpers -----

    fn selected_row(&self) -> Option<&Row> {
        let i = self.table.selected()?;
        self.visible.get(i).map(|r| &self.rows[*r])
    }

    fn recompute(&mut self) {
        let mut vis: Vec<usize> = (0..self.rows.len())
            .filter(|i| self.filters.matches(&self.rows[*i]))
            .collect();
        if let Some(s) = self.sort {
            vis.sort_by(|a, b| hosts::compare(&self.rows[*a], &self.rows[*b], s));
        }
        self.visible = vis;
    }

    /// After a sort or filter change the selection goes to the first row (F7).
    fn view_changed(&mut self) {
        self.recompute();
        self.table.select(if self.visible.is_empty() {
            None
        } else {
            Some(0)
        });
    }

    fn select_name(&mut self, name: Option<&str>) {
        let prev = self.table.selected().unwrap_or(0);
        let pos = name.and_then(|n| self.visible.iter().position(|i| self.rows[*i].name == n));
        self.table.select(match (pos, self.visible.len()) {
            (_, 0) => None,
            (Some(p), _) => Some(p),
            (None, len) => Some(prev.min(len - 1)),
        });
    }

    fn is_dirty(&self, i: usize) -> bool {
        self.editors[i].text() != self.ws.files[i].original
    }

    fn dirty(&self) -> Vec<usize> {
        (0..self.editors.len())
            .filter(|&i| self.is_dirty(i))
            .collect()
    }

    /// A file's path as the screen shows it.
    fn show_path(&self, i: usize) -> String {
        if self.ws.is_multi() {
            self.ws.display(i)
        } else {
            tilde(&self.ws.files[i].path)
        }
    }

    /// Shows file `i` in the editor and selects it in the file list.
    fn show_file(&mut self, i: usize) {
        self.cur = i;
        if let Some(p) = self
            .ws
            .load_order
            .iter()
            .position(|e| e.state == FileState::Loaded(i))
        {
            self.files_sel = p;
        }
    }

    fn refresh(&mut self, select: Option<&str>) {
        let keep = select
            .map(str::to_string)
            .or_else(|| self.selected().map(str::to_string));
        self.rows = hosts::workspace_rows(&self.ws, &self.opts.env);
        self.sections = section_rows(&self.ws);
        self.section_sel = self.section_sel.min(self.sections.len());
        self.files_sel = self
            .files_sel
            .min(self.ws.load_order.len().saturating_sub(1));
        if !self.ws.is_multi() && self.focus == Focus::Files {
            self.focus = Focus::Table;
        }
        if self.sections.is_empty() && self.focus == Focus::Sections {
            self.focus = Focus::Table;
        }
        self.recompute();
        self.select_name(keep.as_deref());
    }

    /// Re-reads file `i`, dropping its buffer's edits.
    fn reload(&mut self, i: usize) -> Result<(), String> {
        self.ws.reload_file(i).map_err(|e| e.to_string())?;
        self.editors[i].set_text(&self.ws.files[i].original);
        self.stamps[i] = stamp(&self.ws.files[i].path);
        self.refresh(None);
        Ok(())
    }

    /// Replaces the whole workspace with `fresh`, every buffer with its
    /// file's text.
    fn replace_workspace(&mut self, fresh: Workspace) {
        if fresh.files.len() == self.editors.len() {
            for (e, f) in self.editors.iter_mut().zip(&fresh.files) {
                e.set_text(&f.original);
            }
        } else {
            self.editors = fresh
                .files
                .iter()
                .map(|f| Editor::new(&f.original))
                .collect();
            self.cur = self.cur.min(fresh.files.len() - 1);
        }
        self.stamps = fresh.files.iter().map(|f| stamp(&f.path)).collect();
        self.ws = fresh;
        self.refresh(None);
    }

    fn error(&mut self, s: impl Into<String>) {
        self.msg = Some(Msg::Error(s.into()));
    }

    fn options(&self) -> WriteOptions {
        WriteOptions {
            no_backup: self.opts.no_backup,
        }
    }

    fn disk_changed(&self, i: usize) -> bool {
        stamp(&self.ws.files[i].path) != self.stamps[i]
    }

    fn any_disk_changed(&self) -> bool {
        (0..self.ws.files.len()).any(|i| self.disk_changed(i))
    }

    // ----- writes -----

    /// Runs `op` against the file: reload-then-apply when the file changed
    /// on disk, a prompt when the target host itself changed (F9).
    /// `Err` carries the message for the form.
    fn run_op(&mut self, op: Op, force: bool) -> Result<bool, String> {
        if !force && self.any_disk_changed() {
            let fresh = Workspace::load_with_home(
                self.ws.files[0].path.clone(),
                self.ws.home.clone().as_deref(),
            )
            .map_err(|e| tui_error(&e))?;
            let conflict = op.target().and_then(|t| {
                (host_text(&self.ws, t) != host_text(&fresh, t)).then(|| t.to_string())
            });
            self.replace_workspace(fresh);
            if let Some(host) = conflict {
                self.mode = Mode::Prompt(Prompt::Overwrite { op, host });
                return Ok(false);
            }
        }
        let msg = if self.ws.is_multi() {
            let mut next = self.ws.clone();
            let msg = apply_ws(&mut next, &op, &self.opts.env).map_err(|e| tui_error(&e))?;
            next.save(self.options()).map_err(|e| tui_error(&e))?;
            self.ws = next;
            msg
        } else {
            let mut next = self.ws.files[0].clone();
            let msg = apply(&mut next.config, &op, &self.opts.env).map_err(|e| tui_error(&e))?;
            next.save(self.options()).map_err(|e| tui_error(&e))?;
            self.ws.files[0] = next;
            msg
        };
        for i in 0..self.ws.files.len() {
            self.stamps[i] = stamp(&self.ws.files[i].path);
            self.editors[i].set_text(&self.ws.files[i].original);
        }
        self.refresh(op.focus_name().as_deref());
        self.msg = Some(Msg::Success(msg));
        Ok(true)
    }

    /// Saves the buffer of file `i` to that file alone. Returns true when
    /// written; a refused save shows that file in the editor.
    fn save_editor(&mut self, i: usize, force: bool) -> bool {
        let text = self.editors[i].text();
        let parsed = match Config::parse(&text) {
            Ok(c) => c,
            Err(e) => {
                self.show_file(i);
                self.error(format!("Not saved: {e}"));
                return false;
            }
        };
        let bad = parsed
            .check(&self.opts.env)
            .problems
            .into_iter()
            .find(|p| p.kind == ProblemKind::UnparsableLine);
        if let Some(p) = bad {
            self.show_file(i);
            if let Some(line) = p.line {
                self.editors[i].jump(line.saturating_sub(1));
            }
            self.focus = Focus::Editor;
            self.error(format!("Not saved: {p}"));
            return false;
        }
        if !force && self.disk_changed(i) {
            self.show_file(i);
            self.mode = Mode::Prompt(Prompt::EditorConflict { file: i });
            return false;
        }
        let mut cfg = parsed;
        cfg.sort_sections();
        let out = cfg.render();
        let saved = if self.ws.is_multi() {
            self.ws.save_text(i, &out, self.options())
        } else {
            let mut next = self.ws.files[0].clone();
            next.save_text(&out, self.options()).map(|()| {
                self.ws.files[0] = next;
            })
        };
        if let Err(e) = saved {
            self.show_file(i);
            self.error(format!("Not saved: {e}"));
            return false;
        }
        self.stamps[i] = stamp(&self.ws.files[i].path);
        self.editors[i].set_text(&out);
        self.refresh(None);
        self.msg = Some(Msg::Success(format!("Saved {}.", self.show_path(i))));
        true
    }

    fn request_quit(&mut self) {
        let dirty = self.dirty();
        match dirty.len() {
            0 => self.quit = true,
            1 => self.mode = Mode::Prompt(Prompt::Quit),
            _ => self.mode = Mode::Prompt(Prompt::QuitAll(dirty)),
        }
    }

    // ----- forms -----

    fn open_form(&mut self, kind: FormKind) {
        if !self.dirty().is_empty() {
            self.error("Save or discard the editor's changes first.");
            return;
        }
        let (title, fields): (String, Vec<(&'static str, String)>) = match &kind {
            FormKind::Add => (
                "Add host".into(),
                vec![
                    ("Name", String::new()),
                    ("Connection URI ([user@]host[:port])", String::new()),
                    ("Identity file (optional)", String::new()),
                    ("Section (optional)", String::new()),
                ],
            ),
            FormKind::Edit {
                name,
                identity,
                section,
            } => {
                let row = self.rows.iter().find(|r| &r.name == name);
                let uri = row.map_or(String::new(), |r| {
                    let host = r.hostname.clone().unwrap_or_default();
                    let host = if host.contains(':') {
                        format!("[{host}]")
                    } else {
                        host
                    };
                    let mut s = String::new();
                    if let Some(u) = &r.user {
                        s.push_str(&format!("{u}@"));
                    }
                    s.push_str(&host);
                    if let Some(p) = &r.port {
                        s.push_str(&format!(":{p}"));
                    }
                    s
                });
                (
                    format!("Edit host {name}"),
                    vec![
                        ("Connection URI", uri),
                        ("Identity file", identity.clone()),
                        ("Section", section.clone().unwrap_or_default()),
                    ],
                )
            }
            FormKind::Clone { source } => {
                let section = self
                    .rows
                    .iter()
                    .find(|r| &r.name == source)
                    .and_then(|r| r.section.clone())
                    .unwrap_or_default();
                (
                    format!("Clone host {source}"),
                    vec![("New name", String::new()), ("Section", section)],
                )
            }
            FormKind::Move { name, section } => (
                format!("Move host {name}"),
                vec![
                    ("Name", name.clone()),
                    ("Section", section.clone().unwrap_or_default()),
                ],
            ),
            FormKind::RenameSection { name, .. } => (
                format!("Rename section {name}"),
                vec![("New name", name.clone())],
            ),
            FormKind::AddSection => ("New section".into(), vec![("Name", String::new())]),
        };
        self.mode = Mode::Form(Form {
            kind,
            title,
            fields,
            focus: 0,
            error: None,
        });
    }

    fn form_op(&self, form: &Form) -> Result<Op, String> {
        let f = |i: usize| form.fields[i].1.trim().to_string();
        let opt = |s: String| (!s.is_empty()).then_some(s);
        match &form.kind {
            FormKind::Add => {
                if f(0).is_empty() || f(1).is_empty() {
                    return Err("Name and Connection URI are required.".into());
                }
                Ok(Op::Add(AddSpec {
                    name: f(0),
                    uri: f(1),
                    identity: opt(f(2)),
                    options: Vec::new(),
                    section: opt(f(3)),
                }))
            }
            FormKind::Edit {
                name,
                identity,
                section,
            } => {
                if f(0).is_empty() {
                    return Err("Connection URI is required.".into());
                }
                let new_id = f(1);
                let (identity, remove_identity) = if new_id == *identity {
                    (None, false)
                } else if new_id.is_empty() {
                    (None, true)
                } else {
                    (Some(new_id), false)
                };
                let new_section = opt(f(2)).filter(|s| {
                    section
                        .as_deref()
                        .is_none_or(|old| !old.eq_ignore_ascii_case(s))
                });
                Ok(Op::Edit {
                    spec: EditSpec {
                        name: name.clone(),
                        uri: f(0),
                        identity,
                        options: Vec::new(),
                        section: new_section,
                    },
                    remove_identity,
                })
            }
            FormKind::Clone { source } => {
                if f(0).is_empty() {
                    return Err("New name is required.".into());
                }
                Ok(Op::Clone(CloneSpec {
                    source: source.clone(),
                    new_name: f(0),
                    keep_hostname: false,
                    overrides: Vec::new(),
                    section: opt(f(1)),
                }))
            }
            FormKind::Move { name, section } => {
                let new_name = opt(f(0)).filter(|n| n != name);
                let new_section = opt(f(1)).filter(|s| {
                    section
                        .as_deref()
                        .is_none_or(|old| !old.eq_ignore_ascii_case(s))
                });
                if new_name.is_none() && new_section.is_none() {
                    return Err("Change the name, the section, or both.".into());
                }
                Ok(Op::Move {
                    name: name.clone(),
                    new_name,
                    section: new_section,
                })
            }
            FormKind::RenameSection { name, file } => {
                if f(0).is_empty() || f(0) == *name {
                    return Err("Give the section a new name.".into());
                }
                Ok(Op::RenameSection {
                    old: name.clone(),
                    new: f(0),
                    file: file.map(|i| self.ws.abs(i).display().to_string()),
                })
            }
            FormKind::AddSection => {
                if f(0).is_empty() {
                    return Err("Section name is required.".into());
                }
                Ok(Op::AddSection { name: f(0) })
            }
        }
    }

    fn handle_form(&mut self, mut form: Form, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                self.msg = Some(Msg::Info("Cancelled.".into()));
                return;
            }
            KeyCode::Tab | KeyCode::Down => form.focus = (form.focus + 1) % form.fields.len(),
            KeyCode::BackTab | KeyCode::Up => {
                form.focus = (form.focus + form.fields.len() - 1) % form.fields.len()
            }
            KeyCode::Backspace => {
                form.fields[form.focus].1.pop();
            }
            KeyCode::Enter => {
                let op = match self.form_op(&form) {
                    Ok(op) => op,
                    Err(e) => {
                        form.error = Some(e);
                        self.mode = Mode::Form(form);
                        return;
                    }
                };
                match self.run_op(op, false) {
                    Ok(_) => {}
                    Err(e) => {
                        form.error = Some(e);
                        if matches!(self.mode, Mode::Normal) {
                            self.mode = Mode::Form(form);
                        }
                    }
                }
                return;
            }
            KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                form.fields[form.focus].1.push(c)
            }
            _ => {}
        }
        self.mode = Mode::Form(form);
    }

    // ----- key handling -----

    /// Handles one key event.
    pub fn handle(&mut self, key: KeyEvent) {
        if key.kind != KeyEventKind::Press {
            return;
        }
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let mode = std::mem::replace(&mut self.mode, Mode::Normal);
        if ctrl && key.code == KeyCode::Char('c') {
            if matches!(mode, Mode::Prompt(Prompt::Quit | Prompt::QuitAll(_))) {
                self.mode = mode;
            } else {
                self.request_quit();
            }
            return;
        }
        match mode {
            Mode::Normal => {
                self.msg = None;
                match self.focus {
                    Focus::Editor => self.handle_editor(key),
                    Focus::Files => self.handle_files(key),
                    Focus::Table => self.handle_table(key),
                    Focus::Sections => self.handle_sections(key),
                }
            }
            Mode::Help => {
                if !matches!(
                    key.code,
                    KeyCode::Esc | KeyCode::Char('?') | KeyCode::Char('q')
                ) {
                    self.mode = Mode::Help;
                }
            }
            Mode::Form(form) => self.handle_form(form, key),
            Mode::Prompt(p) => self.handle_prompt(p, key),
            Mode::PickFilterColumn => {
                if let KeyCode::Char(c) = key.code {
                    match Column::from_digit_in(c, self.ws.is_multi()) {
                        Some(col) => {
                            self.mode = Mode::Filter {
                                column: Some(col),
                                previous: self.filters.get(col).to_string(),
                            }
                        }
                        None if self.ws.is_multi() => self.error("Press 1-8 to pick a column."),
                        None => self.error("Press 1-7 to pick a column."),
                    }
                }
            }
            Mode::Filter { column, previous } => {
                let text = match column {
                    Some(c) => &mut self.filters.columns[c.index()],
                    None => &mut self.filters.global,
                };
                match key.code {
                    KeyCode::Enter => {}
                    KeyCode::Esc => {
                        *text = previous;
                        self.view_changed();
                    }
                    KeyCode::Backspace => {
                        text.pop();
                        self.view_changed();
                        self.mode = Mode::Filter { column, previous };
                    }
                    KeyCode::Char(ch) if !ctrl => {
                        text.push(ch);
                        self.view_changed();
                        self.mode = Mode::Filter { column, previous };
                    }
                    _ => self.mode = Mode::Filter { column, previous },
                }
            }
        }
    }

    fn handle_prompt(&mut self, prompt: Prompt, key: KeyEvent) {
        let ch = match key.code {
            KeyCode::Char(c) => Some(c.to_ascii_lowercase()),
            _ => None,
        };
        match prompt {
            Prompt::Delete { name, .. } => {
                if ch == Some('y') {
                    if let Err(e) = self.run_op(Op::Delete(name), false) {
                        self.error(e);
                    }
                } else {
                    self.msg = Some(Msg::Info("Delete cancelled.".into()));
                }
            }
            Prompt::Overwrite { op, host } => {
                if ch == Some('y') {
                    if let Err(e) = self.run_op(op, true) {
                        self.error(e);
                    }
                } else {
                    self.msg = Some(Msg::Info(format!(
                        "Not saved. Kept the version of {host} on disk."
                    )));
                }
            }
            Prompt::Quit => match ch {
                Some('s') => {
                    let i = self.dirty().first().copied().unwrap_or(self.cur);
                    if self.save_editor(i, false) {
                        self.quit = true;
                    }
                }
                Some('d') => self.quit = true,
                _ => {}
            },
            Prompt::QuitAll(files) => match ch {
                Some('s') => {
                    for i in files {
                        if self.is_dirty(i) && !self.save_editor(i, false) {
                            return;
                        }
                    }
                    self.quit = true;
                }
                Some('d') => self.quit = true,
                _ => {}
            },
            Prompt::EditorConflict { file } => match ch {
                Some('r') => {
                    if let Err(e) = self.reload(file) {
                        self.error(e);
                    } else {
                        self.msg = Some(Msg::Info("Reloaded the file from disk.".into()));
                    }
                }
                Some('o') => {
                    self.save_editor(file, true);
                }
                _ => self.msg = Some(Msg::Info("Not saved.".into())),
            },
            Prompt::Discard => {
                if ch == Some('y') {
                    match self.reload(self.cur) {
                        Ok(()) => {
                            self.msg = Some(Msg::Info("Discarded the editor's changes.".into()))
                        }
                        Err(e) => self.error(e),
                    }
                }
            }
        }
    }

    fn cycle_focus(&mut self, back: bool) {
        let mut order = vec![Focus::Table, Focus::Editor];
        if !self.sections.is_empty() {
            order.insert(0, Focus::Sections);
        }
        if self.ws.is_multi() {
            order.insert(0, Focus::Files);
        }
        let i = order.iter().position(|f| *f == self.focus).unwrap_or(0);
        let n = order.len();
        self.focus = order[if back { (i + n - 1) % n } else { (i + 1) % n }];
    }

    fn handle_editor(&mut self, key: KeyEvent) {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        match key.code {
            KeyCode::Char('s') if ctrl => {
                self.save_editor(self.cur, false);
            }
            KeyCode::Char('r') if ctrl => {
                if self.is_editor_modified() {
                    self.mode = Mode::Prompt(Prompt::Discard);
                }
            }
            KeyCode::Esc => self.focus = Focus::Table,
            KeyCode::Tab => self.cycle_focus(false),
            KeyCode::BackTab => self.cycle_focus(true),
            _ => {
                self.editors[self.cur].area.input(key);
            }
        }
    }

    fn move_sel(&mut self, delta: isize, len: usize, cur: usize) -> usize {
        if len == 0 {
            return 0;
        }
        (cur as isize + delta).clamp(0, len as isize - 1) as usize
    }

    fn handle_table(&mut self, key: KeyEvent) {
        let len = self.visible.len();
        let cur = self.table.selected().unwrap_or(0);
        let need_host = |app: &mut App| -> Option<String> {
            let n = app.selected().map(str::to_string);
            if n.is_none() {
                app.error("No host selected.");
            }
            n
        };
        match key.code {
            KeyCode::Char('q') => self.request_quit(),
            KeyCode::Char('?') => self.mode = Mode::Help,
            KeyCode::Tab => self.cycle_focus(false),
            KeyCode::BackTab => self.cycle_focus(true),
            KeyCode::Char('n') => self.open_form(FormKind::AddSection),
            KeyCode::Char('F') if self.ws.is_multi() => self.focus = Focus::Files,
            KeyCode::Down | KeyCode::Char('j') => {
                let s = self.move_sel(1, len, cur);
                self.table.select((len > 0).then_some(s));
            }
            KeyCode::Up | KeyCode::Char('k') => {
                let s = self.move_sel(-1, len, cur);
                self.table.select((len > 0).then_some(s));
            }
            KeyCode::Home | KeyCode::Char('g') => self.table.select((len > 0).then_some(0)),
            KeyCode::End | KeyCode::Char('G') => {
                self.table.select(len.checked_sub(1));
            }
            KeyCode::Char('0') => {
                self.sort = None;
                self.view_changed();
            }
            KeyCode::Char(c) if Column::from_digit_in(c, self.ws.is_multi()).is_some() => {
                let col = Column::from_digit_in(c, self.ws.is_multi()).expect("checked");
                self.sort = Some(match self.sort {
                    Some(s) if s.column == col => Sort {
                        column: col,
                        ascending: !s.ascending,
                    },
                    _ => Sort {
                        column: col,
                        ascending: true,
                    },
                });
                self.view_changed();
            }
            KeyCode::Char('/') => {
                self.mode = Mode::Filter {
                    column: None,
                    previous: self.filters.global.clone(),
                }
            }
            KeyCode::Char('f') => self.mode = Mode::PickFilterColumn,
            KeyCode::Char('x') => {
                self.filters = Filters::default();
                self.view_changed();
            }
            KeyCode::Char('a') => self.open_form(FormKind::Add),
            KeyCode::Char('e') | KeyCode::Enter => {
                if let Some(name) = need_host(self) {
                    let fi = self.selected_row().map_or(0, |r| r.file_index);
                    let config = &self.ws.files[fi].config;
                    let (identity, section) = match config.find_primary(&name) {
                        Some(l) => (
                            config.host(l).get("IdentityFile").unwrap_or_default(),
                            config.section_name(l).map(str::to_string),
                        ),
                        None => (String::new(), None),
                    };
                    self.open_form(FormKind::Edit {
                        name,
                        identity,
                        section,
                    });
                }
            }
            KeyCode::Char('d') => {
                if let Some(name) = need_host(self) {
                    if !self.dirty().is_empty() {
                        self.error("Save or discard the editor's changes first.");
                    } else {
                        let file = self.selected_row().map_or(0, |r| r.file_index);
                        self.mode = Mode::Prompt(Prompt::Delete { name, file });
                    }
                }
            }
            KeyCode::Char('c') => {
                if let Some(source) = need_host(self) {
                    self.open_form(FormKind::Clone { source });
                }
            }
            KeyCode::Char('m') => {
                if let Some(name) = need_host(self) {
                    let section = self.selected_row().and_then(|r| r.section.clone());
                    self.open_form(FormKind::Move { name, section });
                }
            }
            KeyCode::Char('o') => {
                if let Some(name) = need_host(self) {
                    let fi = self.selected_row().map_or(0, |r| r.file_index);
                    self.show_file(fi);
                    let row = self.editors[fi].find_host_line(&name).or_else(|| {
                        let config = &self.ws.files[fi].config;
                        config
                            .find_primary(&name)
                            .map(|l| config.host_line(l).saturating_sub(1))
                    });
                    if let Some(row) = row {
                        self.editors[fi].jump(row);
                    }
                    self.focus = Focus::Editor;
                }
            }
            _ => {}
        }
    }

    fn handle_sections(&mut self, key: KeyEvent) {
        let len = self.sections.len() + 1;
        match key.code {
            KeyCode::Char('q') => self.request_quit(),
            KeyCode::Char('?') => self.mode = Mode::Help,
            KeyCode::Tab => self.cycle_focus(false),
            KeyCode::BackTab => self.cycle_focus(true),
            KeyCode::Down | KeyCode::Char('j') => {
                self.section_sel = self.move_sel(1, len, self.section_sel)
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.section_sel = self.move_sel(-1, len, self.section_sel)
            }
            KeyCode::Home | KeyCode::Char('g') => self.section_sel = 0,
            KeyCode::End | KeyCode::Char('G') => self.section_sel = len - 1,
            KeyCode::Char('F') if self.ws.is_multi() => self.focus = Focus::Files,
            KeyCode::Enter => {
                let picked = self.section_sel.checked_sub(1).map(|i| &self.sections[i]);
                let section = picked.map_or(String::new(), |s| s.name.clone());
                if self.ws.is_multi() {
                    self.filters.columns[Column::File.index()] =
                        picked.map_or(String::new(), |s| self.ws.file_name(s.file));
                }
                self.filters.columns[Column::Section.index()] = section;
                self.view_changed();
            }
            KeyCode::Char('R') => {
                if self.section_sel == 0 {
                    self.error("Pick a section to rename.");
                } else {
                    let s = &self.sections[self.section_sel - 1];
                    let name = s.name.clone();
                    let file = s.shared.then_some(s.file);
                    self.open_form(FormKind::RenameSection { name, file });
                }
            }
            KeyCode::Char('n') => self.open_form(FormKind::AddSection),
            _ => {}
        }
    }

    fn handle_files(&mut self, key: KeyEvent) {
        let len = self.ws.load_order.len();
        match key.code {
            KeyCode::Char('q') => self.request_quit(),
            KeyCode::Char('?') => self.mode = Mode::Help,
            KeyCode::Tab => self.cycle_focus(false),
            KeyCode::BackTab => self.cycle_focus(true),
            KeyCode::Esc => self.focus = Focus::Table,
            KeyCode::Down | KeyCode::Char('j') => {
                self.files_sel = self.move_sel(1, len, self.files_sel)
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.files_sel = self.move_sel(-1, len, self.files_sel)
            }
            KeyCode::Home | KeyCode::Char('g') => self.files_sel = 0,
            KeyCode::End | KeyCode::Char('G') => self.files_sel = len - 1,
            KeyCode::Enter => {
                let entry = &self.ws.load_order[self.files_sel];
                match &entry.state {
                    FileState::Loaded(i) => {
                        self.show_file(*i);
                        self.focus = Focus::Editor;
                    }
                    FileState::Unreadable(reason) => {
                        let path = self.ws.display_path(&entry.path);
                        self.error(format!("Cannot open {path}: cannot read ({reason})."));
                    }
                }
            }
            _ => {}
        }
    }

    // ----- rendering -----

    fn pane_block(&self, title: String, focused: bool) -> Block<'static> {
        let (title, border, style) = if focused {
            (
                format!("[{title}]"),
                BorderType::Double,
                Style::default().add_modifier(Modifier::BOLD),
            )
        } else {
            (format!(" {title} "), BorderType::Plain, Style::default())
        };
        Block::default()
            .borders(Borders::ALL)
            .border_type(border)
            .title(Span::styled(title, style))
    }

    /// Draws the whole screen.
    pub fn render(&mut self, frame: &mut Frame) {
        let area = frame.area();
        let outer = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(4),
                Constraint::Length(1),
                Constraint::Length(1),
            ])
            .split(area);
        let panes = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(outer[0]);
        let multi = self.ws.is_multi();
        let mut widths = Vec::new();
        if multi {
            widths.push(Constraint::Length(self.files_width()));
        }
        if !self.sections.is_empty() {
            widths.push(Constraint::Length(22));
        }
        widths.push(Constraint::Min(20));
        let top = Layout::default()
            .direction(Direction::Horizontal)
            .constraints(widths)
            .split(panes[0]);
        let mut at = 0;
        if multi {
            self.render_files(frame, top[at]);
            at += 1;
        }
        if !self.sections.is_empty() {
            self.render_sections(frame, top[at]);
            at += 1;
        }
        self.render_table(frame, top[at]);
        self.render_editor(frame, panes[1]);
        self.render_status(frame, outer[1]);
        self.render_help_bar(frame, outer[2]);
        match self.mode.clone() {
            Mode::Help => self.render_help(frame, area),
            Mode::Form(form) => self.render_form(frame, area, &form),
            Mode::Prompt(p) => self.render_prompt(frame, area, &p),
            _ => {}
        }
    }

    /// Width of the file list: the longest path plus the marker and the
    /// count, between 24 and 32 columns; longer paths lose their start.
    fn files_width(&self) -> u16 {
        let longest = self
            .ws
            .load_order
            .iter()
            .map(|e| self.ws.display_path(&e.path).chars().count())
            .max()
            .unwrap_or(0);
        (longest + 9).clamp(24, 32) as u16
    }

    fn render_files(&self, frame: &mut Frame, area: Rect) {
        let theme = self.opts.theme;
        let inner = area.width.saturating_sub(2) as usize;
        let items: Vec<ListItem> = self
            .ws
            .load_order
            .iter()
            .map(|e| {
                let path = self.ws.display_path(&e.path);
                match &e.state {
                    FileState::Loaded(i) => {
                        let hosts = self.rows.iter().filter(|r| r.file_index == *i).count();
                        let w = inner.saturating_sub(7);
                        let path = elide_start(&path, w, theme.ellipsis());
                        let marker = if self.is_dirty(*i) {
                            theme.dirty()
                        } else {
                            " "
                        };
                        ListItem::new(Line::from(vec![
                            Span::styled(marker, Style::default().add_modifier(Modifier::BOLD)),
                            Span::raw(format!(" {path:<w$}{hosts:>4}")),
                        ]))
                    }
                    FileState::Unreadable(_) => {
                        let w = inner.saturating_sub(14);
                        let path = elide_start(&path, w, theme.ellipsis());
                        ListItem::new(Line::from(vec![
                            Span::raw(format!("  {path:<w$}")),
                            Span::styled(" cannot read", theme.muted()),
                        ]))
                    }
                }
            })
            .collect();
        let focused = self.focus == Focus::Files;
        let list = List::new(items)
            .block(self.pane_block("Files".into(), focused))
            .highlight_style(if focused {
                Style::default().add_modifier(Modifier::REVERSED)
            } else {
                Style::default().add_modifier(Modifier::BOLD)
            });
        let mut state = ListState::default().with_selected(Some(self.files_sel));
        frame.render_stateful_widget(list, area, &mut state);
    }

    fn render_sections(&self, frame: &mut Frame, area: Rect) {
        let total = self.rows.len();
        let mut items = vec![ListItem::new(format!("{:<14}{:>4}", "All", total))];
        for s in &self.sections {
            let label = if s.shared {
                format!("{} {}", s.name, self.ws.file_name(s.file))
            } else {
                s.name.clone()
            };
            let name: String = label.chars().take(14).collect();
            items.push(ListItem::new(format!("{name:<14}{:>4}", s.hosts)));
        }
        let focused = self.focus == Focus::Sections;
        let list = List::new(items)
            .block(self.pane_block("Sections".into(), focused))
            .highlight_style(if focused {
                Style::default().add_modifier(Modifier::REVERSED)
            } else {
                Style::default().add_modifier(Modifier::BOLD)
            });
        let mut state = ListState::default().with_selected(Some(self.section_sel));
        frame.render_stateful_widget(list, area, &mut state);
    }

    fn render_table(&mut self, frame: &mut Frame, area: Rect) {
        let theme = self.opts.theme;
        let focused = self.focus == Focus::Table;
        let block = self.pane_block(
            format!("Hosts {}/{}", self.visible.len(), self.rows.len()),
            focused,
        );
        let columns = Column::shown(self.ws.is_multi());
        let header = TRow::new(columns.iter().map(|c| {
            let mut t = c.title().to_string();
            if let Some(s) = self.sort.filter(|s| s.column == *c) {
                t.push_str(theme.arrow(s.ascending));
            }
            if !self.filters.get(*c).is_empty() {
                t.push('*');
            }
            Cell::from(t)
        }))
        .style(Style::default().add_modifier(Modifier::BOLD));
        // The file column and pane take room, so a workspace of several
        // files uses narrower columns and keeps the proxy visible.
        let widths: Vec<Constraint> = if self.ws.is_multi() {
            vec![
                Constraint::Length(10),
                Constraint::Length(12),
                Constraint::Length(14),
                Constraint::Length(10),
                Constraint::Length(18),
                Constraint::Length(5),
                Constraint::Fill(1),
                Constraint::Length(10),
            ]
        } else {
            vec![
                Constraint::Length(14),
                Constraint::Length(16),
                Constraint::Length(12),
                Constraint::Length(22),
                Constraint::Length(6),
                Constraint::Fill(1),
                Constraint::Length(14),
            ]
        };
        if self.visible.is_empty() {
            let inner = block.inner(area);
            frame.render_widget(block, area);
            let line = if self.filters.is_empty() {
                format!("no hosts in {}. Press a to add one.", self.show_path(0))
            } else {
                format!("no hosts match filter: {}", self.filters.describe())
            };
            let table = Table::new(
                vec![TRow::new(vec![Cell::from(line)])],
                [Constraint::Fill(1)],
            )
            .header(header);
            frame.render_widget(table, inner);
            return;
        }
        let missing = theme.missing();
        let rows = self.visible.iter().map(|i| {
            let r = &self.rows[*i];
            TRow::new(columns.iter().map(|c| match r.get(*c) {
                Some(v) => Cell::from(v.to_string()),
                None => Cell::from(Span::styled(missing, theme.muted())),
            }))
        });
        let table = Table::new(rows, widths)
            .header(header)
            .block(block)
            .row_highlight_style(Style::default().add_modifier(Modifier::REVERSED));
        frame.render_stateful_widget(table, area, &mut self.table);
    }

    fn render_editor(&mut self, frame: &mut Frame, area: Rect) {
        let (r, c) = self.editors[self.cur].cursor();
        let modified = if self.is_editor_modified() {
            " [modified]"
        } else {
            ""
        };
        let focused = self.focus == Focus::Editor;
        let block = self.pane_block(
            format!(
                "Editor {}{modified}  ln {} col {}",
                self.show_path(self.cur),
                r + 1,
                c + 1
            ),
            focused,
        );
        let theme = self.opts.theme;
        self.editors[self.cur].render(frame, area, block, &theme, focused);
    }

    fn render_status(&self, frame: &mut Frame, area: Rect) {
        let theme = self.opts.theme;
        let mut spans: Vec<Span> = vec![Span::raw(" ")];
        match &self.mode {
            Mode::Filter { column, .. } => {
                let (label, text) = match column {
                    Some(c) => (c.key(), self.filters.get(*c)),
                    None => ("any column", self.filters.global.as_str()),
                };
                spans.push(Span::styled(
                    format!("Filter {label}: {text}{}", theme.caret()),
                    Style::default().add_modifier(Modifier::BOLD),
                ));
                spans.push(Span::styled("  Enter: keep  Esc: undo", theme.muted()));
            }
            Mode::PickFilterColumn => spans.push(Span::styled(
                if self.ws.is_multi() {
                    "Filter which column? 1 file 2 section 3 host 4 user 5 hostname 6 port 7 proxy 8 jump"
                } else {
                    "Filter which column? 1 section 2 host 3 user 4 hostname 5 port 6 proxy 7 jump"
                },
                Style::default().add_modifier(Modifier::BOLD),
            )),
            _ => {
                match &self.msg {
                    Some(Msg::Error(m)) => {
                        spans.push(Span::styled(format!("Error: {m}"), theme.error()))
                    }
                    Some(Msg::Success(m)) => {
                        spans.push(Span::styled(format!("{} {m}", theme.ok()), theme.success()))
                    }
                    Some(Msg::Info(m)) => spans.push(Span::raw(m.clone())),
                    None => {
                        if let Some(r) = self.selected_row() {
                            spans.push(Span::raw(r.target.clone()));
                        }
                    }
                }
                if !self.filters.is_empty() {
                    spans.push(Span::styled(
                        format!("   filter: {}", self.filters.describe()),
                        theme.muted(),
                    ));
                }
            }
        }
        frame.render_widget(Paragraph::new(Line::from(spans)), area);
    }

    fn render_help_bar(&self, frame: &mut Frame, area: Rect) {
        let multi = self.ws.is_multi();
        let text = match (&self.mode, self.focus) {
            (Mode::Form(_), _) => "Enter:submit  Tab:next field  Shift-Tab:previous  Esc:cancel",
            (Mode::Prompt(_), _) => "answer the prompt  Esc:cancel",
            (Mode::Filter { .. }, _) | (Mode::PickFilterColumn, _) => {
                "type to filter  Enter:keep  Esc:undo"
            }
            (Mode::Help, _) => "Esc/?:close help",
            (_, Focus::Table) if multi => {
                "?:help  q:quit  Tab:focus  F:files  1-8:sort  /:filter  f:column filter  x:clear  a:add  e:edit  d:delete  c:clone  m:move  n:new section  o:editor"
            }
            (_, Focus::Sections) if multi => {
                "?:help  q:quit  Tab:focus  F:files  Enter:filter to section  R:rename section  n:new section"
            }
            (_, Focus::Files) => {
                "?:help  q:quit  Tab:focus  j/k:move  Enter:show in editor  Esc:back to table"
            }
            (_, Focus::Table) => {
                "?:help  q:quit  Tab:focus  1-7:sort  /:filter  f:column filter  x:clear  a:add  e:edit  d:delete  c:clone  m:move  n:new section  o:editor"
            }
            (_, Focus::Sections) => {
                "?:help  q:quit  Tab:focus  Enter:filter to section  R:rename section  n:new section"
            }
            (_, Focus::Editor) => "Ctrl-S:save  Ctrl-R:discard  Esc:back to table  Tab:focus  Ctrl-C:quit",
        };
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                format!(" {text}"),
                self.opts.theme.muted(),
            ))),
            area,
        );
    }

    fn render_help(&self, frame: &mut Frame, area: Rect) {
        let rows = help_rows(self.ws.is_multi());
        let lines: Vec<Line> = rows
            .iter()
            .map(|(k, ctx, act)| Line::from(format!(" {k:<17}{ctx:<22}{act}")))
            .collect();
        let rect = centered(area, 72, rows.len() as u16 + 2);
        frame.render_widget(Clear, rect);
        frame.render_widget(
            Paragraph::new(lines).block(self.pane_block("Keys".into(), true)),
            rect,
        );
    }

    fn render_form(&self, frame: &mut Frame, area: Rect, form: &Form) {
        let theme = self.opts.theme;
        let mut lines: Vec<Line> = Vec::new();
        for (i, (label, value)) in form.fields.iter().enumerate() {
            if i == form.focus {
                lines.push(Line::from(vec![
                    Span::styled(
                        format!("> {label}: "),
                        Style::default().add_modifier(Modifier::BOLD),
                    ),
                    Span::raw(format!("{value}{}", theme.caret())),
                ]));
            } else {
                lines.push(Line::from(format!("  {label}: {value}")));
            }
        }
        lines.push(Line::from(""));
        if let Some(e) = &form.error {
            lines.push(Line::from(Span::styled(
                format!("Error: {e}"),
                theme.error(),
            )));
        }
        lines.push(Line::from(Span::styled(
            "Enter: submit  Tab: next field  Esc: cancel",
            theme.muted(),
        )));
        let rect = centered(area, 80, lines.len() as u16 + 2);
        frame.render_widget(Clear, rect);
        frame.render_widget(
            Paragraph::new(lines).block(self.pane_block(form.title.clone(), true)),
            rect,
        );
    }

    fn prompt_text(&self, p: &Prompt) -> String {
        match p {
            Prompt::Delete { name, file } => {
                format!("Delete host {name} from {}? [y/N]", self.show_path(*file))
            }
            Prompt::Overwrite { host, .. } => {
                format!("{host} changed on disk since it was loaded. Overwrite it? [y/N]")
            }
            Prompt::Quit => "The editor has unsaved changes. [s]ave / [d]iscard / [c]ancel".into(),
            Prompt::QuitAll(files) => {
                let paths: Vec<String> = files.iter().map(|&i| self.show_path(i)).collect();
                format!(
                    "Unsaved changes in {}. [s]ave all / [d]iscard all / [c]ancel",
                    paths.join(", ")
                )
            }
            Prompt::EditorConflict { .. } => {
                "The file changed on disk. [r]eload (drop your edits) / [o]verwrite / [c]ancel"
                    .into()
            }
            Prompt::Discard => "Discard the editor's changes? [y/N]".into(),
        }
    }

    fn render_prompt(&self, frame: &mut Frame, area: Rect, p: &Prompt) {
        let text = format!(" {}", self.prompt_text(p));
        let want = text.chars().count() as u16 + 3;
        let width = want.min(area.width.saturating_sub(4)).max(10);
        let len = text.chars().count() as u16;
        let height = len.div_ceil(width.saturating_sub(2).max(1)) + 2;
        let rect = centered(area, width, height);
        frame.render_widget(Clear, rect);
        frame.render_widget(
            Paragraph::new(text)
                .wrap(Wrap { trim: false })
                .block(self.pane_block("Confirm".into(), true)),
            rect,
        );
    }
}
