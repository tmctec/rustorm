//! Section banners: the comment block that opens a section.
//!
//! A banner is [`BANNER_WIDTH`] columns wide: a rule line, a label line
//! `section: <name>` centered between `#` and `#`, the name in the FIGlet
//! standard font with every row centered inside `#…#`, and a closing rule
//! line. Parsing reads the name from the label line only; the art is
//! decorative.

use crate::figlet;

/// Width of every banner line in columns, `#` delimiters included.
pub const BANNER_WIDTH: usize = 103;

/// Width of the area between the two `#` delimiters.
pub const BANNER_INNER_WIDTH: usize = BANNER_WIDTH - 2;

/// The prefix of a banner label, after trimming the framed content.
pub const LABEL_PREFIX: &str = "section:";

/// Returns the rule line: `#`, 101 dashes, `#`.
pub fn rule_line() -> String {
    format!("#{}#", "-".repeat(BANNER_INNER_WIDTH))
}

fn framed(content: &str) -> String {
    let width = content.chars().count();
    if width >= BANNER_INNER_WIDTH {
        return format!("#{content}#");
    }
    let left = (BANNER_INNER_WIDTH - width) / 2;
    let right = BANNER_INNER_WIDTH - width - left;
    format!("#{}{}{}#", " ".repeat(left), content, " ".repeat(right))
}

/// Returns the banner for section `name`, one string per line, without line
/// terminators.
pub fn banner_lines(name: &str) -> Vec<String> {
    let mut lines = vec![rule_line(), framed(&format!("{LABEL_PREFIX} {name}"))];
    lines.extend(
        figlet::render_wrapped(name, BANNER_INNER_WIDTH)
            .iter()
            .map(|row| framed(row)),
    );
    lines.push(rule_line());
    lines
}

/// Returns the banner for section `name` as text, every line ending in `\n`.
pub fn banner_text(name: &str) -> String {
    banner_lines(name).into_iter().map(|l| l + "\n").collect()
}

/// True when `line` is a banner rule: `#`, at least three dashes, `#`, and
/// optional trailing whitespace. Any width is accepted so hand-written
/// banners are recognized.
pub fn is_rule_line(line: &str) -> bool {
    let t = line.trim_end();
    t.len() >= 5
        && t.starts_with('#')
        && t.ends_with('#')
        && t[1..t.len() - 1].chars().all(|c| c == '-')
}

/// True when `line` is framed: it starts with `#` and, after trailing
/// whitespace is trimmed, ends with `#`, with at least two characters.
pub fn is_framed_line(line: &str) -> bool {
    let t = line.trim_end();
    t.len() >= 2 && t.starts_with('#') && t.ends_with('#')
}

/// Returns the section name from a label line `#  section: <name>  #`, or
/// `None` when `line` is not a label line.
pub fn parse_label(line: &str) -> Option<String> {
    if !is_framed_line(line) || is_rule_line(line) {
        return None;
    }
    let t = line.trim_end();
    let inner = t[1..t.len() - 1].trim();
    let name = inner.strip_prefix(LABEL_PREFIX)?.trim();
    if name.is_empty() {
        None
    } else {
        Some(name.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn label_round_trips() {
        let lines = banner_lines("bob");
        assert_eq!(parse_label(&lines[1]).as_deref(), Some("bob"));
        assert!(lines.iter().all(|l| l.chars().count() == BANNER_WIDTH));
        assert!(is_rule_line(&lines[0]));
        assert!(is_rule_line(lines.last().unwrap()));
    }

    #[test]
    fn rejects_plain_comments() {
        assert!(!is_rule_line("# ---"));
        assert!(parse_label("# section: x").is_none());
        assert!(parse_label("#-----#").is_none());
    }
}
