# rustorm user guide

rustorm manages the hosts in your `~/.ssh/config` from the command line, a terminal UI, or a desktop window. Every tool edits the same file, keeps your comments and formatting, and writes a backup before each change.

## Install

From the repository root:

```
make prereqs        # installs the Rust toolchain and cargo-deny if missing
make install        # builds and symlinks rustorm, rustorm-tui and rustorm-gui into ~/.local/bin
```

`make install-production` copies optimized binaries instead of symlinks. `make uninstall` removes all three.

## The three programs

| Program | What it is | Start it with |
|---|---|---|
| `rustorm` | Command-line tool, one host operation per call | `rustorm list` |
| `rustorm-tui` | Full-screen terminal UI | `rustorm-tui` |
| `rustorm-gui` | Desktop window | `rustorm-gui` |

All three read `~/.ssh/config` by default. `--config FILE` or the `RUSTORM_CONFIG` environment variable points them at another file; the flag wins when both are set.

## Command line

`docs/cli.md` is the complete reference. The everyday commands:

```
rustorm add vps root@vps.example.com:2222     # new host from a connection URI
rustorm list                                  # every host, sorted, with user@hostname:port
rustorm search 'example\.com'                 # regex over names, aliases, keys and values
rustorm set vps User deploy Port 22           # change keys without restating the URI
rustorm clone rails01 rails02                 # copy a host; HostName follows the new name
rustorm move vps --section work               # put a host in a section
rustorm add-section lab                       # create an empty section
rustorm combine ~/.ssh/config ~/.ssh/config.d/cypress   # merge a second file into your config
rustorm delete vps                            # remove a host
rustorm check                                 # report unknown keys, missing files, duplicates
rustorm includes                              # the files your Include lines load
```

`rustorm --help` lists every command; `rustorm <command> --help` shows its options. Add `--json` before a read command (`rustorm --json list`) for machine-readable output.

Shell completion: `rustorm completion zsh > ~/.zfunc/_rustorm` (also `bash`, `fish`, `powershell`).

## Sections

A section groups hosts under a banner comment in the config file; ssh ignores the banner. Use `--section NAME` on `add`, `edit`, `clone` or `move`. The first section you create also creates a catch-all section named `other` for every host not yet under a banner. Hosts inside a section stay in alphabetical order, the catch-all stays last, and `rustorm rename-section other personal` renames it while keeping that role. `rustorm sections` lists them with host counts. `rustorm add-section lab` creates an empty section (`--before work` places it in front of `work`); on a file with no sections it also creates the catch-all.

## Combining config files

`rustorm combine FILE FILE...` merges two or more ssh config files into the first one. Hosts from the later files are added to the first: a host under a section joins the section of the same name, creating it if needed; a host without a section joins the catch-all; `Host *` settings the first file lacks are added and the ones it already has are kept, with a line telling you which. Comments, `Include` and `Match` lines all survive.

```
rustorm combine ~/.ssh/config ~/.ssh/config.d/cypress          # write the result to ~/.ssh/config
rustorm combine a.conf b.conf -o merged.conf                   # write it somewhere else
rustorm combine a.conf b.conf --stdout                         # print it, write nothing
rustorm combine a.conf b.conf --on-conflict keep               # a name in both files: keep a.conf's
rustorm combine a.conf b.conf --on-conflict replace            # ...or take b.conf's
```

A host name that appears in two files is a conflict. Without `--on-conflict`, `combine` lists every conflict and writes nothing, so it never silently overwrites a host. The file that gets written is backed up first, like every other change. If your config has an `Include` line that still loads one of the files you merged, `combine` warns you so you can remove the line yourself.

## Included files

If your config has an `Include` line, rustorm follows it the way ssh does and treats the main config plus every file it loads as one set. `Include` takes absolute paths, `~/` paths or paths relative to `~/.ssh`, and wildcards such as `config.d/*`; included files may hold `Include` lines of their own. Every command sees every host, so `list`, `search` and `check` cover all files, and `list` and `sections` print a heading per file. A config without `Include` works exactly as described above.

Changes go to the right file without you naming it:

- Editing, renaming, deleting or aliasing a host writes the file that holds the host.
- `--section NAME` on `add`, `clone` or `move` writes the file that holds that section. A section held by no file is created in the main config.
- `add` without `--section` writes the main config.
- `-f, --file NAME` chooses a file yourself, by the file name of a loaded file (`--file cypress`) or a path. It works on `add`, `edit`, `clone`, `move`, `set`, `unset`, `delete`, `alias`, `unalias`, `add-section`, `rename-section`, `delete-all` and `dump`.

When more than one file is loaded, every message names the file it wrote. A section name held by two files is refused until you say which with `--file`. A host defined in two files is edited in the first file ssh reads, with a warning naming the other.

```
rustorm includes                                        # every file the Include lines load, with host counts
rustorm add db2 postgres@10.10.158.44 --section "data foundry"   # goes to the file holding that section
rustorm set cypressPro-ext Port 22                      # goes to the file holding cypressPro-ext
rustorm add-section evant --file df-evant               # creates the section in that file
rustorm dump --file cypress                             # print one included file
```

`rustorm check` also reports a host defined in two files (ssh uses the first), an `Include` that loads a file that looks like a backup (`.bak`, `.orig`, `~`), an `Include` placed inside `Host *` (rustorm treats it as global), and an included file it cannot read. Each file written is backed up beside it first. When a wildcard in an `Include` would match that backup, rustorm names it `.<file>~` instead (`~/.ssh/config.d/.cypress~`) so neither ssh nor rustorm ever loads a backup as a config.

## Terminal UI

`rustorm-tui` opens a screen with a section list on the left, the host table in the middle and a status line at the bottom. Press `?` at any time for the key overlay. The essentials:

| Key | Action |
|---|---|
| `↑` `↓` or `j` `k` | Move the selection; the editor below follows, with the selected host's `Host` line at its top |
| `Tab` / `Shift-Tab` | Cycle focus between sections, table and editor |
| `1` … `7` | Sort by section, host, user, hostname, port, proxy, jump; press again to reverse; `0` restores file order |
| `/` | Filter every column; `f` then a column number filters one column; `x` clears filters |
| `Enter` on a section | Show only that section |
| `Enter` on a host | Open its settings: every keyword it can set, grouped, saved in one write |
| `a` `e` `d` `c` `m` | Add, quick-edit (URI, identity, section), delete (with confirmation), clone, move or rename the selected host |
| `R` on a section | Rename the section |
| `n` | Create an empty section |
| `F` | Focus the file list (shown when the config includes other files); the editor shows the highlighted file, and `Enter` moves into it |
| `o` | Open the editor at the selected host, in the file that holds it |
| `Ctrl-S` / `Ctrl-R` | In the editor: save, or discard and reload |
| `Esc` | Cancel a form or prompt, or leave the editor |
| `q` | Quit; asks first if the editor has unsaved edits, listing every unsaved file |

**Settings form.** `Enter` on a host shows the keywords it sets, in six groups: Connection, Authentication, Forwarding, Proxy, Multiplexing and Advanced. `Ctrl-T` shows every keyword. To add one, go to the `Add setting` row at the bottom and start typing its name — `hostk` offers `HostKeyAlias` — then `Tab` to its value. Yes/no and fixed-choice keys such as `Compression` or `ControlMaster` change with `Space` or the arrow keys, including back to not set. Other keys are typed. `Ctrl-U` clears one. Values your `Host *` block supplies show dimmed. `Enter` checks each value (a port must be a number, `StrictHostKeyChecking` one of its documented words) and saves every change at once with a backup; `Esc` cancels.

Every table column sorts and filters, including proxy (`ProxyCommand`) and jump (`ProxyJump`). Hosts without the sorted key sort last. Filters combine: a section filter and a user filter together show only hosts matching both. With included files the table gains a file column and the editor keeps one buffer per file; `Ctrl-S` saves only the file shown. Full key list: `docs/tui.md`.

## Desktop window

`rustorm-gui` opens a window with the sections in a sidebar, a Hosts tab and an Editor tab.

- Click a column header to sort; click again to reverse. The filter box above each column narrows the rows, and filters combine.
- Select a row, or move with `↑` `↓`, to edit it in the detail panel: name, connection URI, identity file and section. Clearing the identity file removes it from the host. Save writes the file; Delete asks first. The Editor tab follows the selection, opening on the selected host's `Host` line.
- **All settings** in the detail panel shows the keywords the host sets, grouped the same way as the terminal UI; switch to **All** for every keyword, or type a name into **Add setting** and press Tab. Yes/no and fixed-choice keys are drop-downs, other keys are text fields, repeatable keys like `LocalForward` get a field per value, and a bad value is flagged under its field. **Save settings** writes every change at once.
- Add opens the same form empty. Clone and Move to section act on the selected host.
- New section… under the sidebar creates an empty section; on a file without sections it also creates the catch-all. With a section selected, Rename section… renames it, or merges it into a section that already has the new name.
- With included files the sidebar lists every file with its host count, the table gains a file column, and the editor has a file selector; Save writes only the selected file. Show in Editor on a host opens its file at its `Host` line. Quitting with several unsaved files lists them in one dialog with Save All.
- Shortcuts: `Cmd-S` (or `Ctrl-S`) save, `Cmd-N` add, `Cmd-F` focus the filter, `Cmd-E` open the editor, `Cmd-1` / `Cmd-2` switch tabs, `Delete` remove the selected host, `Esc` cancel.

## The embedded editor

While you type a keyword at the start of a line the editor suggests the rest in dim text; Space accepts it and fills in the usual value, selected so you can type over it (`por` Space gives `Port 22`). Ctrl-Space on a `yes`/`no` or fixed-choice value swaps it.

Both UIs include an editor for the raw config file with syntax highlighting: comments, section banners, `Host` lines, keys, values, and `ProxyCommand` and `ProxyJump` in their own colors. Saving writes through the same engine as the CLI, so a backup is made, untouched lines stay byte for byte, and sections are re-sorted. An edit that would break the file, such as a `Host` line with no name, is refused with its line number. If the file changed on disk while you were editing, the UI reloads it before applying your change; if the same host changed on both sides it asks which version to keep. Quitting with unsaved edits asks whether to save or discard. With included files the editor holds one buffer per file and saves them one at a time, each with its own backup.

## Files

| File | Purpose |
|---|---|
| `~/.ssh/config` | The only place host data lives. Created with mode 0600 if missing. |
| `~/.ssh/config~` | Backup written before every change. `--no-backup` skips it. |
| Files named by `Include` lines | Read and written as part of the config; each is backed up beside itself before a change, as `.<file>~` when an `Include` wildcard would otherwise load the backup. |
| `~/.config/rustorm/config.toml` (Linux), `~/Library/Application Support/rustorm/config.toml` (macOS), `%AppData%\rustorm\config.toml` (Windows) | Optional command aliases and defaults for the CLI. |

Errors go to standard error prefixed `error:`. Exit codes: 0 success, 1 the operation was refused (host missing, name taken, invalid input), 2 usage error, 3 the config file could not be read or written.
