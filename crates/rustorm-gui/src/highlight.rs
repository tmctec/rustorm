//! ssh_config syntax highlighting for the editor, driven by the core lexer.

use egui::text::{LayoutJob, TextFormat};
use egui::{Color32, FontId, Stroke};
use rustorm_core::{lex_document, SpanKind};

/// The color of `kind` on a dark (`dark`) or light background. Every kind
/// the editor can show has its own color, and each palette keeps 4.5:1
/// contrast against egui's editor background.
pub fn color(kind: SpanKind, dark: bool) -> Color32 {
    let hex = if dark {
        match kind {
            SpanKind::Comment => 0x8b949e,
            SpanKind::Banner => 0xd2a8ff,
            SpanKind::HostKeyword => 0xff7b72,
            SpanKind::HostName => 0xffa657,
            SpanKind::Alias => 0xe3b341,
            SpanKind::Key => 0x79c0ff,
            SpanKind::Value | SpanKind::Whitespace => 0xd0d7de,
            SpanKind::ProxyCommand => 0x7ee787,
            SpanKind::ProxyJump => 0x56d4dd,
            SpanKind::Unknown => 0xf85149,
        }
    } else {
        match kind {
            SpanKind::Comment => 0x57606a,
            SpanKind::Banner => 0x8250df,
            SpanKind::HostKeyword => 0xcf222e,
            SpanKind::HostName => 0x953800,
            SpanKind::Alias => 0x6f4400,
            SpanKind::Key => 0x0550ae,
            SpanKind::Value | SpanKind::Whitespace => 0x24292f,
            SpanKind::ProxyCommand => 0x116329,
            SpanKind::ProxyJump => 0x055d69,
            SpanKind::Unknown => 0xa40e26,
        }
    };
    Color32::from_rgb((hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
}

/// Lays out `text` as a [`LayoutJob`] with one section per lexer span,
/// colored by its [`SpanKind`]. Unknown text is also underlined, so it does
/// not rely on color alone.
pub fn highlight_job(text: &str, dark: bool, font: FontId) -> LayoutJob {
    let mut job = LayoutJob::default();
    for span in lex_document(text) {
        let mut format = TextFormat::simple(font.clone(), color(span.kind, dark));
        if span.kind == SpanKind::Unknown {
            format.underline = Stroke::new(1.0, color(span.kind, dark));
        }
        job.append(&text[span.start..span.end], 0.0, format);
    }
    job
}

/// The [`SpanKind`] each section of a job built by [`highlight_job`]
/// carries, found by its color.
pub fn kind_of(color32: Color32, dark: bool) -> Option<SpanKind> {
    [
        SpanKind::Comment,
        SpanKind::Banner,
        SpanKind::HostKeyword,
        SpanKind::HostName,
        SpanKind::Alias,
        SpanKind::Key,
        SpanKind::Value,
        SpanKind::ProxyCommand,
        SpanKind::ProxyJump,
        SpanKind::Unknown,
    ]
    .into_iter()
    .find(|k| color(*k, dark) == color32)
}
