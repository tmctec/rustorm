//! The Conflicts view's state: every host defined in two or more workspace
//! files as the core's `reconcile` report pairs it (docs/cli.md,
//! reconcile).

use std::path::PathBuf;

use rustorm_core::{is_meta_key, Pair, PairKind, Pick, ReconcileReport, Workspace};

/// What the view shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum View {
    /// Every pair, one per row.
    List,
    /// The highlighted pair's two blocks side by side.
    Pair,
    /// Per-key picks for the highlighted conflict, one per differing key
    /// so far.
    Keys(Vec<Pick>),
}

/// One row of the list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Item {
    /// Item `.1` of report `.0`.
    Pair(usize, usize),
    /// A copy file seen this session with nothing left in it to decide;
    /// only `R` applies.
    File(usize),
}

/// The Conflicts view.
#[derive(Debug, Clone)]
pub(crate) struct Conflicts {
    /// `reconcile_report(None)` first, then one report per copy file
    /// holding orphans, scoped to that file.
    pub reports: Vec<ReconcileReport>,
    /// Conflicts, then identical pairs, then orphans, then emptied files.
    pub rows: Vec<Item>,
    pub sel: usize,
    /// Every conflict and orphan decided this session, for retiring.
    pub decided: Vec<String>,
    /// Every copy file seen this session, so its orphans stay listed once
    /// its copies are gone.
    pub copy_files: Vec<PathBuf>,
    pub view: View,
    /// The file `R` asks to retire.
    pub confirm: Option<usize>,
    /// The key overlay is open over the view.
    pub help: bool,
}

impl Conflicts {
    /// The view over `ws`, read fresh.
    pub fn new(ws: &Workspace) -> Conflicts {
        let mut c = Conflicts {
            reports: Vec::new(),
            rows: Vec::new(),
            sel: 0,
            decided: Vec::new(),
            copy_files: Vec::new(),
            view: View::List,
            confirm: None,
            help: false,
        };
        c.report(ws);
        c
    }

    /// Reads the pairs again from `ws`, keeping the highlight's position.
    pub fn report(&mut self, ws: &Workspace) {
        let main = ws.reconcile_report(None);
        for p in &main.items {
            let path = ws.files[p.copy.index].path.clone();
            if p.copy.index != 0 && !self.copy_files.contains(&path) {
                self.copy_files.push(path);
            }
        }
        let files: Vec<usize> = self
            .copy_files
            .iter()
            .filter_map(|p| ws.files.iter().position(|f| &f.path == p))
            .collect();
        self.copy_files
            .retain(|p| ws.files.iter().any(|f| &f.path == p));
        let mut reports = vec![main];
        for &f in &files {
            let r = ws.reconcile_report(Some(&[f]));
            if r.orphans > 0 {
                reports.push(r);
            }
        }
        let mut rows = Vec::new();
        for kind in [PairKind::Conflict, PairKind::Identical, PairKind::Orphan] {
            for (ri, r) in reports.iter().enumerate() {
                for (ii, p) in r.items.iter().enumerate() {
                    if p.kind == kind && (ri == 0) == (kind != PairKind::Orphan) {
                        rows.push(Item::Pair(ri, ii));
                    }
                }
            }
        }
        for &f in &files {
            let listed = rows.iter().any(|it| match *it {
                Item::Pair(ri, ii) => reports[ri].items[ii].copy.index == f,
                Item::File(_) => false,
            });
            if !listed {
                rows.push(Item::File(f));
            }
        }
        self.reports = reports;
        self.rows = rows;
        self.sel = self.sel.min(self.rows.len().saturating_sub(1));
        if self.pair().is_none() {
            self.view = View::List;
        }
    }

    /// The highlighted row.
    pub fn item(&self) -> Option<Item> {
        self.rows.get(self.sel).copied()
    }

    /// The pair of `item`.
    pub fn pair_of(&self, item: Item) -> Option<&Pair> {
        match item {
            Item::Pair(ri, ii) => self.reports.get(ri)?.items.get(ii),
            Item::File(_) => None,
        }
    }

    /// The highlighted pair.
    pub fn pair(&self) -> Option<&Pair> {
        self.pair_of(self.item()?)
    }

    /// The copy file of the highlighted row.
    pub fn copy_file(&self) -> Option<usize> {
        match self.item()? {
            Item::File(f) => Some(f),
            it => self.pair_of(it).map(|p| p.copy.index),
        }
    }

    /// True when `p` was decided this session.
    pub fn is_decided(&self, p: &Pair) -> bool {
        self.decided.iter().any(|d| p.answers_to(d))
    }

    /// A pair's kind as the list shows it.
    pub fn class(&self, p: &Pair) -> String {
        match p.kind {
            PairKind::Identical => "identical".into(),
            PairKind::Conflict => {
                let mut s = String::from("conflict");
                if !p.keys.is_empty() && p.keys.iter().all(|k| is_meta_key(&k.key)) {
                    s.push_str(", labels only");
                }
                if self.is_decided(p) {
                    s.push_str(", kept live");
                }
                s
            }
            PairKind::Orphan if self.is_decided(p) => "orphan, left out".into(),
            PairKind::Orphan => "orphan".into(),
        }
    }

    /// Listed conflicts not decided this session.
    pub fn remaining(&self) -> usize {
        self.rows
            .iter()
            .filter_map(|it| self.pair_of(*it))
            .filter(|p| p.kind == PairKind::Conflict && !self.is_decided(p))
            .count()
    }

    /// `2 conflicts, 1 identical, 1 orphan` over the listed rows.
    pub fn summary(&self) -> String {
        let count = |k: PairKind| {
            self.rows
                .iter()
                .filter_map(|it| self.pair_of(*it))
                .filter(|p| p.kind == k)
                .count()
        };
        let plural =
            |n: usize, one: &str, many: &str| format!("{n} {}", if n == 1 { one } else { many });
        format!(
            "{}, {} identical, {}",
            plural(count(PairKind::Conflict), "conflict", "conflicts"),
            count(PairKind::Identical),
            plural(count(PairKind::Orphan), "orphan", "orphans")
        )
    }
}

/// A block line as compared for highlighting: whitespace collapsed, the
/// keyword lowercased.
fn norm(line: &str) -> String {
    let mut words = line.split_whitespace();
    match words.next() {
        Some(first) => std::iter::once(first.to_ascii_lowercase())
            .chain(words.map(str::to_string))
            .collect::<Vec<_>>()
            .join(" "),
        None => String::new(),
    }
}

/// Each line of `text`, and whether `other` lacks it (blank lines never
/// count as differing).
pub(crate) fn marked_lines(text: &str, other: &str) -> Vec<(String, bool)> {
    let theirs: Vec<String> = other.lines().map(norm).collect();
    text.lines()
        .map(|l| {
            let n = norm(l);
            let differs = !n.is_empty() && !theirs.contains(&n);
            (l.to_string(), differs)
        })
        .collect()
}
