//! rustorm-gui: the egui desktop interface to rustorm.
//!
//! [`App`] implements `eframe::App` and is built from a config path. Every
//! read and write goes through `rustorm_core`; see docs/gui.md.

mod app;
pub mod highlight;
pub mod ops;
pub mod rows;

pub use app::{
    recommendation_text, App, CleanupView, CombineFile, CombineView, ConflictsView, Dialog, Form,
    FormMode, RetireView, Tab, DISCARD_ALL_LABEL, DISCARD_LABEL,
};
