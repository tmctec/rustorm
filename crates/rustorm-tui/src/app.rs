//! The TUI state machine and its rendering (docs/tui.md).

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, BorderType, Borders, Cell, Clear, List, ListItem, ListState, Paragraph, Row as TRow,
    Table, TableState,
};
use ratatui::Frame;
use rustorm_core::{
    AddSpec, CloneSpec, Config, ConfigFile, EditSpec, Env, Error, HostSelector, ProblemKind,
    SectionRename, SectionSummary, WriteOptions,
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
    Delete { name: String },
    Overwrite { op: Op, host: String },
    Quit,
    EditorConflict,
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
        Op::RenameSection { old, new } => Ok(match cfg.rename_section(old, new)? {
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

fn host_text(cfg: &Config, name: &str) -> Option<String> {
    cfg.find_host(name).map(|l| cfg.host(l).text())
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
    file: ConfigFile,
    stamp: Stamp,
    opts: Options,
    rows: Vec<Row>,
    visible: Vec<usize>,
    sort: Option<Sort>,
    filters: Filters,
    table: TableState,
    sections: Vec<SectionSummary>,
    section_sel: usize,
    focus: Focus,
    mode: Mode,
    editor: Editor,
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
        let file = ConfigFile::load(&path)?;
        let editor = Editor::new(&file.original);
        let mut app = App {
            stamp: stamp(&path),
            file,
            opts,
            rows: Vec::new(),
            visible: Vec::new(),
            sort: None,
            filters: Filters::default(),
            table: TableState::default(),
            sections: Vec::new(),
            section_sel: 0,
            focus: Focus::Table,
            mode: Mode::Normal,
            editor,
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

    /// The editor buffer.
    pub fn editor_text(&self) -> String {
        self.editor.text()
    }

    /// The editor cursor `(row, col)`, zero-based.
    pub fn editor_cursor(&self) -> (usize, usize) {
        self.editor.cursor()
    }

    /// True when the editor buffer differs from the file as loaded.
    pub fn is_editor_modified(&self) -> bool {
        self.editor.text() != self.file.original
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

    fn refresh(&mut self, select: Option<&str>) {
        let keep = select
            .map(str::to_string)
            .or_else(|| self.selected().map(str::to_string));
        self.rows = hosts::rows(&self.file.config, &self.opts.env);
        self.sections = self.file.config.sections();
        self.section_sel = self.section_sel.min(self.sections.len());
        if self.sections.is_empty() && self.focus == Focus::Sections {
            self.focus = Focus::Table;
        }
        self.recompute();
        self.select_name(keep.as_deref());
    }

    fn reload(&mut self) -> Result<(), String> {
        let file = ConfigFile::load(&self.file.path).map_err(|e| e.to_string())?;
        self.editor.set_text(&file.original);
        self.file = file;
        self.stamp = stamp(&self.file.path);
        self.refresh(None);
        Ok(())
    }

    fn error(&mut self, s: impl Into<String>) {
        self.msg = Some(Msg::Error(s.into()));
    }

    fn options(&self) -> WriteOptions {
        WriteOptions {
            no_backup: self.opts.no_backup,
        }
    }

    fn disk_changed(&self) -> bool {
        stamp(&self.file.path) != self.stamp
    }

    // ----- writes -----

    /// Runs `op` against the file: reload-then-apply when the file changed
    /// on disk, a prompt when the target host itself changed (F9).
    /// `Err` carries the message for the form.
    fn run_op(&mut self, op: Op, force: bool) -> Result<bool, String> {
        if !force && self.disk_changed() {
            let fresh = ConfigFile::load(&self.file.path).map_err(|e| tui_error(&e))?;
            let conflict = op.target().and_then(|t| {
                (host_text(&self.file.config, t) != host_text(&fresh.config, t))
                    .then(|| t.to_string())
            });
            self.editor.set_text(&fresh.original);
            self.file = fresh;
            self.stamp = stamp(&self.file.path);
            self.refresh(None);
            if let Some(host) = conflict {
                self.mode = Mode::Prompt(Prompt::Overwrite { op, host });
                return Ok(false);
            }
        }
        let mut next = self.file.clone();
        let msg = apply(&mut next.config, &op, &self.opts.env).map_err(|e| tui_error(&e))?;
        next.save(self.options()).map_err(|e| tui_error(&e))?;
        self.file = next;
        self.stamp = stamp(&self.file.path);
        self.editor.set_text(&self.file.original);
        self.refresh(op.focus_name().as_deref());
        self.msg = Some(Msg::Success(msg));
        Ok(true)
    }

    /// Saves the editor buffer. Returns true when written.
    fn save_editor(&mut self, force: bool) -> bool {
        let text = self.editor.text();
        let parsed = match Config::parse(&text) {
            Ok(c) => c,
            Err(e) => {
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
            if let Some(line) = p.line {
                self.editor.jump(line.saturating_sub(1));
            }
            self.focus = Focus::Editor;
            self.error(format!("Not saved: {p}"));
            return false;
        }
        if !force && self.disk_changed() {
            self.mode = Mode::Prompt(Prompt::EditorConflict);
            return false;
        }
        let mut cfg = parsed;
        cfg.sort_sections();
        let out = cfg.render();
        let mut next = self.file.clone();
        if let Err(e) = next.save_text(&out, self.options()) {
            self.error(format!("Not saved: {e}"));
            return false;
        }
        self.file = next;
        self.stamp = stamp(&self.file.path);
        self.editor.set_text(&out);
        self.refresh(None);
        self.msg = Some(Msg::Success(format!("Saved {}.", tilde(&self.file.path))));
        true
    }

    fn request_quit(&mut self) {
        if self.is_editor_modified() {
            self.mode = Mode::Prompt(Prompt::Quit);
        } else {
            self.quit = true;
        }
    }

    // ----- forms -----

    fn open_form(&mut self, kind: FormKind) {
        if self.is_editor_modified() {
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
            FormKind::RenameSection { name } => (
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
            FormKind::RenameSection { name } => {
                if f(0).is_empty() || f(0) == *name {
                    return Err("Give the section a new name.".into());
                }
                Ok(Op::RenameSection {
                    old: name.clone(),
                    new: f(0),
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
            if matches!(mode, Mode::Prompt(Prompt::Quit)) {
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
                    match Column::from_digit(c) {
                        Some(col) => {
                            self.mode = Mode::Filter {
                                column: Some(col),
                                previous: self.filters.get(col).to_string(),
                            }
                        }
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
            Prompt::Delete { name } => {
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
                    if self.save_editor(false) {
                        self.quit = true;
                    }
                }
                Some('d') => self.quit = true,
                _ => {}
            },
            Prompt::EditorConflict => match ch {
                Some('r') => {
                    if let Err(e) = self.reload() {
                        self.error(e);
                    } else {
                        self.msg = Some(Msg::Info("Reloaded the file from disk.".into()));
                    }
                }
                Some('o') => {
                    self.save_editor(true);
                }
                _ => self.msg = Some(Msg::Info("Not saved.".into())),
            },
            Prompt::Discard => {
                if ch == Some('y') {
                    match self.reload() {
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
        let i = order.iter().position(|f| *f == self.focus).unwrap_or(0);
        let n = order.len();
        self.focus = order[if back { (i + n - 1) % n } else { (i + 1) % n }];
    }

    fn handle_editor(&mut self, key: KeyEvent) {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        match key.code {
            KeyCode::Char('s') if ctrl => {
                self.save_editor(false);
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
                self.editor.area.input(key);
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
            KeyCode::Char(c) if Column::from_digit(c).is_some() => {
                let col = Column::from_digit(c).expect("checked");
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
                    let (identity, section) = match self.file.config.find_primary(&name) {
                        Some(l) => (
                            self.file
                                .config
                                .host(l)
                                .get("IdentityFile")
                                .unwrap_or_default(),
                            self.file.config.section_name(l).map(str::to_string),
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
                    if self.is_editor_modified() {
                        self.error("Save or discard the editor's changes first.");
                    } else {
                        self.mode = Mode::Prompt(Prompt::Delete { name });
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
                    if let Some(row) = self.editor.find_host_line(&name) {
                        self.editor.jump(row);
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
            KeyCode::Enter => {
                self.filters.columns[Column::Section.index()] = match self.section_sel {
                    0 => String::new(),
                    i => self.sections[i - 1].name.clone(),
                };
                self.view_changed();
            }
            KeyCode::Char('R') => {
                if self.section_sel == 0 {
                    self.error("Pick a section to rename.");
                } else {
                    let name = self.sections[self.section_sel - 1].name.clone();
                    self.open_form(FormKind::RenameSection { name });
                }
            }
            KeyCode::Char('n') => self.open_form(FormKind::AddSection),
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
        let top = if self.sections.is_empty() {
            vec![Rect::default(), panes[0]]
        } else {
            Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Length(22), Constraint::Min(20)])
                .split(panes[0])
                .to_vec()
        };
        if !self.sections.is_empty() {
            self.render_sections(frame, top[0]);
        }
        self.render_table(frame, top[1]);
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

    fn render_sections(&self, frame: &mut Frame, area: Rect) {
        let total = self.rows.len();
        let mut items = vec![ListItem::new(format!("{:<14}{:>4}", "All", total))];
        for s in &self.sections {
            let name: String = s.name.chars().take(14).collect();
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
        let header = TRow::new(Column::ALL.iter().map(|c| {
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
        let widths = [
            Constraint::Length(14),
            Constraint::Length(16),
            Constraint::Length(12),
            Constraint::Length(22),
            Constraint::Length(6),
            Constraint::Fill(1),
            Constraint::Length(14),
        ];
        if self.visible.is_empty() {
            let inner = block.inner(area);
            frame.render_widget(block, area);
            let line = if self.filters.is_empty() {
                format!(
                    "no hosts in {}. Press a to add one.",
                    tilde(&self.file.path)
                )
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
            TRow::new(Column::ALL.iter().map(|c| match r.get(*c) {
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
        let (r, c) = self.editor.cursor();
        let modified = if self.is_editor_modified() {
            " [modified]"
        } else {
            ""
        };
        let focused = self.focus == Focus::Editor;
        let block = self.pane_block(
            format!(
                "Editor {}{modified}  ln {} col {}",
                tilde(&self.file.path),
                r + 1,
                c + 1
            ),
            focused,
        );
        let theme = self.opts.theme;
        self.editor.render(frame, area, block, &theme, focused);
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
                "Filter which column? 1 section 2 host 3 user 4 hostname 5 port 6 proxy 7 jump",
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
        let text = match (&self.mode, self.focus) {
            (Mode::Form(_), _) => "Enter:submit  Tab:next field  Shift-Tab:previous  Esc:cancel",
            (Mode::Prompt(_), _) => "answer the prompt  Esc:cancel",
            (Mode::Filter { .. }, _) | (Mode::PickFilterColumn, _) => {
                "type to filter  Enter:keep  Esc:undo"
            }
            (Mode::Help, _) => "Esc/?:close help",
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
        let lines: Vec<Line> = HELP
            .iter()
            .map(|(k, ctx, act)| Line::from(format!(" {k:<17}{ctx:<22}{act}")))
            .collect();
        let rect = centered(area, 72, HELP.len() as u16 + 2);
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
            Prompt::Delete { name } => {
                format!("Delete host {name} from {}? [y/N]", tilde(&self.file.path))
            }
            Prompt::Overwrite { host, .. } => {
                format!("{host} changed on disk since it was loaded. Overwrite it? [y/N]")
            }
            Prompt::Quit => "The editor has unsaved changes. [s]ave / [d]iscard / [c]ancel".into(),
            Prompt::EditorConflict => {
                "The file changed on disk. [r]eload (drop your edits) / [o]verwrite / [c]ancel"
                    .into()
            }
            Prompt::Discard => "Discard the editor's changes? [y/N]".into(),
        }
    }

    fn render_prompt(&self, frame: &mut Frame, area: Rect, p: &Prompt) {
        let text = self.prompt_text(p);
        let rect = centered(area, text.chars().count() as u16 + 4, 3);
        frame.render_widget(Clear, rect);
        frame.render_widget(
            Paragraph::new(format!(" {text}")).block(self.pane_block("Confirm".into(), true)),
            rect,
        );
    }
}
