# Product requirements

## Problem

Hand-editing `~/.ssh/config` is error-prone: a stray character breaks the parse for every host below it, there is no way to search across hosts for a key or value, and nothing stops two hosts from silently sharing a name or a stale identity file. The two prior tools that automate this file are both unmaintained, so anyone who depends on one today is depending on code nobody is fixing.

## Customer

Developers and operators who keep many SSH hosts in one config file and want a single native binary to manage it, not a scripting-language runtime, a browser tab, or a text editor they have to trust not to fat-finger a `Host` line.

## Positioning

For developers and operators who maintain many SSH hosts by hand, this is a single native binary that adds, edits, finds and connects to hosts from the command line; unlike opening `~/.ssh/config` in a text editor, it can't leave the file unparsable and never requires a text search to find a host.

Versus storm: storm proved the shape of this tool — named hosts, a connection URI, a search command — but its bulk-edit command matches host names by an unanchored prefix, its file wipe has no confirmation, and its custom-option flag crashes on a value containing an equals sign. This tool keeps storm's shape and fixes each of those edges.

Versus ssh-config: ssh-config proved that a config-file tool can be strict about preserving whatever the user already wrote by hand, and that per-key `set`/`unset` and a regex search are more useful than storm's cruder equivalents. This tool keeps that discipline and adds what ssh-config never built: multi-valued keys, a machine-readable output mode, and named sections to organize a large file.

## Non-goals

- No graphical interface in this release. A GUI is expected to arrive later by merging with a separate GUI project, not by this project building one.
- No copyleft dependency, ever — not now, not later, regardless of how useful a GPL-, LGPL- or AGPL-licensed component might be. This is a permanent constraint, not a v1-only one.
- Settled at the feature-exclusion review, by feature id from `features.md`:
- F-19 web: not ported.
- F-37 JSON API via web UI: not ported.
- F-38 Library API: not ported.
- F-39 Ecosystem companion apps: not ported.
- F-28 Multi-name `edit` comma-joining: not ported.
- F-40 `-` deletes a key: not ported.

## Success signals

- A user can add a host, find it again, and connect to it, entirely from the command line, without opening a text editor.
- After any single operation, the config file is unchanged byte-for-byte everywhere except the entry that operation touched — comments, spacing and every other host's formatting survive.
- A file a person edited by hand keeps their formatting and comments after the tool next writes to it.
- A separate GUI project can drive every read operation through a machine-readable output mode instead of scraping text.

## Settled questions

- `Include` and `Match`: the tool follows `Include` the way ssh does and treats the root and every file it loads as one workspace, editing each host in the file that holds it (D21 in `cli.md`). `Match` blocks stay opaque.

## Open questions

- What should a command that removes every host at once actually do: prompt, require an explicit confirmation flag, refuse outright, or something else?
- Should every write make a backup automatically and silently, the way one reference tool does, or should backups stay an explicit, user-invoked step, the way the other does?
