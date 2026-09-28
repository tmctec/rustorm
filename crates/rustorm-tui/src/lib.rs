//! rustorm-tui: a ratatui view over an ssh_config file.
//!
//! [`App`] is the whole state machine. It takes key events through
//! [`App::handle`] and draws through [`App::render`], so tests drive it on
//! ratatui's `TestBackend` without a terminal. Every read and write goes
//! through `rustorm-core`.

pub mod app;
pub mod editor;
pub mod hosts;
pub mod theme;

pub use app::{App, Focus, Options};
pub use hosts::{Column, Filters, Row, Sort};
pub use theme::Theme;
