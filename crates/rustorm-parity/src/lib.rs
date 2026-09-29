//! rustorm-parity holds no code of its own. Its tests (`tests/parity.rs`)
//! drive `rustorm-tui` with key events and `rustorm-gui` with kittest
//! clicks and typing through the same operations on identical
//! workspaces, and require both to leave byte-identical files and
//! backups, equal to what `rustorm-core` writes for the same operation.
