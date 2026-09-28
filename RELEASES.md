# Releases

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
