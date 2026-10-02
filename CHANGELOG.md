# Changelog

All notable changes to this project are documented here. Format follows [Keep a Changelog](https://keepachangelog.com).

## [anemone] - 2026-10-02

### Added
- Host metadata (D25): `# note:`, `# location:`, `# privateKeyLocation:`, `# other:` and `# tags:` comment lines directly above a `Host` line are keys. `set`, `unset`, `add -o`, `clone` and the settings forms take them like ssh keywords; `set --tag`/`--untag` edit the tag list; `clone`, `move` and `combine` carry the lines; `search` matches them and prints a matching metadata line under the host; `--json` rows of `list`, `show` and `search` carry a `"meta"` object. `privateKeyLocation` refuses key material.
- Reading output (D26, D27): `show`, `list` and `search` take `--where KEY=VALUE|KEY!=VALUE|KEY~PATTERN` (comma OR, repeated AND, contains on list keys), `--filter KEYS` (exactly the named keys: ssh keywords, metadata keys, `Host`, `section`, `file`), `--format txt|json|csv|yaml|yml`, `--just-value` and `--allow-missing`; `--section` selects hosts on `show` and `search`; `show` runs without a name when `--where`/`--section` select. An unset filtered key warns and exits 4. Shell completion offers the key names and format names.
- rustorm-core: `meta` module (`MetaKey`, `HostMeta`, `HostBlock::set_meta`/`append_meta`/`unset_meta`/`add_tags`/`remove_tags`; `get`/`get_all`/`set`/`append`/`unset` route metadata keys), `projection` module (`HostView`, `Where`, `Format`, `resolve`, `project`, `render`, `completion_keys`), `Config::set_with_tags`, `Workspace::set_with_tags`/`view`/`view_of`/`shown_at`; `KeyGroup::Notes` with a `KeySpec` per metadata key, so `SettingsDraft` covers them; `complete_line` completes `# no` to `# note: `.
- rustorm-tui: Notes & location group first in the settings form with the tags in use listed under the tags row; the status line shows the selected host's location, tags and first note; `/` matches metadata.
- rustorm-gui: the detail panel shows location, tag chips and notes; All settings has the Notes & location group with tag chips (× removes, ▾ offers the tags in use); the filter matches metadata.
- Tests: core meta-1..13 and read-1..24 (jq, Python csv and PyYAML read every format back), CLI cases for flag parsing and exit codes, TUI mt-1..6, GUI mg-1..5, parity par-14..15; docs/cli.md gains a Reading output examples block run from fixtures the binary generates (`make fixtures-read`); `make test-meta`.

### Changed
- `--json list/show/search` rows carry a `"meta"` object (`{}` when the host has none).
- `set`'s KEY VALUE list no longer accepts bare hyphen values, so flags after it parse; a value starting with `-` goes after `--`.
- docs/cli.md: D1 names the repository `rustorm`; new sections Host metadata and Reading output; exit code 4.

## [fieldfare] - 2026-09-29

### Added
- rustorm-tui and rustorm-gui: settings forms open on the host's filled keys (empty groups hidden); TUI `Ctrl-T` / GUI Filled–All toggle shows every keyword. An Add setting field completes keyword names as you type (prefix matches first, substring as fallback; algorithm lists only in full) and adds the first match with its premade value selected.
- rustorm-tui and rustorm-gui: raw-editor keyword completion — dim ghost text after the cursor; Space accepts it with the keyword in canonical case, a space and the premade value selected; `Ctrl-Space` swaps a yes/no or fixed-choice value.
- rustorm-gui: Rename section… in the sidebar (Op::RenameSection), merging into an existing name like the TUI's `R`.
- rustorm-core `keyspec`: `SettingsDraft::filled_rows`/`complete`/`add_key`, `complete_setting`, `complete_line` (`LineCompletion::ghost`/`accept`), `premade_value`, `next_choice`, `swap_value` — shared by both UIs.
- crates/rustorm-parity: test-only crate running 13 operations in the TUI (keys) and the GUI (kittest) on identical workspaces, requiring byte-identical files and backups equal to rustorm-core's result and matching messages; `make test-parity`, `make lint-parity`.
- Regression tests: host_at_top (TUI top-1..8, GUI top-gui-1), settings filled view (fv-*), editor completion (ed-*), parity (par-1..13); 66 new catalog cases.

### Fixed
- rustorm-tui: a followed host (table move, `o`, after a write) puts its `Host` line at the top of the editor view (`Editor::show_at_top`), not on the bottom row after scrolling down.
- rustorm-gui: the Editor tab scrolls a followed host's `Host` line exactly to the top in the same frame (the old `scroll_to_rect` target ignored the TextEdit margin and cut the line off), with bottom space so the last host reaches the top.
- rustorm-gui: the editor save message reads "Saved <file>." as the TUI's does.
- rustorm-tui: a clone keeps the source's section without naming it, so its message matches the CLI's `clone`.

### Changed
- TUI key overlay and editor help bar list Space (accept keyword), Ctrl-Space and Ctrl-T; tf-*/tf-gui-* tests switch to the all view first.
- docs: tui.md, gui.md and USERGUIDE.md describe the host-at-top follow, filled view and Add setting, editor completion, and GUI rename section.

## [eagleray] - 2026-09-28

### Added
- rustorm-tui: the editor follows the browsing panes. A new file-list highlight shows that file; a new selected host (movement, sort, filter, or a write) shows its file with the cursor on its `Host` line, read from the buffer so unsaved edits are honored. Focus never moves, an unchanged selection never moves the cursor, and returning to the table after browsing files re-syncs to the selected host.
- rustorm-tui: `Enter` on a host opens `Settings <host>`: every settable keyword in six groups (Connection, Authentication, Forwarding, Proxy, Multiplexing, Advanced), yes/no and fixed choices cycled with Space/Left/Right including unset, other values typed, `Ctrl-U` clear, `PgUp`/`PgDn` group jump, repeatable keys one row per value, `Host *` values shown as inherited; one write with one backup, `No changes.` when nothing changed.
- rustorm-gui: selecting a host (click, or Up/Down with no text field focused) points the editor at its file and queues the cursor on its `Host` line for the Editor tab, without leaving the Hosts tab.
- rustorm-gui: All settings section in the detail panel with collapsible groups, drop-downs (not set / values) for flags and closed choices, text fields with a ▾ word menu for open choices, inline value problems, add/remove rows for repeatable keys, Save settings (one write) and Reset; disabled under the editor guard.
- rustorm-core: `keyspec` module — `KeySpec`/`KeyType`/`KeyGroup` for every settable keyword, `validate_setting`, `SettingChange`, `SettingsDraft`/`SettingRow` (the form model both UIs share); `Config::apply_settings` and `Workspace::apply_settings` apply a batch of sets and unsets to one host in one write; new `Error::InvalidSetting`.
- Makefile: `all` alias for `build`; `test-gui-walkthrough` records the GUI follow and All settings flows.
- docs: Settings flow and editor-follow behavior in tui.md, gui.md and the user guide; F-57 and F-58 in features.md.

### Changed
- rustorm-tui: `e` alone opens the quick edit form; `Enter` on a host opens Settings. The table help bar reads `/ f:filter … e:edit  Enter:settings …` to fit 150 columns.
- rustorm-gui: `App::editor_line` reports every cursor placement (Show in Editor or a followed host); `App::pending_editor_line` exposes the queued one.
- Test registry: 48 new catalog cases (R-editor-follows, R-typed-host-form), 47 new tests, every earlier case revalidated green.

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
