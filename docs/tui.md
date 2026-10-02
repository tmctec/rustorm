# rustorm-tui — terminal UI

`rustorm-tui` is a full-screen terminal view over the same config file `rustorm` edits. It lists, adds, edits, clones, moves and deletes hosts, edits every keyword a host sets in a settings form, creates and renames sections, and embeds a raw editor for the file with ssh_config highlighting. Every change goes through `rustorm-core`, so the file changes exactly as the matching `rustorm` command would change it, backup included (D2).

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
 ?:help  q:quit  Tab:focus  1-7:sort  / f:filter  x:clear  a:add  e:edit  Enter:settings  d:delete  c:clone  m:move  n:new section  o:editor
```

| Region | Content |
|---|---|
| Section list | `All` then every section in file order with its host count; the last one is the catch-all. `Enter` on a section sets the section filter to it; `Enter` on `All` clears it. Absent on a file without sections. |
| Host table | A ratatui `Table`, one row per host, `Host *` excluded. Columns: **section**, **host** (primary name), **user** (the host's own `User`), **hostname** (`HostName`), **port** (the host's own `Port`), **proxy** (`ProxyCommand`), **jump** (`ProxyJump`). A missing value shows `·`. The title reads `Hosts <shown>/<total>`. A file without hosts shows `no hosts in <path>. Press a to add one.` |
| Editor pane | The whole file in a `tui-textarea`, highlighted by the core lexer. It follows the selected host, cursor on its `Host` line (see Editor). The title shows the path, `[modified]` when the buffer differs from the file, and the cursor line and column. |
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
| `↑` `↓` / `j` `k`, `g` `G` / `Home` `End` | files | Move the highlight; the editor shows the highlighted file. |
| `Enter` | files | Show that file in the editor and focus it. |
| `Esc` | files | Return focus to the table. |
| `↑` `↓` / `j` `k` | table, sections | Move the selection; the editor follows the selected host. |
| `g` `G` / `Home` `End` | table, sections | First or last row. |
| `1` … `7` | table | Sort by section, host, user, hostname, port, proxy, jump. The same key again flips the direction. |
| `0` | table | Restore file order. |
| `/` | table | Edit the global filter; it also matches a host's note, location and tags. |
| `f` then `1` … `7` | table | Edit that column's filter. |
| `x` | table | Clear every filter. |
| `Enter` | sections | Filter the table to that section (`All` clears it). |
| `a` | table | Add a host. |
| `e` | table | Edit the selected host's connection URI, identity file and section. |
| `Enter` | table | Open the settings form: every keyword of the selected host (see Flows, Settings). |
| `d` | table | Delete the selected host, after confirmation. |
| `c` | table | Clone the selected host. |
| `m` | table | Move or rename the selected host. |
| `R` | sections | Rename the selected section. |
| `n` | table, sections | Create an empty section. |
| `o` | table | Open the editor at the selected host's `Host` line, in the file that holds it. |
| `Ctrl-S` | editor | Save the buffer of the file on screen. |
| `Space` | editor | With a keyword suggestion showing, accept it (see Editor, Completion); anywhere else, a space. |
| `Ctrl-Space` | editor | Swap a yes/no or fixed-choice value under the cursor. |
| `Ctrl-R` | editor | Discard the buffer's edits and reload the file, after confirmation. |
| `Esc` | editor | Return focus to the table; the buffer keeps its edits. |
| `Tab` / `Shift-Tab` / `↑` `↓` | form | Next or previous field. |
| `Enter` | form, filter input | Submit. |
| `Esc` | form, filter input, prompt | Cancel; nothing changes. |
| `↑` `↓` / `Tab` `Shift-Tab`, `PgUp` `PgDn`, `Home` `End` | settings form | Previous or next row, previous or next group, first or last row. |
| `Space` / `←` `→` | settings form | Step a yes/no or fixed-choice keyword through not set and its values; `Space` types a space into free text. |
| `Ctrl-U` | settings form | Clear the value, so the key is removed on save. |
| `Enter` | settings form | Save every change in one write. |
| `Ctrl-T` | settings form | Switch between the filled view and every keyword. |
| `Tab` | settings form, Add setting | Add the suggested keyword and move to its value. |
| `y` / `n` | yes/no prompt | Answer; every key but `y` means no. |

`q`, digits and letters type text while a form, the settings form, a filter input or the editor has focus.

## Sorting and filtering

The table opens in file order: the preamble's hosts, then each section in file order, sorted by name inside each (the order of `rustorm list`).

**Sort.** A column key sorts ascending by that column; pressing it again sorts descending; another column key starts that column ascending. Text compares case-insensitively and the port compares as a number. Ties break by host name ascending. Rows missing the value (no section in an unsectioned file, no `User`, no `Port`, no `HostName`, no `ProxyCommand`, no `ProxyJump`) sort last in both directions. `0` returns to file order.

**Filter.** Each column has its own filter and the global filter matches any column. A filter is a case-insensitive substring; a row missing the value never matches a non-empty column filter. Every non-empty filter must match (AND), so `section~bob` and `user~deploy` together keep only bob's hosts with a user containing `deploy`. Typing updates the table on each key; `Enter` keeps the text, `Esc` restores the filter's previous text. Emptying a filter or pressing `x` removes it and restores every row it hid.

**Selection.** After every sort or filter change the selection moves to the first visible row. When no row matches, the table shows one line, `no hosts match filter: section~zzz`, and `e`, `d`, `c`, `m` and `o` report `Error: No host selected.`

**Status line.** With no message showing, the status line reads the selected host's `name -> user@hostname:port` and, dimmed after it, its metadata (`cli.md`, Host metadata): the location, the tags joined with `, ` and the first note line, separated by ` · `. A host without metadata shows the target alone.

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

**Settings** (`Config::apply_settings`)

1. `Enter` on `vps` opens `Settings vps (filled)`: the keywords the host sets, under their groups — Notes & location, Connection, Authentication, Forwarding, Proxy, Multiplexing, Advanced — with groups the host does not use hidden, and an `Add setting` row at the bottom. Notes & location holds the metadata keys `note`, `location`, `privateKeyLocation`, `other` and `tags` (`cli.md`, Host metadata), typed like any other key: `note` has one row per line plus an empty row, `tags` is one comma-separated list, and while the `tags` row has focus the line under the form reads `tags in use: db, prod`, every tag any host carries. Saving writes them as `# key: value` comments directly above the `Host` line; `Add setting` offers them by name (`loca` → `location`). `Ctrl-T` switches to `Settings vps (all)`, every keyword a host can set, and back; edits carry across. The form opens filled every time. In the all view a key the host leaves unset shows `·`, or `(60 from Host *)` when `Host *` gives it a value. A repeatable key (`IdentityFile`, `LocalForward`, `RemoteForward`, `DynamicForward`, `CertificateFile`, `SendEnv`, `SetEnv`) has one row per value plus an empty row for another.
2. Yes/no keys such as `Compression` and fixed choices such as `ControlMaster` (`no`, `yes`, `ask`, `auto`, `autoask`) change with `Space`, `←` and `→`, which step through not set and each value. Every other key is typed. `Ctrl-U` clears a row; a cleared row stays in the filled view, marked `*`, until the form is saved or closed. A changed row shows `*`.

   **Add setting.** `End` (or moving past the last row) focuses `Add setting`. Typing shows the first keyword that starts with the text, completed in dim text, and lists the other matches; with no keyword starting with it, keywords containing it are offered. `Tab` adds that keyword and moves to its value, prefilled with its usual value when it has one (`Port` 22, yes/no keys `yes`, a fixed choice its first value) and selected, so typing replaces it. Adding a single-value keyword the host already sets moves to its row; adding a repeatable one gives another empty row. Text matching no keyword reads `no matching keyword`. `Esc` clears the typed text; `Esc` on an empty row cancels the form.
3. `Enter` checks every changed value against its keyword and writes them all at once, with one backup. A value that does not fit keeps the form open with `Error: Port must be a port from 1 to 65535.` (or `Error: privateKeyLocation holds a reference to a key, not the key itself.`) and the file untouched. A cleared key is removed from the host; a typed value for a key `Host *` sets gives the host its own line and leaves `Host *` alone. Comments and untouched lines keep their place.
4. On success: `✔ vps updated.` With nothing changed: `No changes.` and nothing is written. `Esc` cancels: `Cancelled.`

Keywords rustorm does not know stay in the file and do not show in the form; edit them in the editor.

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

**Follows the selection.** Moving the table selection to another host shows that host in the editor, its `Host` line at the top of the pane with the cursor on it, while focus stays on the table. Near the end of the file the rest of the pane stays empty so the `Host` line can still sit at the top. The line comes from the buffer, so unsaved edits that moved it are honored. The same happens after a sort, a filter or a write selects a different host. Moving the file-list highlight shows the highlighted file; returning to the table then puts the editor back on the selected host. The editor stays put while the selection does not change, so tabbing to the editor, moving the cursor and tabbing back keeps it where it was. An empty table leaves the editor alone.

**Completion.** Typing the first word of a line inside a `Host` block (or `Host`, `Match`, `Include` at column 0) shows the first matching keyword as dim text after the cursor, the same matches as Add setting. `Space` accepts it: the line gets the keyword in its canonical spelling, a space and the keyword's usual value, selected, so typing replaces it — `    por` then `Space` gives `    Port 22` with `22` selected. A fully typed keyword is written in canonical case the same way (`    port` `Space` gives `    Port 22`). Keywords with no usual value, such as `HostKeyAlias` or `ProxyCommand`, get only the space. In a value or a word no keyword matches, `Space` is a plain space. In a comment, the word after `#` completes to a metadata key: `# lo` shows `cation:` dimmed and `Space` gives `# location: ` with the cursor ready for the value; a comment matching no metadata key is left alone. `Ctrl-Space` on a yes/no or fixed-choice value swaps it to the next one: `yes` and `no` swap, `ask` becomes `accept-new` on `StrictHostKeyChecking`.

Usual values: `Port` 22, `ServerAliveInterval` 60, `ConnectTimeout` 10, `ControlPersist` 10m, `ControlPath` `~/.ssh/cm-%r@%h:%p`, `IdentityFile` `~/.ssh/id_ed25519`, `LocalForward` `8080 localhost:80`, yes/no keys `yes`, fixed choices their first documented value.

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

- **File list.** A pane left of the section list: the root first, then every included file in load order, each with its host count. `•` before a path marks a file whose buffer has unsaved edits; an unreadable file shows `cannot read` and cannot be opened. `F` focuses the pane, and it joins the `Tab` cycle before the section list. Moving the highlight shows the highlighted file in the editor, focus staying on the list; an unreadable file leaves the editor where it was. `Enter` on a file shows it in the editor and focuses the editor. Paths longer than the pane lose their start to `…`.
- **Host table.** A **file** column comes first, showing the file name of the file that holds the host. It sorts with `1` and filters with `f` then `1`; the other columns shift one digit right, so `1` … `8` sort. A host's own section is the section inside its file.
- **Sections.** The section list shows every file's sections; a section name held by two files shows once per file, followed by the file name. `Enter` on a section filters the table to that section and its file; `Enter` on `All` clears both filters. `R` renames the section in its own file.
- **Editor.** The editor holds one buffer per file, each with its own cursor and undo. Its title shows the path of the file on screen. Selecting a host shows the file that holds it, at its `Host` line; `o` does the same and focuses the editor.
- **Save.** `Ctrl-S` saves only the file on screen, after its own backup (`<file>~`, or `<dir>/.<name>~` when an `Include` pattern would match `<file>~`; see `cli.md`, Included files), and reports `✔ Saved ~/.ssh/config.d/cypress.` The other buffers keep their edits.
- **Forms.** Add, edit, settings, clone, move and delete write to the file the CLI would pick (see `cli.md`, Included files): a host edit to the file holding the host, a section to the file holding it, a new host without a section to the root. The status message names the file, as the CLI does. A section name held by two files refuses the form with `Error: section lab exists in ~/.ssh/config.d/cypress and ~/.ssh/config.d/gke. Edit the file you mean in the editor (F).` The forms refuse to run while any buffer has unsaved edits.
- **Quit.** `q` or `Ctrl-C` with unsaved edits in several files asks once, listing them: `Unsaved changes in ~/.ssh/config.d/cypress, ~/.ssh/config.d/ranch. [s]ave all / [d]iscard all / [c]ancel`. `s` saves every listed file and quits, unless a save is refused, which leaves the TUI open on that file. With one dirty file the prompt is the single-file one.
