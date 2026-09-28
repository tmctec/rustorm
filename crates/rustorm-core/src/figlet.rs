//! FIGlet rendering in the standard font, used to draw section banners.
//!
//! The font is FIGlet's `standard.flf`, embedded unmodified (BSD-3-Clause, see
//! `fonts/LICENSE-FIGLET`). The renderer implements FIGlet's horizontal
//! smushing with the rules the standard font enables (equal character,
//! underscore, hierarchy, opposite pair), so its output matches `figlet`
//! character for character for printable ASCII.

use std::sync::OnceLock;

const STANDARD_FLF: &str = include_str!("../fonts/standard.flf");

/// A parsed FIGlet font covering printable ASCII (code points 32 to 126).
#[derive(Debug, Clone)]
pub struct Font {
    hardblank: char,
    height: usize,
    smush_rules: u32,
    glyphs: Vec<Vec<Vec<char>>>,
}

const SM_EQUAL: u32 = 1;
const SM_LOWLINE: u32 = 2;
const SM_HIERARCHY: u32 = 4;
const SM_PAIR: u32 = 8;
const SM_BIGX: u32 = 16;
const SM_HARDBLANK: u32 = 32;
const SM_KERN: u32 = 64;
const SM_SMUSH: u32 = 128;

impl Font {
    /// Returns the embedded FIGlet standard font.
    pub fn standard() -> &'static Font {
        static FONT: OnceLock<Font> = OnceLock::new();
        FONT.get_or_init(|| Font::parse(STANDARD_FLF).expect("embedded standard.flf parses"))
    }

    /// Parses a FIGlet `.flf` font. Only code points 32 to 126 are read.
    ///
    /// Returns `None` when the header or a glyph is malformed.
    pub fn parse(text: &str) -> Option<Font> {
        let mut lines = text.lines();
        let header = lines.next()?;
        let mut fields = header.split_whitespace();
        let signature = fields.next()?;
        let hardblank = signature.strip_prefix("flf2a")?.chars().next()?;
        let height: usize = fields.next()?.parse().ok()?;
        let _baseline = fields.next()?;
        let _max_length = fields.next()?;
        let old_layout: i64 = fields.next()?.parse().ok()?;
        let comment_lines: usize = fields.next()?.parse().ok()?;
        let _direction = fields.next();
        let full_layout: Option<i64> = fields.next().and_then(|f| f.parse().ok());
        let smush_rules = match full_layout {
            Some(full) => (full & 0xff) as u32,
            None if old_layout == 0 => SM_KERN,
            None if old_layout < 0 => 0,
            None => (old_layout as u32 & 31) | SM_SMUSH,
        };
        for _ in 0..comment_lines {
            lines.next()?;
        }
        let mut glyphs = Vec::with_capacity(95);
        for _ in 32..=126 {
            let mut rows = Vec::with_capacity(height);
            for _ in 0..height {
                let raw = lines.next()?;
                let endmark = raw.chars().last()?;
                let row = raw.trim_end_matches(endmark);
                rows.push(row.chars().collect::<Vec<char>>());
            }
            glyphs.push(rows);
        }
        Some(Font {
            hardblank,
            height,
            smush_rules,
            glyphs,
        })
    }

    /// Number of rows every rendered line has.
    pub fn height(&self) -> usize {
        self.height
    }

    fn glyph(&self, c: char) -> Option<&Vec<Vec<char>>> {
        let code = c as u32;
        if (32..=126).contains(&code) {
            self.glyphs.get((code - 32) as usize)
        } else {
            None
        }
    }

    /// Renders `text` as one block of `height()` rows, all of equal width.
    ///
    /// Characters outside printable ASCII are skipped. Hardblanks render as
    /// spaces. Rows keep their trailing spaces so the block is rectangular.
    pub fn render(&self, text: &str) -> Vec<String> {
        let mut out: Vec<Vec<char>> = vec![Vec::new(); self.height];
        let mut previous_width = 0usize;
        for c in text.chars() {
            let Some(glyph) = self.glyph(c) else {
                continue;
            };
            let width = glyph.first().map_or(0, Vec::len);
            let amount = self.smush_amount(&out, glyph, previous_width, width);
            let out_len = out[0].len();
            for (row, glyph_row) in out.iter_mut().zip(glyph.iter()) {
                for k in 0..amount {
                    let Some(pos) = (out_len + k).checked_sub(amount) else {
                        continue;
                    };
                    let right = glyph_row.get(k).copied().unwrap_or(' ');
                    let merged = self
                        .smush(row[pos], right, previous_width, width)
                        .unwrap_or(right);
                    row[pos] = merged;
                }
                row.extend(glyph_row.iter().skip(amount));
            }
            previous_width = width;
        }
        out.into_iter()
            .map(|row| {
                row.into_iter()
                    .map(|ch| if ch == self.hardblank { ' ' } else { ch })
                    .collect()
            })
            .collect()
    }

    fn smush_amount(
        &self,
        out: &[Vec<char>],
        glyph: &[Vec<char>],
        previous_width: usize,
        width: usize,
    ) -> usize {
        if self.smush_rules & (SM_SMUSH | SM_KERN) == 0 {
            return 0;
        }
        let mut max = width as isize;
        for (row, glyph_row) in out.iter().zip(glyph.iter()) {
            let out_len = row.len() as isize;
            // Last non-space column of the output row (0 when the row is blank).
            let mut line_bd = out_len;
            let mut ch1: Option<char>;
            loop {
                ch1 = if line_bd < out_len {
                    Some(row[line_bd as usize])
                } else {
                    None
                };
                let blank = matches!(ch1, None | Some(' '));
                if line_bd > 0 && blank {
                    line_bd -= 1;
                } else {
                    break;
                }
            }
            let char_bd = glyph_row.iter().take_while(|&&c| c == ' ').count();
            let ch2 = glyph_row.get(char_bd).copied();
            let mut amount = char_bd as isize + out_len - 1 - line_bd;
            match (ch1, ch2) {
                (None | Some(' '), _) => amount += 1,
                (Some(left), Some(right))
                    if self.smush(left, right, previous_width, width).is_some() =>
                {
                    amount += 1;
                }
                _ => {}
            }
            if amount < max {
                max = amount;
            }
        }
        max.max(0) as usize
    }

    fn smush(&self, left: char, right: char, previous_width: usize, width: usize) -> Option<char> {
        if left == ' ' {
            return Some(right);
        }
        if right == ' ' {
            return Some(left);
        }
        if previous_width < 2 || width < 2 {
            return None;
        }
        let rules = self.smush_rules;
        if rules & SM_SMUSH == 0 {
            return None;
        }
        if rules & 63 == 0 {
            if left == self.hardblank {
                return Some(right);
            }
            if right == self.hardblank {
                return Some(left);
            }
            return Some(right);
        }
        if rules & SM_HARDBLANK != 0 && left == self.hardblank && right == self.hardblank {
            return Some(left);
        }
        if left == self.hardblank || right == self.hardblank {
            return None;
        }
        if rules & SM_EQUAL != 0 && left == right {
            return Some(left);
        }
        if rules & SM_LOWLINE != 0 {
            if left == '_' && "|/\\[]{}()<>".contains(right) {
                return Some(right);
            }
            if right == '_' && "|/\\[]{}()<>".contains(left) {
                return Some(left);
            }
        }
        if rules & SM_HIERARCHY != 0 {
            let classes = ["|", "/\\", "[]", "{}", "()", "<>"];
            for (i, class) in classes.iter().enumerate() {
                let higher: String = classes[i + 1..].concat();
                if class.contains(left) && higher.contains(right) {
                    return Some(right);
                }
                if class.contains(right) && higher.contains(left) {
                    return Some(left);
                }
            }
        }
        if rules & SM_PAIR != 0 {
            let pair = matches!(
                (left, right),
                ('[', ']') | (']', '[') | ('{', '}') | ('}', '{') | ('(', ')') | (')', '(')
            );
            if pair {
                return Some('|');
            }
        }
        if rules & SM_BIGX != 0 {
            match (left, right) {
                ('/', '\\') => return Some('|'),
                ('\\', '/') => return Some('Y'),
                ('>', '<') => return Some('X'),
                _ => {}
            }
        }
        None
    }
}

/// Renders `text` in the FIGlet standard font, wrapping at word boundaries so
/// no row is wider than `max_width` columns.
///
/// Returns the rows of every wrapped block, top to bottom. A word wider than
/// `max_width` on its own is split between characters.
pub fn render_wrapped(text: &str, max_width: usize) -> Vec<String> {
    let font = Font::standard();
    let width_of = |s: &str| font.render(s).first().map_or(0, |r| r.chars().count());
    let mut blocks: Vec<String> = Vec::new();
    let mut current = String::new();
    for word in text.split(' ').filter(|w| !w.is_empty()) {
        let candidate = if current.is_empty() {
            word.to_string()
        } else {
            format!("{current} {word}")
        };
        if width_of(&candidate) <= max_width {
            current = candidate;
            continue;
        }
        if !current.is_empty() {
            blocks.push(std::mem::take(&mut current));
        }
        // The word alone may still be too wide: split it between characters.
        for c in word.chars() {
            let candidate = format!("{current}{c}");
            if !current.is_empty() && width_of(&candidate) > max_width {
                blocks.push(std::mem::take(&mut current));
                current.push(c);
            } else {
                current = candidate;
            }
        }
    }
    if !current.is_empty() || blocks.is_empty() {
        blocks.push(current);
    }
    blocks.iter().flat_map(|block| font.render(block)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_hello_like_figlet() {
        let rows = Font::standard().render("hi");
        assert_eq!(
            rows,
            vec![
                " _     _ ",
                "| |__ (_)",
                "| '_ \\| |",
                "| | | | |",
                "|_| |_|_|",
                "         "
            ]
        );
    }

    #[test]
    fn renders_punctuation_and_digits_like_figlet() {
        let rows = Font::standard().render("a-b_c.9");
        assert_eq!(
            rows,
            vec![
                "             _             ___  ",
                "  __ _      | |__     ___ / _ \\ ",
                " / _` |_____| '_ \\   / __| (_) |",
                "| (_| |_____| |_) | | (__ \\__, |",
                " \\__,_|     |_.__/___\\___(_)/_/ ",
                "                |_____|         ",
            ]
        );
    }

    #[test]
    fn wraps_long_text() {
        let rows = render_wrapped("abcdefghijklmnopqrstuvwxyz abcdefghijklmnopqrstuvwxyz", 101);
        assert!(rows.len() > 6);
        assert!(rows.iter().all(|r| r.chars().count() <= 101));
    }
}
