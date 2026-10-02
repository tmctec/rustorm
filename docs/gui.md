# rustorm-gui — desktop interface

`rustorm-gui` is the egui desktop interface to rustorm. It lists, adds, edits, clones, moves and deletes hosts, edits every keyword a host sets, creates sections, and it edits the raw config file with ssh_config syntax highlighting. Every read and write goes through `rustorm-core`, so hand edits, comments and banners survive exactly as they do with the CLI (F-43).

```
rustorm-gui [--config <FILE>]
```

`--config` defaults to `$RUSTORM_CONFIG`, then `~/.ssh/config`. A missing file opens as an empty config; nothing is created until the first save. The app logic is the library type `rustorm_gui::App`, which implements `eframe::App` and is built from a config path; `main.rs` only parses the flag and opens the window.

## Window

- **Title.** `rustorm — <config path>`, with a `•` suffix while the editor has unsaved text.
- **Minimum size.** 900 × 560 points. The default size is 1200 × 760.
- **Menu bar.** File (Add Host, Save, Discard Changes, Reload from Disk, Quit), Edit (Delete Host, Clone Host, Find), View (Hosts, Editor). Every item shows its shortcut.
- **Sidebar.** On the left, resizable. The first row is "All hosts" with the total count. One row per section follows, in file order, each with its host count. The catch-all section is always last and carries a "catch-all" hint. Selecting a section shows only its hosts, ANDed with the other filters; "All hosts" or a second click clears it. An unsectioned file shows only "All hosts". A **New section…** button under the list opens a dialog asking for a name; Add section creates the empty section before the catch-all (on an unsectioned file the catch-all `other` is created too and receives every host), an existing name is refused inline, and Cancel or Esc leaves the file unchanged. With a section selected, **Rename section…** opens a dialog with the name to change; Rename regenerates the banner, and a name another section already has merges the two. Both buttons are disabled while the editor has unsaved text.
- **Main area.** Two tabs: Hosts and Editor.
- **Status bar.** Along the bottom: the config file path, a dirty flag ("unsaved changes" or "saved"), the path of the last backup (`<config>~`), and the result of the last operation.

## Host table

The Hosts tab shows one row per host from the core's `list`. `Host *` is never a row.

| Column | Source |
|---|---|
| section | the host's section; empty in an unsectioned file |
| host | the primary name; aliases follow in weak text |
| user | the host's own `User`; a value inherited from `Host *` or `$USER` shows in weak text |
| hostname | `HostName` |
| port | the host's own `Port`; an inherited port (from `Host *`, else 22) shows in weak text |
| proxy | `ProxyCommand` |
| jump | `ProxyJump` |

- **Default order.** Sections in file order, hosts alphabetical inside each section, as `rustorm list` prints them.
- **Sort.** Clicking a header sorts ascending by that column; clicking it again sorts descending; a third click restores the default order. The sorted header shows ▲ or ▼. Text compares case-insensitively and port compares numerically. Sorting uses the host's own value; rows missing it, including an inherited user or port, sort last in both directions.
- **Filter.** A text box sits under each column header, plus a global filter above the table that matches any column and the host's metadata lines (`# note:`, `# location:`, `# tags:` and the rest; `cli.md`, Host metadata). Each filter keeps rows whose displayed value, inherited ones included, contains the text, case-insensitively. Filters AND together. Clearing a box, or Clear filters, restores the rows they hid.
- **Empty states.** When filters hide every row the table body reads "no hosts match" with a Clear filters button. A config without hosts reads "no hosts yet" with an Add Host button.
- **Missing values** show an em dash in weak text.
- **Selection.** Clicking a row selects it and opens it in the detail panel. Up and Down move the selection when no text field has focus. Selecting a host also points the editor at it: the editor selects the file that holds the host and, when the Editor tab next shows, puts its `Host` line at the top of the editor with the cursor on it — including the last host in the file, which gets empty space below it. The Hosts tab stays in front. Selecting the host that is already selected, switching tabs, or Esc leaves the editor's cursor where it is.

## Detail panel

The detail panel sits to the right of the table.

- **Metadata.** Under the Edit host heading, a host with metadata shows its location after 📍, each tag as a small chip and each note line in weak text. A host without metadata shows nothing there.
- **Fields.** The web version's fields: name, connection URI (`[user@]host[:port]`), identity file; plus section, a text field with the existing sections offered as buttons below it. Editing a host prefills them from the file; the URI carries the host's own `User` and `Port` only, so inherited values stay inherited. A changed name renames the host.
- **Save.** On a new host, Save runs the core's `add`. On an existing host it runs `edit` with the URI and identity file. An emptied identity file removes every `IdentityFile` line. A changed section moves the host there; a new section name creates the section.
- **Add.** The Add Host button (above the table) opens the same form empty. The section field starts empty, which places the host in the catch-all.
- **Delete.** Asks "Delete host <name>?" in an alert. The Delete button is styled destructive; Cancel is the default and Esc cancels. Cancel leaves the file untouched.
- **Clone.** Asks for the new name and runs `clone`; the copy lands in the source's section with its `HostName` rewritten, and the new host becomes the selection.
- **Move to section.** A section picker with the existing sections plus a new-name field runs `move` with `--section`.
- **Errors.** A refused operation (name exists, invalid URI, invalid name) shows the core's message inline under the form in the error color. The fields keep their input and the file is unchanged.
- **All settings.** Under the actions, a collapsible All settings section edits the host's keywords in seven collapsible groups — Notes & location, Connection, Authentication, Forwarding, Proxy, Multiplexing, Advanced — prefilled with the host's own values; changed rows carry `•`.
  - **Notes & location** holds the metadata keys `note`, `location`, `privateKeyLocation`, `other` and `tags`, saved as `# key: value` comments above the `Host` line. `note` has a field per line plus an empty one. The `tags` row shows one chip per tag with × to remove it, a ▾ menu of the tags other hosts carry, and a text field holding the comma-separated list, so typing `prod, db` or picking from the menu both work. A value that looks like key material in `privateKeyLocation` is refused under the field.
  - A **Filled / All** toggle picks the view. Filled, the default each time a host is selected, shows only the keywords the host sets, with their groups open and groups it does not use hidden; a value you clear stays listed until the save. All shows every keyword a host can set, groups collapsed.
  - In Filled, an **Add setting** field offers keywords as you type: those starting with the text, else those containing it, the first shown as `→ HostKeyAlias` and the rest after it. Tab or Enter adds the first one and focuses its field, prefilled with its usual value (Port 22, yes/no keys `yes`, a fixed choice its first value) and selected so typing replaces it. A single-value keyword the host already sets focuses its existing field; a repeatable one gets another. Text matching no keyword reads "no matching keyword".
  - Yes/no keywords and fixed choices use a drop-down with "not set" and each value, e.g. `ControlMaster`: not set, `no`, `yes`, `ask`, `auto`, `autoask`. An unset key whose value comes from `Host *` reads "— (60 from Host *)".
  - Other keywords are text fields; a value inherited from `Host *` shows as the hint. Keywords that also take free text, such as `ForwardAgent` (a socket path) or `ControlPersist` (a duration), have a ▾ menu of their documented words.
  - A repeatable keyword (`IdentityFile`, `LocalForward`, `RemoteForward`, `DynamicForward`, `CertificateFile`, `SendEnv`, `SetEnv`) shows one field per value plus an empty one; − clears a value.
  - A value that does not fit its keyword shows the problem under the field, e.g. "Port must be a port from 1 to 65535", and Save settings stays disabled.
  - **Save settings** writes every change in one write with one backup; a cleared key is removed; a value for a key `Host *` sets gives the host its own line. It is disabled while nothing changed. **Reset** restores the loaded values. Selecting another host or Cancel drops unsaved changes.
  - Keywords rustorm does not know stay in the file and do not show here; edit them in the editor.
- **Editor guard.** While the editor has unsaved text, Save, Delete, Clone, Move and All settings are disabled and the panel reads "Save or discard the editor first".
- Every successful write backs the file up to `<config>~`, re-reads the table, and reports the result in the status bar.

## Editor

- **Widget.** An egui `TextEdit::multiline` in monospace, filling the tab, with a custom `layouter`. The layouter lexes the text with the core's `Lexer` line by line (banner state carried across lines) and builds a `LayoutJob` with one color per `SpanKind`.
- **Colors.** Comment, Banner, HostKeyword, HostName, Alias, Key, Value, ProxyCommand, ProxyJump and Unknown each have their own color; ProxyCommand and ProxyJump differ from each other and from Value. Two palettes exist, one for light and one for dark appearance, chosen from the current egui visuals.
- **Load.** The editor opens with the file text. It reloads from disk after every form write, and on Reload from Disk.
- **Follows the selection.** The editor shows the file holding the selected host, its `Host` line at the top (see Host table, Selection).
- **Completion.** Typing the first word of a line inside a `Host` block (or `Host`, `Match`, `Include` at the start of a line) shows the first matching keyword in dim text after the cursor. Space accepts it: the keyword in its canonical spelling, a space and its usual value, selected so typing replaces it — `    por` then Space gives `    Port 22` with `22` selected. Keywords with no usual value, such as `HostKeyAlias`, get only the space. In a comment, the word after `#` completes to a metadata key: `# lo` then Space gives `# location: `. In a value or a word nothing matches, Space is a plain space. Ctrl+Space (Ctrl on every system) on a yes/no or fixed-choice value swaps it to the next one.
- **Save.** Save (Cmd/Ctrl+S) parses the buffer with the core, sorts each section's hosts, and writes it through the core with the backup to `<config>~`, reporting "Saved <file>.". Untouched lines stay byte-identical.
- **Invalid edit.** When a line does not parse (a `Host` line with no name, a key without a value) the save is refused with "line N: cannot parse: <text>" and the file is unchanged.
- **Discard.** Discard Changes restores the buffer from the file on disk.
- **Unsaved changes on close.** Closing the window or quitting with a dirty buffer shows an alert with Save, Don't Save (Discard on Linux and Windows) and Cancel. Cancel keeps the window open.
- **External change.** Before every write, from the form or the editor, the app compares the file on disk with the text it last loaded. When it changed, the app reloads the file and re-applies the pending operation on top, so the hand edit survives. A form write that touches a host the hand edit also changed asks "<name> changed on disk" with Overwrite and Cancel. An editor save over a changed file asks the same with Overwrite and Cancel, since the buffer replaces the whole file.

## Shortcuts

Cmd on macOS, Ctrl on Linux and Windows.

| Shortcut | Action |
|---|---|
| Cmd/Ctrl+S | Save: the editor buffer on the Editor tab, the detail form on the Hosts tab |
| Cmd/Ctrl+N | Add Host: open the empty form |
| Delete | Delete the selected host, after confirmation; only when no text field has focus |
| Cmd/Ctrl+F | Focus the global filter |
| Cmd/Ctrl+E | Editor tab |
| Cmd/Ctrl+1, Cmd/Ctrl+2 | Hosts tab, Editor tab |
| Esc | Cancel: close an alert or dialog, or close the form when no text field has focus |
| Return | Activate the focused button; alerts focus Cancel |

## Files

On a workspace of several files (see `cli.md`, Included files) the GUI shows every file and edits them one at a time. On a workspace of one file none of this appears and the window is as described above.

- **Sidebar.** Below the sections, a Files list: the root first, then every included file in load order, each with its host count and a `•` while it has unsaved edits. Selecting a file shows only its hosts, ANDed with the other filters, and selects it in the editor; a second click clears the filter and leaves the editor on that file. An unreadable file shows "cannot read" in weak text and cannot be selected. The section rows show every file's sections; a section name held by two files shows once per file, with the file name in weak text.
- **Host table.** A **file** column comes first, showing the file name of the file that holds the host. It sorts and filters like the other columns.
- **Editor.** A file selector above the editor lists the files in load order, with `•` on each file with unsaved edits. The editor holds one buffer per file; switching files keeps each buffer's text and cursor. **Show in Editor** in the detail panel opens the host in the editor: it selects the file that holds it and puts the cursor on its `Host` line.
- **Save.** Save (Cmd/Ctrl+S) writes only the selected file, after its own backup (see `cli.md`, Included files: `<file>~`, or `<dir>/.<name>~` when an `Include` pattern would load `<file>~`). Discard Changes and Reload from Disk act on the selected file.
- **Forms.** The detail panel writes to the file the CLI would pick (see `cli.md`, Included files), and the status bar names that file. A section name held by two files is refused inline with the CLI's message. The editor guard holds while any file has unsaved edits.
- **Title and status bar.** The title reads `rustorm — <root path> — <selected file>`, with the `•` suffix while any file has unsaved edits. The status bar shows the selected file's path and its backup path.
- **Quit.** Closing the window or quitting with unsaved edits in several files shows one alert listing them all, with Save All, Don't Save (Discard All on Linux and Windows) and Cancel. A refused save keeps the window open on that file.
