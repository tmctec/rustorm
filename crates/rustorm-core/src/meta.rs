//! Host metadata: `# key: value` comment lines directly above a host's
//! `Host` line (docs/cli.md, Host metadata).
//!
//! The lines are plain comments, so `ssh` ignores them and the file stays
//! ssh_config. rustorm reads them as keys: [`HostBlock::get`],
//! [`HostBlock::set`], [`HostBlock::append`] and [`HostBlock::unset`] route a
//! metadata key here, so `set`, `unset`, `add`, `clone` and the settings
//! forms treat `note`, `location`, `privateKeyLocation`, `other` and `tags`
//! like any ssh keyword. Comment lines that are not metadata are never read,
//! moved or rewritten.

use serde::Serialize;

use crate::model::{HostBlock, Line};

/// The metadata keys, in the order new lines are written above `Host`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum MetaKey {
    /// Free text; one `# note:` line per line of the note.
    Note,
    /// Where the machine is.
    Location,
    /// Where the private key lives: a vault name, a vault entry, a path.
    /// A reference only, never key material.
    PrivateKeyLocation,
    /// Free text that fits no other key.
    Other,
    /// Comma-separated labels.
    Tags,
}

impl MetaKey {
    /// Every key, in write order.
    pub const ALL: [MetaKey; 5] = [
        MetaKey::Note,
        MetaKey::Location,
        MetaKey::PrivateKeyLocation,
        MetaKey::Other,
        MetaKey::Tags,
    ];

    /// The spelling rustorm writes.
    pub fn name(self) -> &'static str {
        match self {
            MetaKey::Note => "note",
            MetaKey::Location => "location",
            MetaKey::PrivateKeyLocation => "privateKeyLocation",
            MetaKey::Other => "other",
            MetaKey::Tags => "tags",
        }
    }

    /// The key `key` names, ignoring case; `None` for anything else.
    pub fn parse(key: &str) -> Option<MetaKey> {
        MetaKey::ALL
            .into_iter()
            .find(|k| k.name().eq_ignore_ascii_case(key))
    }

    /// True when the key may hold several lines (`note`).
    pub fn is_multi_line(self) -> bool {
        self == MetaKey::Note
    }

    /// True when the key holds a list: `note` (one value per line) or
    /// `tags` (one line, comma-separated).
    pub fn is_list(self) -> bool {
        matches!(self, MetaKey::Note | MetaKey::Tags)
    }
}

/// True when `key` names a metadata key, ignoring case.
pub fn is_meta_key(key: &str) -> bool {
    MetaKey::parse(key).is_some()
}

/// True when `set --append` on `key` adds a value instead of replacing:
/// `note` gains a line, `tags` gains a tag.
pub fn accumulates(key: &str) -> bool {
    MetaKey::parse(key).is_some_and(MetaKey::is_list)
}

/// Parses `# key: value` into its key and trimmed value. Returns `None` for
/// any comment that is not a metadata line: another key (`# TODO: x`, the
/// banner label `# section: name`), no colon, or no `#`.
pub fn parse_meta_line(text: &str) -> Option<(MetaKey, String)> {
    let rest = text.trim_start().strip_prefix('#')?;
    let rest = rest.trim_start_matches([' ', '\t']);
    let colon = rest.find(':')?;
    let key = MetaKey::parse(rest[..colon].trim_end())?;
    Some((key, rest[colon + 1..].trim().to_string()))
}

/// Splits a `tags` value on commas, trims each tag, drops empty ones and
/// duplicates (compared ignoring case, first spelling kept).
pub fn split_tags(value: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for raw in value.split(',') {
        let tag = raw.trim();
        if tag.is_empty() || out.iter().any(|t| t.eq_ignore_ascii_case(tag)) {
            continue;
        }
        out.push(tag.to_string());
    }
    out
}

/// Why `value` cannot be written under `key`; `Ok` when it can. Every
/// value is one line; `privateKeyLocation` refuses key material; a tag has
/// no whitespace.
pub fn validate_meta(key: MetaKey, value: &str) -> std::result::Result<(), String> {
    if value.contains(['\n', '\r']) {
        return Err("must be one line".to_string());
    }
    match key {
        MetaKey::PrivateKeyLocation
            if value.contains("-----BEGIN") || value.contains("PRIVATE KEY") =>
        {
            Err("holds a reference to a key, not the key itself".to_string())
        }
        MetaKey::Tags => match split_tags(value)
            .into_iter()
            .find(|t| t.contains(char::is_whitespace))
        {
            Some(tag) => Err(format!("has a tag with a space: {tag}")),
            None => Ok(()),
        },
        _ => Ok(()),
    }
}

/// A host's metadata as read from its leading comments. Serializes to the
/// `"meta"` object of `--json` rows: `note` and `tags` as arrays, the rest
/// as strings, absent keys omitted.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct HostMeta {
    /// The `# note:` lines, in file order.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub note: Vec<String>,
    /// `# location:`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location: Option<String>,
    /// `# privateKeyLocation:`.
    #[serde(rename = "privateKeyLocation", skip_serializing_if = "Option::is_none")]
    pub private_key_location: Option<String>,
    /// `# other:`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub other: Option<String>,
    /// The tags of every `# tags:` line, split and deduplicated.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
}

impl HostMeta {
    /// Reads the metadata lines among `lines`. The first line of a
    /// single-valued key wins; every `# note:` line counts; `# tags:` lines
    /// are merged.
    pub fn from_lines<'a>(lines: impl Iterator<Item = &'a Line>) -> HostMeta {
        let mut meta = HostMeta::default();
        for (key, value) in lines.filter_map(|l| parse_meta_line(l.text())) {
            match key {
                MetaKey::Note => meta.note.push(value),
                MetaKey::Location => {
                    meta.location.get_or_insert(value);
                }
                MetaKey::PrivateKeyLocation => {
                    meta.private_key_location.get_or_insert(value);
                }
                MetaKey::Other => {
                    meta.other.get_or_insert(value);
                }
                MetaKey::Tags => {
                    for tag in split_tags(&value) {
                        if !meta.tags.iter().any(|t| t.eq_ignore_ascii_case(&tag)) {
                            meta.tags.push(tag);
                        }
                    }
                }
            }
        }
        meta
    }

    /// True when the host has no metadata at all.
    pub fn is_empty(&self) -> bool {
        self == &HostMeta::default()
    }

    /// The values under `key`: the note lines, the tags, or the single
    /// value as a one-element list; empty when unset.
    pub fn get(&self, key: MetaKey) -> Vec<String> {
        match key {
            MetaKey::Note => self.note.clone(),
            MetaKey::Tags => self.tags.clone(),
            MetaKey::Location => self.location.iter().cloned().collect(),
            MetaKey::PrivateKeyLocation => self.private_key_location.iter().cloned().collect(),
            MetaKey::Other => self.other.iter().cloned().collect(),
        }
    }

    /// `key`'s value as one line of text: the tags joined with `, `, the
    /// note lines joined with newlines, else the value. `None` when unset.
    pub fn display(&self, key: MetaKey) -> Option<String> {
        let values = self.get(key);
        if values.is_empty() {
            return None;
        }
        Some(match key {
            MetaKey::Tags => values.join(", "),
            MetaKey::Note => values.join("\n"),
            _ => values.into_iter().next().unwrap_or_default(),
        })
    }

    /// Every metadata line as rustorm writes it, `# key: value`, in write
    /// order.
    pub fn lines(&self) -> Vec<String> {
        let mut out = Vec::new();
        for key in MetaKey::ALL {
            match key {
                MetaKey::Note => out.extend(self.note.iter().map(|v| render(key, v))),
                MetaKey::Tags if !self.tags.is_empty() => {
                    out.push(render(key, &self.tags.join(", ")));
                }
                MetaKey::Tags => {}
                _ => out.extend(self.get(key).iter().map(|v| render(key, v))),
            }
        }
        out
    }
}

fn render(key: MetaKey, value: &str) -> String {
    format!("# {}: {}", key.name(), value)
}

fn meta_line(key: MetaKey, value: &str) -> Line {
    Line::new(&render(key, value))
}

impl HostBlock {
    /// The host's metadata.
    pub fn meta(&self) -> HostMeta {
        HostMeta::from_lines(self.leading.iter())
    }

    /// The values under `key` (see [`HostMeta::get`]).
    pub fn meta_values(&self, key: MetaKey) -> Vec<String> {
        self.meta().get(key)
    }

    fn meta_line_indices(&self, key: MetaKey) -> Vec<usize> {
        self.leading
            .iter()
            .enumerate()
            .filter(|(_, l)| parse_meta_line(l.text()).is_some_and(|(k, _)| k == key))
            .map(|(i, _)| i)
            .collect()
    }

    /// Where a new `key` line goes: before the first metadata line of a
    /// later key, else directly above the `Host` line.
    fn meta_insert_index(&self, key: MetaKey) -> usize {
        self.leading
            .iter()
            .position(|l| parse_meta_line(l.text()).is_some_and(|(k, _)| k > key))
            .unwrap_or(self.leading.len())
    }

    /// Replaces `key`'s lines with `values`: the first existing line is
    /// rewritten in place (keeping its terminator), the others removed, and
    /// extra values follow it; a key without lines is inserted in write
    /// order. Values are trimmed; empty ones are dropped; no values removes
    /// the key. A single-valued key keeps the first value; `tags` joins
    /// every value into one deduplicated list.
    pub fn set_meta(&mut self, key: MetaKey, values: &[String]) {
        let mut values: Vec<String> = values
            .iter()
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty())
            .collect();
        if key == MetaKey::Tags {
            let tags = split_tags(&values.join(","));
            values = if tags.is_empty() {
                Vec::new()
            } else {
                vec![tags.join(", ")]
            };
        } else if !key.is_multi_line() {
            values.truncate(1);
        }
        let existing = self.meta_line_indices(key);
        if values.is_empty() {
            for &i in existing.iter().rev() {
                self.leading.remove(i);
            }
            return;
        }
        match existing.first() {
            Some(&first) => {
                let eol = match self.leading[first].eol() {
                    "" => "\n".to_string(),
                    e => e.to_string(),
                };
                self.leading[first] = Line::from_raw(format!("{}{eol}", render(key, &values[0])));
                for &i in existing.iter().skip(1).rev() {
                    self.leading.remove(i);
                }
                for (i, v) in values[1..].iter().enumerate() {
                    self.leading.insert(first + 1 + i, meta_line(key, v));
                }
            }
            None => {
                let at = self.meta_insert_index(key);
                for (i, v) in values.iter().enumerate() {
                    self.leading.insert(at + i, meta_line(key, v));
                }
            }
        }
    }

    /// Adds `value` under `key`: a `note` gains a line after its last one,
    /// `tags` gains the tags in `value`, a single-valued key is set.
    pub fn append_meta(&mut self, key: MetaKey, value: &str) {
        let value = value.trim();
        if value.is_empty() {
            return;
        }
        match key {
            MetaKey::Tags => self.add_tags(&split_tags(value)),
            MetaKey::Note => {
                let at = self
                    .meta_line_indices(key)
                    .last()
                    .map_or_else(|| self.meta_insert_index(key), |i| i + 1);
                self.leading.insert(at, meta_line(key, value));
            }
            _ => self.set_meta(key, &[value.to_string()]),
        }
    }

    /// Removes every line of `key`. Returns true when a line was removed.
    pub fn unset_meta(&mut self, key: MetaKey) -> bool {
        let existing = self.meta_line_indices(key);
        for &i in existing.iter().rev() {
            self.leading.remove(i);
        }
        !existing.is_empty()
    }

    /// Adds `tags` to `# tags:`, creating the line when absent; a tag the
    /// host has (ignoring case) is skipped.
    pub fn add_tags(&mut self, tags: &[String]) {
        let mut current = self.meta_values(MetaKey::Tags);
        let before = current.len();
        for tag in tags.iter().flat_map(|t| split_tags(t)) {
            if !current.iter().any(|c| c.eq_ignore_ascii_case(&tag)) {
                current.push(tag);
            }
        }
        if current.len() != before {
            self.set_meta(MetaKey::Tags, &[current.join(", ")]);
        }
    }

    /// Removes `tags` from `# tags:` (ignoring case); removing the last tag
    /// removes the line.
    pub fn remove_tags(&mut self, tags: &[String]) {
        let drop: Vec<String> = tags.iter().flat_map(|t| split_tags(t)).collect();
        let mut current = self.meta_values(MetaKey::Tags);
        let before = current.len();
        current.retain(|c| !drop.iter().any(|d| d.eq_ignore_ascii_case(c)));
        if current.len() != before {
            self.set_meta(MetaKey::Tags, &[current.join(", ")]);
        }
    }
}
