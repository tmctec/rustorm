//! The settings form: every keyword a host can set, grouped, with a
//! control per value type (docs/tui.md, Settings form).

use rustorm_core::{HostBlock, KeyGroup, SettingChange, SettingRow, SettingsDraft};

/// The open settings form of one host.
#[derive(Debug, Clone)]
pub(crate) struct SettingsForm {
    pub draft: SettingsDraft,
    /// Index into `draft.rows`.
    pub focus: usize,
    pub error: Option<String>,
}

impl SettingsForm {
    pub fn new(host: &str, block: &HostBlock, defaults: Option<&HostBlock>) -> SettingsForm {
        SettingsForm {
            draft: SettingsDraft::new(host, block, defaults),
            focus: 0,
            error: None,
        }
    }

    pub fn row(&self) -> &SettingRow {
        &self.draft.rows[self.focus]
    }

    fn row_mut(&mut self) -> &mut SettingRow {
        self.error = None;
        &mut self.draft.rows[self.focus]
    }

    pub fn move_by(&mut self, delta: isize) {
        let last = self.draft.rows.len() as isize - 1;
        self.focus = (self.focus as isize + delta).clamp(0, last) as usize;
    }

    pub fn last(&mut self) {
        self.focus = self.draft.rows.len() - 1;
    }

    /// Jumps to the first row of the next (`forward`) or previous group.
    pub fn jump_group(&mut self, forward: bool) {
        let rows = &self.draft.rows;
        let group = rows[self.focus].spec.group;
        let at = KeyGroup::ALL.iter().position(|g| *g == group).unwrap_or(0);
        let target = if forward {
            KeyGroup::ALL.get(at + 1).copied()
        } else if self
            .focus
            .checked_sub(1)
            .is_some_and(|i| rows[i].spec.group == group)
        {
            Some(group)
        } else {
            at.checked_sub(1).map(|i| KeyGroup::ALL[i])
        };
        if let Some(i) = target.and_then(|g| rows.iter().position(|r| r.spec.group == g)) {
            self.focus = i;
        }
    }

    /// Steps a flag or choice row through unset and its words.
    pub fn cycle(&mut self, forward: bool) {
        let row = self.row_mut();
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
        let row = self.row();
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
        if self.row().typed() {
            self.row_mut().value.push(c);
        }
    }

    pub fn backspace(&mut self) {
        let typed = self.row().typed();
        let row = self.row_mut();
        if typed {
            row.value.pop();
        } else {
            row.value.clear();
        }
    }

    pub fn clear(&mut self) {
        self.row_mut().value.clear();
    }

    pub fn changes(&self) -> Result<Vec<SettingChange>, String> {
        self.draft.changes()
    }

    pub fn grow(&mut self) {
        self.draft.grow(self.focus);
    }
}
