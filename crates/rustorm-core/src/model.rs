//! The line-preserving ssh_config model.
//!
//! [`Config`] keeps every line of the file as raw text, terminator included,
//! so [`Config::render`] reproduces the parsed input byte for byte. The file
//! splits into a preamble (everything before the first section banner) and a
//! list of [`Section`]s. Each part is a sequence of [`Entry`] values: free
//! lines (comments, blank lines, global directives, `Include`), [`HostBlock`]s
//! and opaque [`MatchBlock`]s.
//!
//! A host block owns the comment lines directly above its `Host` line (no
//! blank line between), its `Host` line, and every line up to its last
//! directive. Blank lines and comments after the last directive are free lines
//! of the enclosing part, so sorting and deleting hosts keep separators intact.

use crate::banner;
use crate::keys;

/// One line of the file, stored with its terminator (`\n`, `\r\n` or none for
/// a final line without newline).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Line {
    raw: String,
}

impl Line {
    /// Creates a line from raw text that includes its terminator, if any.
    pub fn from_raw(raw: impl Into<String>) -> Line {
        Line { raw: raw.into() }
    }

    /// Creates a line from `text` (no terminator) and appends `\n`.
    pub fn new(text: &str) -> Line {
        Line {
            raw: format!("{text}\n"),
        }
    }

    /// Returns a blank line (`\n`).
    pub fn blank() -> Line {
        Line::new("")
    }

    /// The raw text, terminator included.
    pub fn raw(&self) -> &str {
        &self.raw
    }

    /// The text without its line terminator.
    pub fn text(&self) -> &str {
        self.raw.trim_end_matches(['\n', '\r'])
    }

    /// The line terminator: `"\n"`, `"\r\n"` or `""`.
    pub fn eol(&self) -> &str {
        &self.raw[self.text().len()..]
    }

    /// True when the line holds only whitespace.
    pub fn is_blank(&self) -> bool {
        self.text().trim().is_empty()
    }

    /// True when the first non-whitespace character is `#`.
    pub fn is_comment(&self) -> bool {
        self.text().trim_start().starts_with('#')
    }

    /// Parses the line as a `Key value` directive. Returns `None` for blank
    /// lines, comments and lines without a value.
    pub fn directive(&self) -> Option<Directive> {
        let parts = DirectiveParts::split(self.text())?;
        let text = self.text();
        Some(Directive {
            key: text[parts.key.clone()].to_string(),
            value: text[parts.value.clone()].to_string(),
        })
    }

    /// True when the line is neither blank, a comment, nor a directive.
    pub fn is_unparsable(&self) -> bool {
        !self.is_blank() && !self.is_comment() && self.directive().is_none()
    }

    pub(crate) fn ensure_terminated(&mut self) {
        if self.eol().is_empty() {
            self.raw.push('\n');
        }
    }
}

/// A `Key value` pair read from a line, key as written in the file.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Directive {
    /// The key as spelled in the file.
    pub key: String,
    /// The value, trailing whitespace removed.
    pub value: String,
}

/// Byte ranges of the parts of a directive line (terminator excluded).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectiveParts {
    /// Leading whitespace.
    pub indent: std::ops::Range<usize>,
    /// The key.
    pub key: std::ops::Range<usize>,
    /// Whitespace and the optional `=` between key and value.
    pub separator: std::ops::Range<usize>,
    /// The value.
    pub value: std::ops::Range<usize>,
    /// Trailing whitespace after the value.
    pub trailing: std::ops::Range<usize>,
}

impl DirectiveParts {
    /// Splits `text` (one line, no terminator) into directive parts. Returns
    /// `None` for blank lines, comments and lines without a value.
    pub fn split(text: &str) -> Option<DirectiveParts> {
        let is_ws = |c: char| c == ' ' || c == '\t';
        let key_start = text.len() - text.trim_start_matches(is_ws).len();
        let rest = &text[key_start..];
        if rest.is_empty() || rest.starts_with('#') || rest.starts_with('=') {
            return None;
        }
        let key_len = rest
            .find(|c: char| is_ws(c) || c == '=')
            .unwrap_or(rest.len());
        let key_end = key_start + key_len;
        let mut value_start = key_end;
        let bytes = text.as_bytes();
        while value_start < text.len() && is_ws(bytes[value_start] as char) {
            value_start += 1;
        }
        if value_start < text.len() && bytes[value_start] == b'=' {
            value_start += 1;
            while value_start < text.len() && is_ws(bytes[value_start] as char) {
                value_start += 1;
            }
        }
        let value_end = text.trim_end_matches(is_ws).len().max(value_start);
        if value_end <= value_start {
            return None;
        }
        Some(DirectiveParts {
            indent: 0..key_start,
            key: key_start..key_end,
            separator: key_end..value_start,
            value: value_start..value_end,
            trailing: value_end..text.len(),
        })
    }
}

/// Splits a `Host` pattern list on whitespace, keeping double-quoted
/// patterns (quotes removed) as one token.
pub fn split_patterns(value: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut quoted = false;
    let mut has_token = false;
    for c in value.chars() {
        match c {
            '"' => {
                quoted = !quoted;
                has_token = true;
            }
            ' ' | '\t' if !quoted => {
                if has_token {
                    out.push(std::mem::take(&mut current));
                    has_token = false;
                }
            }
            _ => {
                current.push(c);
                has_token = true;
            }
        }
    }
    if has_token {
        out.push(current);
    }
    out
}

fn quote_pattern(name: &str) -> String {
    if name.contains([' ', '\t']) {
        format!("\"{name}\"")
    } else {
        name.to_string()
    }
}

/// A `Host` entry: the comments directly above it, its `Host` line and its
/// body up to the last directive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostBlock {
    /// Comment lines directly above the `Host` line; they move and delete
    /// with the host.
    pub leading: Vec<Line>,
    /// The `Host` line.
    pub header: Line,
    /// Directive lines, plus comments and blank lines between directives.
    pub body: Vec<Line>,
}

impl HostBlock {
    /// Creates an empty block `Host <names…>` with no body.
    pub fn new(names: &[String]) -> HostBlock {
        let names: Vec<String> = names.iter().map(|n| quote_pattern(n)).collect();
        HostBlock {
            leading: Vec::new(),
            header: Line::new(&format!("Host {}", names.join(" "))),
            body: Vec::new(),
        }
    }

    /// Every pattern on the `Host` line: the primary name first, then aliases.
    pub fn names(&self) -> Vec<String> {
        self.header
            .directive()
            .map(|d| split_patterns(&d.value))
            .unwrap_or_default()
    }

    /// The first pattern on the `Host` line.
    pub fn primary(&self) -> String {
        self.names().into_iter().next().unwrap_or_default()
    }

    /// Every pattern after the first.
    pub fn aliases(&self) -> Vec<String> {
        self.names().into_iter().skip(1).collect()
    }

    /// True when `name` is the primary name or an alias.
    pub fn answers_to(&self, name: &str) -> bool {
        self.names().iter().any(|n| n == name)
    }

    /// True for the `Host *` defaults block.
    pub fn is_defaults(&self) -> bool {
        self.names() == ["*"]
    }

    /// Every directive in the body, in file order.
    pub fn directives(&self) -> Vec<Directive> {
        self.body.iter().filter_map(Line::directive).collect()
    }

    /// The first value of `key` (matched case-insensitively).
    pub fn get(&self, key: &str) -> Option<String> {
        self.directives()
            .into_iter()
            .find(|d| d.key.eq_ignore_ascii_case(key))
            .map(|d| d.value)
    }

    /// Every value of `key` (matched case-insensitively), in file order.
    pub fn get_all(&self, key: &str) -> Vec<String> {
        self.directives()
            .into_iter()
            .filter(|d| d.key.eq_ignore_ascii_case(key))
            .map(|d| d.value)
            .collect()
    }

    /// The block as text: leading comments, `Host` line and body.
    pub fn text(&self) -> String {
        let mut s = String::new();
        for line in self.lines() {
            s.push_str(line.raw());
        }
        s
    }

    /// Every line of the block in file order.
    pub fn lines(&self) -> impl Iterator<Item = &Line> {
        self.leading
            .iter()
            .chain(std::iter::once(&self.header))
            .chain(self.body.iter())
    }

    /// Rewrites the `Host` line with `names`, keeping its indentation and
    /// separator.
    pub fn set_names(&mut self, names: &[String]) {
        let text = self.header.text().to_string();
        let eol = self.header.eol().to_string();
        let list: Vec<String> = names.iter().map(|n| quote_pattern(n)).collect();
        let raw = match DirectiveParts::split(&text) {
            Some(p) => format!(
                "{}Host{}{}{}",
                &text[p.indent.clone()],
                &text[p.separator.clone()],
                list.join(" "),
                if eol.is_empty() { "\n" } else { &eol }
            ),
            None => format!("Host {}\n", list.join(" ")),
        };
        self.header = Line::from_raw(raw);
    }

    fn indent(&self) -> String {
        self.body
            .iter()
            .find_map(|l| DirectiveParts::split(l.text()).map(|p| l.text()[p.indent].to_string()))
            .unwrap_or_else(|| "    ".to_string())
    }

    fn last_directive_index(&self) -> Option<usize> {
        self.body.iter().rposition(|l| l.directive().is_some())
    }

    /// Sets `key` to `value`: the first line with that key is rewritten in
    /// canonical key case (keeping its indentation and separator), every
    /// other line with that key is removed, and a missing key is appended.
    pub fn set(&mut self, key: &str, value: &str) {
        let canonical = keys::canonical_key(key);
        let mut first: Option<usize> = None;
        let mut i = 0;
        while i < self.body.len() {
            let matches = self.body[i]
                .directive()
                .is_some_and(|d| d.key.eq_ignore_ascii_case(key));
            if matches {
                if first.is_none() {
                    first = Some(i);
                    let line = &self.body[i];
                    let text = line.text();
                    let p = DirectiveParts::split(text).expect("directive line splits");
                    let eol = if line.eol().is_empty() {
                        "\n"
                    } else {
                        line.eol()
                    };
                    let raw = format!(
                        "{}{}{}{}{}",
                        &text[p.indent.clone()],
                        canonical,
                        &text[p.separator.clone()],
                        value,
                        eol
                    );
                    self.body[i] = Line::from_raw(raw);
                    i += 1;
                } else {
                    self.body.remove(i);
                }
            } else {
                i += 1;
            }
        }
        if first.is_none() {
            self.append(key, value);
        }
    }

    /// Adds a `key value` line after the last line with the same key, or
    /// after the last directive when the key is absent.
    pub fn append(&mut self, key: &str, value: &str) {
        let canonical = keys::canonical_key(key);
        let line = Line::new(&format!("{}{} {}", self.indent(), canonical, value));
        let same_key = self.body.iter().rposition(|l| {
            l.directive()
                .is_some_and(|d| d.key.eq_ignore_ascii_case(key))
        });
        let at = same_key
            .or_else(|| self.last_directive_index())
            .map_or(self.body.len(), |i| i + 1);
        if at > 0 {
            if let Some(prev) = self.body.get_mut(at - 1) {
                prev.ensure_terminated();
            }
        } else {
            self.header.ensure_terminated();
        }
        self.body.insert(at, line);
    }

    /// Removes every line with `key`. Returns true when a line was removed.
    pub fn unset(&mut self, key: &str) -> bool {
        let before = self.body.len();
        self.body.retain(|l| {
            !l.directive()
                .is_some_and(|d| d.key.eq_ignore_ascii_case(key))
        });
        before != self.body.len()
    }

    pub(crate) fn last_line_mut(&mut self) -> &mut Line {
        self.body.last_mut().unwrap_or(&mut self.header)
    }
}

/// A `Match` line and the lines under it, kept verbatim and never
/// interpreted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MatchBlock {
    /// The `Match` line.
    pub header: Line,
    /// Lines up to the last directive under the `Match` line.
    pub body: Vec<Line>,
}

/// One element of the preamble or of a section.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Entry {
    /// A line outside any block: comment, blank line, global directive,
    /// `Include`, or a line the parser could not classify.
    Line(Line),
    /// A `Host` entry.
    Host(HostBlock),
    /// A `Match` block, preserved verbatim.
    Match(MatchBlock),
}

impl Entry {
    fn lines(&self) -> Box<dyn Iterator<Item = &Line> + '_> {
        match self {
            Entry::Line(l) => Box::new(std::iter::once(l)),
            Entry::Host(h) => Box::new(h.lines()),
            Entry::Match(m) => Box::new(std::iter::once(&m.header).chain(m.body.iter())),
        }
    }

    fn last_line_mut(&mut self) -> &mut Line {
        match self {
            Entry::Line(l) => l,
            Entry::Host(h) => h.last_line_mut(),
            Entry::Match(m) => m.body.last_mut().unwrap_or(&mut m.header),
        }
    }

    pub(crate) fn is_blank_line(&self) -> bool {
        matches!(self, Entry::Line(l) if l.is_blank())
    }

    pub(crate) fn is_block(&self) -> bool {
        !matches!(self, Entry::Line(_))
    }
}

/// A section banner as it appears in the file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Banner {
    /// The name read from the label line.
    pub name: String,
    /// The banner's lines, verbatim.
    pub lines: Vec<Line>,
}

impl Banner {
    /// Generates a fresh 103-column banner for `name`.
    pub fn generate(name: &str) -> Banner {
        Banner {
            name: name.to_string(),
            lines: banner::banner_lines(name)
                .iter()
                .map(|l| Line::new(l))
                .collect(),
        }
    }
}

/// A banner and the entries under it, up to the next banner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Section {
    /// The banner that opens the section.
    pub banner: Banner,
    /// Entries between this banner and the next one.
    pub entries: Vec<Entry>,
}

impl Section {
    /// Creates a section with a generated banner and one blank line under it.
    pub fn new(name: &str) -> Section {
        Section {
            banner: Banner::generate(name),
            entries: vec![Entry::Line(Line::blank())],
        }
    }

    /// The section name.
    pub fn name(&self) -> &str {
        &self.banner.name
    }

    /// Host blocks in the section, `Host *` included.
    pub fn hosts(&self) -> impl Iterator<Item = &HostBlock> {
        self.entries.iter().filter_map(|e| match e {
            Entry::Host(h) => Some(h),
            _ => None,
        })
    }
}

/// A parsed ssh_config file.
///
/// `preamble` holds every entry before the first banner; `sections` holds the
/// banners and their entries in file order. A file without banners keeps
/// everything in `preamble`. When sections exist, the last one is the
/// catch-all.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Config {
    /// Entries before the first banner.
    pub preamble: Vec<Entry>,
    /// Sections in file order; the last is the catch-all.
    pub sections: Vec<Section>,
}

/// Where a host block lives inside a [`Config`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HostLocation {
    /// Section index, or `None` for the preamble.
    pub section: Option<usize>,
    /// Index in that part's entries.
    pub index: usize,
}

struct OpenBlock {
    host: bool,
    leading: Vec<Line>,
    header: Line,
    body: Vec<Line>,
}

fn close_block(block: OpenBlock, entries: &mut Vec<Entry>) {
    let OpenBlock {
        host,
        leading,
        header,
        mut body,
    } = block;
    let keep = body
        .iter()
        .rposition(|l| !l.is_blank() && !l.is_comment())
        .map_or(0, |i| i + 1);
    let trailing: Vec<Line> = body.split_off(keep);
    if host {
        entries.push(Entry::Host(HostBlock {
            leading,
            header,
            body,
        }));
    } else {
        entries.extend(leading.into_iter().map(Entry::Line));
        entries.push(Entry::Match(MatchBlock { header, body }));
    }
    entries.extend(trailing.into_iter().map(Entry::Line));
}

fn take_leading_comments(entries: &mut Vec<Entry>) -> Vec<Line> {
    let mut start = entries.len();
    while start > 0 {
        match &entries[start - 1] {
            Entry::Line(l) if l.is_comment() && !l.is_blank() => start -= 1,
            _ => break,
        }
    }
    entries
        .split_off(start)
        .into_iter()
        .map(|e| match e {
            Entry::Line(l) => l,
            _ => unreachable!("only lines are taken"),
        })
        .collect()
}

fn keyword(line: &Line) -> Option<String> {
    line.directive().map(|d| d.key.to_ascii_lowercase())
}

impl Config {
    /// Parses ssh_config text. Every line is kept; the parse never fails on
    /// content, and lines it cannot classify stay as free lines.
    pub fn parse(text: &str) -> crate::Result<Config> {
        let lines: Vec<Line> = text.split_inclusive('\n').map(Line::from_raw).collect();
        let mut config = Config::default();
        let mut open: Option<OpenBlock> = None;
        let mut i = 0;
        while i < lines.len() {
            let line = &lines[i];
            let starts_banner = banner::is_rule_line(line.text())
                && lines
                    .get(i + 1)
                    .and_then(|l| banner::parse_label(l.text()))
                    .is_some();
            if starts_banner {
                if let Some(block) = open.take() {
                    close_block(block, config.part_mut(config.sections.len()));
                }
                let name = banner::parse_label(lines[i + 1].text()).unwrap_or_default();
                let mut banner_lines = vec![lines[i].clone(), lines[i + 1].clone()];
                i += 2;
                while i < lines.len() && banner::is_framed_line(lines[i].text()) {
                    let rule = banner::is_rule_line(lines[i].text());
                    banner_lines.push(lines[i].clone());
                    i += 1;
                    if rule {
                        break;
                    }
                }
                config.sections.push(Section {
                    banner: Banner {
                        name,
                        lines: banner_lines,
                    },
                    entries: Vec::new(),
                });
                continue;
            }
            let part_index = config.sections.len();
            match keyword(line).as_deref() {
                Some(k @ ("host" | "match")) => {
                    let entries = config.part_mut(part_index);
                    if let Some(block) = open.take() {
                        close_block(block, entries);
                    }
                    let host = k == "host";
                    let leading = if host {
                        take_leading_comments(entries)
                    } else {
                        Vec::new()
                    };
                    open = Some(OpenBlock {
                        host,
                        leading,
                        header: line.clone(),
                        body: Vec::new(),
                    });
                }
                _ => match open.as_mut() {
                    Some(block) => block.body.push(line.clone()),
                    None => config.part_mut(part_index).push(Entry::Line(line.clone())),
                },
            }
            i += 1;
        }
        if let Some(block) = open.take() {
            let n = config.sections.len();
            close_block(block, config.part_mut(n));
        }
        Ok(config)
    }

    /// Renders the model back to text. An unmodified parse renders the input
    /// byte for byte.
    pub fn render(&self) -> String {
        let mut out = String::new();
        for line in self.lines() {
            out.push_str(line.raw());
        }
        out
    }

    /// Every line of the file in order.
    pub fn lines(&self) -> impl Iterator<Item = &Line> {
        self.preamble
            .iter()
            .flat_map(Entry::lines)
            .chain(self.sections.iter().flat_map(|s| {
                s.banner
                    .lines
                    .iter()
                    .chain(s.entries.iter().flat_map(Entry::lines))
            }))
    }

    /// Entries of the preamble (`part == 0` when there are no sections is
    /// the preamble; otherwise part `n` is section `n - 1`).
    fn part_mut(&mut self, part: usize) -> &mut Vec<Entry> {
        if part == 0 {
            &mut self.preamble
        } else {
            &mut self.sections[part - 1].entries
        }
    }

    /// The entries of the preamble (`None`) or of section `index`.
    pub fn entries(&self, section: Option<usize>) -> &Vec<Entry> {
        match section {
            None => &self.preamble,
            Some(i) => &self.sections[i].entries,
        }
    }

    /// Mutable entries of the preamble (`None`) or of section `index`.
    pub fn entries_mut(&mut self, section: Option<usize>) -> &mut Vec<Entry> {
        match section {
            None => &mut self.preamble,
            Some(i) => &mut self.sections[i].entries,
        }
    }

    /// True when the file has at least one section banner.
    pub fn has_sections(&self) -> bool {
        !self.sections.is_empty()
    }

    /// Index of the catch-all section (the last one), if any.
    pub fn catch_all(&self) -> Option<usize> {
        self.sections.len().checked_sub(1)
    }

    /// Index of the section named `name`, matched case-insensitively.
    pub fn find_section(&self, name: &str) -> Option<usize> {
        let wanted = name.to_lowercase();
        self.sections
            .iter()
            .position(|s| s.name().to_lowercase() == wanted)
    }

    /// Every host block with its location, in file order, `Host *` included.
    pub fn host_locations(&self) -> Vec<HostLocation> {
        let mut out = Vec::new();
        let parts = std::iter::once(None).chain((0..self.sections.len()).map(Some));
        for section in parts {
            for (index, e) in self.entries(section).iter().enumerate() {
                if matches!(e, Entry::Host(_)) {
                    out.push(HostLocation { section, index });
                }
            }
        }
        out
    }

    /// Every host block in file order, `Host *` included.
    pub fn hosts(&self) -> Vec<&HostBlock> {
        self.host_locations()
            .into_iter()
            .map(|l| self.host(l))
            .collect()
    }

    /// The host block at `loc`. Panics when `loc` does not point at a host.
    pub fn host(&self, loc: HostLocation) -> &HostBlock {
        match &self.entries(loc.section)[loc.index] {
            Entry::Host(h) => h,
            _ => panic!("no host block at {loc:?}"),
        }
    }

    /// The mutable host block at `loc`. Panics when `loc` does not point at a
    /// host.
    pub fn host_mut(&mut self, loc: HostLocation) -> &mut HostBlock {
        match &mut self.entries_mut(loc.section)[loc.index] {
            Entry::Host(h) => h,
            _ => panic!("no host block at {loc:?}"),
        }
    }

    /// The first host whose primary name or alias equals `name`.
    pub fn find_host(&self, name: &str) -> Option<HostLocation> {
        self.host_locations()
            .into_iter()
            .find(|l| self.host(*l).answers_to(name))
    }

    /// The first host whose primary name equals `name`.
    pub fn find_primary(&self, name: &str) -> Option<HostLocation> {
        self.host_locations()
            .into_iter()
            .find(|l| self.host(*l).primary() == name)
    }

    /// The first `Host *` block.
    pub fn defaults(&self) -> Option<&HostBlock> {
        self.hosts().into_iter().find(|h| h.is_defaults())
    }

    /// The 1-based line number of the `Host` line of the host at `loc`, for
    /// editors that jump to it.
    pub fn host_line(&self, loc: HostLocation) -> usize {
        let mut n = 0;
        let parts = std::iter::once(None).chain((0..self.sections.len()).map(Some));
        for section in parts {
            if let Some(i) = section {
                n += self.sections[i].banner.lines.len();
            }
            for (index, e) in self.entries(section).iter().enumerate() {
                if section == loc.section && index == loc.index {
                    if let Entry::Host(h) = e {
                        return n + h.leading.len() + 1;
                    }
                }
                n += e.lines().count();
            }
        }
        n
    }

    /// The section name of the host at `loc`, `None` for the preamble.
    pub fn section_name(&self, loc: HostLocation) -> Option<&str> {
        loc.section.map(|i| self.sections[i].name())
    }

    /// Appends `\n` to the last line of the file when it has no terminator,
    /// so new lines can follow it.
    pub fn ensure_trailing_newline(&mut self) {
        let last_part = if let Some(s) = self.sections.last_mut() {
            if s.entries.is_empty() {
                if let Some(l) = s.banner.lines.last_mut() {
                    l.ensure_terminated();
                }
                return;
            }
            &mut s.entries
        } else {
            &mut self.preamble
        };
        if let Some(e) = last_part.last_mut() {
            e.last_line_mut().ensure_terminated();
        }
    }

    fn followed_by_banner(&self, section: Option<usize>) -> bool {
        match section {
            None => !self.sections.is_empty(),
            Some(i) => i + 1 < self.sections.len(),
        }
    }

    /// Inserts `block` into the preamble (`None`) or section `index`: after
    /// the last non-`*` block with a blank line in between; before the first
    /// `Host *` block when the part has no other block; otherwise after the
    /// part's free lines.
    pub fn insert_host(&mut self, section: Option<usize>, block: HostBlock) -> HostLocation {
        self.ensure_trailing_newline();
        let before_banner = self.followed_by_banner(section);
        let entries = self.entries_mut(section);
        let is_defaults = |e: &Entry| matches!(e, Entry::Host(h) if h.is_defaults());
        let anchor = entries
            .iter()
            .rposition(|e| e.is_block() && !is_defaults(e));
        let first_defaults = entries.iter().position(is_defaults);
        let index = if let (None, Some(d)) = (anchor, first_defaults) {
            entries.insert(d, Entry::Host(block));
            entries.insert(d + 1, Entry::Line(Line::blank()));
            d
        } else if let Some(last) = anchor {
            entries.insert(last + 1, Entry::Line(Line::blank()));
            entries.insert(last + 2, Entry::Host(block));
            last + 2
        } else {
            if entries.last().is_some_and(|e| !e.is_blank_line()) {
                entries.push(Entry::Line(Line::blank()));
            }
            entries.push(Entry::Host(block));
            let index = entries.len() - 1;
            if before_banner {
                entries.push(Entry::Line(Line::blank()));
            }
            index
        };
        HostLocation { section, index }
    }

    /// Removes and returns the host block at `loc`, collapsing the blank
    /// line that separated it from its neighbours.
    pub fn remove_host(&mut self, loc: HostLocation) -> HostBlock {
        let last_part = match loc.section {
            None => self.sections.is_empty(),
            Some(i) => i + 1 == self.sections.len(),
        };
        let entries = self.entries_mut(loc.section);
        let Entry::Host(block) = entries.remove(loc.index) else {
            panic!("no host block at {loc:?}");
        };
        let i = loc.index;
        let prev_blank = i == 0 || entries[i - 1].is_blank_line();
        if i < entries.len() {
            if entries[i].is_blank_line() && prev_blank {
                entries.remove(i);
            }
        } else if last_part && i > 0 && entries[i - 1].is_blank_line() {
            entries.remove(i - 1);
        }
        block
    }

    /// Sorts the hosts of every section by primary name (case-insensitive,
    /// then case-sensitive). `Host *` and `Match` blocks keep their places;
    /// hosts fill the remaining host positions. The preamble never moves.
    pub fn sort_sections(&mut self) {
        for section in &mut self.sections {
            sort_entries(&mut section.entries);
        }
    }

    fn ensure_part_ends_blank(&mut self, section: Option<usize>) {
        let entries = self.entries_mut(section);
        if entries.last().is_some_and(|e| !e.is_blank_line()) {
            entries.push(Entry::Line(Line::blank()));
        }
    }

    /// Returns the index of section `name`, creating it when missing.
    ///
    /// On a file without sections this creates `name` and the catch-all
    /// `other`: every entry from the first non-`*` host onward moves into the
    /// catch-all, and the rest of the preamble stays. On a file with sections
    /// the new section is inserted before the catch-all.
    pub fn ensure_section(&mut self, name: &str) -> usize {
        if let Some(i) = self.find_section(name) {
            return i;
        }
        self.ensure_trailing_newline();
        if self.sections.is_empty() {
            let split = self
                .preamble
                .iter()
                .position(|e| matches!(e, Entry::Host(h) if !h.is_defaults()))
                .unwrap_or(self.preamble.len());
            let moved: Vec<Entry> = self.preamble.split_off(split);
            self.ensure_part_ends_blank(None);
            let mut catch_all = Section::new("other");
            catch_all.entries.extend(moved);
            if name.eq_ignore_ascii_case("other") {
                self.sections.push(catch_all);
                sort_entries(&mut self.sections[0].entries);
                return 0;
            }
            self.sections.push(Section::new(name));
            self.sections.push(catch_all);
            sort_entries(&mut self.sections[1].entries);
            return 0;
        }
        let at = self.sections.len() - 1;
        let prev = if at == 0 { None } else { Some(at - 1) };
        self.ensure_part_ends_blank(prev);
        self.sections.insert(at, Section::new(name));
        at
    }
}

fn sort_key(h: &HostBlock) -> (String, String) {
    let p = h.primary();
    (p.to_lowercase(), p)
}

fn sort_entries(entries: &mut [Entry]) {
    let slots: Vec<usize> = entries
        .iter()
        .enumerate()
        .filter(|(_, e)| matches!(e, Entry::Host(h) if !h.is_defaults()))
        .map(|(i, _)| i)
        .collect();
    let mut hosts: Vec<HostBlock> = slots
        .iter()
        .map(|&i| match &entries[i] {
            Entry::Host(h) => h.clone(),
            _ => unreachable!(),
        })
        .collect();
    hosts.sort_by_key(sort_key);
    // The last line of the file may lack a terminator; keep it on whichever
    // host now ends the part so no two lines join.
    for h in &mut hosts {
        h.last_line_mut().ensure_terminated();
    }
    for (slot, host) in slots.into_iter().zip(hosts) {
        entries[slot] = Entry::Host(host);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_directives() {
        let p = DirectiveParts::split("  HostName = a.example.com  ").unwrap();
        assert_eq!(p.indent, 0..2);
        assert_eq!(p.key, 2..10);
        assert_eq!(p.value, 13..26);
        assert!(DirectiveParts::split("Host").is_none());
        assert!(DirectiveParts::split("  # c").is_none());
        assert!(DirectiveParts::split("").is_none());
    }

    #[test]
    fn host_block_owns_comment_above() {
        let c = Config::parse("# top\n\n# vps box\nHost vps\n  HostName v\n\n").unwrap();
        assert_eq!(c.preamble.len(), 4);
        let Entry::Host(h) = &c.preamble[2] else {
            panic!()
        };
        assert_eq!(h.leading.len(), 1);
        assert_eq!(h.body.len(), 1);
    }

    #[test]
    fn quoted_patterns() {
        assert_eq!(split_patterns("a \"b c\"  d"), vec!["a", "b c", "d"]);
    }
}
