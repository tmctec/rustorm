# rustorm-tui — terminal UI

`rustorm-tui` is a full-screen terminal view over the same config file `rustorm` edits. It lists, adds, edits, clones, moves and deletes hosts, creates and renames sections, and embeds a raw editor for the file with ssh_config highlighting. Every change goes through `rustorm-core`, so the file changes exactly as the matching `rustorm` command would change it, backup included (D2).

```
rustorm-tui [-c, --config <FILE>] [--no-backup]
```

| Option | Effect |
|---|---|
| `-c, --config <FILE>` | Operate on `FILE`. Without it: `$RUSTORM_CONFIG`, else `~/.ssh/config`. |
| `--no-backup` | Do not write `<config>~` before a change. |
| `-h, --help` / `-V, --version` | Print help or the version and exit 0. |

An unknown option exits 2. Without a terminal on stdin and stdout it prints `error: rustorm-tui needs an interactive terminal. Use rustorm list or rustorm dump in scripts.` and exits 2. A missing config file shows an empty table and creates nothing until the first save. Raw mode and the alternate screen are left on every exit path, panics included, and `q` exits 0.

## Layout

```
┌ Sections ─────┐┌[Hosts 3/3]════════════════════════════════════════════════════════════════════┐
│ All       3   │║ Section▲  Host   User      HostName          Port  Proxy            Jump      ║
│ bob       1   │║ bob       vps    root      vps.example.com   2222  ·                ·         ║
│ other     2   │║ other     db     postgres  db.internal       ·     ·                bastion   ║
│               │║ other     web    deploy    web.example.com   ·     ssh -W %h:%p bas ·         ║
└───────────────┘└═══════════════════════════════════════════════════════════════════════════════┘
┌ Editor ~/.ssh/config  ln 1 col 1 ─────────────────────────────────────────────────────────────────┐
│# my ssh config                                                                                     │
│Host *                                                                                              │
│    ServerAliveInterval 60                                                                          │
└────────────────────────────────────────────────────────────────────────────────────────────────────┘
 vps -> root@vps.example.com:2222
 ?:help  q:quit  Tab:focus  1-7:sort  /:filter  f:column filter  a:add  e:edit  d:delete  c:clone  m:move
```

| Region | Content |
|---|---|
| Section list | `All` then every section in file order with its host count; the last one is the catch-all. `Enter` on a section sets the section filter to it; `Enter` on `All` clears it. Absent on a file without sections. |
| Host table | A ratatui `Table`, one row per host, `Host *` excluded. Columns: **section**, **host** (primary name), **user** (the host's own `User`), **hostname** (`HostName`), **port** (the host's own `Port`), **proxy** (`ProxyCommand`), **jump** (`ProxyJump`). A missing value shows `·`. The title reads `Hosts <shown>/<total>`. A file without hosts shows `no hosts in <path>. Press a to add one.` |
| Editor pane | The whole file in a `tui-textarea`, highlighted by the core lexer. The title shows the path, `[modified]` when the buffer differs from the file, and the cursor line and column. |
| Status line | The selected host's resolved `name -> user@hostname:port`, the active filters (`filter: section~bob user~deploy`), and the last message. An error starts with `Error:`, a success with `✔`. |
| Help bar | The keys valid in the focused pane. `?` opens the full key overlay. |

The focused pane has a double border and a bracketed title; the selected row is reverse video; the sorted column carries `▲` or `▼`; a filtered column carries `*` after its name. Under a non-UTF-8 locale or `TERM=dumb` the glyphs are `^`, `v` and `-`. With `NO_COLOR` set or `TERM=dumb`, colors are off and every state keeps its text or modifier signal.

## Keybindings

| Key | Context | Action |
|---|---|---|
| `q` | table, sections | Quit. Asks first when the editor has unsaved edits. |
| `Ctrl-C` | any | Quit, through the same unsaved-edits prompt. |
| `?` | table, sections | Toggle the key overlay. `Esc` or `?` closes it. |
| `Tab` / `Shift-Tab` | table, sections, editor | Cycle focus: files (on a workspace of several files), sections, table, editor. |
| `F` | table, sections | Focus the file list; only on a workspace of several files (see Files). |
| `Enter` | files | Show that file in the editor and focus it. |
| `Esc` | files | Return focus to the table. |
| `↑` `↓` / `j` `k` | table, sections | Move the selection. |
| `g` `G` / `Home` `End` | table, sections | First or last row. |
| `1` … `7` | table | Sort by section, host, user, hostname, port, proxy, jump. The same key again flips the direction. |
| `0` | table | Restore file order. |
| `/` | table | Edit the global filter. |
| `f` then `1` … `7` | table | Edit that column's filter. |
| `x` | table | Clear every filter. |
| `Enter` | sections | Filter the table to that section (`All` clears it). |
| `a` | table | Add a host. |
| `e` / `Enter` | table | Edit the selected host. |
| `d` | table | Delete the selected host, after confirmation. |
| `c` | table | Clone the selected host. |
| `m` | table | Move or rename the selected host. |
| `R` | sections | Rename the selected section. |
| `n` | table, sections | Create an empty section. |
| `o` | table | Open the editor at the selected host's `Host` line, in the file that holds it. |
| `Ctrl-S` | editor | Save the buffer of the file on screen. |
| `Ctrl-R` | editor | Discard the buffer's edits and reload the file, after confirmation. |
| `Esc` | editor | Return focus to the table; the buffer keeps its edits. |
| `Tab` / `Shift-Tab` / `↑` `↓` | form | Next or previous field. |
| `Enter` | form, filter input | Submit. |
| `Esc` | form, filter input, prompt | Cancel; nothing changes. |
| `y` / `n` | yes/no prompt | Answer; every key but `y` means no. |

`q`, digits and letters type text while a form, a filter input or the editor has focus.

## Sorting and filtering

The table opens in file order: the preamble's hosts, then each section in file order, sorted by name inside each (the order of `rustorm list`).

**Sort.** A column key sorts ascending by that column; pressing it again sorts descending; another column key starts that column ascending. Text compares case-insensitively and the port compares as a number. Ties break by host name ascending. Rows missing the value (no section in an unsectioned file, no `User`, no `Port`, no `HostName`, no `ProxyCommand`, no `ProxyJump`) sort last in both directions. `0` returns to file order.

**Filter.** Each column has its own filter and the global filter matches any column. A filter is a case-insensitive substring; a row missing the value never matches a non-empty column filter. Every non-empty filter must match (AND), so `section~bob` and `user~deploy` together keep only bob's hosts with a user containing `deploy`. Typing updates the table on each key; `Enter` keeps the text, `Esc` restores the filter's previous text. Emptying a filter or pressing `x` removes it and restores every row it hid.

**Selection.** After every sort or filter change the selection moves to the first visible row. When no row matches, the table shows one line, `no hosts match filter: section~zzz`, and `e`, `d`, `c`, `m` and `o` report `Error: No host selected.`

## Flows

Every flow applies one core operation, writes through the core with a backup (unless `--no-backup`), refreshes the table and section list, and keeps the affected host selected. On an error the form stays open with the message under it and the file is untouched. A core message that names a `rustorm` command is cut to its first sentence and followed by the TUI remedy.

**Add** (`Config::add`)

1. `a` opens the form `Add host` with fields `Name`, `Connection URI ([user@]host[:port])`, `Identity file (optional)`, `Section (optional)`.
2. `Enter` submits. An empty `Name` or URI shows `Error: Name and Connection URI are required.`
3. On a duplicate name: `Error: vps already exists. Press e on vps to edit it.`
4. On success the status reads `✔ vps added. Connect with: ssh vps`, or `✔ vps added to section bob. Connect with: ssh vps`. When that call created the catch-all it adds `Hosts without a section moved to other.`

**Edit** (`Config::edit`, then `Config::unset` for an emptied identity)

1. `e` on `vps` opens `Edit host vps` with `Connection URI`, `Identity file`, `Section`, prefilled from the host's own `User`, `HostName`, `Port`, first `IdentityFile` and section.
2. `Enter` submits. The URI replaces `HostName`, `User` and `Port`. A changed identity replaces every `IdentityFile`; an emptied identity removes them; an unchanged one leaves them. A changed section moves the host.
3. On success: `✔ vps updated.`

**Delete** (`Config::delete`)

1. `d` on `vps` asks `Delete host vps from ~/.ssh/config? [y/N]`.
2. `y` deletes the entry and the comments directly above it: `✔ vps deleted.`
3. Any other key cancels: `Delete cancelled.` The file is untouched.

**Clone** (`Config::clone_host`)

1. `c` on `rails01` opens `Clone host rails01` with `New name` and `Section`, the section prefilled with the source's.
2. `Enter` submits. `HostName` gets `rails01` replaced by the new name (D5).
3. On success: `✔ rails02 added. Connect with: ssh rails02`. On an existing name: `Error: rails02 already exists.`

**Move to section** (`Config::move_host`)

1. `m` on `vps` opens `Move host vps` with `Name` and `Section`, both prefilled.
2. `Enter` submits. An unchanged name is not renamed; a changed section moves the host, creating the section when missing.
3. On success: `✔ vps moved to section bob.`, `✔ vps renamed to vps2. Connect with: ssh vps2`, or `✔ vps renamed to vps2 and moved to section bob. Connect with: ssh vps2`. With neither changed: `Error: Change the name, the section, or both.`

**Rename section** (`Config::rename_section`)

1. `R` on `bob` in the section list opens `Rename section bob` with `New name` prefilled.
2. `Enter` submits. The banner is regenerated; the catch-all stays last and stays the catch-all.
3. On success: `✔ section bob renamed to cypresspt.`, or `✔ section bob merged into personal.` when the name belongs to another section (D17).

**New section** (`Config::add_section`)

1. `n` in the table or the section list opens `New section` with one field, `Name`.
2. `Enter` submits. The section is inserted before the catch-all; on a file without sections the catch-all `other` is created too and receives every host. An empty name shows `Error: Section name is required.`; an existing name shows `Error: section lab already exists.`
3. On success: `✔ section lab added.`, or `✔ section work added; other created with 3 hosts.` The section list shows the new section with `0` hosts.

**External changes.** Before every write the TUI compares the file's modification time and size with the values at load. When they differ it reloads the file, re-applies the operation to the fresh model and writes that, so a hand edit to another host survives. When the target host's own text changed on disk it asks `vps changed on disk since it was loaded. Overwrite it? [y/N]`; any key but `y` cancels, reloads and leaves the file untouched.

## Editor

The editor is a `tui-textarea` holding the file's text. It never soft-wraps; long lines, such as the 103-column banners, scroll horizontally with the cursor.

**Highlighting.** Each visible line is styled by the spans of `rustorm_core::Lexer::next_line`, which carries banner state from the top of the file. One style per `SpanKind`:

| SpanKind | Color | Modifier (kept without color) |
|---|---|---|
| Comment | dark gray | italic |
| Banner | magenta | dim |
| HostKeyword | yellow | bold |
| HostName | green | bold |
| Alias | green | none |
| Key | cyan | none |
| Value | default | none |
| ProxyCommand | light red | underlined |
| ProxyJump | light blue | bold, underlined |
| Whitespace | default | none |
| Unknown | red | reversed |

**Save.** `Ctrl-S` runs the core `check` on the buffer. When it reports an unparsable line the save is refused with `Error: Not saved: line 3: cannot parse: Host`, the cursor moves to line 3 and the file is untouched. Otherwise the buffer is parsed by the core, its sections re-sorted, and the result written with `ConfigFile::save_text`: a backup to `<config>~`, then an atomic replace. Untouched lines stay byte-identical. The status reads `✔ Saved ~/.ssh/config.` and the table refreshes.

**Changed on disk.** When the file's modification time or size changed since the buffer was loaded, `Ctrl-S` asks `The file changed on disk. [r]eload (drop your edits) / [o]verwrite / [c]ancel`. `r` loads the disk text into the buffer, `o` saves the buffer, anything else cancels.

**Unsaved edits.** While the buffer differs from the file its title shows `[modified]`, and the forms refuse to run with `Error: Save or discard the editor's changes first.` so the two views never diverge. `q` or `Ctrl-C` with unsaved edits asks `The editor has unsaved changes. [s]ave / [d]iscard / [c]ancel`: `s` saves and quits (a refused save stays open), `d` quits without writing, anything else returns. `Ctrl-R` asks `Discard the editor's changes? [y/N]` and reloads the file on `y`.

## Files

On a workspace of several files (see `cli.md`, Included files) the TUI shows every file and edits them one at a time. On a workspace of one file none of this appears and the screen is as described above.

- **File list.** A pane left of the section list: the root first, then every included file in load order, each with its host count. `•` before a path marks a file whose buffer has unsaved edits; an unreadable file shows `cannot read` and cannot be opened. `F` focuses the pane, and it joins the `Tab` cycle before the section list. `Enter` on a file shows it in the editor and focuses the editor. Paths longer than the pane lose their start to `…`.
- **Host table.** A **file** column comes first, showing the file name of the file that holds the host. It sorts with `1` and filters with `f` then `1`; the other columns shift one digit right, so `1` … `8` sort. A host's own section is the section inside its file.
- **Sections.** The section list shows every file's sections; a section name held by two files shows once per file, followed by the file name. `Enter` on a section filters the table to that section and its file; `Enter` on `All` clears both filters. `R` renames the section in its own file.
- **Editor.** The editor holds one buffer per file, each with its own cursor and undo. Its title shows the path of the file on screen. `o` on a host shows the file that holds it, at its `Host` line.
- **Save.** `Ctrl-S` saves only the file on screen, after its own backup (`<file>~`, or `<dir>/.<name>~` when an `Include` pattern would match `<file>~`; see `cli.md`, Included files), and reports `✔ Saved ~/.ssh/config.d/cypress.` The other buffers keep their edits.
- **Forms.** Add, edit, clone, move and delete write to the file the CLI would pick (see `cli.md`, Included files): a host edit to the file holding the host, a section to the file holding it, a new host without a section to the root. The status message names the file, as the CLI does. A section name held by two files refuses the form with `Error: section lab exists in ~/.ssh/config.d/cypress and ~/.ssh/config.d/gke. Edit the file you mean in the editor (F).` The forms refuse to run while any buffer has unsaved edits.
- **Quit.** `q` or `Ctrl-C` with unsaved edits in several files asks once, listing them: `Unsaved changes in ~/.ssh/config.d/cypress, ~/.ssh/config.d/ranch. [s]ave all / [d]iscard all / [c]ancel`. `s` saves every listed file and quits, unless a save is refused, which leaves the TUI open on that file. With one dirty file the prompt is the single-file one.
