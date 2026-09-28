//! The ssh_config syntax highlighter shared by the TUI and GUI editors.
//!
//! [`lex`] splits one line into [`Span`]s that cover every byte exactly once,
//! in order. [`Lexer`] carries banner state across lines so every line of a
//! section banner gets [`SpanKind::Banner`]; [`lex_document`] lexes a whole
//! text with absolute offsets.

use crate::banner;

/// What a span of text is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize)]
pub enum SpanKind {
    /// A `#` comment.
    Comment,
    /// A line of a section banner.
    Banner,
    /// The `Host` keyword.
    HostKeyword,
    /// The first pattern on a `Host` line.
    HostName,
    /// A pattern after the first on a `Host` line.
    Alias,
    /// A directive key.
    Key,
    /// A directive value (other than `ProxyCommand` and `ProxyJump`).
    Value,
    /// The value of a `ProxyCommand` directive.
    ProxyCommand,
    /// The value of a `ProxyJump` directive.
    ProxyJump,
    /// Spaces, tabs, the `=` separator and line terminators.
    Whitespace,
    /// Text the lexer cannot classify, such as a key without a value.
    Unknown,
}

/// A classified byte range `start..end` of the lexed text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub struct Span {
    /// First byte, inclusive.
    pub start: usize,
    /// Last byte, exclusive.
    pub end: usize,
    /// What the bytes are.
    pub kind: SpanKind,
}

fn push(spans: &mut Vec<Span>, start: usize, end: usize, kind: SpanKind) {
    if start >= end {
        return;
    }
    if let Some(last) = spans.last_mut() {
        if last.kind == kind && last.end == start && kind == SpanKind::Whitespace {
            last.end = end;
            return;
        }
    }
    spans.push(Span { start, end, kind });
}

fn is_ws(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\r' | b'\n')
}

/// Lexes one line (a trailing `\n` or `\r\n` is allowed) without banner
/// context: rule lines (`#---#`) are [`SpanKind::Banner`], other comments are
/// [`SpanKind::Comment`].
pub fn lex(line: &str) -> Vec<Span> {
    lex_line(line, false)
}

/// Lexes one line. With `in_banner`, a framed `#…#` line is
/// [`SpanKind::Banner`] instead of [`SpanKind::Comment`].
pub fn lex_line(line: &str, in_banner: bool) -> Vec<Span> {
    let bytes = line.as_bytes();
    let mut spans = Vec::new();
    let body_end = line.trim_end_matches(['\n', '\r']).len();
    let content_end = line[..body_end].trim_end_matches([' ', '\t']).len();
    let mut i = 0;
    while i < content_end && is_ws(bytes[i]) {
        i += 1;
    }
    push(&mut spans, 0, i, SpanKind::Whitespace);
    if i < content_end {
        let content = &line[i..content_end];
        if content.starts_with('#') {
            let framed = banner::is_framed_line(content);
            let kind = if banner::is_rule_line(content)
                || (in_banner && framed)
                || banner::parse_label(content).is_some()
            {
                SpanKind::Banner
            } else {
                SpanKind::Comment
            };
            push(&mut spans, i, content_end, kind);
        } else {
            lex_directive(line, i, content_end, &mut spans);
        }
    }
    push(
        &mut spans,
        content_end.max(i),
        line.len(),
        SpanKind::Whitespace,
    );
    spans
}

fn lex_directive(line: &str, start: usize, end: usize, spans: &mut Vec<Span>) {
    let bytes = line.as_bytes();
    let mut i = start;
    while i < end && !is_ws(bytes[i]) && bytes[i] != b'=' {
        i += 1;
    }
    let key = &line[start..i];
    if key.is_empty() {
        push(spans, start, end, SpanKind::Unknown);
        return;
    }
    let lower = key.to_ascii_lowercase();
    let mut sep_end = i;
    while sep_end < end && is_ws(bytes[sep_end]) {
        sep_end += 1;
    }
    if sep_end < end && bytes[sep_end] == b'=' {
        sep_end += 1;
        while sep_end < end && is_ws(bytes[sep_end]) {
            sep_end += 1;
        }
    }
    if sep_end >= end {
        push(spans, start, end, SpanKind::Unknown);
        return;
    }
    if lower == "host" {
        push(spans, start, i, SpanKind::HostKeyword);
        push(spans, i, sep_end, SpanKind::Whitespace);
        let mut j = sep_end;
        let mut first = true;
        while j < end {
            if is_ws(bytes[j]) {
                let s = j;
                while j < end && is_ws(bytes[j]) {
                    j += 1;
                }
                push(spans, s, j, SpanKind::Whitespace);
                continue;
            }
            let s = j;
            let mut quoted = false;
            while j < end && (quoted || !is_ws(bytes[j])) {
                if bytes[j] == b'"' {
                    quoted = !quoted;
                }
                j += 1;
            }
            let kind = if first {
                SpanKind::HostName
            } else {
                SpanKind::Alias
            };
            push(spans, s, j, kind);
            first = false;
        }
        return;
    }
    push(spans, start, i, SpanKind::Key);
    push(spans, i, sep_end, SpanKind::Whitespace);
    let kind = match lower.as_str() {
        "proxycommand" => SpanKind::ProxyCommand,
        "proxyjump" => SpanKind::ProxyJump,
        _ => SpanKind::Value,
    };
    push(spans, sep_end, end, kind);
}

/// A line-by-line lexer that knows when it is inside a section banner.
///
/// A rule line followed by a `section:` label opens a banner; framed lines
/// continue it; the next rule line closes it.
#[derive(Debug, Clone, Default)]
pub struct Lexer {
    in_banner: bool,
}

impl Lexer {
    /// Creates a lexer outside any banner.
    pub fn new() -> Lexer {
        Lexer::default()
    }

    /// True while the previous line left the lexer inside a banner.
    pub fn in_banner(&self) -> bool {
        self.in_banner
    }

    /// Lexes the next line and updates the banner state.
    pub fn next_line(&mut self, line: &str) -> Vec<Span> {
        let spans = lex_line(line, self.in_banner);
        let text = line.trim();
        if banner::is_rule_line(text) {
            // A rule opens a banner, and the rule after the art closes it.
            self.in_banner = !self.in_banner;
        } else if banner::parse_label(text).is_some() {
            self.in_banner = true;
        } else if !(self.in_banner && banner::is_framed_line(text)) {
            self.in_banner = false;
        }
        spans
    }
}

/// Lexes a whole text; span offsets are absolute byte offsets into `text`.
pub fn lex_document(text: &str) -> Vec<Span> {
    let mut lexer = Lexer::new();
    let mut out = Vec::new();
    let mut offset = 0;
    for line in text.split_inclusive('\n') {
        for s in lexer.next_line(line) {
            out.push(Span {
                start: s.start + offset,
                end: s.end + offset,
                kind: s.kind,
            });
        }
        offset += line.len();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(line: &str) -> Vec<(SpanKind, &str)> {
        lex(line)
            .into_iter()
            .map(|s| (s.kind, &line[s.start..s.end]))
            .collect()
    }

    #[test]
    fn host_line() {
        assert_eq!(
            kinds("Host vps v\n"),
            vec![
                (SpanKind::HostKeyword, "Host"),
                (SpanKind::Whitespace, " "),
                (SpanKind::HostName, "vps"),
                (SpanKind::Whitespace, " "),
                (SpanKind::Alias, "v"),
                (SpanKind::Whitespace, "\n"),
            ]
        );
    }

    #[test]
    fn proxy_values() {
        assert_eq!(
            kinds("  ProxyCommand ssh -W %h:%p bastion"),
            vec![
                (SpanKind::Whitespace, "  "),
                (SpanKind::Key, "ProxyCommand"),
                (SpanKind::Whitespace, " "),
                (SpanKind::ProxyCommand, "ssh -W %h:%p bastion"),
            ]
        );
        assert_eq!(
            kinds("proxyjump=bastion")[2],
            (SpanKind::ProxyJump, "bastion")
        );
    }

    #[test]
    fn banner_state() {
        let mut lx = Lexer::new();
        let rule = crate::banner::rule_line();
        assert_eq!(lx.next_line(&rule)[0].kind, SpanKind::Banner);
        assert!(lx.in_banner());
        assert_eq!(lx.next_line("#   art   #")[0].kind, SpanKind::Banner);
        lx.next_line(&rule);
        assert!(!lx.in_banner());
        assert_eq!(lx.next_line("#   art   #")[0].kind, SpanKind::Comment);
    }
}
