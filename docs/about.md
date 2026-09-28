# About rustorm

rustorm is a Rust command-line manager for the hosts in `~/.ssh/config`. It adds, edits, clones, moves, deletes, lists, shows and searches host entries, and groups them into sections, all while preserving comments, blank lines and hand-written formatting on every write.

The binary and crate are both named `rustorm`, short for "rust storm." The repository that holds the source is `rs-storm`.

## Origins

rustorm is a Rust re-implementation of [stormssh](https://github.com/emre/storm) (MIT license, v0.7.0 baseline, commit `c752def`, 2018), informed by the design of [dbrady/ssh-config](https://github.com/dbrady/ssh-config) (MIT license, v0.1.3, commit `a407952`, 2020). Both projects are read for their behavior, not for their code: rustorm's implementation is original Rust, nothing is copied from either source tree.

## Why rustorm exists

stormssh is Python 2/3-era code and ssh-config is Ruby; neither project is maintained. Both give a useful command-line workflow for editing `~/.ssh/config` without hand-editing it directly. rustorm carries that workflow into a single native binary with no interpreter dependency, adds a sections feature that groups hosts under named banners, and treats every hand edit to the config file as something to preserve rather than overwrite.

## Direction

rustorm starts as a CLI, the same shape as stormssh's command set. Once the CLI is solid, it merges with another project to grow a graphical interface around the same core. The `--json` output on read commands (see `docs/cli.md`) exists for that GUI to consume; it is not a general scripting feature bolted on after the fact.

## Licensing rule

rustorm depends only on permissively licensed crates: MIT, Apache-2.0, BSD, ISC, Zlib, Unicode or MPL-2.0 at file scope. No copyleft dependency is acceptable, direct or transitive: no GPL, LGPL, AGPL, SSPL, EUPL or similar license anywhere in the dependency tree (see D20 in `docs/cli.md`). This is stricter than either reference tool: stormssh depends on the LGPL-licensed `paramiko`; ssh-config has no runtime dependencies at all.

## The config file is the database

`~/.ssh/config` is the only place rustorm keeps host state. Every command reads it fresh and writes it back; there is no separate cache, index or database file for hosts. rustorm's own config file (see D10 and D15 in `docs/cli.md`) holds only command aliases and defaults, such as whether backups are on by default, never host data.

## Status

rustorm is in the definition stage. No Rust code or Cargo project exists yet. `docs/cli.md` is the contract the implementation will be built against, with its Decisions table (D1 through D20) settled and binding.

The feature-exclusion review is complete: `docs/features.md` marks 45 capabilities as ported and 6 as not, by id. rustorm has no web UI, no web JSON routes, no embeddable library, no companion apps, no comma-joined multi-name `edit`, and no `-` value that deletes a key. `--json` on the read commands is the machine interface the GUI project consumes.

## Read next

- `docs/features.md` — the full feature inventory, numbered F-NN, with origin and status per feature.
- `docs/cli.md` — the command-line reference and the Decisions table (D1-D20).
- `docs/functionality.md` — how the config-file parser, sections and defaults behave.
- `docs/prd.md` — the product requirements this implementation is scoped against.
