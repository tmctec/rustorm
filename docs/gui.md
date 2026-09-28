# rustorm-gui — desktop interface

`rustorm-gui` is the egui desktop interface to rustorm. It lists, adds, edits, clones, moves and deletes hosts, creates sections, and it edits the raw config file with ssh_config syntax highlighting. Every read and write goes through `rustorm-core`, so hand edits, comments and banners survive exactly as they do with the CLI (F-43).

```
rustorm-gui [--config <FILE>]
```

`--config` defaults to `$RUSTORM_CONFIG`, then `~/.ssh/config`. A missing file opens as an empty config; nothing is created until the first save. The app logic is the library type `rustorm_gui::App`, which implements `eframe::App` and is built from a config path; `main.rs` only parses the flag and opens the window.

## Window

- **Title.** `rustorm — <config path>`, with a `•` suffix while the editor has unsaved text.
- **Minimum size.** 900 × 560 points. The default size is 1200 × 760.
- **Menu bar.** File (Add Host, Save, Discard Changes, Reload from Disk, Quit), Edit (Delete Host, Clone Host, Find), View (Hosts, Editor). Every item shows its shortcut.
- **Sidebar.** On the left, resizable. The first row is "All hosts" with the total count. One row per section follows, in file order, each with its host count. The catch-all section is always last and carries a "catch-all" hint. Selecting a section shows only its hosts, ANDed with the other filters; "All hosts" or a second click clears it. An unsectioned file shows only "All hosts". A **New section…** button under the list opens a dialog asking for a name; Add section creates the empty section before the catch-all (on an unsectioned file the catch-all `other` is created too and receives every host), an existing name is refused inline, and Cancel or Esc leaves the file unchanged. The button is disabled while the editor has unsaved text.
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
- **Filter.** A text box sits under each column header, plus a global filter above the table that matches any column. Each filter keeps rows whose displayed value, inherited ones included, contains the text, case-insensitively. Filters AND together. Clearing a box, or Clear filters, restores the rows they hid.
- **Empty states.** When filters hide every row the table body reads "no hosts match" with a Clear filters button. A config without hosts reads "no hosts yet" with an Add Host button.
- **Missing values** show an em dash in weak text.
- **Selection.** Clicking a row selects it and opens it in the detail panel. Up and Down move the selection.

## Detail panel

The detail panel sits to the right of the table.

- **Fields.** The web version's fields: name, connection URI (`[user@]host[:port]`), identity file; plus section, a text field with the existing sections offered as buttons below it. Editing a host prefills them from the file; the URI carries the host's own `User` and `Port` only, so inherited values stay inherited. A changed name renames the host.
- **Save.** On a new host, Save runs the core's `add`. On an existing host it runs `edit` with the URI and identity file. An emptied identity file removes every `IdentityFile` line. A changed section moves the host there; a new section name creates the section.
- **Add.** The Add Host button (above the table) opens the same form empty. The section field starts empty, which places the host in the catch-all.
- **Delete.** Asks "Delete host <name>?" in an alert. The Delete button is styled destructive; Cancel is the default and Esc cancels. Cancel leaves the file untouched.
- **Clone.** Asks for the new name and runs `clone`; the copy lands in the source's section with its `HostName` rewritten, and the new host becomes the selection.
- **Move to section.** A section picker with the existing sections plus a new-name field runs `move` with `--section`.
- **Errors.** A refused operation (name exists, invalid URI, invalid name) shows the core's message inline under the form in the error color. The fields keep their input and the file is unchanged.
- **Editor guard.** While the editor has unsaved text, Save, Delete, Clone and Move are disabled and the panel reads "Save or discard the editor first".
- Every successful write backs the file up to `<config>~`, re-reads the table, and reports the result in the status bar.

## Editor

- **Widget.** An egui `TextEdit::multiline` in monospace, filling the tab, with a custom `layouter`. The layouter lexes the text with the core's `Lexer` line by line (banner state carried across lines) and builds a `LayoutJob` with one color per `SpanKind`.
- **Colors.** Comment, Banner, HostKeyword, HostName, Alias, Key, Value, ProxyCommand, ProxyJump and Unknown each have their own color; ProxyCommand and ProxyJump differ from each other and from Value. Two palettes exist, one for light and one for dark appearance, chosen from the current egui visuals.
- **Load.** The editor opens with the file text. It reloads from disk after every form write, and on Reload from Disk.
- **Save.** Save (Cmd/Ctrl+S) parses the buffer with the core, sorts each section's hosts, and writes it through the core with the backup to `<config>~`. Untouched lines stay byte-identical.
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

- **Sidebar.** Below the sections, a Files list: the root first, then every included file in load order, each with its host count and a `•` while it has unsaved edits. Selecting a file shows only its hosts, ANDed with the other filters, and selects it in the editor; a second click clears it. An unreadable file shows "cannot read" in weak text and cannot be selected. The section rows show every file's sections; a section name held by two files shows once per file, with the file name in weak text.
- **Host table.** A **file** column comes first, showing the file name of the file that holds the host. It sorts and filters like the other columns.
- **Editor.** A file selector above the editor lists the files in load order, with `•` on each file with unsaved edits. The editor holds one buffer per file; switching files keeps each buffer's text and cursor. **Show in Editor** in the detail panel opens the host in the editor: it selects the file that holds it and puts the cursor on its `Host` line.
- **Save.** Save (Cmd/Ctrl+S) writes only the selected file, after its own backup (see `cli.md`, Included files: `<file>~`, or `<dir>/.<name>~` when an `Include` pattern would load `<file>~`). Discard Changes and Reload from Disk act on the selected file.
- **Forms.** The detail panel writes to the file the CLI would pick (see `cli.md`, Included files), and the status bar names that file. A section name held by two files is refused inline with the CLI's message. The editor guard holds while any file has unsaved edits.
- **Title and status bar.** The title reads `rustorm — <root path> — <selected file>`, with the `•` suffix while any file has unsaved edits. The status bar shows the selected file's path and its backup path.
- **Quit.** Closing the window or quitting with unsaved edits in several files shows one alert listing them all, with Save All, Don't Save (Discard All on Linux and Windows) and Cancel. A refused save keeps the window open on that file.
