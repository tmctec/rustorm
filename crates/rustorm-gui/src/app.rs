//! [`App`]: the whole rustorm window as an `eframe::App`.

use std::path::{Path, PathBuf};

use egui::{
    Button, Color32, FontId, Key, KeyboardShortcut, Modifiers, RichText, Sense, TextEdit, Ui,
    ViewportCommand,
};
use egui_extras::{Column as TableColumn, TableBuilder};
use rustorm_core::{
    Config, Env, FileState, KeyGroup, SectionSummary, SettingsDraft, Workspace, WorkspaceSection,
    WriteOptions,
};

use crate::highlight::highlight_job;
use crate::ops::{host_text, Op};
use crate::rows::{sort_rows, workspace_rows, Column, Filters, HostRow, SortDir};

/// The label of the button that closes without saving: "Don't Save" on
/// macOS, "Discard" elsewhere.
pub const DISCARD_LABEL: &str = if cfg!(target_os = "macos") {
    "Don't Save"
} else {
    "Discard"
};

/// The button that closes without saving several files: "Don't Save" on
/// macOS, "Discard All" elsewhere.
pub const DISCARD_ALL_LABEL: &str = if cfg!(target_os = "macos") {
    "Don't Save"
} else {
    "Discard All"
};

const SAVE: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::S);
const ADD: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::N);
const FIND: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::F);
const EDITOR: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::E);
const TAB_HOSTS: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::Num1);
const TAB_EDITOR: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::Num2);

/// The main-area tab.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    /// The host table and detail panel.
    Hosts,
    /// The raw config editor.
    Editor,
}

/// Whether the detail form adds a host or edits one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FormMode {
    /// A new host.
    Add,
    /// The host with this primary name.
    Edit(String),
}

/// The detail panel's fields: the web version's name, connection URI and
/// identity file, plus section.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Form {
    /// Add or edit.
    pub mode: FormMode,
    /// Host name.
    pub name: String,
    /// `[user@]host[:port]`.
    pub uri: String,
    /// `IdentityFile`.
    pub identity: String,
    /// Section.
    pub section: String,
    /// The core's message for the last refused save.
    pub error: Option<String>,
}

impl Form {
    fn add() -> Form {
        Form {
            mode: FormMode::Add,
            name: String::new(),
            uri: String::new(),
            identity: String::new(),
            section: String::new(),
            error: None,
        }
    }

    fn edit(row: &HostRow) -> Form {
        Form {
            mode: FormMode::Edit(row.name.clone()),
            name: row.name.clone(),
            uri: row.uri(),
            identity: row.identity.clone().unwrap_or_default(),
            section: row.section.clone().unwrap_or_default(),
            error: None,
        }
    }

    fn op(&self) -> Op {
        match &self.mode {
            FormMode::Add => Op::Add {
                name: self.name.clone(),
                uri: self.uri.clone(),
                identity: self.identity.clone(),
                section: self.section.clone(),
            },
            FormMode::Edit(original) => Op::Edit {
                original: original.clone(),
                name: self.name.clone(),
                uri: self.uri.clone(),
                identity: self.identity.clone(),
                section: self.section.clone(),
            },
        }
    }
}

/// A modal alert or small dialog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Dialog {
    /// "Delete host <name>?"
    ConfirmDelete(String),
    /// Asks for the clone's name.
    Clone {
        /// The host to copy.
        source: String,
        /// The typed new name.
        new_name: String,
        /// The core's message for a refused clone.
        error: Option<String>,
    },
    /// Picks a destination section.
    Move {
        /// The host.
        name: String,
        /// The typed or picked section.
        section: String,
        /// The core's message for a refused move.
        error: Option<String>,
    },
    /// Asks for a new section's name.
    AddSection {
        /// The typed name.
        name: String,
        /// The core's message for a refused section.
        error: Option<String>,
    },
    /// The file changed on disk and the pending write touches a host that
    /// changed there too.
    Conflict(Op),
    /// The file changed on disk while the editor holds unsaved text.
    EditorConflict,
    /// Close requested with unsaved editor text in one file.
    UnsavedClose,
    /// Close requested with unsaved editor text in several files; each
    /// entry is a file's `~/` path.
    UnsavedCloseAll(Vec<String>),
}

/// The rustorm desktop app. Build it with [`App::new`] from a config path
/// and run it with eframe, or drive [`App::show`] from any `egui::Ui`.
pub struct App {
    ws: Workspace,
    env: Env,
    rows: Vec<HostRow>,
    sections: Vec<SectionSummary>,
    file_sections: Vec<WorkspaceSection>,
    /// One editor buffer per file of the workspace.
    buffers: Vec<String>,
    /// The file the editor, title and status bar show.
    current: usize,
    /// A Host line to put the editor's cursor on at the next frame.
    goto_line: Option<usize>,
    /// The Host line the editor's cursor was last put on.
    editor_line: Option<usize>,
    /// The selected tab.
    pub tab: Tab,
    /// The sorted column and direction; `None` keeps the file order.
    pub sort: Option<(Column, SortDir)>,
    /// Every filter.
    pub filters: Filters,
    selected: Option<String>,
    form: Option<Form>,
    /// The selected host's All settings rows, as edited.
    settings: Option<SettingsDraft>,
    /// The last refused All settings save.
    settings_error: Option<String>,
    dialog: Option<Dialog>,
    editor_error: Option<String>,
    status: String,
    last_backup: Option<PathBuf>,
    focus_filter: bool,
    close_requested: bool,
    allow_close: bool,
    closing: bool,
    title: String,
}

impl App {
    /// Loads `path` (a missing file opens empty and is not created) and
    /// every file its `Include` lines load, with the process's `$USER` and
    /// home directory.
    pub fn new(path: impl Into<PathBuf>) -> anyhow::Result<App> {
        App::with_env(path, Env::from_process())
    }

    /// Like [`App::new`] with an explicit environment; `env.home` also
    /// resolves `~/` and relative `Include` patterns.
    pub fn with_env(path: impl Into<PathBuf>, env: Env) -> anyhow::Result<App> {
        let ws = Workspace::load_with_home(path.into(), env.home.as_deref())?;
        let mut app = App {
            buffers: ws.files.iter().map(|f| f.original.clone()).collect(),
            ws,
            env,
            rows: Vec::new(),
            sections: Vec::new(),
            file_sections: Vec::new(),
            current: 0,
            goto_line: None,
            editor_line: None,
            tab: Tab::Hosts,
            sort: None,
            filters: Filters::default(),
            selected: None,
            form: None,
            settings: None,
            settings_error: None,
            dialog: None,
            editor_error: None,
            status: String::new(),
            last_backup: None,
            focus_filter: false,
            close_requested: false,
            allow_close: false,
            closing: false,
            title: String::new(),
        };
        app.refresh();
        Ok(app)
    }

    // ----- state the UI and tests read -------------------------------------

    /// The root config file path.
    pub fn path(&self) -> &Path {
        &self.ws.files[0].path
    }

    /// The workspace: the root and every file its `Include` lines load.
    pub fn workspace(&self) -> &Workspace {
        &self.ws
    }

    /// True when the workspace has several files, so the Files list, the
    /// editor's file selector and the file column show.
    pub fn is_multi(&self) -> bool {
        self.ws.is_multi()
    }

    /// The file the editor shows, an index into the workspace's files.
    pub fn current_file(&self) -> usize {
        self.current
    }

    /// Shows file `i` in the editor, keeping every buffer's text.
    pub fn select_file(&mut self, i: usize) {
        if i < self.buffers.len() {
            self.current = i;
            self.editor_error = None;
        }
    }

    /// True when file `i`'s buffer differs from the file as last loaded.
    pub fn file_dirty(&self, i: usize) -> bool {
        self.buffers
            .get(i)
            .is_some_and(|b| *b != self.ws.files[i].original)
    }

    /// Indices of the files with unsaved editor text, in load order.
    pub fn dirty_files(&self) -> Vec<usize> {
        self.load_indices()
            .into_iter()
            .filter(|&i| self.file_dirty(i))
            .collect()
    }

    /// The editor buffer of file `i`.
    pub fn buffer(&self, i: usize) -> &str {
        &self.buffers[i]
    }

    /// The 1-based line the editor's cursor was last put on, by
    /// [`App::open_in_editor`] or by the Editor tab showing a newly
    /// selected host.
    pub fn editor_line(&self) -> Option<usize> {
        self.editor_line
    }

    /// The 1-based line the editor's cursor goes to when the Editor tab
    /// next shows: the `Host` line of a newly selected host.
    pub fn pending_editor_line(&self) -> Option<usize> {
        self.goto_line
    }

    /// The loaded files' indices in load order.
    fn load_indices(&self) -> Vec<usize> {
        self.ws
            .load_order
            .iter()
            .filter_map(|e| match e.state {
                FileState::Loaded(i) => Some(i),
                FileState::Unreadable(_) => None,
            })
            .collect()
    }

    /// Every host row in file order.
    pub fn rows(&self) -> &[HostRow] {
        &self.rows
    }

    /// The rows the table shows: filtered, then sorted.
    pub fn visible_rows(&self) -> Vec<HostRow> {
        let mut out: Vec<HostRow> = self
            .rows
            .iter()
            .filter(|r| self.filters.matches(r))
            .cloned()
            .collect();
        if let Some((column, dir)) = self.sort {
            sort_rows(&mut out, column, dir);
        }
        out
    }

    /// The names of [`App::visible_rows`], in table order.
    pub fn visible_names(&self) -> Vec<String> {
        self.visible_rows().into_iter().map(|r| r.name).collect()
    }

    /// The sections with host counts, catch-all last.
    pub fn sections(&self) -> &[SectionSummary] {
        &self.sections
    }

    /// The selected host.
    pub fn selected(&self) -> Option<&str> {
        self.selected.as_deref()
    }

    /// The open detail form.
    pub fn form(&self) -> Option<&Form> {
        self.form.as_ref()
    }

    /// The open dialog.
    pub fn dialog(&self) -> Option<&Dialog> {
        self.dialog.as_ref()
    }

    /// The editor buffer of the selected file.
    pub fn editor_text(&self) -> &str {
        &self.buffers[self.current]
    }

    /// Replaces the selected file's editor buffer, as typing would.
    pub fn set_editor_text(&mut self, text: impl Into<String>) {
        self.buffers[self.current] = text.into();
    }

    /// The editor's last refusal ("line N: cannot parse: …").
    pub fn editor_error(&self) -> Option<&str> {
        self.editor_error.as_deref()
    }

    /// True when any file's editor buffer differs from the file as last
    /// loaded.
    pub fn editor_dirty(&self) -> bool {
        (0..self.buffers.len()).any(|i| self.file_dirty(i))
    }

    /// The status-bar message of the last operation.
    pub fn status(&self) -> &str {
        &self.status
    }

    /// The backup the last write made.
    pub fn last_backup(&self) -> Option<&Path> {
        self.last_backup.as_deref()
    }

    /// The window title: `rustorm — <root>`, plus `— <selected file>` on
    /// a workspace of several files, and ` •` while any file has unsaved
    /// editor text.
    pub fn title(&self) -> String {
        let file = if self.is_multi() {
            format!(" — {}", self.ws.display(self.current))
        } else {
            String::new()
        };
        format!(
            "rustorm — {}{file}{}",
            self.path().display(),
            if self.editor_dirty() { " •" } else { "" }
        )
    }

    /// Asks to close the window, as the close button or Cmd+Q does.
    pub fn request_close(&mut self) {
        self.close_requested = true;
    }

    /// True once the app has told the window to close.
    pub fn is_closing(&self) -> bool {
        self.closing
    }

    // ----- actions -----------------------------------------------------------

    fn refresh(&mut self) {
        self.rows = workspace_rows(&self.ws, &self.env);
        self.file_sections = self.ws.sections();
        self.sections = self.file_sections.iter().map(|s| s.summary()).collect();
        if let Some(sel) = &self.selected {
            if !self.rows.iter().any(|r| &r.name == sel) {
                self.selected = None;
            }
        }
        if self.filters.file.is_some_and(|f| f >= self.ws.files.len()) {
            self.filters.file = None;
        }
    }

    /// Replaces the workspace with `fresh`, matching files by path: a
    /// buffer with unsaved text keeps it, every other buffer takes the
    /// file's new text, and the selected file stays selected.
    fn adopt(&mut self, fresh: Workspace) {
        let old: Vec<(PathBuf, bool, String)> = self
            .ws
            .files
            .iter()
            .enumerate()
            .map(|(i, f)| (f.path.clone(), self.file_dirty(i), self.buffers[i].clone()))
            .collect();
        let current = self.ws.files[self.current].path.clone();
        let filter_file = self.filters.file.map(|i| self.ws.files[i].path.clone());
        let section_file = self
            .filters
            .section_file
            .map(|i| self.ws.files[i].path.clone());
        let index_of = |ws: &Workspace, p: &Path| ws.files.iter().position(|f| f.path == p);
        self.buffers = fresh
            .files
            .iter()
            .map(|f| match old.iter().find(|(p, _, _)| *p == f.path) {
                Some((_, true, text)) => text.clone(),
                _ => f.original.clone(),
            })
            .collect();
        self.current = index_of(&fresh, &current).unwrap_or(0);
        self.filters.file = filter_file.and_then(|p| index_of(&fresh, &p));
        self.filters.section_file = section_file.and_then(|p| index_of(&fresh, &p));
        self.ws = fresh;
    }

    fn load_fresh(&self) -> rustorm_core::Result<Workspace> {
        Workspace::load_with_home(self.ws.files[0].path.clone(), self.env.home.as_deref())
    }

    /// After a write to one file: when its `Include` lines changed the set
    /// of files, the workspace reloads so the new set shows.
    fn follow_includes(&mut self) {
        if let Ok(fresh) = self.load_fresh() {
            let paths = |ws: &Workspace| -> Vec<PathBuf> {
                ws.load_order.iter().map(|e| e.path.clone()).collect()
            };
            if paths(&fresh) != paths(&self.ws) {
                self.adopt(fresh);
            }
        }
    }

    fn disk_text(path: &Path) -> std::io::Result<String> {
        match std::fs::read_to_string(path) {
            Ok(t) => Ok(t),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
            Err(e) => Err(e),
        }
    }

    /// Records the backup of file `i` when the write made one.
    fn note_backup(&mut self, i: usize, existed: bool) {
        let backup = self.ws.backup_path_for(i);
        if existed && backup.exists() {
            self.last_backup = Some(backup);
        }
    }

    /// Selects `name` and opens it in the detail form. A newly selected
    /// host also becomes the editor's: its file is selected there and the
    /// cursor goes to its `Host` line when the Editor tab next shows.
    pub fn select(&mut self, name: &str) {
        if self.selected.as_deref() != Some(name) {
            self.follow_host(name);
        }
        self.selected = Some(name.to_string());
        self.form = self.rows.iter().find(|r| r.name == name).map(Form::edit);
        self.settings = self.ws.find_host(name).map(|wl| {
            let defaults = self.ws.files.iter().find_map(|f| f.config.defaults());
            SettingsDraft::new(name, self.ws.host(wl), defaults)
        });
        self.settings_error = None;
    }

    /// Points the editor at host `name` without leaving the Hosts tab: its
    /// file, and its `Host` line in that file's buffer (so unsaved edits
    /// that moved it are honored), else the line on disk.
    fn follow_host(&mut self, name: &str) {
        let Some(wl) = self.ws.find_host(name) else {
            return;
        };
        self.select_file(wl.file);
        let line =
            buffer_host_line(&self.buffers[wl.file], name).unwrap_or_else(|| self.ws.host_line(wl));
        self.goto_line = Some(line);
    }

    /// Moves the selection `delta` rows through the visible hosts.
    fn move_selection(&mut self, delta: isize) {
        let names = self.visible_names();
        if names.is_empty() {
            return;
        }
        let at = self
            .selected
            .as_ref()
            .and_then(|s| names.iter().position(|n| n == s));
        let next = match at {
            Some(i) => (i as isize + delta).clamp(0, names.len() as isize - 1) as usize,
            None => 0,
        };
        let name = names[next].clone();
        self.select(&name);
    }

    /// The selected host's All settings rows, as edited.
    pub fn settings(&self) -> Option<&SettingsDraft> {
        self.settings.as_ref()
    }

    /// The last refused All settings save.
    pub fn settings_error(&self) -> Option<&str> {
        self.settings_error.as_deref()
    }

    /// Writes the All settings changes of the selected host in one write.
    /// Refuses a value that does not fit its keyword, and writes nothing
    /// when nothing changed or the editor holds unsaved text.
    pub fn save_settings(&mut self) -> bool {
        let Some(draft) = &self.settings else {
            return false;
        };
        if self.editor_dirty() {
            return false;
        }
        let changes = match draft.changes() {
            Ok(c) => c,
            Err(e) => {
                self.status = format!("error: {e}");
                self.settings_error = Some(e);
                return false;
            }
        };
        if changes.is_empty() {
            self.status = "no changes.".to_string();
            return false;
        }
        let name = draft.host.clone();
        let ok = self.run(Op::Settings { name, changes });
        if !ok {
            if let Some(f) = &mut self.form {
                self.settings_error = f.error.take();
            }
        }
        ok
    }

    /// Opens the empty add form.
    pub fn open_add(&mut self) {
        self.selected = None;
        self.settings = None;
        self.form = Some(Form::add());
        self.tab = Tab::Hosts;
    }

    /// Opens the host `name` in the editor: selects the file that holds it
    /// and puts the cursor on its `Host` line. False when no file holds it.
    pub fn open_in_editor(&mut self, name: &str) -> bool {
        let Some(wl) = self.ws.find_host(name) else {
            return false;
        };
        let line = self.ws.host_line(wl);
        self.select_file(wl.file);
        self.tab = Tab::Editor;
        self.goto_line = Some(line);
        self.editor_line = Some(line);
        true
    }

    /// Runs `op` through the core and writes the files it changed,
    /// reloading first when a file changed on disk. Returns true when the
    /// write happened.
    pub fn run(&mut self, op: Op) -> bool {
        self.run_op(op, false)
    }

    fn fail(&mut self, message: String) {
        match &mut self.dialog {
            Some(Dialog::Clone { error, .. })
            | Some(Dialog::Move { error, .. })
            | Some(Dialog::AddSection { error, .. }) => *error = Some(message.clone()),
            _ => {
                if let Some(form) = &mut self.form {
                    form.error = Some(message.clone());
                }
            }
        }
        self.status = format!("error: {message}");
    }

    /// The first loaded file whose disk text differs from what the app
    /// last loaded; `Err` names a file that cannot be read.
    fn changed_on_disk(&self) -> Result<bool, String> {
        for f in &self.ws.files {
            match App::disk_text(&f.path) {
                Ok(t) if t != f.original => return Ok(true),
                Ok(_) => {}
                Err(e) => return Err(format!("cannot read {}: {e}", f.path.display())),
            }
        }
        Ok(false)
    }

    fn run_op(&mut self, op: Op, force: bool) -> bool {
        match self.changed_on_disk() {
            Err(e) => {
                self.fail(e);
                return false;
            }
            Ok(true) => {
                let fresh = match self.load_fresh() {
                    Ok(w) => w,
                    Err(e) => {
                        self.fail(e.to_string());
                        return false;
                    }
                };
                let conflict = !force
                    && op
                        .hosts()
                        .iter()
                        .any(|n| host_text(&self.ws, n) != host_text(&fresh, n));
                self.adopt(fresh);
                self.refresh();
                self.status = "reloaded: the file changed on disk.".to_string();
                if conflict {
                    self.dialog = Some(Dialog::Conflict(op));
                    return false;
                }
            }
            Ok(false) => {}
        }
        let mut ws = self.ws.clone();
        let outcome = match op.apply(&mut ws, &self.env) {
            Ok(o) => o,
            Err(e) => {
                self.fail(e.to_string());
                return false;
            }
        };
        let existed: Vec<bool> = self.ws.files.iter().map(|f| f.existed).collect();
        let previous = std::mem::replace(&mut self.ws, ws);
        let written = match self.ws.save(WriteOptions::default()) {
            Ok(w) => w,
            Err(e) => {
                self.ws = previous;
                self.fail(e.to_string());
                return false;
            }
        };
        let old_paths: Vec<PathBuf> = previous.files.iter().map(|f| f.path.clone()).collect();
        let clean: Vec<bool> = (0..previous.files.len())
            .map(|i| self.buffers[i] == previous.files[i].original)
            .collect();
        self.buffers = self
            .ws
            .files
            .iter()
            .map(|f| match old_paths.iter().position(|p| *p == f.path) {
                Some(j) if !clean[j] => self.buffers[j].clone(),
                _ => f.original.clone(),
            })
            .collect();
        for &i in &written {
            self.note_backup(i, existed.get(i).copied().unwrap_or(false));
        }
        self.editor_error = None;
        self.follow_includes();
        self.refresh();
        self.status = outcome.message;
        self.dialog = None;
        match outcome.select {
            Some(name) => self.select(&name),
            None if matches!(op, Op::AddSection { .. }) => {}
            None => {
                self.selected = None;
                self.form = None;
            }
        }
        true
    }

    /// Saves the detail form (add or edit).
    pub fn save_form(&mut self) -> bool {
        match self.form.as_ref().map(Form::op) {
            Some(op) if !self.editor_dirty() => self.run(op),
            _ => false,
        }
    }

    /// Saves the selected file's editor buffer through the core, after its
    /// own backup. Refuses an unparsable line with its number; asks before
    /// replacing a file changed on disk. Other files are not written.
    pub fn save_editor(&mut self) -> bool {
        self.save_editor_inner(false)
    }

    fn save_editor_inner(&mut self, force: bool) -> bool {
        let i = self.current;
        let config = match Config::parse(&self.buffers[i]) {
            Ok(c) => c,
            Err(e) => {
                self.editor_error = Some(e.to_string());
                return false;
            }
        };
        if let Some((n, line)) = config.lines().enumerate().find(|(_, l)| l.is_unparsable()) {
            let message = format!("line {}: cannot parse: {}", n + 1, line.text().trim());
            self.status = format!("error: {message}");
            self.editor_error = Some(message);
            return false;
        }
        let path = self.ws.files[i].path.clone();
        match App::disk_text(&path) {
            Ok(disk) if disk != self.ws.files[i].original && !force => {
                self.dialog = Some(Dialog::EditorConflict);
                return false;
            }
            Ok(_) => {}
            Err(e) => {
                self.editor_error = Some(format!("cannot read {}: {e}", path.display()));
                return false;
            }
        }
        let mut config = config;
        config.sort_sections();
        let text = config.render();
        let existed = self.ws.files[i].existed;
        if let Err(e) = self.ws.save_text(i, &text, WriteOptions::default()) {
            self.editor_error = Some(e.to_string());
            return false;
        }
        self.buffers[i] = self.ws.files[i].original.clone();
        self.note_backup(i, existed);
        self.editor_error = None;
        self.follow_includes();
        self.refresh();
        self.status = if self.is_multi() {
            format!("saved {}.", self.ws.display(self.current))
        } else {
            "saved.".to_string()
        };
        if let Some(sel) = self.selected.clone() {
            self.select(&sel);
        }
        true
    }

    /// Restores the selected file's editor buffer from the file on disk.
    pub fn discard_editor(&mut self) {
        let i = self.current;
        match self.ws.reload_file(i) {
            Ok(()) => {
                self.buffers[i] = self.ws.files[i].original.clone();
                self.editor_error = None;
                self.follow_includes();
                self.refresh();
                self.status = "changes discarded.".to_string();
            }
            Err(e) => self.status = format!("error: {e}"),
        }
    }

    /// Saves every file with unsaved editor text, in load order. Stops at
    /// the first refused save with that file selected on the Editor tab.
    fn save_all(&mut self) -> bool {
        for i in self.dirty_files() {
            self.current = i;
            if !self.save_editor_inner(false) {
                self.tab = Tab::Editor;
                return false;
            }
        }
        true
    }

    // ----- drawing -------------------------------------------------------------

    /// Draws the whole window into `ui` and handles its input.
    pub fn show(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        self.handle_close(&ctx);
        self.handle_shortcuts(&ctx);
        let title = self.title();
        if title != self.title {
            ctx.send_viewport_cmd(ViewportCommand::Title(title.clone()));
            self.title = title;
        }

        egui::Panel::top("menu").show(ui, |ui| self.menu_bar(ui));
        egui::Panel::bottom("status").show(ui, |ui| self.status_bar(ui));
        egui::Panel::left("sections")
            .resizable(true)
            .default_size(190.0)
            .show(ui, |ui| self.sidebar(ui));
        egui::CentralPanel::default_margins().show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.tab, Tab::Hosts, "Hosts");
                ui.selectable_value(&mut self.tab, Tab::Editor, "Editor");
            });
            ui.separator();
            match self.tab {
                Tab::Hosts => self.hosts_tab(ui),
                Tab::Editor => self.editor_tab(ui),
            }
        });
        self.dialogs(&ctx);
    }

    fn handle_close(&mut self, ctx: &egui::Context) {
        let requested = ctx.input(|i| i.viewport().close_requested())
            || std::mem::take(&mut self.close_requested);
        if !requested {
            return;
        }
        let dirty = self.dirty_files();
        if !dirty.is_empty() && !self.allow_close {
            ctx.send_viewport_cmd(ViewportCommand::CancelClose);
            if dirty.len() == 1 {
                self.current = dirty[0];
                self.dialog = Some(Dialog::UnsavedClose);
            } else {
                let names = dirty.iter().map(|&i| self.ws.display(i)).collect();
                self.dialog = Some(Dialog::UnsavedCloseAll(names));
            }
        } else {
            self.close_now(ctx);
        }
    }

    fn close_now(&mut self, ctx: &egui::Context) {
        self.allow_close = true;
        self.closing = true;
        ctx.send_viewport_cmd(ViewportCommand::Close);
    }

    fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        if self.dialog.is_some() {
            return;
        }
        if ctx.input_mut(|i| i.consume_shortcut(&SAVE)) {
            match self.tab {
                Tab::Editor => {
                    self.save_editor();
                }
                Tab::Hosts => {
                    self.save_form();
                }
            }
        }
        if ctx.input_mut(|i| i.consume_shortcut(&ADD)) {
            self.open_add();
        }
        if ctx.input_mut(|i| i.consume_shortcut(&FIND)) {
            self.tab = Tab::Hosts;
            self.focus_filter = true;
        }
        if ctx.input_mut(|i| i.consume_shortcut(&EDITOR) || i.consume_shortcut(&TAB_EDITOR)) {
            self.tab = Tab::Editor;
        }
        if ctx.input_mut(|i| i.consume_shortcut(&TAB_HOSTS)) {
            self.tab = Tab::Hosts;
        }
        let typing = ctx.memory(|m| m.focused().is_some());
        if !typing && self.tab == Tab::Hosts {
            let delete = ctx.input(|i| {
                i.key_pressed(Key::Delete) || (i.modifiers.command && i.key_pressed(Key::Backspace))
            });
            if delete && !self.editor_dirty() {
                if let Some(sel) = self.selected.clone() {
                    self.dialog = Some(Dialog::ConfirmDelete(sel));
                }
            }
            if ctx.input(|i| i.key_pressed(Key::Escape)) {
                self.form = None;
                self.selected = None;
            }
            if ctx.input(|i| i.key_pressed(Key::ArrowDown)) {
                self.move_selection(1);
            }
            if ctx.input(|i| i.key_pressed(Key::ArrowUp)) {
                self.move_selection(-1);
            }
        }
    }

    fn menu_bar(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        let sc = |s: &KeyboardShortcut| ctx.format_shortcut(s);
        egui::MenuBar::new().ui(ui, |ui| {
            ui.menu_button("File", |ui| {
                if ui
                    .add(Button::new("Add Host").shortcut_text(sc(&ADD)))
                    .clicked()
                {
                    self.open_add();
                }
                if ui
                    .add(Button::new("Save").shortcut_text(sc(&SAVE)))
                    .clicked()
                {
                    match self.tab {
                        Tab::Editor => {
                            self.save_editor();
                        }
                        Tab::Hosts => {
                            self.save_form();
                        }
                    }
                }
                let current_dirty = self.file_dirty(self.current);
                if ui
                    .add_enabled(current_dirty, Button::new("Discard Changes"))
                    .clicked()
                {
                    self.discard_editor();
                }
                if ui
                    .add_enabled(!current_dirty, Button::new("Reload from Disk"))
                    .clicked()
                {
                    self.discard_editor();
                }
                ui.separator();
                if ui.button("Quit").clicked() {
                    self.request_close();
                }
            });
            ui.menu_button("Edit", |ui| {
                let has_sel = self.selected.is_some() && !self.editor_dirty();
                if ui
                    .add_enabled(has_sel, Button::new("Delete Host…").shortcut_text("Delete"))
                    .clicked()
                {
                    if let Some(s) = self.selected.clone() {
                        self.dialog = Some(Dialog::ConfirmDelete(s));
                    }
                }
                if ui
                    .add_enabled(has_sel, Button::new("Clone Host…"))
                    .clicked()
                {
                    self.open_clone();
                }
                if ui
                    .add(Button::new("Find").shortcut_text(sc(&FIND)))
                    .clicked()
                {
                    self.tab = Tab::Hosts;
                    self.focus_filter = true;
                }
            });
            ui.menu_button("View", |ui| {
                if ui
                    .add(Button::new("Hosts").shortcut_text(sc(&TAB_HOSTS)))
                    .clicked()
                {
                    self.tab = Tab::Hosts;
                }
                if ui
                    .add(Button::new("Editor").shortcut_text(sc(&EDITOR)))
                    .clicked()
                {
                    self.tab = Tab::Editor;
                }
            });
        });
    }

    fn status_bar(&mut self, ui: &mut Ui) {
        let multi = self.is_multi();
        ui.horizontal(|ui| {
            if multi {
                ui.label(self.ws.display(self.current));
            } else {
                ui.label(self.path().display().to_string());
            }
            ui.separator();
            if self.file_dirty(self.current) {
                ui.label(RichText::new("● unsaved changes").color(ui.visuals().warn_fg_color));
            } else if self.editor_dirty() {
                ui.label(
                    RichText::new("● unsaved changes in other files")
                        .color(ui.visuals().warn_fg_color),
                );
            } else {
                ui.label("saved");
            }
            ui.separator();
            if multi {
                let backup = self.ws.backup_path_for(self.current);
                ui.label(format!("backup: {}", self.ws.display_path(&backup)));
            } else {
                match &self.last_backup {
                    Some(b) => ui.label(format!("last backup: {}", b.display())),
                    None => ui.label("no backup yet"),
                };
            }
            if !self.status.is_empty() {
                ui.separator();
                if self.status.starts_with("error:") {
                    ui.colored_label(ui.visuals().error_fg_color, &self.status);
                } else {
                    ui.label(&self.status);
                }
            }
        });
    }

    fn sidebar(&mut self, ui: &mut Ui) {
        let multi = self.is_multi();
        ui.heading("Sections");
        let total = self.rows.len();
        if ui
            .selectable_label(
                self.filters.sidebar.is_none() && self.filters.file.is_none(),
                format!("All hosts  {total}"),
            )
            .clicked()
        {
            self.filters.sidebar = None;
            self.filters.section_file = None;
            self.filters.file = None;
        }
        let weak = ui.visuals().weak_text_color();
        for s in self.file_sections.clone() {
            let file = multi.then_some(s.index);
            let selected = self.filters.sidebar.as_deref() == Some(s.name.as_str())
                && self.filters.section_file == file;
            let text = if s.catch_all {
                format!("{}  {}  (catch-all)", s.name, s.hosts)
            } else {
                format!("{}  {}", s.name, s.hosts)
            };
            let shared = multi
                && self
                    .file_sections
                    .iter()
                    .any(|o| o.index != s.index && o.name.eq_ignore_ascii_case(&s.name));
            let label: egui::WidgetText = if shared {
                let mut job = egui::text::LayoutJob::default();
                let style = ui.style().clone();
                RichText::new(text).append_to(
                    &mut job,
                    &style,
                    egui::FontSelection::Default,
                    egui::Align::Center,
                );
                RichText::new(format!("  {}", self.ws.file_name(s.index)))
                    .color(weak)
                    .append_to(
                        &mut job,
                        &style,
                        egui::FontSelection::Default,
                        egui::Align::Center,
                    );
                job.into()
            } else {
                text.into()
            };
            let resp = ui.selectable_label(selected, label);
            let resp = if multi {
                resp.on_hover_text(self.ws.display(s.index))
            } else {
                resp
            };
            if resp.clicked() {
                if selected {
                    self.filters.sidebar = None;
                    self.filters.section_file = None;
                } else {
                    self.filters.sidebar = Some(s.name.clone());
                    self.filters.section_file = file;
                }
            }
        }
        ui.add_space(6.0);
        if ui
            .add_enabled(!self.editor_dirty(), Button::new("New section…"))
            .clicked()
        {
            self.dialog = Some(Dialog::AddSection {
                name: String::new(),
                error: None,
            });
        }
        if multi {
            ui.add_space(10.0);
            ui.heading("Files");
            self.files_list(ui);
        }
    }

    /// The sidebar's Files list: every file in load order with its host
    /// count and `•` while it has unsaved editor text.
    fn files_list(&mut self, ui: &mut Ui) {
        let weak = ui.visuals().weak_text_color();
        for entry in self.ws.load_order.clone() {
            let indent = 12.0 * entry.depth as f32;
            let name = entry
                .path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            ui.horizontal(|ui| {
                ui.add_space(indent);
                match entry.state {
                    FileState::Loaded(i) => {
                        let hosts = self.ws.files[i].config.host_count();
                        let dot = if self.file_dirty(i) { "  •" } else { "" };
                        let selected = self.filters.file == Some(i);
                        let resp = ui
                            .selectable_label(selected, format!("{name}  {hosts}{dot}"))
                            .on_hover_text(self.ws.display(i));
                        if resp.clicked() {
                            if selected {
                                self.filters.file = None;
                            } else {
                                self.filters.file = Some(i);
                                self.select_file(i);
                            }
                        }
                    }
                    FileState::Unreadable(reason) => {
                        ui.label(RichText::new(format!("{name}  cannot read")).color(weak))
                            .on_hover_text(format!(
                                "{} ({reason})",
                                self.ws.display_path(&entry.path)
                            ));
                    }
                }
            });
        }
    }

    fn hosts_tab(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            if ui
                .add_enabled(!self.editor_dirty(), Button::new("Add Host"))
                .clicked()
            {
                self.open_add();
            }
            let resp = ui.add(
                TextEdit::singleline(&mut self.filters.global)
                    .hint_text("Filter all columns")
                    .desired_width(260.0),
            );
            set_label(ui, resp.id, "filter all");
            if std::mem::take(&mut self.focus_filter) {
                resp.request_focus();
            }
            if self.filters.active() && ui.button("Clear filters").clicked() {
                self.filters.clear();
            }
        });
        ui.add_space(4.0);
        if self.form.is_some() {
            egui::Panel::right("detail")
                .resizable(true)
                .default_size(320.0)
                .show(ui, |ui| self.detail_panel(ui));
        }
        egui::CentralPanel::default_margins().show(ui, |ui| self.table(ui));
    }

    fn table(&mut self, ui: &mut Ui) {
        let visible = self.visible_rows();
        ui.style_mut().interaction.selectable_labels = false;
        let weak = ui.visuals().weak_text_color();
        let mut clicked_header: Option<Column> = None;
        let mut clicked_row: Option<String> = None;
        let sort = self.sort;
        let filters = &mut self.filters;
        let selected = self.selected.clone();
        let columns = Column::shown(self.ws.is_multi());
        TableBuilder::new(ui)
            .id_salt("hosts")
            .striped(true)
            .sense(Sense::click())
            .columns(
                TableColumn::auto().at_least(70.0).resizable(true),
                columns.len() - 1,
            )
            .column(TableColumn::remainder().at_least(80.0))
            .header(50.0, |mut header| {
                for &column in columns {
                    header.col(|ui| {
                        ui.vertical(|ui| {
                            let arrow = match sort {
                                Some((c, SortDir::Ascending)) if c == column => " ▲",
                                Some((c, SortDir::Descending)) if c == column => " ▼",
                                _ => "",
                            };
                            let label = format!("{}{arrow}", column.title());
                            if ui
                                .add(Button::new(RichText::new(label).strong()).frame(false))
                                .on_hover_text("Sort")
                                .clicked()
                            {
                                clicked_header = Some(column);
                            }
                            let resp = ui.add(
                                TextEdit::singleline(filters.get_mut(column))
                                    .hint_text("filter")
                                    .desired_width(f32::INFINITY),
                            );
                            set_label(ui, resp.id, &format!("filter {}", column.title()));
                        });
                    });
                }
            })
            .body(|mut body| {
                for row in &visible {
                    body.row(22.0, |mut tr| {
                        tr.set_selected(selected.as_deref() == Some(row.name.as_str()));
                        for &column in columns {
                            tr.col(|ui| {
                                if column == Column::Host {
                                    ui.label(&row.name);
                                    if !row.aliases.is_empty() {
                                        ui.label(RichText::new(row.aliases.join(" ")).color(weak));
                                    }
                                    return;
                                }
                                let text = row.display(column);
                                if column == Column::File {
                                    ui.label(text).on_hover_text(self.ws.display(row.file));
                                    return;
                                }
                                if text.is_empty() {
                                    ui.label(RichText::new("—").color(weak));
                                } else if row.inherited(column) {
                                    ui.label(RichText::new(text).italics().color(weak))
                                        .on_hover_text("inherited from Host * or the environment");
                                } else {
                                    ui.label(text);
                                }
                            });
                        }
                        if tr.response().clicked() {
                            clicked_row = Some(row.name.clone());
                        }
                    });
                }
            });
        if visible.is_empty() {
            ui.add_space(12.0);
            ui.vertical_centered(|ui| {
                if self.rows.is_empty() {
                    ui.label("no hosts yet");
                    if ui
                        .add_enabled(!self.editor_dirty(), Button::new("Add Host…"))
                        .clicked()
                    {
                        self.open_add();
                    }
                } else {
                    ui.label("no hosts match");
                    if ui.button("Clear all filters").clicked() {
                        self.filters.clear();
                    }
                }
            });
        }
        if let Some(column) = clicked_header {
            self.sort = match self.sort {
                Some((c, SortDir::Ascending)) if c == column => Some((column, SortDir::Descending)),
                Some((c, SortDir::Descending)) if c == column => None,
                _ => Some((column, SortDir::Ascending)),
            };
        }
        if let Some(name) = clicked_row {
            self.select(&name);
        }
    }

    fn detail_panel(&mut self, ui: &mut Ui) {
        let blocked = self.editor_dirty();
        let multi = self.is_multi();
        let section_names = self.section_names();
        let Some(form) = self.form.as_mut() else {
            return;
        };
        let adding = form.mode == FormMode::Add;
        ui.heading(if adding { "Add host" } else { "Edit host" });
        ui.add_space(6.0);
        egui::Grid::new("form")
            .num_columns(2)
            .spacing([8.0, 6.0])
            .show(ui, |ui| {
                field(ui, "Name", &mut form.name, "vps");
                field(ui, "Connection URI", &mut form.uri, "user@host:port");
                field(ui, "Identity file", &mut form.identity, "~/.ssh/id_ed25519");
                field(
                    ui,
                    "Section",
                    &mut form.section,
                    if adding { "catch-all" } else { "" },
                );
            });
        if !section_names.is_empty() {
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new("sections:").color(ui.visuals().weak_text_color()));
                for s in &section_names {
                    if ui.small_button(s).clicked() {
                        form.section = s.clone();
                    }
                }
            });
        }
        if let Some(err) = &form.error {
            ui.add_space(4.0);
            ui.colored_label(ui.visuals().error_fg_color, format!("⚠ {err}"));
        }
        if blocked {
            ui.add_space(4.0);
            ui.colored_label(
                ui.visuals().warn_fg_color,
                "Save or discard the editor first",
            );
        }
        ui.add_space(8.0);
        let mut save = false;
        let mut cancel = false;
        let mut delete = false;
        let mut clone = false;
        let mut move_to = false;
        let mut show_in_editor = false;
        ui.horizontal(|ui| {
            save = ui.add_enabled(!blocked, Button::new("Save")).clicked();
            cancel = ui.button("Cancel").clicked();
            if multi && !adding {
                show_in_editor = ui
                    .button("Show in Editor")
                    .on_hover_text("Open the file that holds this host at its Host line")
                    .clicked();
            }
        });
        if !adding {
            ui.separator();
            ui.horizontal(|ui| {
                clone = ui.add_enabled(!blocked, Button::new("Clone…")).clicked();
                move_to = ui
                    .add_enabled(!blocked, Button::new("Move to Section…"))
                    .clicked();
                delete = ui.add_enabled(!blocked, Button::new("Delete…")).clicked();
            });
        }
        if save {
            self.save_form();
        }
        if cancel {
            self.form = None;
            self.selected = None;
        }
        if delete {
            if let Some(s) = self.selected.clone() {
                self.dialog = Some(Dialog::ConfirmDelete(s));
            }
        }
        if clone {
            self.open_clone();
        }
        if show_in_editor {
            if let Some(s) = self.selected.clone() {
                self.open_in_editor(&s);
            }
        }
        if move_to {
            if let Some(s) = self.selected.clone() {
                self.dialog = Some(Dialog::Move {
                    name: s,
                    section: String::new(),
                    error: None,
                });
            }
        }
        if !adding {
            self.settings_section(ui, blocked);
        }
    }

    /// The All settings section of the detail panel: every keyword the host
    /// can set, grouped, a control per value type (docs/gui.md).
    fn settings_section(&mut self, ui: &mut Ui, blocked: bool) {
        if self.settings.is_none() || self.form.is_none() {
            return;
        }
        ui.separator();
        let mut save = false;
        let mut reset = false;
        egui::CollapsingHeader::new("All settings")
            .id_salt("all-settings")
            .show(ui, |ui| {
                let Some(draft) = self.settings.as_mut() else {
                    return;
                };
                let weak = ui.visuals().weak_text_color();
                let err = ui.visuals().error_fg_color;
                egui::ScrollArea::vertical()
                    .id_salt("all-settings-scroll")
                    .max_height(460.0)
                    .show(ui, |ui| {
                        ui.add_enabled_ui(!blocked, |ui| {
                            for group in KeyGroup::ALL {
                                egui::CollapsingHeader::new(group.title())
                                    .id_salt(("settings-group", group.title()))
                                    .default_open(false)
                                    .show(ui, |ui| {
                                        settings_group(ui, draft, group, weak, err);
                                    });
                            }
                        });
                    });
                let changes = draft.changes();
                let pending = changes.as_ref().is_ok_and(|c| !c.is_empty());
                ui.horizontal(|ui| {
                    save = ui
                        .add_enabled(!blocked && pending, Button::new("Save settings"))
                        .clicked();
                    reset = ui
                        .add_enabled(draft.rows.iter().any(|r| r.changed()), Button::new("Reset"))
                        .clicked();
                });
                if let Some(e) = &self.settings_error {
                    ui.colored_label(err, format!("⚠ {e}"));
                }
            });
        if save {
            self.save_settings();
        }
        if reset {
            if let Some(name) = self.selected.clone() {
                self.selected = None;
                let pending = self.goto_line;
                self.select(&name);
                self.goto_line = pending;
            }
        }
    }

    /// Every section name once, in load order, for the section buttons.
    fn section_names(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for s in &self.sections {
            if !out.iter().any(|o| o.eq_ignore_ascii_case(&s.name)) {
                out.push(s.name.clone());
            }
        }
        out
    }

    fn open_clone(&mut self) {
        if let Some(s) = self.selected.clone() {
            self.dialog = Some(Dialog::Clone {
                new_name: String::new(),
                source: s,
                error: None,
            });
        }
    }

    fn editor_tab(&mut self, ui: &mut Ui) {
        let current_dirty = self.file_dirty(self.current);
        ui.horizontal(|ui| {
            if self.is_multi() {
                self.file_selector(ui);
                ui.separator();
            }
            if ui.add_enabled(current_dirty, Button::new("Save")).clicked() {
                self.save_editor();
            }
            if ui
                .add_enabled(current_dirty, Button::new("Discard Changes"))
                .clicked()
            {
                self.discard_editor();
            }
            if let Some(err) = &self.editor_error {
                ui.colored_label(ui.visuals().error_fg_color, format!("⚠ {err}"));
            }
        });
        ui.separator();
        let dark = ui.visuals().dark_mode;
        let font = FontId::monospace(13.0);
        let mut layouter = |ui: &Ui, buf: &dyn egui::TextBuffer, _wrap: f32| {
            let mut job = highlight_job(buf.as_str(), dark, font.clone());
            job.wrap.max_width = f32::INFINITY;
            ui.ctx().fonts_mut(|f| f.layout_job(job))
        };
        let goto = self.goto_line.take();
        let current = self.current;
        let salt = self.ws.files[current].path.clone();
        let buffer = &mut self.buffers[current];
        let margin = egui::Margin::symmetric(4, 2);
        // Row tops within the text, laid out as the editor lays it out.
        let row_tops: Vec<f32> = layouter(ui, &*buffer, f32::INFINITY)
            .rows
            .iter()
            .map(|r| r.rect().top())
            .collect();
        let mut scroll = egui::ScrollArea::both()
            .id_salt(("editor-scroll", &salt))
            .auto_shrink(false);
        if let Some(&top) = goto.and_then(|line| row_tops.get(line.saturating_sub(1))) {
            scroll = scroll.vertical_scroll_offset(f32::from(margin.top) + top);
        }
        let mut changed = false;
        scroll.show(ui, |ui| {
            let resp = ui.add(
                TextEdit::multiline(buffer)
                    .id_salt(("editor", &salt))
                    .code_editor()
                    .margin(margin)
                    .desired_width(f32::INFINITY)
                    .desired_rows(30)
                    .layouter(&mut layouter),
            );
            set_label(ui, resp.id, "config editor");
            changed = resp.changed();
            // Room below the text for the last row to scroll to the top.
            let last = f32::from(margin.top) + row_tops.last().copied().unwrap_or(0.0);
            ui.add_space((last + ui.clip_rect().height() - resp.rect.height()).max(0.0));
            if let Some(line) = goto {
                let char_index: usize = buffer
                    .split_inclusive('\n')
                    .take(line.saturating_sub(1))
                    .map(|l| l.chars().count())
                    .sum();
                let mut state = TextEdit::load_state(ui.ctx(), resp.id).unwrap_or_default();
                state
                    .cursor
                    .set_char_range(Some(egui::text::CCursorRange::one(
                        egui::text::CCursor::new(char_index),
                    )));
                TextEdit::store_state(ui.ctx(), resp.id, state);
                resp.request_focus();
            }
        });
        if changed {
            self.editor_error = None;
        }
        if goto.is_some() {
            self.editor_line = goto;
        }
    }

    /// The popup above the editor that picks the file it shows: every file
    /// in load order, `•` on each with unsaved text; unreadable files are
    /// listed but cannot be picked.
    fn file_selector(&mut self, ui: &mut Ui) {
        let label = |app: &App, i: usize| {
            let dot = if app.file_dirty(i) { " •" } else { "" };
            format!("{}{dot}", app.ws.display(i))
        };
        let l = ui.label("File");
        let mut picked = self.current;
        let resp = egui::ComboBox::from_id_salt("editor-file")
            .selected_text(label(self, self.current))
            .width(320.0)
            .show_ui(ui, |ui| {
                for entry in &self.ws.load_order {
                    match &entry.state {
                        FileState::Loaded(i) => {
                            ui.selectable_value(&mut picked, *i, label(self, *i));
                        }
                        FileState::Unreadable(_) => {
                            ui.add_enabled(
                                false,
                                Button::selectable(
                                    false,
                                    format!("{}  cannot read", self.ws.display_path(&entry.path)),
                                ),
                            );
                        }
                    }
                }
            });
        let resp = resp.response.labelled_by(l.id);
        set_label(ui, resp.id, "editor file");
        if picked != self.current {
            self.select_file(picked);
        }
    }

    fn dialogs(&mut self, ctx: &egui::Context) {
        let Some(dialog) = self.dialog.clone() else {
            return;
        };
        let modal = egui::Modal::new(egui::Id::new("dialog"));
        let mut close = false;
        let shown = dialog.clone();
        let response = modal.show(ctx, |ui| {
            ui.set_max_width(420.0);
            match shown {
                Dialog::ConfirmDelete(name) => {
                    ui.heading(format!("Delete host {name}?"));
                    ui.label("The Host block and the comments directly above it are removed. A backup goes to the config file with a ~ suffix.");
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        let cancel = ui.button("Cancel");
                        if ui.memory(|m| m.focused().is_none()) {
                            cancel.request_focus();
                        }
                        let delete = ui.add(
                            Button::new(RichText::new("Delete").color(Color32::WHITE))
                                .fill(ui.visuals().error_fg_color),
                        );
                        if cancel.clicked() {
                            close = true;
                        }
                        if delete.clicked() {
                            self.run(Op::Delete(name.clone()));
                            close = true;
                        }
                    });
                }
                Dialog::Clone {
                    source,
                    mut new_name,
                    error,
                } => {
                    ui.heading(format!("Clone {source}"));
                    let l = ui.label("New name");
                    let resp = ui.text_edit_singleline(&mut new_name).labelled_by(l.id);
                    if ui.memory(|m| m.focused().is_none()) {
                        resp.request_focus();
                    }
                    if let Some(e) = &error {
                        ui.colored_label(ui.visuals().error_fg_color, format!("⚠ {e}"));
                    }
                    let mut go = resp.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
                    ui.horizontal(|ui| {
                        if ui.button("Cancel").clicked() {
                            close = true;
                        }
                        go |= ui.button("Clone").clicked();
                    });
                    self.dialog = Some(Dialog::Clone {
                        source: source.clone(),
                        new_name: new_name.clone(),
                        error,
                    });
                    if go && !close {
                        self.run(Op::Clone { source, new_name });
                    }
                }
                Dialog::AddSection { mut name, error } => {
                    ui.heading("New section");
                    let l = ui.label("Section name");
                    let resp = ui.text_edit_singleline(&mut name).labelled_by(l.id);
                    if ui.memory(|m| m.focused().is_none()) {
                        resp.request_focus();
                    }
                    if let Some(e) = &error {
                        ui.colored_label(ui.visuals().error_fg_color, format!("⚠ {e}"));
                    }
                    let mut go = resp.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
                    ui.horizontal(|ui| {
                        if ui.button("Cancel").clicked() {
                            close = true;
                        }
                        go |= ui.button("Add section").clicked();
                    });
                    self.dialog = Some(Dialog::AddSection {
                        name: name.clone(),
                        error,
                    });
                    if go && !close {
                        self.run(Op::AddSection { name });
                    }
                }
                Dialog::Move {
                    name,
                    mut section,
                    error,
                } => {
                    ui.heading(format!("Move {name} to section"));
                    let l = ui.label("Section");
                    ui.text_edit_singleline(&mut section).labelled_by(l.id);
                    ui.horizontal_wrapped(|ui| {
                        for s in self.section_names() {
                            if ui.small_button(&s).clicked() {
                                section = s;
                            }
                        }
                    });
                    if let Some(e) = &error {
                        ui.colored_label(ui.visuals().error_fg_color, format!("⚠ {e}"));
                    }
                    let mut go = false;
                    ui.horizontal(|ui| {
                        if ui.button("Cancel").clicked() {
                            close = true;
                        }
                        go = ui.button("Move").clicked();
                    });
                    self.dialog = Some(Dialog::Move {
                        name: name.clone(),
                        section: section.clone(),
                        error,
                    });
                    if go && !close {
                        self.run(Op::Move { name, section });
                    }
                }
                Dialog::Conflict(op) => {
                    let names = op.hosts().join(", ");
                    ui.heading(format!("{names} changed on disk"));
                    ui.label("The file was edited outside rustorm and reloaded. Overwrite applies your change on top of the reloaded file.");
                    ui.horizontal(|ui| {
                        if ui.button("Cancel").clicked() {
                            close = true;
                        }
                        if ui.button("Overwrite").clicked() {
                            self.dialog = None;
                            self.run_op(op.clone(), true);
                            close = true;
                        }
                    });
                }
                Dialog::EditorConflict => {
                    ui.heading("The config file changed on disk");
                    ui.label("Saving replaces the whole file with the editor text. Discard Changes loads the file from disk instead.");
                    ui.horizontal(|ui| {
                        if ui.button("Cancel").clicked() {
                            close = true;
                        }
                        if ui.button("Overwrite").clicked() {
                            self.dialog = None;
                            self.save_editor_inner(true);
                            close = true;
                        }
                    });
                }
                Dialog::UnsavedClose => {
                    if self.is_multi() {
                        ui.heading(format!(
                            "Save changes to {}?",
                            self.ws.display(self.current)
                        ));
                    } else {
                        ui.heading("Save changes to the config file?");
                    }
                    ui.label("The editor has unsaved changes.");
                    ui.horizontal(|ui| {
                        if ui.button("Cancel").clicked() {
                            close = true;
                        }
                        if ui.button(DISCARD_LABEL).clicked() {
                            self.dialog = None;
                            self.close_now(ctx);
                            close = true;
                        }
                        if ui.button("Save").clicked() {
                            self.dialog = None;
                            if self.save_editor() {
                                self.close_now(ctx);
                            }
                            close = true;
                        }
                    });
                }
                Dialog::UnsavedCloseAll(names) => {
                    ui.heading(format!("Save changes to {} files?", names.len()));
                    ui.label("These files have unsaved changes:");
                    for n in &names {
                        ui.label(RichText::new(format!("• {n}")).monospace());
                    }
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        let cancel = ui.button("Cancel");
                        if ui.memory(|m| m.focused().is_none()) {
                            cancel.request_focus();
                        }
                        if cancel.clicked() {
                            close = true;
                        }
                        if ui.button(DISCARD_ALL_LABEL).clicked() {
                            self.dialog = None;
                            self.close_now(ctx);
                            close = true;
                        }
                        if ui.button("Save All").clicked() {
                            self.dialog = None;
                            if self.save_all() {
                                self.close_now(ctx);
                            }
                            close = true;
                        }
                    });
                }
            }
        });
        let same = self
            .dialog
            .as_ref()
            .is_some_and(|d| std::mem::discriminant(d) == std::mem::discriminant(&dialog));
        if same && (close || response.should_close()) {
            self.dialog = None;
        }
    }
}

fn set_label(ui: &Ui, id: egui::Id, label: &str) {
    ui.ctx()
        .accesskit_node_builder(id, |b| b.set_label(label.to_string()));
}

fn field(ui: &mut Ui, label: &str, value: &mut String, hint: &str) {
    let l = ui.label(label);
    ui.add(
        TextEdit::singleline(value)
            .hint_text(hint)
            .desired_width(200.0),
    )
    .labelled_by(l.id);
    ui.end_row();
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        self.show(ui);
    }
}

/// The 1-based line of the `Host` line naming `name` in `text`.
fn buffer_host_line(text: &str, name: &str) -> Option<usize> {
    text.lines()
        .position(|l| {
            let mut words = l.split_whitespace();
            words.next().is_some_and(|w| w.eq_ignore_ascii_case("host"))
                && words.any(|w| w.trim_matches('"') == name)
        })
        .map(|i| i + 1)
}

/// One group of the All settings section: a row per value, the control
/// fitting the keyword's type, the problem with a value under it.
fn settings_group(
    ui: &mut Ui,
    draft: &mut SettingsDraft,
    group: KeyGroup,
    weak: Color32,
    err: Color32,
) {
    let mut grow = None;
    egui::Grid::new(("settings-grid", group.title()))
        .num_columns(2)
        .min_col_width(190.0)
        .spacing([8.0, 4.0])
        .show(ui, |ui| {
            for (i, row) in draft.rows.iter_mut().enumerate() {
                if row.spec.group != group {
                    continue;
                }
                let key = row.spec.key;
                let label = if row.changed() {
                    RichText::new(format!("{key} •"))
                } else {
                    RichText::new(key)
                };
                ui.label(label);
                let before = row.value.clone();
                if !row.typed() {
                    let shown = if row.value.is_empty() {
                        match &row.inherited {
                            Some(v) => format!("— ({v} from Host *)"),
                            None => "—".to_string(),
                        }
                    } else {
                        row.value.clone()
                    };
                    let resp = egui::ComboBox::from_id_salt(("setting", i))
                        .width(220.0)
                        .selected_text(shown)
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut row.value, String::new(), "not set");
                            for w in row.choices().unwrap_or_default() {
                                ui.selectable_value(&mut row.value, (*w).to_string(), *w);
                            }
                        });
                    set_label(ui, resp.response.id, key);
                } else {
                    ui.horizontal(|ui| {
                        let hint = row
                            .inherited
                            .as_ref()
                            .map(|v| format!("{v} (from Host *)"))
                            .unwrap_or_default();
                        let resp = ui.add(
                            TextEdit::singleline(&mut row.value)
                                .hint_text(RichText::new(hint).color(weak))
                                .desired_width(220.0),
                        );
                        set_label(ui, resp.id, key);
                        if let Some(words) = row.choices() {
                            ui.menu_button("▾", |ui| {
                                for w in words {
                                    if ui.button(*w).clicked() {
                                        row.value = (*w).to_string();
                                        ui.close();
                                    }
                                }
                            });
                        }
                        if row.spec.multi && !row.value.is_empty() && ui.small_button("−").clicked()
                        {
                            row.value.clear();
                        }
                    });
                }
                ui.end_row();
                if let Some(p) = row.problem() {
                    ui.label("");
                    ui.colored_label(err, format!("{key} {p}"));
                    ui.end_row();
                }
                if row.value != before {
                    grow = Some(i);
                }
            }
        });
    if let Some(i) = grow {
        draft.grow(i);
    }
}
