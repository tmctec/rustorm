//! The host table's rows, and how they sort and filter.
//!
//! Rows come from the core's `list`; sorting and filtering are pure
//! functions here so the table widget only draws.

use std::cmp::Ordering;

use rustorm_core::{Config, Env, HostMeta, Workspace};

/// A column of the host table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Column {
    /// The file that holds the host; shown only on a workspace of several
    /// files.
    File,
    /// The host's section.
    Section,
    /// The primary name.
    Host,
    /// `User`.
    User,
    /// `HostName`.
    HostName,
    /// `Port`.
    Port,
    /// `ProxyCommand` ("proxies").
    Proxy,
    /// `ProxyJump` ("jump machines").
    Jump,
}

impl Column {
    /// Every column in display order.
    pub const ALL: [Column; 8] = [
        Column::File,
        Column::Section,
        Column::Host,
        Column::User,
        Column::HostName,
        Column::Port,
        Column::Proxy,
        Column::Jump,
    ];

    /// The header text.
    pub fn title(self) -> &'static str {
        match self {
            Column::File => "file",
            Column::Section => "section",
            Column::Host => "host",
            Column::User => "user",
            Column::HostName => "hostname",
            Column::Port => "port",
            Column::Proxy => "proxy",
            Column::Jump => "jump",
        }
    }

    /// The columns the table shows: every one on a workspace of several
    /// files, all but [`Column::File`] on a workspace of one.
    pub fn shown(multi: bool) -> &'static [Column] {
        if multi {
            &Column::ALL
        } else {
            &Column::ALL[1..]
        }
    }

    /// Position in [`Column::ALL`].
    pub fn index(self) -> usize {
        Column::ALL.iter().position(|c| *c == self).unwrap_or(0)
    }
}

/// Sort direction of the sorted column.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortDir {
    /// A to Z, low to high.
    Ascending,
    /// Z to A, high to low.
    Descending,
}

/// One host as the table shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostRow {
    /// Index into the workspace's files of the file that holds the host.
    pub file: usize,
    /// That file's name (`cypress`); empty on a workspace of one file.
    pub file_name: String,
    /// Primary name.
    pub name: String,
    /// Names after the first.
    pub aliases: Vec<String>,
    /// Section, `None` in an unsectioned file.
    pub section: Option<String>,
    /// The host's own `User`.
    pub user: Option<String>,
    /// `User` after `Host *` and `$USER` (empty when unknown).
    pub user_effective: String,
    /// `HostName`.
    pub hostname: Option<String>,
    /// The host's own `Port`.
    pub port: Option<u16>,
    /// `Port` after `Host *` and 22.
    pub port_effective: u16,
    /// `ProxyCommand`.
    pub proxy: Option<String>,
    /// `ProxyJump`.
    pub jump: Option<String>,
    /// First `IdentityFile`.
    pub identity: Option<String>,
    /// The host's metadata comments (docs/cli.md, Host metadata).
    pub meta: HostMeta,
}

impl HostRow {
    /// The value used for sorting: the host's own value, `None` when absent.
    pub fn own(&self, column: Column) -> Option<String> {
        match column {
            Column::File => (!self.file_name.is_empty()).then(|| self.file_name.clone()),
            Column::Section => self.section.clone(),
            Column::Host => Some(self.name.clone()),
            Column::User => self.user.clone(),
            Column::HostName => self.hostname.clone(),
            Column::Port => self.port.map(|p| p.to_string()),
            Column::Proxy => self.proxy.clone(),
            Column::Jump => self.jump.clone(),
        }
    }

    /// The text the cell shows, inherited user and port included. Filters
    /// match this text.
    pub fn display(&self, column: Column) -> String {
        match column {
            Column::User => self.user_effective.clone(),
            Column::Port => self.port_effective.to_string(),
            Column::Host if !self.aliases.is_empty() => {
                format!("{} {}", self.name, self.aliases.join(" "))
            }
            c => self.own(c).unwrap_or_default(),
        }
    }

    /// True when the cell shows a value inherited from `Host *` or the
    /// environment rather than the host's own.
    pub fn inherited(&self, column: Column) -> bool {
        match column {
            Column::User => self.user.is_none() && !self.user_effective.is_empty(),
            Column::Port => self.port.is_none(),
            _ => false,
        }
    }

    /// `[user@]hostname[:port]` for the edit form, from the host's own
    /// `User` and `Port` so inherited values stay inherited.
    pub fn uri(&self) -> String {
        let host = self.hostname.clone().unwrap_or_else(|| self.name.clone());
        let host = if host.contains(':') {
            format!("[{host}]")
        } else {
            host
        };
        let mut uri = String::new();
        if let Some(user) = &self.user {
            uri.push_str(user);
            uri.push('@');
        }
        uri.push_str(&host);
        if let Some(port) = self.port {
            uri.push(':');
            uri.push_str(&port.to_string());
        }
        uri
    }
}

/// Builds the rows of a single config from the core's `list`, in its
/// order: sections in file order, hosts alphabetical inside each.
pub fn rows(config: &Config, env: &Env) -> Vec<HostRow> {
    config
        .list(env)
        .into_iter()
        .map(|r| host_row(config, r, 0, String::new()))
        .collect()
}

/// Builds the rows of every file of `ws` from the core's `list`: files in
/// load order, then sections in file order, hosts alphabetical inside
/// each. The file name is filled only on a workspace of several files.
pub fn workspace_rows(ws: &Workspace, env: &Env) -> Vec<HostRow> {
    let multi = ws.is_multi();
    ws.list(env)
        .into_iter()
        .map(|w| {
            let name = if multi {
                ws.file_name(w.file)
            } else {
                String::new()
            };
            host_row(&ws.files[w.file].config, w.row, w.file, name)
        })
        .collect()
}

fn host_row(config: &Config, r: rustorm_core::ListRow, file: usize, file_name: String) -> HostRow {
    let block = config.find_primary(&r.name).map(|l| config.host(l));
    let own = |key: &str| block.and_then(|b| b.get(key));
    HostRow {
        file,
        file_name,
        user: own("User"),
        port: own("Port").and_then(|p| p.parse().ok()),
        identity: own("IdentityFile"),
        name: r.name,
        aliases: r.aliases,
        section: r.section,
        user_effective: r.user,
        hostname: r.hostname,
        port_effective: r.port,
        proxy: r.proxy_command,
        jump: r.proxy_jump,
        meta: r.meta,
    }
}

fn compare(column: Column, a: &str, b: &str) -> Ordering {
    if column == Column::Port {
        if let (Ok(x), Ok(y)) = (a.parse::<u32>(), b.parse::<u32>()) {
            return x.cmp(&y);
        }
    }
    a.to_lowercase()
        .cmp(&b.to_lowercase())
        .then_with(|| a.cmp(b))
}

/// Sorts `rows` by `column` in `dir`. Rows missing the value go last in
/// both directions; ties keep their current order.
pub fn sort_rows(rows: &mut [HostRow], column: Column, dir: SortDir) {
    rows.sort_by(|a, b| match (a.own(column), b.own(column)) {
        (None, None) => Ordering::Equal,
        (None, Some(_)) => Ordering::Greater,
        (Some(_), None) => Ordering::Less,
        (Some(x), Some(y)) => {
            let o = compare(column, &x, &y);
            match dir {
                SortDir::Ascending => o,
                SortDir::Descending => o.reverse(),
            }
        }
    });
}

/// The filters above the table.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Filters {
    /// One text per column, indexed by [`Column::index`].
    pub columns: [String; 8],
    /// Matches any column.
    pub global: String,
    /// The section picked in the sidebar, matched exactly.
    pub sidebar: Option<String>,
    /// The file of the picked section row, on a workspace of several
    /// files: the section is matched in that file only.
    pub section_file: Option<usize>,
    /// The file picked in the sidebar's Files list.
    pub file: Option<usize>,
}

impl Filters {
    /// The filter text of `column`.
    pub fn get(&self, column: Column) -> &str {
        &self.columns[column.index()]
    }

    /// Mutable filter text of `column`.
    pub fn get_mut(&mut self, column: Column) -> &mut String {
        &mut self.columns[column.index()]
    }

    /// True when any filter is set.
    pub fn active(&self) -> bool {
        self.sidebar.is_some()
            || self.file.is_some()
            || !self.global.trim().is_empty()
            || self.columns.iter().any(|c| !c.trim().is_empty())
    }

    /// Clears every filter.
    pub fn clear(&mut self) {
        *self = Filters::default();
    }

    /// True when `row` passes every filter (they AND together).
    pub fn matches(&self, row: &HostRow) -> bool {
        if let Some(s) = &self.sidebar {
            if row.section.as_deref() != Some(s.as_str()) {
                return false;
            }
            if self.section_file.is_some_and(|f| f != row.file) {
                return false;
            }
        }
        if self.file.is_some_and(|f| f != row.file) {
            return false;
        }
        let contains =
            |hay: &str, needle: &str| hay.to_lowercase().contains(&needle.to_lowercase());
        for column in Column::ALL {
            let needle = self.get(column).trim();
            if !needle.is_empty() && !contains(&row.display(column), needle) {
                return false;
            }
        }
        let global = self.global.trim();
        global.is_empty()
            || Column::ALL
                .iter()
                .any(|c| contains(&row.display(*c), global))
            || row.meta.lines().iter().any(|l| contains(l, global))
    }
}
