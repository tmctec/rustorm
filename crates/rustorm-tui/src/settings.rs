//! The settings form: the keywords a host sets, or every keyword it can
//! set, grouped, with a control per value type and an Add setting row
//! (docs/tui.md, Settings form).

use rustorm_core::{premade_value, HostBlock, KeyGroup, SettingChange, SettingRow, SettingsDraft};

/// The open settings form of one host.
#[derive(Debug, Clone)]
pub(crate) struct SettingsForm {
    pub draft: SettingsDraft,
    /// Index into `draft.rows`; `None` on the Add setting row.
    pub focus: Option<usize>,
    /// Every keyword shows, not only the filled ones.
    pub all: bool,
    /// The text typed into Add setting.
    pub adding: String,
    /// The focused value is a premade one, selected: typing replaces it.
    pub fresh: bool,
    pub error: Option<String>,
}

impl SettingsForm {
    /// The form in the filled view, focus on the first filled row.
    pub fn new(host: &str, block: &HostBlock, defaults: Option<&HostBlock>) -> SettingsForm {
        let draft = SettingsDraft::new(host, block, defaults);
        let focus = draft.filled_rows().first().copied();
        SettingsForm {
            draft,
            focus,
            all: false,
            adding: String::new(),
            fresh: false,
            error: None,
        }
    }

    /// The rows the view shows, in order: every row, or the filled ones
    /// plus the focused one.
    pub fn shown(&self) -> Vec<usize> {
        if self.all {
            return (0..self.draft.rows.len()).collect();
        }
        (0..self.draft.rows.len())
            .filter(|&i| self.draft.rows[i].filled() || Some(i) == self.focus)
            .collect()
    }

    /// The focused row; `None` on Add setting.
    pub fn row(&self) -> Option<&SettingRow> {
        self.focus.map(|i| &self.draft.rows[i])
    }

    fn row_mut(&mut self) -> Option<&mut SettingRow> {
        self.error = None;
        self.focus.map(|i| &mut self.draft.rows[i])
    }

    /// The focus as a position in `shown()`, Add setting being one past
    /// the last row.
    fn position(&self, shown: &[usize]) -> usize {
        match self.focus {
            Some(f) => shown.iter().position(|&i| i == f).unwrap_or(0),
            None => shown.len(),
        }
    }

    fn set_position(&mut self, shown: &[usize], p: usize) {
        self.focus = shown.get(p).copied();
        if self.focus.is_none() && self.all {
            self.focus = shown.last().copied();
        }
    }

    pub fn move_by(&mut self, delta: isize) {
        let shown = self.shown();
        let last = shown.len() as isize - isize::from(self.all);
        let p = (self.position(&shown) as isize + delta).clamp(0, last.max(0));
        self.set_position(&shown, p as usize);
    }

    pub fn first(&mut self) {
        let shown = self.shown();
        self.set_position(&shown, 0);
    }

    pub fn last(&mut self) {
        let shown = self.shown();
        self.set_position(&shown, shown.len());
    }

    /// Switches between the filled and the all view, keeping edits. The
    /// filled view focuses the first filled row when the focused one is
    /// not filled; the all view has no Add setting row.
    pub fn toggle(&mut self) {
        self.all = !self.all;
        self.adding.clear();
        self.fresh = false;
        if self.all {
            if self.focus.is_none() {
                self.focus = Some(0);
            }
        } else if self.row().is_some_and(|r| !r.filled()) {
            self.focus = self.draft.filled_rows().first().copied();
        }
    }

    /// Jumps to the first shown row of the next (`forward`) or previous
    /// group.
    pub fn jump_group(&mut self, forward: bool) {
        let shown = self.shown();
        let rows = &self.draft.rows;
        let Some(f) = self.focus else {
            if !forward {
                if let Some(&i) = shown.last() {
                    let group = rows[i].spec.group;
                    self.focus = shown.iter().copied().find(|&j| rows[j].spec.group == group);
                }
            }
            return;
        };
        let p = self.position(&shown);
        let group = rows[f].spec.group;
        let at = KeyGroup::ALL.iter().position(|g| *g == group).unwrap_or(0);
        let targets: Vec<KeyGroup> = if forward {
            KeyGroup::ALL[at + 1..].to_vec()
        } else if p
            .checked_sub(1)
            .is_some_and(|q| rows[shown[q]].spec.group == group)
        {
            vec![group]
        } else {
            KeyGroup::ALL[..at].iter().rev().copied().collect()
        };
        for g in targets {
            if let Some(&i) = shown.iter().find(|&&i| rows[i].spec.group == g) {
                self.focus = Some(i);
                return;
            }
        }
        if forward && !self.all {
            self.focus = None;
        }
    }

    /// Steps a flag or choice row through unset and its words.
    pub fn cycle(&mut self, forward: bool) {
        self.fresh = false;
        let Some(row) = self.row_mut() else {
            return;
        };
        let Some(words) = row.choices() else {
            return;
        };
        let mut values = vec![""];
        values.extend_from_slice(words);
        let at = values
            .iter()
            .position(|v| v.eq_ignore_ascii_case(row.value.trim()))
            .unwrap_or(0);
        let n = values.len();
        let next = if forward {
            (at + 1) % n
        } else {
            (at + n - 1) % n
        };
        row.value = values[next].to_string();
    }

    /// Space cycles a flag or choice showing one of its words (or nothing),
    /// and types a space into free text.
    pub fn space(&mut self) {
        let Some(row) = self.row() else {
            return;
        };
        let cycles = row.choices().is_some()
            && (row.value.trim().is_empty()
                || row
                    .choices()
                    .unwrap()
                    .iter()
                    .any(|v| v.eq_ignore_ascii_case(row.value.trim())));
        if cycles {
            self.cycle(true);
        } else {
            self.type_char(' ');
        }
    }

    pub fn type_char(&mut self, c: char) {
        if self.focus.is_none() {
            if !c.is_whitespace() {
                self.error = None;
                self.adding.push(c);
            }
            return;
        }
        let fresh = std::mem::take(&mut self.fresh);
        if self.row().is_some_and(SettingRow::typed) {
            let row = self.row_mut().expect("focused row");
            if fresh {
                row.value.clear();
            }
            row.value.push(c);
        }
    }

    pub fn backspace(&mut self) {
        if self.focus.is_none() {
            self.adding.pop();
            return;
        }
        let fresh = std::mem::take(&mut self.fresh);
        let typed = self.row().is_some_and(SettingRow::typed);
        let row = self.row_mut().expect("focused row");
        if typed && !fresh {
            row.value.pop();
        } else {
            row.value.clear();
        }
    }

    pub fn clear(&mut self) {
        self.fresh = false;
        match self.row_mut() {
            Some(row) => row.value.clear(),
            None => self.adding.clear(),
        }
    }

    /// The keywords Add setting offers for its text, best first.
    pub fn matches(&self) -> Vec<&'static str> {
        self.draft.complete(&self.adding)
    }

    /// Adds the first keyword matching the Add setting text and focuses
    /// its value, prefilled with the premade value, selected. With no
    /// match the text stays and the row says so.
    pub fn accept(&mut self) {
        let Some(key) = self.matches().first().copied() else {
            self.error = Some("no matching keyword".into());
            return;
        };
        let Some(i) = self.draft.add_key(key) else {
            return;
        };
        self.adding.clear();
        self.error = None;
        self.focus = Some(i);
        let row = &mut self.draft.rows[i];
        if row.value.trim().is_empty() {
            if let Some(v) = premade_value(key) {
                row.value = v.to_string();
                self.fresh = true;
            }
        }
    }

    pub fn changes(&self) -> Result<Vec<SettingChange>, String> {
        self.draft.changes()
    }

    pub fn grow(&mut self) {
        if let Some(i) = self.focus {
            self.draft.grow(i);
        }
    }
}
