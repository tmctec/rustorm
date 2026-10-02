//! Reading output (docs/cli.md): `--where` picks hosts, `--filter` picks
//! keys, `--format` picks txt, json, csv or yaml, `--just-value` drops the
//! keys. Shared by `show`, `list` and `search`.
//!
//! A key is an ssh_config keyword (falling back to `Host *`), a metadata key
//! ([`crate::meta`]) or a pseudo-key: `Host`, `section`, `file`. The output
//! holds exactly the keys named, in the order named.

use std::path::PathBuf;

use crate::keys;
use crate::meta::{self, MetaKey};
use crate::model::HostBlock;
use crate::ops::Matcher;
use crate::{Error, Result};

/// One host as the read commands see it: its block, where it lives and the
/// `Host *` block its unset keys fall back to.
#[derive(Debug, Clone)]
pub struct HostView<'a> {
    /// The host block.
    pub block: &'a HostBlock,
    /// Its section, `None` outside every section.
    pub section: Option<&'a str>,
    /// The absolute path of the file holding it.
    pub file: PathBuf,
    /// The workspace's `Host *` block.
    pub defaults: Option<&'a HostBlock>,
}

/// The output format.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Format {
    /// Lines as the file spells them (the default).
    #[default]
    Txt,
    /// One JSON document.
    Json,
    /// RFC 4180 rows.
    Csv,
    /// A YAML sequence.
    Yaml,
}

impl Format {
    /// Parses `txt`, `json`, `csv`, `yaml` or `yml`, ignoring case.
    pub fn parse(name: &str) -> Result<Format> {
        match name.to_ascii_lowercase().as_str() {
            "txt" | "text" => Ok(Format::Txt),
            "json" => Ok(Format::Json),
            "csv" => Ok(Format::Csv),
            "yaml" | "yml" => Ok(Format::Yaml),
            _ => Err(Error::Usage(format!(
                "{name} is not a format; use txt, json, csv or yaml."
            ))),
        }
    }

    /// The name as `--format` spells it.
    pub fn name(self) -> &'static str {
        match self {
            Format::Txt => "txt",
            Format::Json => "json",
            Format::Csv => "csv",
            Format::Yaml => "yaml",
        }
    }
}

/// A key's values on a host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    /// The host does not set the key, nor does `Host *`.
    Unset,
    /// One value.
    One(String),
    /// A list key: every `IdentityFile`, every note line, every tag.
    Many(Vec<String>),
}

impl Value {
    /// Every value as a list; empty when unset.
    pub fn values(&self) -> Vec<String> {
        match self {
            Value::Unset => Vec::new(),
            Value::One(v) => vec![v.clone()],
            Value::Many(v) => v.clone(),
        }
    }

    /// True for [`Value::Unset`].
    pub fn is_unset(&self) -> bool {
        matches!(self, Value::Unset)
    }
}

/// One key resolved on one host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cell {
    /// The key as the caller spelled it.
    pub key: String,
    /// Its values.
    pub value: Value,
    /// The file's own lines for `txt` output: the `Host` line, the matching
    /// directive lines (from `Host *` when the host lacks the key), the
    /// metadata comment lines, or `    section NAME` / `    file PATH`.
    pub lines: Vec<String>,
    /// True when the key can be missing and is: an ssh or metadata key with
    /// no line on the host or `Host *`. Pseudo-keys and `tags` are never
    /// missing.
    pub missing: bool,
}

/// Resolves `key` on `view`.
pub fn resolve(view: &HostView, key: &str) -> Cell {
    let key_lc = key.to_ascii_lowercase();
    let block = view.block;
    let indent = "    ";
    if key_lc == "host" {
        return Cell {
            key: key.to_string(),
            value: Value::One(block.primary()),
            lines: vec![block.header.text().to_string()],
            missing: false,
        };
    }
    if key_lc == "section" {
        let name = view.section.unwrap_or("").to_string();
        return Cell {
            key: key.to_string(),
            lines: vec![format!("{indent}section {name}")],
            value: Value::One(name),
            missing: false,
        };
    }
    if key_lc == "file" {
        let path = view.file.display().to_string();
        return Cell {
            key: key.to_string(),
            lines: vec![format!("{indent}file {path}")],
            value: Value::One(path),
            missing: false,
        };
    }
    if let Some(m) = MetaKey::parse(key) {
        let values = block.meta_values(m);
        let lines: Vec<String> = block
            .leading
            .iter()
            .filter(|l| meta::parse_meta_line(l.text()).is_some_and(|(k, _)| k == m))
            .map(|l| l.text().to_string())
            .collect();
        let value = match (m.is_list(), values.is_empty()) {
            (true, _) => Value::Many(values),
            (false, true) => Value::Unset,
            (false, false) => Value::One(values.into_iter().next().unwrap_or_default()),
        };
        return Cell {
            key: key.to_string(),
            missing: value.is_unset(),
            value,
            lines,
        };
    }
    let own: Vec<&crate::model::Line> = block
        .body
        .iter()
        .filter(|l| l.directive().is_some_and(|d| d.key.eq_ignore_ascii_case(key)))
        .collect();
    let source: Vec<&crate::model::Line> = if own.is_empty() {
        view.defaults
            .map(|d| {
                d.body
                    .iter()
                    .filter(|l| l.directive().is_some_and(|d| d.key.eq_ignore_ascii_case(key)))
                    .collect()
            })
            .unwrap_or_default()
    } else {
        own
    };
    let values: Vec<String> = source
        .iter()
        .filter_map(|l| l.directive().map(|d| d.value))
        .collect();
    let lines: Vec<String> = source.iter().map(|l| l.text().to_string()).collect();
    let value = if values.is_empty() {
        Value::Unset
    } else if keys::is_multi_valued(key) {
        Value::Many(values)
    } else {
        Value::One(values.into_iter().next().unwrap_or_default())
    };
    Cell {
        key: key.to_string(),
        missing: value.is_unset(),
        value,
        lines,
    }
}

/// Splits `--filter KEYS` on commas, trimming and dropping empty names.
pub fn parse_filter(spec: &str) -> Result<Vec<String>> {
    let keys: Vec<String> = spec
        .split(',')
        .map(str::trim)
        .filter(|k| !k.is_empty())
        .map(str::to_string)
        .collect();
    if keys.is_empty() {
        return Err(Error::Usage("--filter needs at least one key.".to_string()));
    }
    Ok(keys)
}

/// How a `--where` compares.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WhereOp {
    /// `KEY=A,B`: equals any value, ignoring case; contains, on a list.
    Eq,
    /// `KEY!=A,B`: the negation of `Eq`; true for an unset key.
    Ne,
    /// `KEY~PATTERN`: a regular expression matches a value.
    Match,
}

/// One `--where` clause.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Where {
    /// The key as typed.
    pub key: String,
    /// The comparison.
    pub op: WhereOp,
    /// The values (`Eq`, `Ne`: the comma-separated alternatives) or the
    /// pattern (`Match`: one element).
    pub values: Vec<String>,
}

impl Where {
    /// Parses `KEY=VALUE`, `KEY!=VALUE` or `KEY~PATTERN`.
    pub fn parse(spec: &str) -> Result<Where> {
        let usage = || {
            Error::Usage(format!(
                "{spec} is not KEY=VALUE, KEY!=VALUE or KEY~PATTERN."
            ))
        };
        let pos = spec.find(['=', '~']).ok_or_else(usage)?;
        let (key, rest) = spec.split_at(pos);
        let (key, op, raw) = match rest.strip_prefix('~') {
            Some(pattern) => (key, WhereOp::Match, pattern),
            None => match key.strip_suffix('!') {
                Some(k) => (k, WhereOp::Ne, &rest[1..]),
                None => (key, WhereOp::Eq, &rest[1..]),
            },
        };
        let key = key.trim();
        if key.is_empty() {
            return Err(usage());
        }
        let values = match op {
            WhereOp::Match => {
                Matcher::new(raw, false)?;
                vec![raw.to_string()]
            }
            _ => raw.split(',').map(|v| v.trim().to_string()).collect(),
        };
        Ok(Where {
            key: key.to_string(),
            op,
            values,
        })
    }

    /// True when the clause holds on `view`.
    pub fn holds(&self, view: &HostView) -> bool {
        let cell = resolve(view, &self.key);
        let values = cell.value.values();
        match self.op {
            WhereOp::Eq => values
                .iter()
                .any(|v| self.values.iter().any(|w| w.eq_ignore_ascii_case(v))),
            WhereOp::Ne => !values
                .iter()
                .any(|v| self.values.iter().any(|w| w.eq_ignore_ascii_case(v))),
            WhereOp::Match => {
                let m = Matcher::new(&self.values[0], false).expect("checked at parse");
                values.iter().any(|v| m.is_match(v))
            }
        }
    }
}

/// True when every clause holds on `view`.
pub fn selected(clauses: &[Where], view: &HostView) -> bool {
    clauses.iter().all(|w| w.holds(view))
}

/// One host's projected cells, in filter order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Projected {
    /// The host's primary name.
    pub host: String,
    /// One cell per filter key.
    pub cells: Vec<Cell>,
}

/// Resolves every key of `filter` on `view`.
pub fn project(view: &HostView, filter: &[String]) -> Projected {
    Projected {
        host: view.block.primary(),
        cells: filter.iter().map(|k| resolve(view, k)).collect(),
    }
}

/// The `(host, key)` pairs whose key is missing, in output order.
pub fn missing(rows: &[Projected]) -> Vec<(String, String)> {
    rows.iter()
        .flat_map(|r| {
            r.cells
                .iter()
                .filter(|c| c.missing)
                .map(move |c| (r.host.clone(), c.key.clone()))
        })
        .collect()
}

/// Renders `rows` in `format`, as docs/cli.md's Reading output table says.
pub fn render(rows: &[Projected], format: Format, just_value: bool) -> String {
    match format {
        Format::Txt => render_txt(rows, just_value),
        Format::Json => render_json(rows, just_value),
        Format::Csv => render_csv(rows, just_value),
        Format::Yaml => render_yaml(rows, just_value),
    }
}

fn render_txt(rows: &[Projected], just_value: bool) -> String {
    let mut out = String::new();
    for r in rows {
        for c in &r.cells {
            if just_value {
                match &c.value {
                    Value::Unset => out.push('\n'),
                    Value::One(v) => {
                        out.push_str(v);
                        out.push('\n');
                    }
                    Value::Many(vs) => {
                        if vs.is_empty() {
                            out.push('\n');
                        }
                        for v in vs {
                            out.push_str(v);
                            out.push('\n');
                        }
                    }
                }
            } else {
                for l in &c.lines {
                    out.push_str(l);
                    out.push('\n');
                }
            }
        }
    }
    out
}

fn json_text(v: &Value) -> String {
    match v {
        Value::Unset => "null".to_string(),
        Value::One(s) => serde_json::to_string(s).expect("a string serializes"),
        Value::Many(vs) => serde_json::to_string(vs).expect("strings serialize"),
    }
}

/// Written by hand so the keys keep filter order (`serde_json::Map` sorts).
fn render_json(rows: &[Projected], just_value: bool) -> String {
    let mut s = String::from("[");
    for (i, r) in rows.iter().enumerate() {
        if i > 0 {
            s.push(',');
        }
        let (open, close) = if just_value { ('[', ']') } else { ('{', '}') };
        s.push(open);
        for (j, c) in r.cells.iter().enumerate() {
            if j > 0 {
                s.push(',');
            }
            if !just_value {
                s.push_str(&serde_json::to_string(&c.key).expect("a string serializes"));
                s.push(':');
            }
            s.push_str(&json_text(&c.value));
        }
        s.push(close);
    }
    s.push_str("]\n");
    s
}

/// Quotes a CSV field when it holds `,`, `"`, a line break or an outer
/// space (RFC 4180; `"` doubles).
pub fn csv_field(text: &str) -> String {
    if text.contains([',', '"', '\n', '\r']) || text.starts_with(' ') || text.ends_with(' ') {
        format!("\"{}\"", text.replace('"', "\"\""))
    } else {
        text.to_string()
    }
}

fn csv_cell(v: &Value) -> String {
    match v {
        Value::Unset => String::new(),
        Value::One(s) => csv_field(s),
        Value::Many(vs) => csv_field(&vs.join(";")),
    }
}

fn render_csv(rows: &[Projected], just_value: bool) -> String {
    let mut out = String::new();
    if !just_value {
        if let Some(first) = rows.first() {
            let header: Vec<String> = first.cells.iter().map(|c| csv_field(&c.key)).collect();
            out.push_str(&header.join(","));
            out.push('\n');
        }
    }
    for r in rows {
        let cells: Vec<String> = r.cells.iter().map(|c| csv_cell(&c.value)).collect();
        out.push_str(&cells.join(","));
        out.push('\n');
    }
    out
}

/// Quotes `text` whenever YAML would read it as anything but that string:
/// empty, a boolean or null word, a number, text starting with an
/// indicator character or a space, or holding `: ` or ` #`.
pub fn yaml_scalar(text: &str) -> String {
    const WORDS: &[&str] = &[
        "yes", "no", "on", "off", "true", "false", "null", "y", "n", "~", "nan", "inf", "-inf",
        "+inf", ".nan", ".inf", "-.inf", "+.inf",
    ];
    let lower = text.to_ascii_lowercase();
    let first = text.chars().next();
    // A number: what f64 reads, hex/octal, or YAML 1.1's `1_000` and
    // sexagesimal `1:30`. A dotted IP fails every test and prints bare.
    let numeric = {
        let t = text.trim_start_matches(['+', '-']);
        let digits = t
            .strip_prefix("0x")
            .or_else(|| t.strip_prefix("0o"))
            .unwrap_or("");
        !t.is_empty()
            && t.chars().next().is_some_and(|c| c.is_ascii_digit() || c == '.')
            && (t.parse::<f64>().is_ok()
                || (!digits.is_empty() && i64::from_str_radix(digits, 16).is_ok())
                || t.chars().all(|c| c.is_ascii_digit() || c == '_' || c == ':'))
    };
    let needs_quotes = text.is_empty()
        || WORDS.contains(&lower.as_str())
        || numeric
        || first.is_some_and(|c| {
            " -?:,[]{}#&*!|>'\"%@`".contains(c)
        })
        || text.ends_with([' ', ':'])
        || text.contains(": ")
        || text.contains(" #")
        || text.contains(['\n', '\r', '\t']);
    if needs_quotes {
        format!(
            "\"{}\"",
            text.replace('\\', "\\\\")
                .replace('"', "\\\"")
                .replace('\n', "\\n")
                .replace('\r', "\\r")
                .replace('\t', "\\t")
        )
    } else {
        text.to_string()
    }
}

fn yaml_value(v: &Value) -> String {
    match v {
        Value::Unset => "null".to_string(),
        Value::One(s) => yaml_scalar(s),
        Value::Many(vs) => format!(
            "[{}]",
            vs.iter().map(|s| yaml_scalar(s)).collect::<Vec<_>>().join(", ")
        ),
    }
}

fn render_yaml(rows: &[Projected], just_value: bool) -> String {
    let mut out = String::new();
    for r in rows {
        if just_value {
            let items: Vec<String> = r.cells.iter().map(|c| yaml_value(&c.value)).collect();
            out.push_str(&format!("- [{}]\n", items.join(", ")));
            continue;
        }
        if r.cells.is_empty() {
            out.push_str("- {}\n");
            continue;
        }
        for (i, c) in r.cells.iter().enumerate() {
            let prefix = if i == 0 { "- " } else { "  " };
            out.push_str(&format!(
                "{prefix}{}: {}\n",
                yaml_scalar(&c.key),
                yaml_value(&c.value)
            ));
        }
    }
    out
}

/// Every key name completion offers for `--filter` and `--where`: the
/// pseudo-keys, the metadata keys and the ssh_config keywords.
pub fn completion_keys() -> Vec<&'static str> {
    let mut out = vec!["Host", "section", "file"];
    out.extend(MetaKey::ALL.iter().map(|k| k.name()));
    out.extend(
        keys::KNOWN_KEYS
            .iter()
            .copied()
            .filter(|k| !matches!(*k, "Host" | "Match" | "Include")),
    );
    out
}
