# Changelog

All notable changes to this project are documented here. Format follows [Keep a Changelog](https://keepachangelog.com).

## [dhole] - 2026-09-28

### Added
- `Include` support: rustorm resolves the root config's `Include` lines the way ssh does (absolute, `~/`, relative to `~/.ssh`, globs in lexical order, nested includes with a cycle guard, `Include` inside `Host *` treated as global) and works on the root plus every loaded file as one workspace. A config without `Include` behaves byte for byte as before (D21).
- Routing (D22): a host edit writes the file holding the host; `--section` writes the file holding the section, creating a section in no file in the root; `add` without `--section` writes the root; `delete-all` sweeps every file. A section held by two files is refused with `--file` as the remedy; a host defined in two files is edited in ssh's first file with a warning.
- `-f, --file <NAME|FILE>` global option on `add`, `edit`, `clone`, `move`, `set`, `unset`, `delete`, `alias`, `unalias`, `add-section`, `rename-section`, `delete-all` and `dump`; other commands refuse it with exit 2.
- `rustorm includes`: lists every file the `Include` lines load, nested, with host counts, in text and `--json`.
- `rustorm check` reports a host defined in two files, an `Include` that loads a backup-looking file, an `Include` inside `Host *`, and an unreadable include.
- `list` and `sections` print a heading per file on a multi-file workspace; every `--json` row carries `"file"` (D23); write messages name the file when more than one is loaded.
- rustorm-core: `include.rs` (`resolve_includes`, shared glob matcher moved out of `combine.rs`), `workspace.rs` (`Workspace`, `WorkspaceLocation`, per-file save with a writability pre-check so a cross-file move never half-applies, `backup_path_for`), workspace-routed ops returning `Change{messages, files, warnings}`, new errors `AmbiguousSection`, `AmbiguousFile`, `UnknownFile`, `HostExistsIn`, `UnreadableInclude`.
- rustorm-tui: file list pane on `F` (in the Tab cycle), a file column, one editor buffer per file with per-file `Ctrl-S` and backup, `o` opens the host's file at its `Host` line, one quit prompt listing every dirty file.
- rustorm-gui: Files list in the sidebar, file column, editor file selector, per-file Save and backup, Show in Editor, one Quit alert listing every dirty file with Save All.
- docs/cli.md: `## Included files`, `### includes`, D21 to D24; F-54 to F-56 in features.md; `## Files` in tui.md and gui.md; user guide section.

### Changed
- Backups (D24): when an `Include` pattern would match a file's `<file>~`, the backup is written to `<dir>/.<name>~` so neither ssh nor rustorm loads rustorm's own backup; the root keeps `<config>~`.
- An unreadable include is skipped with a warning by read commands and refused with exit 3 by writes routed to it.
- Test registry: 51 new tests (229 total), every catalog case revalidated against the new tree; Makefile `test-core` help text names include and workspace.

## [carp] - 2026-09-28

### Added
- `rustorm combine <FILE> <FILE>... [-o <OUTPUT>] [--on-conflict fail|keep|replace] [--stdout]` (alias `merge`): merges config files into the first one, sections by name, unsectioned hosts into the catch-all, `Host *` key by key with a report, `Include` and `Match` lines kept; duplicate names fail and write nothing unless `keep` or `replace` is given; `--json` summary; backup before the write. Warns when an `Include` still loads an input file.
- `rustorm add-section <NAME> [--before <SECTION>]`: creates an empty section before the catch-all or before a named section, creating the catch-all on an unsectioned file.
- rustorm-core: `combine`, `OnConflict`, `CombineReport`, `Config::add_section`, `Error::SectionExists`, `Error::CombineConflicts`, a `*`/`?` glob matcher for `Include` patterns.
- rustorm-tui: `n` in the table or section list opens a New section form.
- rustorm-gui: New section… button in the sidebar with a name dialog.
- docs/cli.md sections for both commands (F-52, F-53), examples run by the examples test (22 blocks).
- Makefile: `test-core`, `test-cli`, `test-tui`, `test-gui`.

### Changed
- Test registry: 25 new tests (178 total); every catalog case revalidated against the new tree.

## [basilisk] - 2026-09-25

### Added
- rustorm-core: byte-preserving parser and writer for ssh_config with section banners in the FIGlet standard font, canonical key case, multi-valued keys, alphabetical sections with a last catch-all, 0600 file creation, backup before write, atomic writes, a shared ssh_config lexer, every documented operation as a library call with exact messages and exit codes, JSON models, and rustorm's own TOML config in the per-OS config dir (62 tests).
- rustorm CLI: all 20 commands and aliases from docs/cli.md, global options anywhere on the line, exact messages and exit codes, --json output, color rules, shell completion; verified by a test that runs every example block in the reference (13 tests).
- rustorm-tui: terminal UI with a sections list, a host table sortable and filterable by section, host, user, hostname, port, proxy and jump machine, add/edit/delete/clone/move forms, and an embedded config editor with syntax highlighting saving through the core (45 tests plus a real pty run).
- rustorm-gui: egui desktop app with a sections sidebar, the same sortable and filterable table, an add/edit/delete/clone/move form, and the highlighted embedded editor (36 tests plus a real launch).
- Cargo workspace with deny.toml enforcing permissive-only licences via cargo deny.
- docs/tui.md and docs/gui.md design documents audited by UXMASTER; docs/USERGUIDE.md operating guide.

### Changed
- Makefile: workspace-wide build and test, run-tui and run-gui, per-crate lint targets, licences included in check, docs-check covers every product doc.
- docs/cli.md: the global --section option is listed; one example's column padding corrected.
- TESTMASTER registry: 153 tests adopted with measured durations; all 98 derived cases linked and valid (internal).

## [anoa] - 2026-09-24

### Added
- docs/about.md: what rustorm is, origins in stormssh and dbrady/ssh-config, direction toward a GUI, the no-copyleft rule, and the config-file-is-the-database principle.
- docs/features.md: 51 numbered capabilities (F-01 to F-51) from stormssh, ssh-config and the new sections feature, each with origin, MoSCoW priority and a settled Port decision (45 yes, 6 no).
- docs/cli.md: the command reference the implementation is built against: 20 commands, global options, connection-URI grammar, sections chapter with the banner grammar, option placement, files, environment, exit codes, and the agreed Decisions table D1 to D20.
- docs/functionality.md: URI grammar and default resolution, config-file semantics, section semantics, rustorm's own config file, error catalog, and parity notes against both reference tools.
- docs/prd.md: problem, customer, positioning, non-goals (including the six excluded features and the permanent no-copyleft rule), success signals, open questions.
- Makefile and makehelp.sh (2-layer convention): help, docs-check, licenses (cargo deny gate), run (docs smoke path until code exists), and Rust build/test/lint targets that no-op until Cargo.toml appears.

### Changed
- Reference inventories of stormssh (v0.7.0, c752def) and ssh-config (v0.1.3, a407952) with a 39-row capability comparison, under .claude/iterate/research/ (internal).
- TESTMASTER adopted with an empty registry and catalog under .claude/testmaster/ (internal).
