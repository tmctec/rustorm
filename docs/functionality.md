# rustorm — functionality

This document describes rustorm's target behavior: how the config file is parsed and written, how connection URIs resolve, how sections work, and where rustorm's behavior is a deliberate departure from storm or ssh-config. `docs/cli.md` is the command reference; this document summarizes and links to it rather than repeating per-command flag detail.

## Commands

| Command | Summary |
|---|---|
| [add](cli.md#add) | Appends a new host entry from a connection URI. |
| [edit](cli.md#edit) | Replaces an existing entry's HostName/User/Port from a URI, keeping other keys. |
| [set](cli.md#set) | Sets one or more keys on a host, or on every host matching a regex with `--regex`. |
| [unset](cli.md#unset) | Removes named keys from a host. |
| [clone](cli.md#clone) | Copies a host to a new name, rewriting HostName by default. |
| [move](cli.md#move) | Renames a host, moves it to another section, or both. |
| [delete](cli.md#delete) | Removes whole host entries by name. |
| [delete-all](cli.md#delete-all) | Removes every host entry, asking for confirmation first. |
| [list](cli.md#list) | Prints one line per host, sorted by name, grouped by section. |
| [show](cli.md#show) | Prints one or more entries verbatim, including their comments. |
| [dump](cli.md#dump) | Prints the whole config file as parsed. |
| [search](cli.md#search) | Finds hosts whose name, alias, key or value matches a regular expression. |
| [alias](cli.md#alias) | Adds extra names to a host's `Host` line. |
| [unalias](cli.md#unalias) | Removes extra names from a host's `Host` line. |
| [sections](cli.md#sections) | Lists sections in file order with host counts. |
| [rename-section](cli.md#rename-section) | Renames a section and regenerates its banner. |
| [add-section](cli.md#add-section) | Creates an empty section by name. |
| [combine](cli.md#combine) | Merges two or more config files into the first, refusing duplicate names unless told how to resolve them. |
| [backup](cli.md#backup) | Copies the config file to a named target. |
| [check](cli.md#check) | Reports config problems without changing the file. |
| [completion](cli.md#completion) | Prints a shell completion script. |
| [version](cli.md#version) | Prints rustorm's version. |

## Connection URI

`add` and `edit` take a connection URI in one of three forms:

- `user@host:port` — every part given explicitly.
- `host:port` — user comes from `Host *`'s `User` if set, else `$USER`.
- `host` — user resolves as above; port comes from `Host *`'s `Port` if set, else `22`.

Resolution order for user and port is always: the value in the URI itself, then the `Host *` defaults block, then `$USER` (user) or `22` (port). See [add](cli.md#add) and [edit](cli.md#edit).

## Config-file semantics

The config file is the only state rustorm keeps. Every command parses it fresh and, for a write, serializes it straight back.

- **Parse.** Each line becomes a comment, a blank line, or part of a `Host` block. Keys match case-insensitively.
- **Preserve.** Comments, blank lines and any formatting rustorm does not touch round-trip unchanged; nothing is reformatted as a side effect of an unrelated write. [dump](cli.md#dump) exists specifically to confirm this byte-for-byte.
- **Dump.** [dump](cli.md#dump) prints the file exactly as parsed, with `Host` lines and keys colored on a terminal.
- **Multi-valued keys.** `IdentityFile`, `LocalForward`, `RemoteForward`, `DynamicForward`, `CertificateFile`, `SendEnv` and `SetEnv` accumulate as repeated lines instead of the last one winning. `set` replaces every value on a multi-valued key; `set --append` adds one more without disturbing the rest.
- **`Host *` defaults.** The `Host *` block is parsed like any other entry but also feeds URI-resolution defaults (see Connection URI, above) and prints as its own `(*) defaults` section in `list -l`.
- **Aliases on the `Host` line.** A host's extra names live as additional tokens on its own `Host` line (`Host primary alias1 alias2`), not in a separate structure. [alias](cli.md#alias) and [unalias](cli.md#unalias) edit that line.
- **Canonical key case.** Keys are matched case-insensitively but always written in ssh_config(5)'s canonical case — `HostName`, `IdentityFile`, `Port` — regardless of how they were typed or how they appeared in a hand-edited file.
- **File creation.** A missing config file (and its parent directory) is created with mode `0600` on the first write.
- **Backup.** Every write copies the file to `<config>~` first, unless `--no-backup` is given — for the default config file, that backup lands at `~/.ssh/config~`. [backup](cli.md#backup) additionally makes a named copy on demand.

## Sections

A section groups hosts under a banner comment, itself only comment lines, so a sectioned file stays a valid ssh_config file that `ssh` reads unchanged.

**Banner grammar.** A banner is 103 columns wide and has four parts, top to bottom: a rule line, a label line reading `section: <name>`, the name rendered as FIGlet standard-font art, and a closing rule line. The label line is what rustorm parses back; the art is decorative and is regenerated whenever the section is renamed.

**Preamble.** Comments, blank lines and `Host *` appearing before the first banner form the preamble. The preamble stays outside every section and never moves.

**Catch-all.** The first use of `--section` on an unsectioned file creates two sections: the named one, and a catch-all that receives every host that was not already under a banner. The catch-all is created with the name `other` and is always the last section in the file. It keeps the catch-all role because it stays last, not because of its name — it can be renamed to anything and remains the catch-all. If a file's last section is removed by hand, the new last section takes over the role.

**Order.** Hosts inside a section sort alphabetically by primary name on every write. Sections themselves keep the order they were created in; a newly created section is inserted immediately before the catch-all, never after it.

**Create.** `add-section` inserts an empty banner before the catch-all, or before a named section; on an unsectioned file it also creates the catch-all, exactly as the first `--section` does.

**Rename and merge.** Renaming a section rewrites its banner in place. Renaming a section onto the name of a section that already exists merges the hosts of the first into the second and removes the first's banner.

## rustorm configuration

Separately from the SSH config file, rustorm reads its own small configuration file for command aliases and defaults. It holds no host data — the SSH config file remains the only database. The file is TOML, and its path follows the OS convention documented in `cli.md`'s Files chapter (the XDG config directory on Linux, Application Support on macOS, `%AppData%` on Windows). It has two tables: `[aliases]`, mapping a canonical command name to a list of extra spellings accepted on the command line, and `[defaults]`, holding startup defaults such as whether backups and color are on.

## Error messages

Every error a command can print in `cli.md`'s examples, with the exit code it carries:

| Command | Message | Exit code |
|---|---|---|
| [add](cli.md#add) | `error: vps already exists. Use rustorm edit or rustorm set to modify it.` | 1 |
| [edit](cli.md#edit) | `error: nope does not exist. Use rustorm add to create it.` | 1 |
| [set](cli.md#set) | `error: no host matches nomatch-.*` | 1 |
| [clone](cli.md#clone) | `error: rails02 already exists.` | 1 |
| [move](cli.md#move) | `error: give a new name, a --section, or both.` | 2 |
| [delete](cli.md#delete) | `error: nope does not exist.` | 1 |
| [delete-all](cli.md#delete-all) | `error: refusing to delete 14 hosts without --yes on a non-interactive terminal.` | 1 |
| [show](cli.md#show) | `error: nope does not exist.` | 1 |
| [rename-section](cli.md#rename-section) | `error: section nope does not exist.` | 1 |
| [add-section](cli.md#add-section) | `error: section lab already exists.` | 1 |
| [combine](cli.md#combine) | `error: 2 hosts are defined more than once; nothing written. Use --on-conflict keep or replace.` | 1 |
| [combine](cli.md#combine) | `error: combine needs at least two files.` | 2 |

Every error prints to stderr prefixed `error: `, per `cli.md`'s Exit status chapter.

## Parity notes

Places rustorm's target behavior departs from storm or ssh-config, each traced to a feature id in `docs/features.md`:

- **Search: regex, not substring.** storm's `search` is a case-sensitive substring match; ssh-config's is already a regular expression. rustorm follows ssh-config (F-12, F-42), with `-F` available for storm-style fixed-string matching.
- **`update` folded into `set --regex`.** storm has a separate `update` command whose host-name matching is an unanchored `re.match` prefix, not a full match. rustorm has no `update`; `set --regex` does the same job, anchored to the whole host name (F-03, F-27).
- **Canonical key case.** storm writes every key lowercased. rustorm writes ssh_config(5)'s canonical case (`HostName`, not `hostname`) while still matching case-insensitively on read (F-51).
- **Automatic backup.** ssh-config backs up to `<config>~` on every save with no way to turn it off from its CLI. storm only backs up when the user runs `backup` explicitly. rustorm follows ssh-config's automatic model, but exposes `--no-backup` to opt out (F-33).
- **`clone` rewrites HostName by default.** storm's `clone` copies the options dict verbatim, HostName included, so both hosts point at the same address until edited. ssh-config's `copy` rewrites HostName by nickname substitution. rustorm follows ssh-config's rewrite as the default, with `--keep-hostname` for storm's verbatim behavior (F-05).
- **`delete-all` asks first.** storm's `delete_all` wipes every host with no prompt, and also destroys comments, blank lines and the `Host *` block. rustorm's `delete-all` asks for confirmation, requires `--yes` off a terminal, and never removes comments, banners or `Host *` (F-08).
- **`alias` means host-line aliases.** storm's only alias concept is command-name aliasing (`~/.stormssh/config`'s JSON map). ssh-config's `alias`/`unalias` manage extra names on a host's `Host` line. rustorm keeps both, but the bare word "alias" in its command grammar means ssh-config's meaning; command aliasing lives in rustorm's own config file (F-13, F-35).
- **`-o` splits on the first `=` only.** storm's `--o` (and its `update`) splits every `=` in the value, so a value containing `=` (a `ProxyCommand` with embedded options, for example) crashes with an unhandled exception. rustorm's `-o` splits once, on the first `=` (F-24).
- **Multi-valued keys generalized.** storm special-cases exactly three keys (`identityfile`, `localforward`, `remoteforward`) to accumulate; every other repeated key silently loses all but its first value on round-trip. ssh-config has no multi-valued handling at all — one line per key, always. rustorm accumulates the full ssh_config(5) multi-valued key set, and `set --append` is the explicit way to add rather than replace (F-31).
- **Sections are new.** Neither storm nor ssh-config groups hosts into named, orderable, renamable banner sections with an automatic catch-all. This is new in rustorm (F-20, F-21, F-22, F-45, F-46, F-47).
- **Combining files is new.** Neither storm nor ssh-config merges config files; `Include` is ssh's own answer, which leaves every included file separate. `combine` folds files into one, merging sections by name and `Host *` key by key, and fails on a duplicate name unless told to keep or replace (F-52). `add-section` creates a section without adding a host, which the `--section` flag alone cannot do (F-53).
