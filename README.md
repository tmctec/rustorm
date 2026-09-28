# rustorm

Manage the hosts in your `~/.ssh/config` from the command line, a terminal UI or a desktop window. rustorm is a Rust re-implementation of the features of [stormssh](https://github.com/emre/storm) and [ssh-config](https://github.com/dbrady/ssh-config), with one addition: **sections**, banner comments that group hosts, keep them alphabetical and stay valid ssh_config.

The config file is the only database. Every write backs the file up first, keeps untouched lines byte for byte, and creates the file with mode 0600 when missing.

## Programs

| Binary | What it is | Try |
|---|---|---|
| `rustorm` | Command-line tool, one host operation per call | `rustorm list` |
| `rustorm-tui` | Full-screen terminal UI (ratatui) with an embedded highlighted editor | `rustorm-tui` |
| `rustorm-gui` | Desktop window (egui) with the same table, forms and editor | `rustorm-gui` |

## Install

Requires a Rust toolchain (rustup).

```
make install            # debug symlinks into ~/.local/bin
make install-production # release copies
```

## Quick start

```
rustorm add vps root@vps.example.com:2222     # new host from a connection URI
rustorm list                                  # every host, sorted, with user@hostname:port
rustorm set vps User deploy Port 22           # change keys
rustorm move vps --section work               # put a host in a section (creates it)
rustorm add-section lab                       # create an empty section
rustorm combine ~/.ssh/config ~/.ssh/config.d/cypress   # merge a second file into your config
rustorm search 'example\.com'                 # regex over names, aliases, keys and values
rustorm check                                 # unknown keys, missing identity files, duplicates
```

`rustorm --help` lists every command. `--json` gives machine-readable output on read commands. Options may appear anywhere on the line.

## Documentation

- [User guide](docs/USERGUIDE.md)
- [Command-line reference](docs/cli.md), the contract every message and exit code follows
- [Terminal UI](docs/tui.md) and [desktop window](docs/gui.md)
- [Features](docs/features.md), [functionality](docs/functionality.md), [about](docs/about.md), [PRD](docs/prd.md)

## Development

```
make help        # every target
make build       # whole workspace
make test        # every crate's tests, including one that runs every example in docs/cli.md
make check       # clippy + tests + docs-check + licenses
```

Workspace crates: `rustorm-core` (parser, writer, sections, operations), `rustorm` (CLI, clap), `rustorm-tui` (ratatui), `rustorm-gui` (egui/eframe).

## License

Apache License 2.0. See [LICENSE](LICENSE) and [NOTICE](NOTICE). No copyleft dependencies: `make licenses` runs cargo-deny against the allow list in `deny.toml`.
