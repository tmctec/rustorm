# Releases

## 2026-09-28 — The editor follows you, and every setting is one keystroke away

**New**
- In the terminal UI and the desktop app, the config editor now shows whatever host you are looking at: move through the host list and the editor jumps to that host, in whichever file holds it. Browse the file list in the terminal UI and the editor shows each file as you go.
- Press Enter on a host in the terminal UI to see every setting it can have, grouped into Connection, Authentication, Forwarding, Proxy, Multiplexing and Advanced. Yes/no and fixed-choice settings flip with the space bar or arrow keys, and everything else you type.
- The desktop app's host panel has the same thing under All settings, with drop-downs for choices and a warning right under any value that won't work.
- Both check every value before saving (a port has to be a number, `StrictHostKeyChecking` has to be one of its real options), then save all your changes at once with a backup. Settings your `Host *` block already supplies are shown so you know what the host inherits.

**Improved**
- In the desktop app, the Up and Down arrow keys move through the host list.

## 2026-09-28 — rustorm follows your Include lines

**New**
- If your `~/.ssh/config` has an `Include` line, rustorm now reads every file it loads, the same way ssh does, and treats them all as your config. `rustorm list` shows every host with a heading per file, and `rustorm includes` shows which files are loaded and how many hosts each holds.
- Changes land in the right file without you naming it: editing a host writes the file that holds it, `--section` writes the file that holds the section, and a plain `add` goes to your main config. Say `--file cypress` when you want to choose yourself.
- `rustorm check` now warns when a host is defined in two files, when an `Include` wildcard is loading a backup or a stray file, when an `Include` sits inside `Host *`, and when an included file cannot be read.
- The terminal UI and the desktop app list your files, show which file each host lives in, and let you edit and save each file on its own. Quitting with several unsaved files lists them all.

**Improved**
- Backups of included files are named so that an `Include` wildcard never loads them, so a backup can no longer resurrect a host you deleted.
- A config without `Include` works exactly as before.

## 2026-09-28 — Merge config files and create sections directly

**New**
- You can now merge several ssh config files into one with `rustorm combine`, for example your main config and a file from `~/.ssh/config.d`. Sections with the same name join up, hosts without a section land in the catch-all, and settings under `Host *` are added only when your config does not already have them.
- A host name that appears in two files never gets overwritten silently: `combine` lists every duplicate and writes nothing until you choose to keep the first file's version or take the second's.
- If your config still has an `Include` line that loads a file you just merged, `combine` tells you so you can remove it.
- You can create an empty section with `rustorm add-section`, from the terminal UI with `n`, or from the desktop app with the New section… button, and place it before any section you like.

## 2026-09-25 — rustorm ships its command line, terminal UI and desktop app

**New**
- `rustorm` manages the hosts in your `~/.ssh/config` from the command line: add, edit, clone, move, delete, list, search and check, with every message and exit code as documented, machine-readable JSON output, and shell completion.
- `rustorm-tui` gives the same operations a full-screen terminal interface, with a host table you can sort and filter by section, host, user, proxy and jump machine, and an editor for the raw file with syntax highlighting.
- `rustorm-gui` brings the same table, forms and highlighted editor to a desktop window.
- Sections: group hosts under banner comments that ssh ignores; hosts stay alphabetical inside a section and a catch-all section holds the rest.
- Every change backs up your config first and leaves untouched lines exactly as you wrote them.

**Improved**
- A user guide covers installation, the three programs, sections, keys and shortcuts.

## 2026-09-24 — rustorm is defined

**New**
- rustorm now has a written definition: what it is, why it exists, and every command it will offer, agreed before any code is written.
- The command reference covers adding, editing, cloning, moving, deleting, listing, showing and searching SSH hosts, plus grouping them into named sections with decorative banners that ssh itself ignores.
- The feature list records which capabilities of the two tools rustorm replaces will carry over and which will not: no web interface, no embedded library, no companion apps.
