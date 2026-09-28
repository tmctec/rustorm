//! The settings form: every keyword a host can set, grouped, with a
//! control per value type (docs/tui.md, Settings form).

use rustorm_core::{
    key_specs, validate_setting, HostBlock, KeyGroup, KeySpec, KeyType, SettingChange,
};

/// One value line of the form. A multi-valued key has one row per value
/// plus one empty row to add another.
#[derive(Debug, Clone)]
pub(crate) struct SettingRow {
    pub spec: KeySpec,
    /// The value as edited; empty means the key is not set.
    pub value: String,
    /// The value as loaded.
    pub original: String,
    /// What `Host *` gives the host for this key, shown while unset.
    pub inherited: Option<String>,
}

impl SettingRow {
    pub fn changed(&self) -> bool {
        self.value.trim() != self.original.trim()
    }

    /// The values a row cycles through: unset, then the documented words.
    fn cycle(&self) -> Option<Vec<&'static str>> {
        let values: &[&'static str] = match self.spec.kind {
            KeyType::Flag => &["yes", "no"],
            KeyType::Choice { values, .. } => values,
            _ => return None,
        };
        let mut out = vec![""];
        out.extend_from_slice(values);
        Some(out)
    }

    /// True when typing edits the value; a flag or a closed choice only
    /// cycles.
    fn typed(&self) -> bool {
        !matches!(
            self.spec.kind,
            KeyType::Flag | KeyType::Choice { open: false, .. }
        )
    }
}

/// The open settings form of one host.
#[derive(Debug, Clone)]
pub(crate) struct SettingsForm {
    pub host: String,
    pub rows: Vec<SettingRow>,
    /// Index into `rows`.
    pub focus: usize,
    pub error: Option<String>,
}

impl SettingsForm {
    /// The form for `block`, with `defaults` (the `Host *` block) supplying
    /// the inherited values.
    pub fn new(host: &str, block: &HostBlock, defaults: Option<&HostBlock>) -> SettingsForm {
        let mut rows = Vec::new();
        for group in KeyGroup::ALL {
            for spec in key_specs().into_iter().filter(|s| s.group == group) {
                let inherited = defaults.and_then(|d| d.get(spec.key));
                let row = |v: String| SettingRow {
                    spec,
                    value: v.clone(),
                    original: v,
                    inherited: inherited.clone(),
                };
                let values = block.get_all(spec.key);
                if spec.multi {
                    rows.extend(values.into_iter().map(row));
                    rows.push(row(String::new()));
                } else {
                    rows.push(row(values.into_iter().next().unwrap_or_default()));
                }
            }
        }
        SettingsForm {
            host: host.to_string(),
            rows,
            focus: 0,
            error: None,
        }
    }

    pub fn row(&self) -> &SettingRow {
        &self.rows[self.focus]
    }

    pub fn move_by(&mut self, delta: isize) {
        let last = self.rows.len() as isize - 1;
        self.focus = (self.focus as isize + delta).clamp(0, last) as usize;
    }

    /// Jumps to the first row of the next (`forward`) or previous group.
    pub fn jump_group(&mut self, forward: bool) {
        let group = self.row().spec.group;
        let at = KeyGroup::ALL.iter().position(|g| *g == group).unwrap_or(0);
        let target = if forward {
            KeyGroup::ALL.get(at + 1).copied()
        } else if self
            .focus
            .checked_sub(1)
            .is_some_and(|i| self.rows[i].spec.group == group)
        {
            Some(group)
        } else {
            at.checked_sub(1).map(|i| KeyGroup::ALL[i])
        };
        if let Some(g) = target {
            if let Some(i) = self.rows.iter().position(|r| r.spec.group == g) {
                self.focus = i;
            }
        }
    }

    /// Steps a flag or choice row through unset and its values.
    pub fn cycle(&mut self, forward: bool) {
        let row = &mut self.rows[self.focus];
        let Some(values) = row.cycle() else {
            return;
        };
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
        self.error = None;
    }

    /// Space cycles a flag or choice showing one of its words (or nothing),
    /// and types a space into free text.
    pub fn space(&mut self) {
        let row = self.row();
        let cycles = row.cycle().is_some_and(|values| {
            values
                .iter()
                .any(|v| v.eq_ignore_ascii_case(row.value.trim()))
        });
        if cycles {
            self.cycle(true);
        } else {
            self.type_char(' ');
        }
    }

    pub fn type_char(&mut self, c: char) {
        let row = &mut self.rows[self.focus];
        if row.typed() {
            row.value.push(c);
            self.error = None;
        }
    }

    pub fn backspace(&mut self) {
        let row = &mut self.rows[self.focus];
        if row.typed() {
            row.value.pop();
        } else {
            row.value.clear();
        }
        self.error = None;
    }

    pub fn clear(&mut self) {
        self.rows[self.focus].value.clear();
        self.error = None;
    }

    /// The changes to write, one per keyword whose values differ from the
    /// loaded ones; `Err` names the first value that does not fit its key.
    pub fn changes(&self) -> Result<Vec<SettingChange>, String> {
        let mut out: Vec<SettingChange> = Vec::new();
        let mut keys: Vec<&'static str> = Vec::new();
        for r in &self.rows {
            if !keys.contains(&r.spec.key) {
                keys.push(r.spec.key);
            }
        }
        for key in keys {
            let rows: Vec<&SettingRow> = self.rows.iter().filter(|r| r.spec.key == key).collect();
            let values: Vec<String> = rows
                .iter()
                .map(|r| r.value.trim().to_string())
                .filter(|v| !v.is_empty())
                .collect();
            let original: Vec<String> = rows
                .iter()
                .map(|r| r.original.trim().to_string())
                .filter(|v| !v.is_empty())
                .collect();
            if values == original {
                continue;
            }
            for v in &values {
                validate_setting(key, v).map_err(|reason| format!("{key} {reason}."))?;
            }
            out.push(SettingChange {
                key: key.to_string(),
                values,
            });
        }
        Ok(out)
    }

    /// The row gains an empty sibling once a multi-valued key's last row
    /// is filled, so another value can always be added.
    pub fn grow(&mut self) {
        let row = &self.rows[self.focus];
        if !row.spec.multi || row.value.trim().is_empty() {
            return;
        }
        let key = row.spec.key;
        let last = self.rows.iter().rposition(|r| r.spec.key == key).unwrap();
        if !self.rows[last].value.trim().is_empty() {
            let mut blank = self.rows[last].clone();
            blank.value.clear();
            blank.original.clear();
            self.rows.insert(last + 1, blank);
        }
    }
}
