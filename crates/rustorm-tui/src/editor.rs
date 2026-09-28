//! The embedded editor (docs/tui.md, Editor).
//!
//! A `tui-textarea` holds the text and handles editing keys. Drawing is done
//! here: every visible line is styled by the core lexer's spans, with banner
//! state carried from the top of the file, and scrolls horizontally instead
//! of wrapping.

use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph};
use ratatui::Frame;
use rustorm_core::Lexer;
use tui_textarea::{CursorMove, TextArea};

use crate::theme::Theme;

/// Editor state: the text area and the viewport.
pub struct Editor {
    /// The text and cursor.
    pub area: TextArea<'static>,
    top: usize,
    left: usize,
}

fn split(text: &str) -> Vec<String> {
    text.split('\n').map(str::to_string).collect()
}

impl Editor {
    /// An editor holding `text`.
    pub fn new(text: &str) -> Editor {
        Editor {
            area: TextArea::new(split(text)),
            top: 0,
            left: 0,
        }
    }

    /// The buffer; `text()` of `Editor::new(t)` is `t` byte for byte.
    pub fn text(&self) -> String {
        self.area.lines().join("\n")
    }

    /// Replaces the buffer, keeping the cursor line where possible.
    pub fn set_text(&mut self, text: &str) {
        let (row, _) = self.area.cursor();
        self.area = TextArea::new(split(text));
        self.jump(row);
    }

    /// `(row, col)`, zero-based.
    pub fn cursor(&self) -> (usize, usize) {
        self.area.cursor()
    }

    /// Moves the cursor to the start of `row` (zero-based, clamped).
    pub fn jump(&mut self, row: usize) {
        let row = row.min(u16::MAX as usize) as u16;
        self.area.move_cursor(CursorMove::Jump(row, 0));
    }

    /// Row of the `Host` line that names `name`.
    pub fn find_host_line(&self, name: &str) -> Option<usize> {
        self.area.lines().iter().position(|l| {
            let mut words = l.split_whitespace();
            words.next().is_some_and(|w| w.eq_ignore_ascii_case("host"))
                && words.any(|w| w.trim_matches('"') == name)
        })
    }

    /// Draws `block` and the highlighted visible lines inside it.
    pub fn render(
        &mut self,
        frame: &mut Frame,
        area: Rect,
        block: Block,
        theme: &Theme,
        focused: bool,
    ) {
        let inner = block.inner(area);
        frame.render_widget(block, area);
        let (h, w) = (inner.height as usize, inner.width as usize);
        if h == 0 || w == 0 {
            return;
        }
        let (crow, ccol) = self.area.cursor();
        if crow < self.top {
            self.top = crow;
        } else if crow >= self.top + h {
            self.top = crow + 1 - h;
        }
        if ccol < self.left {
            self.left = ccol;
        } else if ccol >= self.left + w {
            self.left = ccol + 1 - w;
        }
        let mut lexer = Lexer::new();
        let mut out = Vec::new();
        for (i, line) in self.area.lines().iter().enumerate() {
            if i >= self.top + h {
                break;
            }
            let spans = lexer.next_line(line);
            if i < self.top {
                continue;
            }
            let mut cells: Vec<(char, Style)> = Vec::new();
            for s in &spans {
                let style = theme.span(s.kind);
                for ch in line[s.start..s.end].chars() {
                    match ch {
                        '\r' | '\n' => {}
                        '\t' => cells.push((' ', style)),
                        c => cells.push((c, style)),
                    }
                }
            }
            if focused && i == crow {
                while cells.len() <= ccol {
                    cells.push((' ', Style::default()));
                }
                cells[ccol].1 = cells[ccol].1.add_modifier(Modifier::REVERSED);
            }
            let mut spans_out: Vec<Span> = Vec::new();
            let mut run = String::new();
            let mut run_style = Style::default();
            for (c, st) in cells.into_iter().skip(self.left).take(w) {
                if st != run_style && !run.is_empty() {
                    spans_out.push(Span::styled(std::mem::take(&mut run), run_style));
                }
                run_style = st;
                run.push(c);
            }
            if !run.is_empty() {
                spans_out.push(Span::styled(run, run_style));
            }
            out.push(Line::from(spans_out));
        }
        frame.render_widget(Paragraph::new(out), inner);
    }
}
