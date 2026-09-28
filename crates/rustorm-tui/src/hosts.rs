//! Host table rows, sorting and filtering (docs/tui.md, Sorting and
//! filtering).

use std::cmp::Ordering;

use rustorm_core::{Config, Env};

/// A host table column.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Column {
    /// The host's section.
    Section,
    /// The primary name.
    Host,
    /// The host's own `User`.
    User,
    /// `HostName`.
    HostName,
    /// The host's own `Port`.
    Port,
    /// `ProxyCommand` ("proxies").
    Proxy,
    /// `ProxyJump` ("jump machines").
    Jump,
}

impl Column {
    /// Every column in display order; key `1` is the first.
    pub const ALL: [Column; 7] = [
        Column::Section,
        Column::Host,
        Column::User,
        Column::HostName,
        Column::Port,
        Column::Proxy,
        Column::Jump,
    ];

    /// Position in [`Column::ALL`].
    pub fn index(self) -> usize {
        self as usize
    }

    /// The column for a digit key `1`..`7`.
    pub fn from_digit(c: char) -> Option<Column> {
        let d = c.to_digit(10)? as usize;
        (1..=7).contains(&d).then(|| Column::ALL[d - 1])
    }

    /// Header text.
    pub fn title(self) -> &'static str {
        match self {
            Column::Section => "Section",
            Column::Host => "Host",
            Column::User => "User",
            Column::HostName => "HostName",
            Column::Port => "Port",
            Column::Proxy => "Proxy",
            Column::Jump => "Jump",
        }
    }

    /// Name used in the filter summary (`section~bob`).
    pub fn key(self) -> &'static str {
        match self {
            Column::Section => "section",
            Column::Host => "host",
            Column::User => "user",
            Column::HostName => "hostname",
            Column::Port => "port",
            Column::Proxy => "proxy",
            Column::Jump => "jump",
        }
    }
}

/// One host as the table shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    /// Primary name.
    pub name: String,
    /// Section, `None` in an unsectioned file or the preamble.
    pub section: Option<String>,
    /// The host's own `User`.
    pub user: Option<String>,
    /// `HostName`.
    pub hostname: Option<String>,
    /// The host's own `Port`.
    pub port: Option<String>,
    /// `ProxyCommand`.
    pub proxy: Option<String>,
    /// `ProxyJump`.
    pub jump: Option<String>,
    /// `name -> user@hostname:port`, user and port resolved as `list` does.
    pub target: String,
}

impl Row {
    /// The value in `column`, `None` when missing.
    pub fn get(&self, column: Column) -> Option<&str> {
        match column {
            Column::Section => self.section.as_deref(),
            Column::Host => Some(&self.name),
            Column::User => self.user.as_deref(),
            Column::HostName => self.hostname.as_deref(),
            Column::Port => self.port.as_deref(),
            Column::Proxy => self.proxy.as_deref(),
            Column::Jump => self.jump.as_deref(),
        }
    }
}

/// Builds the rows in file order (the order of `rustorm list`).
pub fn rows(config: &Config, env: &Env) -> Vec<Row> {
    config
        .list(env)
        .into_iter()
        .map(|r| {
            let own = config.find_primary(&r.name).map(|l| config.host(l));
            let get = |k: &str| own.and_then(|h| h.get(k));
            Row {
                target: r.line(),
                user: get("User"),
                port: get("Port"),
                name: r.name,
                section: r.section,
                hostname: r.hostname,
                proxy: r.proxy_command,
                jump: r.proxy_jump,
            }
        })
        .collect()
}

/// The active sort.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sort {
    /// The sorted column.
    pub column: Column,
    /// Ascending when true.
    pub ascending: bool,
}

fn cmp_text(a: &str, b: &str) -> Ordering {
    a.to_lowercase()
        .cmp(&b.to_lowercase())
        .then_with(|| a.cmp(b))
}

/// Orders two rows: missing values last in both directions, ties by name.
pub fn compare(a: &Row, b: &Row, sort: Sort) -> Ordering {
    let ord = match (a.get(sort.column), b.get(sort.column)) {
        (None, None) => Ordering::Equal,
        (None, Some(_)) => Ordering::Greater,
        (Some(_), None) => Ordering::Less,
        (Some(x), Some(y)) => {
            let o = match (sort.column, x.parse::<u64>(), y.parse::<u64>()) {
                (Column::Port, Ok(p), Ok(q)) => p.cmp(&q),
                _ => cmp_text(x, y),
            };
            if sort.ascending {
                o
            } else {
                o.reverse()
            }
        }
    };
    ord.then_with(|| cmp_text(&a.name, &b.name))
}

fn contains_ci(haystack: &str, needle: &str) -> bool {
    haystack.to_lowercase().contains(&needle.to_lowercase())
}

/// Per-column filters plus a global one; non-empty filters AND together.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Filters {
    /// One filter per column, indexed by [`Column::index`].
    pub columns: [String; 7],
    /// Matches any column.
    pub global: String,
}

impl Filters {
    /// The filter text for `column`.
    pub fn get(&self, column: Column) -> &str {
        &self.columns[column.index()]
    }

    /// True when no filter is set.
    pub fn is_empty(&self) -> bool {
        self.global.is_empty() && self.columns.iter().all(String::is_empty)
    }

    /// True when `row` passes every non-empty filter.
    pub fn matches(&self, row: &Row) -> bool {
        for c in Column::ALL {
            let f = self.get(c);
            if !f.is_empty() && !row.get(c).is_some_and(|v| contains_ci(v, f)) {
                return false;
            }
        }
        self.global.is_empty()
            || Column::ALL
                .iter()
                .any(|c| row.get(*c).is_some_and(|v| contains_ci(v, &self.global)))
    }

    /// `section~bob user~deploy any~x`, or empty.
    pub fn describe(&self) -> String {
        let mut parts: Vec<String> = Column::ALL
            .iter()
            .filter(|c| !self.get(**c).is_empty())
            .map(|c| format!("{}~{}", c.key(), self.get(*c)))
            .collect();
        if !self.global.is_empty() {
            parts.push(format!("any~{}", self.global));
        }
        parts.join(" ")
    }
}
