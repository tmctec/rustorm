//! Terminal output: ANSI styling and stdout/stderr writers that ignore a
//! closed pipe.

use std::io::Write;
use std::ops::Range;

use rustorm_core::{lex_document, SpanKind};

/// ANSI styles rustorm uses.
#[derive(Debug, Clone, Copy)]
pub enum Style {
    /// Host names in `list` and `search`.
    Name,
    /// The `Host` keyword in `dump`.
    Keyword,
    /// Aliases on a `Host` line.
    Alias,
    /// Directive keys.
    Key,
    /// Comments and banners.
    Comment,
    /// `search` matches.
    Match,
    /// Section headings.
    Heading,
    /// The `error:` prefix.
    Error,
    /// The `warning:` prefix.
    Warning,
}

impl Style {
    fn code(self) -> &'static str {
        match self {
            Style::Name => "1;36",
            Style::Keyword => "1;35",
            Style::Alias => "36",
            Style::Key => "33",
            Style::Comment => "2",
            Style::Match => "1;31",
            Style::Heading => "1",
            Style::Error => "1;31",
            Style::Warning => "1;33",
        }
    }
}

/// Wraps `text` in `style` when `color` is on.
pub fn paint(text: &str, style: Style, color: bool) -> String {
    if color && !text.is_empty() {
        format!("\x1b[{}m{}\x1b[0m", style.code(), text)
    } else {
        text.to_string()
    }
}

/// `text` with every byte range in `ranges` painted as a match.
pub fn highlight(text: &str, ranges: &[Range<usize>], color: bool) -> String {
    if !color || ranges.is_empty() {
        return text.to_string();
    }
    let mut out = String::new();
    let mut at = 0;
    for r in ranges {
        if r.start < at || r.end > text.len() || r.start == r.end {
            continue;
        }
        out.push_str(&text[at..r.start]);
        out.push_str(&paint(&text[r.start..r.end], Style::Match, true));
        at = r.end;
    }
    out.push_str(&text[at..]);
    out
}

/// The config text with `Host` lines, keys and comments colored.
pub fn color_dump(text: &str) -> String {
    let mut out = String::with_capacity(text.len() * 2);
    for span in lex_document(text) {
        let piece = &text[span.start..span.end];
        let style = match span.kind {
            SpanKind::HostKeyword => Some(Style::Keyword),
            SpanKind::HostName => Some(Style::Name),
            SpanKind::Alias => Some(Style::Alias),
            SpanKind::Key => Some(Style::Key),
            SpanKind::Comment | SpanKind::Banner => Some(Style::Comment),
            _ => None,
        };
        match style {
            Some(s) => {
                // Keep line terminators outside the escape sequence.
                let body = piece.trim_end_matches(['\n', '\r']);
                out.push_str(&paint(body, s, true));
                out.push_str(&piece[body.len()..]);
            }
            None => out.push_str(piece),
        }
    }
    out
}

/// Writes to stdout; a closed pipe is not an error.
pub fn stdout(text: &str) {
    let mut out = std::io::stdout().lock();
    let _ = out.write_all(text.as_bytes());
    let _ = out.flush();
}

/// Writes a line to stdout.
pub fn line(text: &str) {
    stdout(&format!("{text}\n"));
}

/// Writes to stderr.
pub fn stderr(text: &str) {
    let mut err = std::io::stderr().lock();
    let _ = err.write_all(text.as_bytes());
    let _ = err.flush();
}
