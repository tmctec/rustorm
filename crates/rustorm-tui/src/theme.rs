//! Styles and glyphs, resolved once at startup (docs/tui.md, Layout and
//! Editor). Nothing outside this module names a color.

use ratatui::style::{Color, Modifier, Style};
use rustorm_core::SpanKind;

/// Whether colors and Unicode glyphs are on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Theme {
    /// Foreground colors. Off under `NO_COLOR` or `TERM=dumb`; modifiers stay.
    pub color: bool,
    /// Unicode glyphs (`▲ ▼ · ✔`). Off without a UTF-8 locale or on `TERM=dumb`.
    pub unicode: bool,
}

impl Theme {
    /// Colors and Unicode on.
    pub fn full() -> Theme {
        Theme {
            color: true,
            unicode: true,
        }
    }

    /// Colors and Unicode off.
    pub fn plain() -> Theme {
        Theme {
            color: false,
            unicode: false,
        }
    }

    /// Resolves the theme from the environment.
    pub fn detect() -> Theme {
        let var = |k: &str| std::env::var(k).ok().filter(|v| !v.is_empty());
        let dumb = var("TERM").is_none_or(|t| t == "dumb");
        let no_color = var("NO_COLOR").is_some();
        let utf8 = ["LC_ALL", "LC_CTYPE", "LANG"]
            .iter()
            .find_map(|k| var(k))
            .is_some_and(|v| {
                let v = v.to_ascii_uppercase();
                v.contains("UTF-8") || v.contains("UTF8")
            });
        Theme {
            color: !no_color && !dumb,
            unicode: utf8 && !dumb,
        }
    }

    fn fg(&self, color: Color) -> Style {
        if self.color {
            Style::default().fg(color)
        } else {
            Style::default()
        }
    }

    /// The editor style for one lexer span kind.
    pub fn span(&self, kind: SpanKind) -> Style {
        match kind {
            SpanKind::Comment => self.fg(Color::DarkGray).add_modifier(Modifier::ITALIC),
            SpanKind::Banner => self.fg(Color::Magenta).add_modifier(Modifier::DIM),
            SpanKind::HostKeyword => self.fg(Color::Yellow).add_modifier(Modifier::BOLD),
            SpanKind::HostName => self.fg(Color::Green).add_modifier(Modifier::BOLD),
            SpanKind::Alias => self.fg(Color::Green),
            SpanKind::Key => self.fg(Color::Cyan),
            SpanKind::Value | SpanKind::Whitespace => Style::default(),
            SpanKind::ProxyCommand => self.fg(Color::LightRed).add_modifier(Modifier::UNDERLINED),
            SpanKind::ProxyJump => self
                .fg(Color::LightBlue)
                .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
            SpanKind::Unknown => self.fg(Color::Red).add_modifier(Modifier::REVERSED),
        }
    }

    /// Error message style.
    pub fn error(&self) -> Style {
        self.fg(Color::Red).add_modifier(Modifier::BOLD)
    }

    /// Success message style.
    pub fn success(&self) -> Style {
        self.fg(Color::Green)
    }

    /// Hints and missing values.
    pub fn muted(&self) -> Style {
        Style::default().add_modifier(Modifier::DIM)
    }

    /// Marker for a missing cell value.
    pub fn missing(&self) -> &'static str {
        if self.unicode {
            "·"
        } else {
            "-"
        }
    }

    /// Sort direction marker.
    pub fn arrow(&self, ascending: bool) -> &'static str {
        match (self.unicode, ascending) {
            (true, true) => "▲",
            (true, false) => "▼",
            (false, true) => "^",
            (false, false) => "v",
        }
    }

    /// Success prefix.
    pub fn ok(&self) -> &'static str {
        if self.unicode {
            "✔"
        } else {
            "[ok]"
        }
    }

    /// Text cursor in form and filter inputs.
    pub fn caret(&self) -> &'static str {
        if self.unicode {
            "▏"
        } else {
            "_"
        }
    }

    /// Marker before a file whose editor buffer has unsaved edits. Also
    /// bold, so it reads without the glyph.
    pub fn dirty(&self) -> &'static str {
        if self.unicode {
            "•"
        } else {
            "*"
        }
    }

    /// Stands for the elided start of a long path.
    pub fn ellipsis(&self) -> &'static str {
        if self.unicode {
            "…"
        } else {
            "..."
        }
    }
}
