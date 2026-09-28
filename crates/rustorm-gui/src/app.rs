//! [`App`]: the whole rustorm window as an `eframe::App`.

use std::path::{Path, PathBuf};

use egui::{
    Button, Color32, FontId, Key, KeyboardShortcut, Modifiers, RichText, Sense, TextEdit, Ui,
    ViewportCommand,
};
use egui_extras::{Column as TableColumn, TableBuilder};
use rustorm_core::{backup_path, Config, ConfigFile, Env, SectionSummary, WriteOptions};

use crate::highlight::highlight_job;
use crate::ops::{host_text, Op};
use crate::rows::{rows, sort_rows, Column, Filters, HostRow, SortDir};

/// The label of the button that closes without saving: "Don't Save" on
/// macOS, "Discard" elsewhere.
pub const DISCARD_LABEL: &str = if cfg!(target_os = "macos") {
    "Don't Save"
} else {
    "Discard"
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
    /// Close requested with unsaved editor text.
    UnsavedClose,
}

/// The rustorm desktop app. Build it with [`App::new`] from a config path
/// and run it with eframe, or drive [`App::show`] from any `egui::Ui`.
pub struct App {
    file: ConfigFile,
    env: Env,
    rows: Vec<HostRow>,
    sections: Vec<SectionSummary>,
    /// The selected tab.
    pub tab: Tab,
    /// The sorted column and direction; `None` keeps the file order.
    pub sort: Option<(Column, SortDir)>,
    /// Every filter.
    pub filters: Filters,
    selected: Option<String>,
    form: Option<Form>,
    dialog: Option<Dialog>,
    editor_text: String,
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
    /// Loads `path` (a missing file opens empty and is not created) with
    /// the process's `$USER` and home directory.
    pub fn new(path: impl Into<PathBuf>) -> anyhow::Result<App> {
        App::with_env(path, Env::from_process())
    }

    /// Like [`App::new`] with an explicit environment.
    pub fn with_env(path: impl Into<PathBuf>, env: Env) -> anyhow::Result<App> {
        let file = ConfigFile::load(path.into())?;
        let mut app = App {
            editor_text: file.original.clone(),
            file,
            env,
            rows: Vec::new(),
            sections: Vec::new(),
            tab: Tab::Hosts,
            sort: None,
            filters: Filters::default(),
            selected: None,
            form: None,
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

    /// The config file path.
    pub fn path(&self) -> &Path {
        &self.file.path
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

    /// The editor buffer.
    pub fn editor_text(&self) -> &str {
        &self.editor_text
    }

    /// Replaces the editor buffer, as typing would.
    pub fn set_editor_text(&mut self, text: impl Into<String>) {
        self.editor_text = text.into();
    }

    /// The editor's last refusal ("line N: cannot parse: …").
    pub fn editor_error(&self) -> Option<&str> {
        self.editor_error.as_deref()
    }

    /// True when the editor buffer differs from the file as last loaded.
    pub fn editor_dirty(&self) -> bool {
        self.editor_text != self.file.original
    }

    /// The status-bar message of the last operation.
    pub fn status(&self) -> &str {
        &self.status
    }

    /// The backup the last write made.
    pub fn last_backup(&self) -> Option<&Path> {
        self.last_backup.as_deref()
    }

    /// The window title.
    pub fn title(&self) -> String {
        format!(
            "rustorm — {}{}",
            self.file.path.display(),
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
        self.rows = rows(&self.file.config, &self.env);
        self.sections = self.file.config.sections();
        if let Some(sel) = &self.selected {
            if !self.rows.iter().any(|r| &r.name == sel) {
                self.selected = None;
            }
        }
    }

    fn disk_text(&self) -> std::io::Result<String> {
        match std::fs::read_to_string(&self.file.path) {
            Ok(t) => Ok(t),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
            Err(e) => Err(e),
        }
    }

    fn after_write(&mut self, existed: bool) {
        let backup = backup_path(&self.file.path);
        if existed && backup.exists() {
            self.last_backup = Some(backup);
        }
        self.editor_text = self.file.original.clone();
        self.editor_error = None;
        self.refresh();
    }

    /// Selects `name` and opens it in the detail form.
    pub fn select(&mut self, name: &str) {
        self.selected = Some(name.to_string());
        self.form = self.rows.iter().find(|r| r.name == name).map(Form::edit);
    }

    /// Opens the empty add form.
    pub fn open_add(&mut self) {
        self.selected = None;
        self.form = Some(Form::add());
        self.tab = Tab::Hosts;
    }

    /// Runs `op` through the core and writes the file, reloading first when
    /// the file changed on disk. Returns true when the file was written.
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

    fn run_op(&mut self, op: Op, force: bool) -> bool {
        let disk = match self.disk_text() {
            Ok(t) => t,
            Err(e) => {
                self.fail(format!("cannot read {}: {e}", self.file.path.display()));
                return false;
            }
        };
        if disk != self.file.original {
            let fresh = match ConfigFile::load(self.file.path.clone()) {
                Ok(f) => f,
                Err(e) => {
                    self.fail(e.to_string());
                    return false;
                }
            };
            let conflict = !force
                && op
                    .hosts()
                    .iter()
                    .any(|n| host_text(&self.file.config, n) != host_text(&fresh.config, n));
            let editor_clean = !self.editor_dirty();
            self.file = fresh;
            if editor_clean {
                self.editor_text = self.file.original.clone();
            }
            self.refresh();
            self.status = "reloaded: the file changed on disk.".to_string();
            if conflict {
                self.dialog = Some(Dialog::Conflict(op));
                return false;
            }
        }
        let mut config = self.file.config.clone();
        let outcome = match op.apply(&mut config, &self.env) {
            Ok(o) => o,
            Err(e) => {
                self.fail(e.to_string());
                return false;
            }
        };
        let existed = self.file.existed;
        let previous = std::mem::replace(&mut self.file.config, config);
        if let Err(e) = self.file.save(WriteOptions::default()) {
            self.file.config = previous;
            self.fail(e.to_string());
            return false;
        }
        self.after_write(existed);
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

    /// Saves the editor buffer through the core. Refuses an unparsable
    /// line with its number; asks before replacing a file changed on disk.
    pub fn save_editor(&mut self) -> bool {
        self.save_editor_inner(false)
    }

    fn save_editor_inner(&mut self, force: bool) -> bool {
        let config = match Config::parse(&self.editor_text) {
            Ok(c) => c,
            Err(e) => {
                self.editor_error = Some(e.to_string());
                return false;
            }
        };
        if let Some((i, line)) = config.lines().enumerate().find(|(_, l)| l.is_unparsable()) {
            let message = format!("line {}: cannot parse: {}", i + 1, line.text().trim());
            self.status = format!("error: {message}");
            self.editor_error = Some(message);
            return false;
        }
        match self.disk_text() {
            Ok(disk) if disk != self.file.original && !force => {
                self.dialog = Some(Dialog::EditorConflict);
                return false;
            }
            Ok(_) => {}
            Err(e) => {
                self.editor_error = Some(format!("cannot read {}: {e}", self.file.path.display()));
                return false;
            }
        }
        let mut config = config;
        config.sort_sections();
        let text = config.render();
        let existed = self.file.existed;
        if let Err(e) = self.file.save_text(&text, WriteOptions::default()) {
            self.editor_error = Some(e.to_string());
            return false;
        }
        self.after_write(existed);
        self.status = "saved.".to_string();
        if let Some(sel) = self.selected.clone() {
            self.select(&sel);
        }
        true
    }

    /// Restores the editor buffer from the file on disk.
    pub fn discard_editor(&mut self) {
        match ConfigFile::load(self.file.path.clone()) {
            Ok(f) => {
                self.file = f;
                self.editor_text = self.file.original.clone();
                self.editor_error = None;
                self.refresh();
                self.status = "changes discarded.".to_string();
            }
            Err(e) => self.status = format!("error: {e}"),
        }
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
        if self.editor_dirty() && !self.allow_close {
            ctx.send_viewport_cmd(ViewportCommand::CancelClose);
            self.dialog = Some(Dialog::UnsavedClose);
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
                if ui
                    .add_enabled(self.editor_dirty(), Button::new("Discard Changes"))
                    .clicked()
                {
                    self.discard_editor();
                }
                if ui
                    .add_enabled(!self.editor_dirty(), Button::new("Reload from Disk"))
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
        ui.horizontal(|ui| {
            ui.label(self.file.path.display().to_string());
            ui.separator();
            if self.editor_dirty() {
                ui.label(RichText::new("● unsaved changes").color(ui.visuals().warn_fg_color));
            } else {
                ui.label("saved");
            }
            ui.separator();
            match &self.last_backup {
                Some(b) => ui.label(format!("last backup: {}", b.display())),
                None => ui.label("no backup yet"),
            };
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
        ui.heading("Sections");
        let total = self.rows.len();
        if ui
            .selectable_label(
                self.filters.sidebar.is_none(),
                format!("All hosts  {total}"),
            )
            .clicked()
        {
            self.filters.sidebar = None;
        }
        for s in self.sections.clone() {
            let selected = self.filters.sidebar.as_deref() == Some(s.name.as_str());
            let text = if s.catch_all {
                format!("{}  {}  (catch-all)", s.name, s.hosts)
            } else {
                format!("{}  {}", s.name, s.hosts)
            };
            if ui.selectable_label(selected, text).clicked() {
                self.filters.sidebar = if selected { None } else { Some(s.name.clone()) };
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
        TableBuilder::new(ui)
            .id_salt("hosts")
            .striped(true)
            .sense(Sense::click())
            .columns(TableColumn::auto().at_least(70.0).resizable(true), 6)
            .column(TableColumn::remainder().at_least(80.0))
            .header(50.0, |mut header| {
                for column in Column::ALL {
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
                        for column in Column::ALL {
                            tr.col(|ui| {
                                if column == Column::Host {
                                    ui.label(&row.name);
                                    if !row.aliases.is_empty() {
                                        ui.label(RichText::new(row.aliases.join(" ")).color(weak));
                                    }
                                    return;
                                }
                                let text = row.display(column);
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
        let section_names: Vec<String> = self.sections.iter().map(|s| s.name.clone()).collect();
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
        ui.horizontal(|ui| {
            save = ui.add_enabled(!blocked, Button::new("Save")).clicked();
            cancel = ui.button("Cancel").clicked();
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
        if move_to {
            if let Some(s) = self.selected.clone() {
                self.dialog = Some(Dialog::Move {
                    name: s,
                    section: String::new(),
                    error: None,
                });
            }
        }
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
        let blocked_save = !self.editor_dirty();
        ui.horizontal(|ui| {
            if ui.add_enabled(!blocked_save, Button::new("Save")).clicked() {
                self.save_editor();
            }
            if ui
                .add_enabled(!blocked_save, Button::new("Discard Changes"))
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
        let mut layouter = |ui: &Ui, buf: &dyn egui::TextBuffer, _wrap: f32| {
            let mut job = highlight_job(buf.as_str(), dark, FontId::monospace(13.0));
            job.wrap.max_width = f32::INFINITY;
            ui.ctx().fonts_mut(|f| f.layout_job(job))
        };
        egui::ScrollArea::both().auto_shrink(false).show(ui, |ui| {
            let resp = ui.add(
                TextEdit::multiline(&mut self.editor_text)
                    .code_editor()
                    .desired_width(f32::INFINITY)
                    .desired_rows(30)
                    .layouter(&mut layouter),
            );
            set_label(ui, resp.id, "config editor");
            if resp.changed() {
                self.editor_error = None;
            }
        });
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
                        for s in &self.sections {
                            if ui.small_button(&s.name).clicked() {
                                section = s.name.clone();
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
                    ui.heading("Save changes to the config file?");
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
