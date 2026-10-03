# rustorm — command-line reference

`rustorm` (rust storm) manages the hosts in your `~/.ssh/config`. It adds, edits, clones, deletes, lists, searches and groups host entries into sections, and it never trashes the file: comments, blank lines and hand-written formatting survive every write. It is written in Rust.

This document is the contract the implementation is built against. Each command carries an `Origin` line (which reference tool it comes from) and a `Status` line (`v1` after the feature review).

## Decisions

| # | Decision | Alternative | Status |
|---|---|---|---|
| D1 | The binary, crate and repository are `rustorm`, short for rust storm. The implementation is Rust. | `storm`, `sshc` | agreed |
| D2 | Every write first backs the file up to `<config>~`. `--no-backup` skips it. `backup <file>` still exists for named copies. | Explicit backup only, as in stormssh | agreed |
| D3 | `set --regex <pattern>` replaces stormssh's `update`. One command edits keys; a flag widens it to many hosts. | Keep `update` as a separate command | agreed |
| D4 | `alias` and `unalias` manage extra names on the `Host` line. Command aliases (`rm` for `delete`) live in rustorm's own config file. | stormssh's meaning of "alias" (command aliases only) | agreed |
| D5 | `clone` rewrites `HostName` by substituting the new name for the old one. `--keep-hostname` disables it. | Copy verbatim, as in stormssh | agreed |
| D6 | Keys match case-insensitively and are written in the canonical case from ssh_config(5): `HostName`, `IdentityFile`. | Lowercase, as stormssh writes them | agreed |
| D7 | `search` takes a regular expression. `-F` searches a fixed string. | Substring only, as in stormssh | agreed |
| D8 | `delete-all` asks for confirmation on a terminal and refuses without `--yes` elsewhere. | No confirmation, as in stormssh | agreed |
| D9 | `--json` switches every read command to JSON on stdout, for the GUI that comes later. | No machine-readable output | agreed |
| D10 | rustorm's own config lives in the OS config directory (see Files). | `~/.stormssh/config` | agreed |
| D11 | There is no `web` command; the GUI project covers that need. stormssh's web UI, its JSON routes and its library API are not ported (F-19, F-37, F-38). | Port the Flask web UI | agreed |
| D12 | `check` and `completion` are new commands taken from ssh-config's TODO and from clap's generator. | Parity only, nothing new | agreed |
| D13 | On multi-valued keys (`IdentityFile`, `LocalForward`, `RemoteForward`, `DynamicForward`, `CertificateFile`, `SendEnv`, `SetEnv`), `set` replaces every value and `set --append` adds one. | Replace the first value only, as in ssh-config | agreed |
| D14 | stormssh's `--id_file` becomes `-i, --identity`; `--o` becomes `-o, --option`, both mirroring `ssh`. | Keep stormssh's spellings | agreed |
| D15 | The config file is the database. rustorm keeps no host state anywhere else; its own config file holds only aliases and defaults. | A cache or index beside the config | agreed |
| D16 | A section banner's label line reads `section: <name>` so the name parses back without decoding the ASCII art below it. The art is decorative and regenerated on rename. | Label line reads only `section`; the name lives in the art | agreed |
| D17 | The preamble (comments, blank lines and `Host *` before the first banner) stays outside every section. The catch-all section is always the last one in the file; it is created as `other` and can be renamed like any other section. Renaming a section to an existing name merges its hosts into that section and drops the banner. | Preamble folds into the catch-all; `other` reserved; no merge on rename | agreed |
| D18 | `sections` lists sections with host counts. Not in either reference tool. | No listing command | agreed |
| D19 | Options go anywhere on the command line, before, between or after positional arguments, for every command. | Options only before positionals | agreed |
| D20 | No copyleft code, direct or transitive: no GPL, LGPL, AGPL, SSPL, EUPL or CC-BY-SA libraries, crates or vendored sources. Permissive only: MIT, Apache-2.0, BSD, ISC, Zlib, Unicode, MPL-2.0 at file scope. | Allow LGPL under dynamic linking | agreed |
| D21 | rustorm follows the root's `Include` lines and works on every matched file as one workspace; a root without `Include` is a workspace of one and behaves exactly as a single file does. | Treat `Include` as opaque and edit the root only | agreed |
| D22 | A write goes to one file: `--file` when given; else the file holding the host for an edit of an existing host; else the file holding the section for `--section`, the root when no file holds it; else the root. A section in two files is an error naming both; a host in two files is edited where ssh reads it first, with a warning. `delete-all` sweeps every file. | Write every change to the root | agreed |
| D23 | Every `--json` row carries a `"file"` field with the absolute path of the file holding it, on a workspace of one file too. | Add the field only when more than one file is loaded | agreed |
| D24 | An included file that cannot be read is skipped by read commands with a warning; a write routed to it fails with exit 3. | Fail every command while any include is unreadable | agreed |
| D25 | Host metadata lives in `# key: value` comment lines directly above the `Host` line. The keys are `note`, `location`, `privateKeyLocation`, `other` and `tags`; `set`, `unset`, `add` and `clone` take them like ssh keys. `ssh` ignores the lines and the file stays plain ssh_config. | A sidecar file or rustorm's own config | agreed |
| D26 | `show`, `list` and `search` take `--where`, `--filter`, `--format txt\|json\|csv\|yaml` and `--just-value`. Output holds exactly the keys named in `--filter`; `Host` is a key like any other. `--json` means `--format json`. | A separate `get` command | agreed |
| D27 | A key named in `--filter` that a host does not set, directly or through `Host *`, prints empty and the command exits 4 after a warning per host and key. `--allow-missing` exits 0. `Host`, `section`, `file` and the metadata list `tags` are never missing. | Exit 0 with an empty value | agreed |
| D28 | `reconcile` (alias `resolve`) resolves every host defined in two or more workspace files, one copy at a time against the live definition, and never deletes a file: a fully resolved copy retires to `~/.ssh/retired/`, outside every `Include` glob. | Delete backup files; reuse `combine --on-conflict` | agreed |
| D29 | The live definition is the one ssh reads first: the files an `Include` line loads are read where the line stands, so a host in an included file is live over the same host below that `Include` in the root, and a file that looks like a backup is live when it sorts first. `reconcile` says so whenever a backup is the live one. | Treat the non-backup file, or the root, as live | agreed |
| D30 | Two definitions are identical when their directives (canonical key case, whitespace collapsed, in order) and their metadata lines match. Plain comments, blank lines and indentation never make a conflict; a metadata difference does. | Compare bytes | agreed |
| D31 | Taking the copy keeps the live `Host` line (its aliases), its section and its place; only the body lines and the metadata lines come from the copy. Per-key picks exist on a terminal and in the TUI and GUI. | Replace the whole block | agreed |

## Synopsis

```
rustorm [GLOBAL OPTIONS] <COMMAND> [ARGS]
```

## Global options

| Option | Effect |
|---|---|
| `-c, --config <FILE>` | Operate on `FILE` instead of `~/.ssh/config`. |
| `--no-backup` | Do not write `<config>~` before changing the file. |
| `--json` | Emit JSON instead of text on `list`, `show`, `dump`, `search`, `check`, `includes`, `reconcile`. This is the machine interface; there is no web API. On `list`, `show` and `search` it is the same as `--format json` (see Reading output). |
| `--no-color` | Disable ANSI color. `NO_COLOR` in the environment does the same. |
| `-q, --quiet` | Suppress success messages. Errors still print. |
| `-s, --section <NAME>` | Section for `add`, `edit`, `clone`, `move`, `list`, `show` and `search`; accepted before or after the command. On the three read commands it selects hosts, the same as `--where section=NAME`. |
| `-f, --file <NAME\|FILE>` | The workspace file for `add`, `edit`, `clone`, `move`, `set`, `unset`, `delete`, `alias`, `unalias`, `add-section`, `rename-section`, `delete-all` and `dump`, overriding routing (see Included files), and the destination of `reconcile --add`; every other command refuses it with exit 2. `NAME` is a loaded file's name, such as `cypress`; anything else is a path. |
| `-V, --version` | Print the version and exit. |
| `-h, --help` | Print help and exit. |

## Command mapping

Canonical name first. Every alias in the third column is accepted on the command line.

| rustorm command | stormssh | ssh-config | Aliases accepted | F-NN |
|---|---|---|---|---|
| `add` | `add` | `set` on a missing host | — | F-01 |
| `edit` | `edit` | — | — | F-02 |
| `set` | `update` — replaced by `--regex` | `set` | `update` | F-03 |
| `unset` | `--id_file DELETED` | `unset`, `set KEY -` | — | F-04 |
| `clone` | `clone` | `copy`, `cp` | `copy`, `cp` | F-05 |
| `move` | `move` | — | `rename`, `mv` | F-06, F-22 (`--section`) |
| `delete` | `delete` | `rm`, `del`, `delete` | `rm`, `del` | F-07 |
| `delete-all` | `delete_all` | — | `delete_all` | F-08 |
| `list` | `list` | `list` | `ls` | F-09 |
| `show` | — | `show` | — | F-10 |
| `dump` | — | `dump` | `cat` | F-11 |
| `search` | `search` | `search` | `find`, `grep` | F-12 |
| `alias` | — | `alias` | — | F-13 |
| `unalias` | — | `unalias` | — | F-14 |
| `backup` | `backup` | automatic `config~` | — | F-15 |
| `check` | — | TODO item | `lint` | F-16 |
| `reconcile` | — | — | `resolve` | F-61 |
| `completion` | contrib script | — | — | F-17 |
| `sections` | — | — | — | F-20 |
| `rename-section` | — | — | — | F-21 |
| `add-section` | — | — | — | F-53 |
| `combine` | — | — | `merge` | F-52 |
| `includes` | — | — | — | F-54 |
| `version` | `version` | — | — | F-18 |

F-NN ids refer to `docs/features.md`.

## Connection URI

`add` and `edit` take a connection URI:

```
[user@]host[:port]
```

- `root@vps.example.com:2222` sets all three.
- `vps.example.com:2222` takes the user from `Host *` if it sets `User`, else from `$USER`.
- `vps.example.com` also takes the port from `Host *` if it sets `Port`, else `22`.
- IPv6 literals go in brackets: `[2001:db8::1]:22`.
- A non-numeric port is an error.

## Host names

A name is any token without whitespace or `@`. `*` alone is reserved for the defaults section. Glob patterns such as `*.example.com` are allowed; `add` writes them verbatim.

## Option placement

Options and flags may appear anywhere: before, between or after positional arguments (D19). These four lines do the same thing:

```
rustorm move vps vps2 --section bob
rustorm move --section bob vps vps2
rustorm move vps --section bob vps2
rustorm --section bob move vps vps2
```

Global options are accepted before or after the command name.

## Sections

A section groups hosts under a banner comment. A banner is only comments, so the file stays a valid ssh_config and `ssh` ignores it. rustorm creates, recognizes, sorts and renames sections; hand-written banners in the same shape are recognized too.

**Banner.** 103 columns wide; a rule line, a label line `section: <name>` (D16), the name rendered in the FIGlet standard font (FIGlet and its fonts are BSD-licensed, so D20 holds), and a closing rule line:

```
#-----------------------------------------------------------------------------------------------------#
#                                        section: data foundry                                        #
#                        _       _           __                       _                               #
#                     __| | __ _| |_ __ _   / _| ___  _   _ _ __   __| |_ __ _   _                    #
#                    / _` |/ _` | __/ _` | | |_ / _ \| | | | '_ \ / _` | '__| | | |                   #
#                   | (_| | (_| | || (_| | |  _| (_) | |_| | | | | (_| | |  | |_| |                   #
#                    \__,_|\__,_|\__\__,_| |_|  \___/ \__,_|_| |_|\__,_|_|   \__, |                   #
#                                                                            |___/                    #
#-----------------------------------------------------------------------------------------------------#
```

**Rules.**

- A file with no banner keeps its hand-written order. The first `--section` creates two sections: the named one, and a catch-all named `other`, which receives every host that was not under a banner. From then on every host lives in exactly one section.
- The catch-all is always the last section in the file (D17). `other` is only its initial name: `rename-section other personal` keeps it the catch-all. A file with sections always has one; if the last section is deleted by hand, the new last section takes the role.
- Hosts inside a section are sorted alphabetically by primary name on every write. Sections keep the order they were created in; a new section is inserted before the catch-all (D17).
- The preamble stays put: comments, blank lines and `Host *` before the first banner are outside every section and never move (D17).
- `add`, `edit`, `clone` and `move` take `-s, --section <NAME>`. A section that does not exist is created. `add` without `--section` goes to the catch-all; `clone` without it goes to the source's section.
- Section names may contain spaces (quote them) and match case-insensitively. No name is reserved.
- `list` prints a heading per section and takes `--section <NAME>` to show one. `--json` output carries a `"section"` field.
- `delete-all` removes hosts and leaves the banners, so the sections survive empty.
- `rename-section` rewrites the banner; `sections` lists them; `add-section` creates an empty one.
- `combine` merges sections by name: hosts from a same-named section in a later file join it; a section only a later file has is created before the catch-all; a later file's unsectioned hosts join the catch-all.

## Included files

rustorm follows the root config's `Include` lines the way ssh does and works on the root and every file they load as one **workspace** (D21). The root is `~/.ssh/config`, or the file `--config` names. A root without `Include` is a workspace of one file, and every command behaves exactly as the rest of this document describes: no file headings, no ` in <file>` in messages, the same bytes written.

The examples in this chapter use this workspace:

```
~/.ssh/config               Include ~/.ssh/config.d/*; host github
~/.ssh/config.d/cypress     hosts cypressPro, cypressPro-ext
~/.ssh/config.d/df-austin   section data foundry: db1, dcaustin-pfsense
~/.ssh/config.d/ranch       Include ranch.d/*; hosts dcevant, ranch-nas
~/.ssh/ranch.d/lab          host lab-1
```

**Resolution.**

- An `Include` line takes one or more patterns separated by whitespace. Each pattern is an absolute path, a path starting with `~/` (the home directory), or a path relative to `~/.ssh`, also when `--config` names a root elsewhere.
- A pattern may hold glob characters (`*`, `?`, `[...]`). Its matches load in lexical order of their paths. A pattern that matches nothing loads nothing and is not an error. Only regular files load; a matched directory is skipped.
- An included file's own `Include` lines are followed the same way. A file already in the workspace is not loaded again, so an `Include` cycle ends at the first repeat.
- **Load order** is the order ssh opens the files: the root first, then each `Include`'s matches where the line stands, depth first. Headings, the `includes` listing and the editors' file lists follow it.
- An `Include` inside `Host *` is treated as global: its files load as if the line stood at the top level, since `Host *` matches every host. `check` notes it.
- `Match` blocks stay opaque. rustorm never edits an `Include` line.
- The paths in text output show the home directory as `~`. The `"file"` field of `--json` output carries the absolute path.

**Reading.** Every command sees every host of the workspace: `list`, `show`, `search`, `check` and completion cover all files, and a host name is unique across the workspace for `add`, `clone`, `move` and `alias`. `list` and `sections` print a heading per file (see those commands). `dump` prints the root; `--file` makes it print another file. Every `--json` row carries a `"file"` field, on a workspace of one file too (D23).

**Routing.** A write goes to one file, chosen in this order (D22):

| Change | File written |
|---|---|
| Any command given `-f, --file` | The file `--file` names. |
| An edit of an existing host: `edit`, `set`, `unset`, `alias`, `unalias`, `delete`, a `move` rename | The file that holds the host. |
| `add`, `clone`, `move` with `--section` | The file that holds the section. A section in no file is created in the root. |
| `add` without `--section` | The root, in its catch-all when it has sections. |
| `clone` without `--section` | The source's file and section. |
| `rename-section` | The file that holds the section. |
| `add-section` | The root. |
| `delete-all` | Every file of the workspace. |

- **Section in two files.** A section name held by two files is ambiguous for every write that names it, and the command fails with exit 1 before writing: `error: section lab exists in ~/.ssh/config.d/cypress and ~/.ssh/config.d/gke. Say which with --file.` Each file keeps its own sections and its own catch-all (D17 applies per file), so two files each with an `other` section make `--section other` need `--file`.
- **Host in two files.** When two files define the same host name, the edit goes to the first definition in ssh's reading order, the one ssh uses, and a warning on stderr names the others: `warning: dcaustin-pfsense is also defined in ~/.ssh/config.d/df-austin.bak.20260628232757; ssh uses the first.` `--file` picks another definition.
- **Moving between files.** A `move` whose destination section is in another file removes the entry from its file and writes it into the destination, comments included. The destination is written first, so a failure never loses the host.
- **Backups.** Each file written is backed up to its own `<file>~` first (D2). When an `Include` pattern would match that `<file>~`, the backup goes to `<dir>/.<name>~` instead (`~/.ssh/config.d/.cypress~`), since a glob never matches a leading dot, so neither ssh nor rustorm loads it. A move between files writes two files and two backups.
- **Unreadable include.** A matched file rustorm cannot read is skipped by read commands with `warning: cannot read ~/.ssh/config.d/private (permission denied); skipped.` on stderr. A write routed to it fails with `error: cannot read ~/.ssh/config.d/private (permission denied).` and exit 3 (D24).

**`--file`.** `-f, --file <NAME|FILE>` names a workspace file. A bare `NAME` matches the file name of one loaded file (`cypress` is `~/.ssh/config.d/cypress`, `config` is the root); anything else is a path, `~/` expanded and relative to the current directory. A path not in the workspace is accepted only when one of the workspace's `Include` patterns matches it; the file is then created with mode `0600`. Otherwise the command fails with exit 1: `error: no such file nas in the workspace.` A bare name matching two loaded files fails with `error: file lab matches ~/.ssh/ranch.d/lab and ~/.ssh/work/lab. Give a path.` On a workspace of one file, `--file` must name the root. `--file` applies to `add`, `edit`, `clone`, `move`, `add-section`, `rename-section`, `delete-all` and `dump`.

**Messages.** On a workspace of more than one file, success messages name the file written. Each example starts from the workspace above:

```
$ rustorm add db2 postgres@db2.example.com --section "data foundry"
db2 added to section data foundry in ~/.ssh/config.d/df-austin. Connect with: ssh db2

$ rustorm add vps root@vps.example.com:2222
vps added in ~/.ssh/config. Connect with: ssh vps

$ rustorm set cypressPro-ext User deploy
cypressPro-ext updated in ~/.ssh/config.d/cypress.

$ rustorm move dcevant --section "data foundry"
dcevant moved from ~/.ssh/config.d/ranch to section data foundry in ~/.ssh/config.d/df-austin.

$ rustorm move ranch-nas nas
ranch-nas renamed to nas in ~/.ssh/config.d/ranch. Connect with: ssh nas

$ rustorm delete lab-1
lab-1 deleted from ~/.ssh/ranch.d/lab.

$ rustorm alias cypressPro cp
cypressPro in ~/.ssh/config.d/cypress now answers to: cypressPro cp

$ rustorm add-section evant --file ~/.ssh/config.d/df-evant
section evant added to ~/.ssh/config.d/df-evant.

$ rustorm rename-section "data foundry" dfa
section data foundry renamed to dfa in ~/.ssh/config.d/df-austin.

$ rustorm add db1 postgres@db1.example.com
error: db1 already exists in ~/.ssh/config.d/df-austin. Use rustorm edit or rustorm set to modify it.

$ rustorm delete-all --yes
8 hosts deleted from 5 files.
```

`set --regex` reports `3 hosts updated in 2 files: cypressPro, cypressPro-ext, dcevant`. `delete-all` asks `Delete 8 hosts from 5 files? [y/N]`; with `--file` it sweeps that file alone and names it as on a single file. Errors that name no host or section keep their single-file text.

## Host metadata

ssh_config has no place for a note, a location or a tag, so rustorm keeps them in comment lines directly above the `Host` line, in the block of leading comments the host already owns (D25). The lines are plain comments: `ssh` and every other tool ignore them, and the file stays valid ssh_config.

```
# Primary build box. Reboot only after 18:00.
# note: Primary build box
# note: Reboot only after 18:00
# location: Austin DC, rack 4, U12
# privateKeyLocation: keepassxc
# other: owner alice
# tags: prod, austin, db
Host buildbox
    HostName 10.0.4.12
    User deploy
```

**Line shape.** `#`, optional spaces, the key, `:`, optional spaces, the value. A comment that does not fit this shape, or whose key is not one of the five below, is an ordinary comment and is never read, moved or rewritten as metadata. `# TODO: fix` is a comment. `# section: <name>` is a banner label line (D16) and is never metadata.

**Keys.**

| Key | Holds | Lines |
|---|---|---|
| `note` | Free text. | One or more; each `# note:` line is one line of the note, in file order. |
| `location` | Where the machine is, free text. | One. |
| `privateKeyLocation` | Where the private key lives: a vault name such as `keepassxc`, a vault entry, a path. A reference only; rustorm never reads a key store or fetches a key, and rejects a value that looks like key material (`-----BEGIN`). | One. |
| `other` | Free text that fits none of the above. | One. |
| `tags` | A list of labels separated by commas. A tag has no spaces or commas; `-` and `_` are fine. Tags compare case-insensitively and duplicates are dropped. | One. |

Keys are read case-insensitively (`# Location:` and `# location:` are the same key) and written in exactly the spelling of the table. A key's value is the text after the colon with surrounding whitespace trimmed; an empty value removes the line.

**Where the lines go.** An existing metadata line is edited in place. A new one goes directly above the `Host` line, below every other leading comment, and new lines keep the order of the table: `note`, `location`, `privateKeyLocation`, `other`, `tags`. Other comment lines above the host are never moved or changed.

**Commands.** Metadata keys are accepted wherever an ssh key is: `set NAME note "text"`, `unset NAME location`, `add NAME URI -o location="Austin DC"`, `clone NAME NEW location "Austin DC"`. `set` on `note` replaces every `# note:` line with one; `set --append NAME note "text"` adds a line (D13). `set NAME tags "prod, db"` replaces the list; `set --tag prod` adds one tag and `set --untag prod` removes one, and both may repeat. `--regex` applies as it does to ssh keys. The lines travel with the host through `clone`, `move`, `combine` and `delete` because they are leading comments. `show` and `dump` print them where they are; `--json` rows of `list`, `show` and `search` carry a `"meta"` object (`note` and `tags` as arrays, the rest as strings, absent keys omitted); `search` matches their text and prints the matching line under the host; `--filter` and `--where` take them as keys (see Reading output). The TUI settings form and the GUI detail panel edit them in a Notes & location group.

## Reading output

`show`, `list` and `search` share four options that pick hosts, pick keys and pick a format (D26). Without them the commands print what their own sections describe.

```
rustorm show  [<NAME>...] [-s NAME] [--where KEY=VALUE]... [--filter KEYS] [--format FMT] [--just-value] [--allow-missing]
rustorm list              [-s NAME] [--where KEY=VALUE]... [--filter KEYS] [--format FMT] [--just-value] [--allow-missing]
rustorm search <PATTERN>  [-s NAME] [--where KEY=VALUE]... [--filter KEYS] [--format FMT] [--just-value] [--allow-missing]
```

**Keys.** A key is an ssh_config keyword (`HostName`, `Port`, `IdentityFile`), a metadata key (`note`, `location`, `privateKeyLocation`, `other`, `tags`) or one of three pseudo-keys: `Host` (the primary name), `section` (the section name, empty for a host outside every section) and `file` (the absolute path of the file holding the host). Keys match case-insensitively; `hostname`, `HostName` and `HOSTNAME` are the same key. A value an ssh key does not set on the host falls back to `Host *`, as `list` already does for `User` and `Port`. The catch-all section is called `other` and so is a metadata key; `--section other` and `--where section=other` select hosts, `--filter other` selects the key.

**`--where KEY<op>VALUE`** selects hosts. Repeatable; every `--where` must hold (AND).

| Form | Holds when |
|---|---|
| `KEY=VALUE` | The host's value equals `VALUE`, ignoring case. On a list key (`tags`, `IdentityFile`, every multi-valued key of D13) when the list contains it. |
| `KEY=A,B` | Any of the comma-separated values matches (OR). |
| `KEY!=VALUE` | `KEY=VALUE` does not hold. True for a host that does not set the key. |
| `KEY~PATTERN` | A regular expression (the `search` syntax, D7) matches the value, or any value of a list key. |

`-s, --section NAME` is `--where section=NAME` with one difference kept from `list`: a section that exists in no file is an error, exit 1. A `--where` that selects no host prints nothing: `list` exits 0, `show` and `search` exit 1. `show` needs a name unless `--where` or `--section` is given; with both, the named hosts are kept only when they also match.

**`--filter KEYS`** picks the keys to print, comma-separated, in the order given. The output holds exactly these keys and nothing else: leave `Host` out and no name is printed. Without `--filter`, `txt` and `json` print the command's usual output (`list` and `search` the rows, `show` the entries); `csv` and `yaml` use the keys `Host,hostname,user,port,section,file`.

**`--format FMT`** is `txt` (the default), `json`, `csv` or `yaml`; `yml` names the same format as `yaml`. `--json` is `--format json`; naming both with different formats is a usage error, exit 2.

| Format | With `--filter` | With `--just-value` |
|---|---|---|
| `txt` | Per host, one line per key: the file's own line, as spelled and indented in the file, for ssh keys and metadata (`    HostName 10.7.112.72`, `# location: Austin DC`); `Host NAME` for `Host`; `    section NAME` and `    file PATH` for the pseudo-keys. A list key prints every line. A key the host does not set prints nothing. Hosts follow each other without a separator. | One value per line, no keys, no `Host` line. A list key prints one value per line. A key the host does not set prints an empty line, so the line count matches the filter. |
| `json` | An array with one object per host, the keys spelled as typed in `--filter`, strings for single values, arrays for `note`, `tags` and multi-valued ssh keys, `null` for a key the host does not set. | An array with one array per host, values in filter order. |
| `csv` | A header row of the keys as typed, then one row per host. A list joins its values with `;`. A field holding `,`, `"` or a line break is quoted; `"` doubles. A key the host does not set is an empty field. | The rows without the header. |
| `yaml` | A sequence of mappings, keys as typed. A list is a flow sequence `[prod, db]`. A value is quoted whenever YAML would read it as anything but that string: `yes`, `no`, `on`, `off`, `true`, `false`, `null`, `~`, a number such as `22`, or text starting with a YAML indicator or holding `: ` or ` #`. `10.7.112.72` is a string and prints bare. | A sequence of flow sequences, values in filter order. |

**`--just-value`** drops the keys and prints values only, as the table says.

**`--allow-missing`** makes a key the host does not set an ordinary empty value: no warning, exit 0 (D27). Without it the output is still complete; each unset key on each printed host adds `warning: NAME has no 'KEY'` on stderr and the command exits 4 once everything is printed. `Host`, `section`, `file` and `tags` are never missing: a host outside every section has an empty `section` and a host without `# tags:` has an empty list.

**Examples**

```
$ rustorm show D72 --filter Host,hostname,user
Host D72
    hostname 10.7.112.72
    user travis

$ rustorm show D72 --filter hostname,user
    hostname 10.7.112.72
    user travis

$ rustorm show D72 --filter hostname --just-value
10.7.112.72

$ rustorm show D72 --filter Host,hostname,user --format yaml
- Host: D72
  hostname: 10.7.112.72
  user: travis

$ rustorm show D72 D73 --filter Host,hostname --format csv
Host,hostname
D72,10.7.112.72
D73,10.7.112.73

$ rustorm show buildbox --filter Host,note,location,privateKeyLocation --format json
[{"Host":"buildbox","note":["Primary build box","Reboot only after 18:00"],"location":"Austin DC, rack 4, U12","privateKeyLocation":"keepassxc"}]

$ rustorm list --section "df austin" --filter Host,hostname,user --format csv
Host,hostname,user
D72,10.7.112.72,travis
D73,10.7.112.73,travis

$ rustorm list --where tags=prod --where location~Austin --filter Host,tags --format yml
- Host: buildbox
  tags: [prod, austin, db]

$ rustorm list --filter Host,section,file --format yaml
- Host: github
  section: other
  file: /home/me/.ssh/config
- Host: D72
  section: df austin
  file: /home/me/.ssh/config.d/df-austin

$ rustorm search keepassxc --filter Host,privateKeyLocation --format yaml
- Host: buildbox
  privateKeyLocation: keepassxc

$ rustorm show D72 --filter Host,proxyjump --format json
warning: D72 has no 'proxyjump'
[{"Host":"D72","proxyjump":null}]
$ echo $?
4

$ rustorm show D72 --filter Host,proxyjump --format json --allow-missing
[{"Host":"D72","proxyjump":null}]
$ echo $?
0

$ rustorm show --where tags=db,cache --filter hostname --just-value
10.7.112.72
10.0.4.12
10.7.112.80

$ rustorm list --json --format csv
error: --json and --format csv conflict.
$ echo $?
2
```

## Commands

### add

Origin: stormssh · Status: v1

**Synopsis**

```
rustorm add <NAME> <URI> [-i <FILE>] [-o KEY=VALUE]...
```

**Description**

Appends a new `Host NAME` entry with `HostName`, `User` and `Port` from the URI. Creates `~/.ssh/config` with mode `0600` if it does not exist.

**Arguments**

| Argument | Meaning |
|---|---|
| `NAME` | The alias you type after `ssh`. Must not already exist. |
| `URI` | `[user@]host[:port]`. |

**Options**

| Option | Effect |
|---|---|
| `-i, --identity <FILE>` | Writes `IdentityFile FILE`. |
| `-o, --option KEY=VALUE` | Writes any ssh_config directive, or a metadata key as a `# key: value` comment (see Host metadata). Repeatable. Splits on the first `=` only. |
| `-s, --section <NAME>` | Places the entry in that section, creating it if needed. Without it: the catch-all section when sections exist, else the end of the file. |

**Examples**

```
$ rustorm add vps root@vps.example.com:2222
vps added. Connect with: ssh vps

$ rustorm add db1 postgres@db1.example.com --section "data foundry"
db1 added to section data foundry. Connect with: ssh db1

$ rustorm add web-prod web@webprod.example.com -i ~/.ssh/prod.pem -o StrictHostKeyChecking=no -o "ProxyCommand=ssh -W %h:%p bastion"
web-prod added. Connect with: ssh web-prod

$ rustorm add vps root@vps.example.com
error: vps already exists. Use rustorm edit or rustorm set to modify it.
```

**Exit status**

0 added · 1 name exists or URI invalid · 2 usage · 3 config file unreadable or unwritable.

### edit

Origin: stormssh · Status: v1

**Synopsis**

```
rustorm edit <NAME> <URI> [-i <FILE>] [-o KEY=VALUE]...
```

**Description**

Replaces `HostName`, `User` and `Port` of an existing entry from the URI. Other keys stay. `-i` and `-o` set or replace the named keys; `-s, --section <NAME>` moves the entry to that section. Use `set` to change one key without restating the URI.

**Examples**

```
$ rustorm edit vps emre@vps.example.com:2400
vps updated.

$ rustorm edit nope emre@vps.example.com
error: nope does not exist. Use rustorm add to create it.
```

**Exit status**

0 updated · 1 not found or URI invalid · 2 usage · 3 config file error.

### set

Origin: both · Status: v1

**Synopsis**

```
rustorm set [-r] [-a] <NAME|PATTERN> <KEY> <VALUE> [<KEY> <VALUE>]...
```

**Description**

Sets one or more keys on a host. The host must exist. Keys match case-insensitively and are written in canonical case (D6). On a multi-valued key `set` replaces every existing value; `--append` adds one more line (D13). A metadata key (`note`, `location`, `privateKeyLocation`, `other`, `tags`, see Host metadata) is set the same way and lands in a `# key: value` comment above the `Host` line; `note` is multi-valued, `tags` takes a comma-separated list, and an empty value removes the line.

**Options**

| Option | Effect |
|---|---|
| `-r, --regex` | Treat the first argument as a regular expression anchored to the whole host name and apply the change to every matching host. Replaces stormssh's `update`. |
| `-a, --append` | Add a value to a multi-valued key instead of replacing. |
| `--tag <TAG>` | Add one tag to `# tags:`, creating the line when absent. Repeatable. May be the only change: `set NAME --tag prod`. |
| `--untag <TAG>` | Remove one tag; removing the last tag removes the line. Repeatable. |

**Examples**

```
$ rustorm set vps User deploy Port 22
vps updated.

$ rustorm set -r 'vps-[1-5]' User emre
5 hosts updated: vps-1, vps-2, vps-3, vps-4, vps-5

$ rustorm set -a vps IdentityFile ~/.ssh/second.pem
vps updated.

$ rustorm set vps note "Primary build box" location "Austin DC, rack 4" --tag prod --tag db
vps updated.

$ rustorm set vps -- privateKeyLocation "-----BEGIN OPENSSH PRIVATE KEY-----"
error: privateKeyLocation holds a reference to a key, not the key itself.

$ rustorm set -r 'nomatch-.*' User x
error: no host matches nomatch-.*
```

**Exit status**

0 updated · 1 no host matched · 2 usage (odd number of key/value arguments) · 3 config file error.

### unset

Origin: both · Status: v1

**Synopsis**

```
rustorm unset [-r] <NAME|PATTERN> <KEY>...
```

**Description**

Removes the named keys from a host. Removing every key leaves an empty `Host` line; use `delete` to remove the entry. Removing a key the host does not have is not an error. A metadata key removes its `# key:` lines (every `# note:` line for `note`); other comments stay.

**Examples**

```
$ rustorm unset vps IdentityFile ProxyCommand
vps updated.

$ rustorm unset vps note tags
vps updated.
```

**Exit status**

0 updated · 1 host not found · 2 usage · 3 config file error.

### clone

Origin: both · Status: v1 · Aliases: `copy`, `cp`

**Synopsis**

```
rustorm clone [--keep-hostname] [-s <SECTION>] <NAME> <NEW-NAME> [<KEY> <VALUE>]...
```

**Description**

Copies every line of `NAME` into a new entry `NEW-NAME`. The copy lands in the source's section, or in `--section`, or at the end of an unsectioned file. If `HostName` contains `NAME` as a substring, the copy gets `NAME` replaced by `NEW-NAME` (D5). Trailing key/value pairs override or add keys on the copy.

**Examples**

```
$ rustorm show rails01
Host rails01
    HostName rails01.example.com
    User deploy

$ rustorm clone rails01 rails02
rails02 added. Connect with: ssh rails02

$ rustorm show rails02
Host rails02
    HostName rails02.example.com
    User deploy

$ rustorm clone rails01 rails03 HostName rails-03.example.com User dbrady
rails03 added. Connect with: ssh rails03

$ rustorm clone rails01 rails02
error: rails02 already exists.
```

**Exit status**

0 added · 1 source missing or target exists · 2 usage · 3 config file error.

### move

Origin: stormssh, sections new · Status: v1 · Aliases: `rename`, `mv`

**Synopsis**

```
rustorm move [-s <SECTION>] <NAME> [<NEW-NAME>]
```

**Description**

Renames an entry, moves it to another section, or both. `NEW-NAME` is required unless `--section` is given. A rename keeps the entry's aliases and keys and does not rewrite `HostName`. A section move re-sorts the entry into its new section; a missing section is created.

**Options**

| Option | Effect |
|---|---|
| `-s, --section <NAME>` | Destination section. |

**Examples**

```
$ rustorm move rails02 staging
rails02 renamed to staging. Connect with: ssh staging

$ rustorm move --section bob vps
vps moved to section bob.

$ rustorm move vps vps2 --section bob
vps renamed to vps2 and moved to section bob. Connect with: ssh vps2

$ rustorm move vps
error: give a new name, a --section, or both.
```

**Exit status**

0 · 1 source missing or target name exists · 2 usage · 3 config file error.

### delete

Origin: both · Status: v1 · Aliases: `rm`, `del`

**Synopsis**

```
rustorm delete <NAME>...
```

**Description**

Removes whole entries. Comments directly above an entry go with it. Any name that does not exist makes the command fail before anything is written.

**Examples**

```
$ rustorm delete vps staging
vps deleted.
staging deleted.

$ rustorm delete nope
error: nope does not exist.
```

**Exit status**

0 deleted · 1 a name was not found · 2 usage · 3 config file error.

### delete-all

Origin: stormssh · Status: v1 · Aliases: `delete_all`

**Synopsis**

```
rustorm delete-all [-y]
```

**Description**

Removes every host entry. Comments, blank lines, section banners and the `Host *` defaults section stay. On a terminal the command asks `Delete 14 hosts from ~/.ssh/config? [y/N]`; without a terminal it refuses unless `-y, --yes` is given (D8). On a workspace of several files it sweeps every file and asks `Delete 8 hosts from 5 files? [y/N]`; `--file` limits it to one file.

**Examples**

```
$ rustorm delete-all
Delete 14 hosts from /home/me/.ssh/config? [y/N] y
14 hosts deleted.

$ rustorm delete-all < /dev/null
error: refusing to delete 14 hosts without --yes on a non-interactive terminal.
```

**Exit status**

0 deleted · 1 declined or refused · 2 usage · 3 config file error.

### list

Origin: both · Status: v1 · Aliases: `ls`

**Synopsis**

```
rustorm list [-l] [-n] [-s NAME] [--where KEY=VALUE]... [--filter KEYS] [--format FMT] [--just-value] [--allow-missing]
```

**Description**

Prints one line per host, sorted by name, in the form `name -> user@hostname:port`. `--where`, `--filter`, `--format`, `--just-value` and `--allow-missing` select hosts, pick keys and change the format as Reading output describes; `--json` rows carry a `"meta"` object with the host's metadata (Host metadata). User and port fall back to the `Host *` defaults, then to `$USER` and `22`. A host without `HostName` shows `[no hostname]`. On a workspace of several files the hosts are grouped by file in load order, each group under a heading with the file's path, and a file's section headings follow its path heading; a file without hosts has no heading. `--json` rows carry the `"file"` field (D23).

**Options**

| Option | Effect |
|---|---|
| `-l, --long` | Also print every other key of each host, and the `Host *` defaults at the end. |
| `-n, --names` | Print only names, one per line. Shell completion uses this. |
| `-s, --section <NAME>` | Print only that section. |

**Examples**

```
$ rustorm list
github   -> git@github.com:22
vps      -> root@vps.example.com:2222
web-prod -> web@webprod.example.com:22

$ rustorm list -l
github   -> git@github.com:22
vps      -> root@vps.example.com:2222
    IdentityFile ~/.ssh/vps.pem
web-prod -> web@webprod.example.com:22
    StrictHostKeyChecking no
    ProxyCommand ssh -W %h:%p bastion

(*) defaults
    ServerAliveInterval 60

$ rustorm list
[data foundry]
db1    -> postgres@db1.example.com:22
[other]
github -> git@github.com:22
vps    -> root@vps.example.com:2222

$ rustorm --json list
[{"name":"db1","file":"/home/me/.ssh/config","section":"data foundry","aliases":[],"hostname":"db1.example.com","user":"postgres","port":22,"options":{},"meta":{}}, ...]
```

The section headings appear only when the file has sections. On the workspace of Included files:

```
$ rustorm list
~/.ssh/config
github           -> git@github.com:22
~/.ssh/config.d/cypress
cypressPro       -> travis@10.10.0.2:22
cypressPro-ext   -> travis@cypress.example.com:22
~/.ssh/config.d/df-austin
[data foundry]
db1              -> postgres@db1.example.com:22
dcaustin-pfsense -> admin@10.20.0.1:22
~/.ssh/config.d/ranch
dcevant          -> travis@dcevant.ranch.lan:22
ranch-nas        -> travis@nas.ranch.lan:22
~/.ssh/ranch.d/lab
lab-1            -> travis@10.30.0.5:22
```

The arrows align across the whole listing. `--section` keeps every file that holds the section.

**Exit status**

0 · 3 config file unreadable.

### show

Origin: ssh-config · Status: v1

**Synopsis**

```
rustorm show [<NAME>...] [-s NAME] [--where KEY=VALUE]... [--filter KEYS] [--format FMT] [--just-value] [--allow-missing]
```

**Description**

Prints the entries verbatim as they appear in the file, including their comments. `NAME` may be a primary name or an alias. At least one `NAME` is required unless `--where` or `--section` selects the hosts (Reading output); `--filter` replaces the verbatim entry with the named keys. `--json` objects carry `name`, `section`, `file`, `text` and a `"meta"` object (Host metadata).

**Examples**

```
$ rustorm show vps
# main box
Host vps
    HostName vps.example.com
    User root
    Port 2222

$ rustorm show nope
error: nope does not exist.
```

**Exit status**

0 · 1 a name was not found · 3 config file unreadable.

### dump

Origin: ssh-config · Status: v1 · Aliases: `cat`

**Synopsis**

```
rustorm dump
```

**Description**

Prints the whole config file as rustorm parsed it, with `Host` lines and keys colored on a terminal. Useful to confirm the parser preserves the file byte for byte. On a workspace of several files it prints the root; `--file` prints another workspace file.

**Examples**

```
$ rustorm dump | diff - ~/.ssh/config
$ echo $?
0
```

**Exit status**

0 · 3 config file unreadable.

### search

Origin: both · Status: v1 · Aliases: `find`, `grep`

**Synopsis**

```
rustorm search [-F] <PATTERN> [-s NAME] [--where KEY=VALUE]... [--filter KEYS] [--format FMT] [--just-value] [--allow-missing]
```

**Description**

Prints every host whose name, alias, key, value or metadata matches `PATTERN`, a regular expression (D7), in `list` format with matches highlighted. `-F, --fixed-strings` searches the literal text. When the match is in a metadata line and nowhere else on the row, the matching `# key: value` line prints indented under the host, so the output names the field. `--where`, `--filter`, `--format`, `--just-value` and `--allow-missing` work as Reading output describes and apply after the pattern.

**Examples**

```
$ rustorm search git
github -> git@github.com:22

$ rustorm search 'example\.com:2[0-9]+'
vps -> root@vps.example.com:2222

$ rustorm search "rack 4"
buildbox -> deploy@10.0.4.12:22
    # location: Austin DC, rack 4, U12

$ rustorm search zzz
no results found.
```

**Exit status**

0 matches found · 1 no match · 2 invalid pattern · 3 config file unreadable.

### alias

Origin: ssh-config · Status: v1

**Synopsis**

```
rustorm alias <NAME> <ALIAS>...
```

**Description**

Adds names to the entry's `Host` line so `ssh ALIAS` reaches the same host. An alias already present is skipped silently. An alias that is already another host's name is an error.

**Examples**

```
$ rustorm alias vps v box
vps now answers to: vps v box

$ rustorm show v
Host vps v box
    HostName vps.example.com
```

**Exit status**

0 · 1 host not found or alias taken · 2 usage · 3 config file error.

### unalias

Origin: ssh-config · Status: v1

**Synopsis**

```
rustorm unalias [<NAME>] <ALIAS>...
```

**Description**

Removes aliases from an entry's `Host` line. With `NAME`, removes the aliases from that host. With one argument, finds the host that carries the alias and removes it. The primary name cannot be removed; use `move` to rename.

**Examples**

```
$ rustorm unalias vps box
vps now answers to: vps v

$ rustorm unalias v
vps now answers to: vps
```

**Exit status**

0 · 1 host or alias not found · 2 usage · 3 config file error.

### sections

Origin: new · Status: v1 (D18)

**Synopsis**

```
rustorm sections
```

**Description**

Lists sections in file order with their host counts; the last one is the catch-all. Prints `no sections` on an unsectioned file. On a workspace of several files each file with sections gets a heading with its path, in load order, and its sections follow; files without sections are left out, and `no sections` prints only when no file has one:

```
$ rustorm sections
~/.ssh/config.d/df-austin
data foundry   2
```

**Examples**

```
$ rustorm sections
data foundry   1
bob            2
other          12
```

**Exit status**

0 · 3 config file unreadable.

### rename-section

Origin: new · Status: v1

**Synopsis**

```
rustorm rename-section <OLD> <NEW>
```

**Description**

Renames a section and regenerates its banner. Renaming onto an existing section merges the hosts into it and removes the old banner (D17). The catch-all can be renamed too; it stays the catch-all because it stays last.

**Examples**

```
$ rustorm rename-section bob cypresspt
section bob renamed to cypresspt.

$ rustorm rename-section other personal
section other renamed to personal.

$ rustorm rename-section cypresspt personal
section cypresspt merged into personal.

$ rustorm rename-section nope x
error: section nope does not exist.
```

**Exit status**

0 · 1 section not found · 2 usage · 3 config file error.

### add-section

Origin: new · Status: v1

**Synopsis**

```
rustorm add-section <NAME> [--before <SECTION>]
```

**Description**

Creates an empty section: a banner with no hosts under it. On a file with sections the new one goes immediately before the catch-all (D17); `--before` puts it in front of the named section instead. On a file without sections this is the first `--section` use: the named section is created and the catch-all `other` receives every existing host. A name that already exists is an error and the file is not written.

**Arguments**

| Argument | Meaning |
|---|---|
| `NAME` | The section name; spaces allowed when quoted; matched case-insensitively. |

**Options**

| Option | Effect |
|---|---|
| `--before <SECTION>` | Insert in front of `SECTION`, which must exist. |

**Examples**

```
$ rustorm add-section work
section work added; other created with 3 hosts.

$ rustorm add-section lab
section lab added.

$ rustorm sections
data foundry   1
bob            2
lab            0
other          12

$ rustorm add-section home --before bob
section home added before bob.

$ rustorm add-section lab
error: section lab already exists.
```

**Exit status**

0 · 1 section exists or `--before` section missing · 2 usage · 3 config file error.

### combine

Origin: new · Status: v1 · Aliases: `merge`

**Synopsis**

```
rustorm combine <FILE> <FILE>... [-o <OUTPUT>] [--on-conflict fail|keep|replace] [--stdout]
```

**Description**

Merges two or more ssh config files into one. The first `FILE` is the base: its preamble, comments, `Include` and `Match` lines, sections and hand-written order are kept, and every later file is folded into it in the order given. The result is written to the base unless `-o` or `--stdout` says otherwise. `--config` is ignored; the files are the arguments.

How each part of a later file lands:

- A host under a section joins the same-named section of the result, created before the catch-all when missing. Its leading comments travel with it. Hosts inside a section are sorted on write, as always.
- A host not under a section joins the catch-all when the result has sections, else follows the base's last host, in the order it had.
- `Host *` merges into the base's `Host *`: a key the base lacks is added, a key it has is kept and the later value is skipped; both are reported, one line per key. A base without `Host *` takes the later file's block whole.
- Other lines (comments not attached to a host, `Include`, `Match` blocks) are appended to the part they were in.
- `Include` lines are never edited. When an `Include` in the result still matches one of the input files, a warning names it, because deleting the line is your call.

A **conflict** is a host whose primary name or alias is already in the result. `--on-conflict` decides:

| Policy | Effect |
|---|---|
| `fail` (default) | Print every conflict, write nothing, exit 1. |
| `keep` | Keep the earlier file's block byte for byte; drop the later one. |
| `replace` | Replace the earlier block with the later one, in the earlier block's place and section. |

The summary line counts the result: `combined N files into OUTPUT: H hosts, S sections, C conflicts.` With `--stdout` the merged text is stdout and the summary goes to stderr. `--json` replaces every line with one object: `{"files": N, "hosts": H, "sections": S, "conflicts": [...], "added": [...], "skipped": [...], "includes": [...], "output": "..."}`; with `--stdout`, `output` is `null` and a `text` field carries the merged file.

**Options**

| Option | Effect |
|---|---|
| `-o, --output <FILE>` | Write the result to `FILE` instead of the first input. Backed up first only if it exists. |
| `--on-conflict <POLICY>` | `fail`, `keep` or `replace`. Default `fail`. |
| `--stdout` | Print the result; write no file and no backup. |

**Examples**

```
$ rustorm combine ~/.ssh/config ~/.ssh/config.d/cypress
Host *: Compression yes added from /home/me/.ssh/config.d/cypress.
Host *: ServerAliveInterval 60 from /home/me/.ssh/config.d/cypress skipped, keeping 30.
warning: Include config.d/* in /home/me/.ssh/config still loads /home/me/.ssh/config.d/cypress.
combined 2 files into /home/me/.ssh/config: 5 hosts, 2 sections, 0 conflicts.

$ rustorm combine ~/.ssh/config ~/.ssh/config.d/legacy
error: 2 hosts are defined more than once; nothing written. Use --on-conflict keep or replace.
  nas: /home/me/.ssh/config, /home/me/.ssh/config.d/legacy
  vps: /home/me/.ssh/config, /home/me/.ssh/config.d/legacy

$ rustorm combine ~/.ssh/config ~/.ssh/config.d/legacy --on-conflict keep
warning: Include config.d/* in /home/me/.ssh/config still loads /home/me/.ssh/config.d/legacy.
combined 2 files into /home/me/.ssh/config: 6 hosts, 2 sections, 2 conflicts.

$ rustorm combine ~/.ssh/config.d/cypress ~/.ssh/config.d/legacy --stdout
Host *
    ServerAliveInterval 60
    Compression yes

Host cypress
    HostName cypress.example.com
    User travis

Host cypress-db
    HostName 10.0.0.5
    User postgres

Host vps
    HostName old-vps.example.com
    User admin

Host nas
    HostName nas.local

Host printer
    HostName 192.168.1.20
combined 2 files: 5 hosts, 0 sections, 0 conflicts.

$ rustorm combine ~/.ssh/config
error: combine needs at least two files.
```

**Exit status**

0 written · 1 conflicts under `fail` · 2 usage, fewer than two files · 3 an input unreadable or the output unwritable.

### includes

Origin: new · Status: v1

**Synopsis**

```
rustorm includes
```

**Description**

Lists the workspace: every `Include` line of the root and of the files it loads, in load order, with the files each one matched and their host counts (see Included files). Each `Include` prints `<file>: Include <patterns>`, where `<file>` holds the line; its matched files follow, indented two spaces deeper, each with its host count, `Host *` excluded. A matched file's own `Include` lines follow it, two spaces deeper again. A pattern that matches nothing prints `matches no files`; a file already loaded prints `already loaded` after its path instead of a count; an unreadable one prints `cannot read (<reason>)`. The counts align in one column, right-aligned. A summary line closes the listing: `N files, H hosts.`, counting the root and every loaded file. A root without `Include` prints `no Include lines in <root>`.

`--json` prints a list with one object per matched file of each root `Include`: `{"pattern": "...", "from": "...", "file": "...", "hosts": N, "nested": [...]}`, where `pattern` is the pattern as written, `from` the absolute path of the file holding the line, `file` the absolute path matched, and `nested` the same objects for that file's own `Include` lines. A pattern that matches nothing gives one object with `"file": null` and `"hosts": 0`.

**Examples**

```
$ rustorm includes
~/.ssh/config: Include ~/.ssh/config.d/*
  ~/.ssh/config.d/cypress    2 hosts
  ~/.ssh/config.d/df-austin  2 hosts
  ~/.ssh/config.d/ranch      2 hosts
    ~/.ssh/config.d/ranch: Include ranch.d/*
      ~/.ssh/ranch.d/lab     1 host
5 files, 8 hosts.

$ rustorm --json includes
[{"pattern":"~/.ssh/config.d/*","from":"/home/me/.ssh/config","file":"/home/me/.ssh/config.d/cypress","hosts":2,"nested":[]}, ...]

$ rustorm includes
~/.ssh/config: Include ~/.ssh/work/*
  matches no files
1 file, 1 host.

$ rustorm includes
no Include lines in ~/.ssh/config
```

**Exit status**

0 · 3 root config file unreadable.

### backup

Origin: both · Status: v1

**Synopsis**

```
rustorm backup [<FILE>]
```

**Description**

Copies the config file to `FILE`, or to `<config>~` when omitted. Every writing command already does this unless `--no-backup` is given (D2); `backup` is for a named copy before hand-editing.

**Examples**

```
$ rustorm backup ~/ssh-config-2026-09-24
/home/me/.ssh/config copied to /home/me/ssh-config-2026-09-24
```

**Exit status**

0 · 3 read or write failed.

### check

Origin: new (ssh-config TODO) · Status: v1 · Aliases: `lint`

**Synopsis**

```
rustorm check
```

**Description**

Reads the config and reports problems without changing anything: keys not in ssh_config(5), duplicate keys in one entry, hosts without `HostName`, `IdentityFile` paths that do not exist, duplicate names across entries, and lines the parser could not classify.

On a workspace of several files it checks every file and also reports:

- **A host defined in two files.** ssh uses the first definition it reads; the others are dead text. `rustorm reconcile` drops the identical copies and walks through the real conflicts.
- **An `Include` that loads a backup.** A matched file whose name ends in `~`, `.bak`, `.orig` or `.old`, or contains `.bak.`, looks like a backup, and its hosts shadow or duplicate the real ones.
- **An `Include` inside `Host *`.** rustorm and ssh both read it as global (see Included files); moving the line above `Host *` says so plainly.

An included file that cannot be read is reported as a problem too (D24). The count line counts hosts across the workspace. `--json` findings carry the `"file"` field (D23).

On a root whose `Include ~/.ssh/config.d/*` sits below `Host *`, with a second line `Include ~/.ssh/config.d/private` and a stray `df-austin.bak.20260628232757` holding `dcaustin-pfsense`:

```
$ rustorm check
dcaustin-pfsense: defined in ~/.ssh/config.d/df-austin and ~/.ssh/config.d/df-austin.bak.20260628232757; ssh uses the first
Include ~/.ssh/config.d/*: loads ~/.ssh/config.d/df-austin.bak.20260628232757, which looks like a backup
Include ~/.ssh/config.d/*: inside Host * in ~/.ssh/config; treated as global
Include ~/.ssh/config.d/private: cannot read (permission denied)
4 problems in 9 hosts.
```

**Examples**

```
$ rustorm check
vps: IdentityFile ~/.ssh/old.pem does not exist
web-prod: unknown key StrictHostKeyChekcing
2 problems in 14 hosts.

$ rustorm check
no problems in 14 hosts.
```

**Exit status**

0 no problems · 1 problems found · 3 config file unreadable.

### reconcile

Origin: new · Status: v1 · Aliases: `resolve`

**Synopsis**

```
rustorm reconcile [<FILE>...] [--list]
                  [--take-copy <HOST>]... [--keep-live <HOST>]... [--add <HOST>]...
                  [--all-live | --all-copy] [--drop-identical] [--retire]
```

**Description**

Resolves the hosts defined in two or more workspace files (D28), the ones `check` reports as `defined in ... ssh uses the first`. Each pair is a **live** definition, the one ssh reads first (D29), and a **copy**, a later one. A host in three files makes two pairs with the same live definition. When the live definition sits in a file that looks like a backup (see `check`), the output says so: `note: github: ssh reads ~/.ssh/config.d/cypress.bak first, so its definition is the live one.`

With `FILE...` only the copies in those files are in scope. `FILE` is a loaded file's name or a path, as for `--file`.

Each pair is one of:

| Kind | Meaning |
|---|---|
| identical | The directives (canonical key case, whitespace collapsed, in order) and the metadata lines match (D30). Plain comments, blank lines and indentation are ignored. |
| conflict | Anything else, a metadata difference included. |
| orphan | A host in a named `FILE` that no other file defines. Orphans appear only when `FILE...` is given. |

**Listing.** `--list` prints `N conflicts, M identical, K orphans across F files`, where `F` counts the files holding a pair or an orphan, then a unified diff per conflict, live first, over the normalized block (metadata lines, the `Host` line, one line per directive), then one `orphan: <host> in <file>` line per orphan. A blank line separates the diffs. Listing is what `reconcile` does off a terminal when no decision is given. `--json` prints the report as one object, `{"conflicts": N, "identical": M, "orphans": K, "files": F, "items": [...]}`; each item carries `name`, `names` (every name the pair shares), `kind`, `live` and `copy` (each `{"file", "line", "section", "text"}`, `live` `null` for an orphan), `live_is_backup`, `note`, `keys` (each differing key with its `live` and `copy` values) and `diff`. `--json` applies nothing; given with a decision it is a usage error.

**On a terminal**, without a decision, each conflict prints its two blocks side by side, live on the left, and asks `[l]ive / [c]opy / [k]eys / [s]kip / [q]uit`. `k` asks once per differing key, `[l]ive / [c]opy`, and builds the result from the picks. With `FILE...` each orphan asks `[a]dd / [s]kip / [q]uit`. `q`, or the end of input, stops asking and applies the decisions made so far. The blocks and prompts go to stderr, the messages to stdout. With `--retire` the questions come first.

**Decisions** without a terminal:

| Decision | Effect |
|---|---|
| `--take-copy HOST` | The live block takes the copy's body and metadata lines. Its `Host` line (aliases), its section and its place stay (D31). |
| `--keep-live HOST` | Writes nothing; the conflict counts as decided for this run. On an orphan, leaves it out of the live config. |
| `--add HOST` | Moves the orphan into the root, or into the file `--file` names, in its section when that file has a section of the same name. |
| `--all-live` | `--keep-live` for every conflict not named otherwise. |
| `--all-copy` | `--take-copy` for every conflict not named otherwise. |
| `--drop-identical` | Removes every identical copy's block, its leading comments included, from the copy's file. The live definition is never touched. |

A host flag repeats for several hosts. A host with copies in two files needs the copy named as `FILE`. Every changed file is written once, after its own backup (see Included files). Messages name each decision, then `N conflicts remain.` or `no conflicts remain.` counts the conflicts in scope left without a decision.

**Retiring.** `--retire` needs `FILE...`. After the decisions it moves each named file to `~/.ssh/retired/<name>` (created with mode `0700`; an existing name gets `.1`, `.2`, ...) when every host in it is resolved: identical to its live definition, a conflict decided in this run, or an orphan added or left out. A file that still holds the live definition of a host whose copy elsewhere differs is not resolved, since ssh would read that copy instead. Otherwise the file stays and the error names what remains. The root never retires. No file is ever deleted.

**Options**

| Option | Effect |
|---|---|
| `--list` | Print the report; decide nothing. Given with a decision it is a usage error. |
| `--take-copy <HOST>` | Take the copy for `HOST`. Repeatable. |
| `--keep-live <HOST>` | Keep the live definition of `HOST`, or leave the orphan `HOST` out. Repeatable. |
| `--add <HOST>` | Move the orphan `HOST` into the root or the `--file` target. Repeatable. |
| `--all-live` | Keep the live definition of every other conflict. |
| `--all-copy` | Take the copy of every other conflict. Conflicts with `--all-live`. |
| `--drop-identical` | Remove every identical copy from its file. |
| `--retire` | Move each named `FILE` to `~/.ssh/retired/` once fully resolved. |

**Examples**

On the root `Include ~/.ssh/config.d/*` with `github`, `~/.ssh/config.d/cypress` holding `cypressPro`, `cypressPro-ext` and `lab-1`, and a stray `~/.ssh/config.d/cypress.bak` holding the same three plus `printer`:

```
$ rustorm reconcile --list
2 conflicts, 1 identical, 0 orphans across 2 files
--- ~/.ssh/config.d/cypress (live)
+++ ~/.ssh/config.d/cypress.bak (copy)
@@ -1,3 +1,3 @@ cypressPro
 Host cypressPro
-    HostName 10.10.0.2
+    HostName 10.10.0.9
     User travis

--- ~/.ssh/config.d/cypress (live)
+++ ~/.ssh/config.d/cypress.bak (copy)
@@ -1,4 +1,3 @@ lab-1
-# location: Austin DC, rack 4
 Host lab-1
     HostName 10.10.0.30
     User travis

$ rustorm reconcile --drop-identical
1 identical copy dropped from ~/.ssh/config.d/cypress.bak.
2 conflicts remain.

$ rustorm reconcile --take-copy cypressPro --keep-live lab-1
cypressPro: took the copy from ~/.ssh/config.d/cypress.bak into ~/.ssh/config.d/cypress.
lab-1: kept ~/.ssh/config.d/cypress.
no conflicts remain.

$ rustorm reconcile cypress.bak --list
1 conflict, 1 identical, 1 orphan across 2 files
--- ~/.ssh/config.d/cypress (live)
+++ ~/.ssh/config.d/cypress.bak (copy)
@@ -1,4 +1,3 @@ lab-1
-# location: Austin DC, rack 4
 Host lab-1
     HostName 10.10.0.30
     User travis

orphan: printer in ~/.ssh/config.d/cypress.bak

$ rustorm reconcile cypress.bak --retire
error: ~/.ssh/config.d/cypress.bak not retired; undecided: lab-1 (conflict), printer (orphan).

$ rustorm reconcile cypress.bak --keep-live lab-1 --add printer --retire
lab-1: kept ~/.ssh/config.d/cypress.
printer added to ~/.ssh/config from ~/.ssh/config.d/cypress.bak.
no conflicts remain.
~/.ssh/config.d/cypress.bak retired to ~/.ssh/retired/cypress.bak.

$ rustorm reconcile --take-copy nas
error: nas is not defined in two workspace files.
```

**Exit status**

0 no conflict in scope left undecided · 1 conflicts remain, or a `--retire` refused · 2 usage: an unknown or undecidable host, a host decided twice, `--retire` without `FILE`, `--json` or `--list` with a decision, `--all-live` with `--all-copy` · 3 a file unreadable, unwritable or not movable.

### completion

Origin: new (replaces stormssh's contrib bash script) · Status: v1

**Synopsis**

```
rustorm completion <bash|zsh|fish|powershell>
```

**Description**

Prints a completion script for the named shell to stdout. Host-name positions complete from `rustorm list -n`.

**Examples**

```
$ rustorm completion zsh > ~/.zfunc/_storm
```

**Exit status**

0 · 2 unknown shell.

### version

Origin: stormssh · Status: v1

**Synopsis**

```
rustorm version
rustorm -V
```

**Description**

Prints `rustorm <semver>`.

**Examples**

```
$ rustorm version
rustorm 0.1.0
```

**Exit status**

0.


## Files

| File | Role |
|---|---|
| `~/.ssh/config` | The file every command reads and writes. `--config` overrides it. Created with mode `0600` when missing. |
| `~/.ssh/config~` | Backup written before every change (D2). |
| Included files | Every file the root's `Include` lines load (see Included files). Each is written only when a change routes to it, after a backup to its own `<file>~`, or to `<dir>/.<name>~` when an `Include` pattern would match `<file>~`. |
| `~/.ssh/retired/` | Copies `reconcile --retire` moved out of the workspace, outside every `Include` glob. |
| rustorm config | Command aliases and defaults, TOML. Linux: `$XDG_CONFIG_HOME/rustorm/config.toml` (`~/.config/rustorm/config.toml`). macOS: `~/Library/Application Support/rustorm/config.toml`. Windows: `%AppData%\rustorm\config.toml`. |

rustorm config example:

```toml
[aliases]
delete = ["rm", "del"]
add = ["create", "touch"]

[defaults]
backup = true
color = "auto"
```

## Environment

| Variable | Effect |
|---|---|
| `USER` | Default user when neither the URI nor `Host *` gives one. |
| `NO_COLOR` | Disables color when set to any value. |
| `RUSTORM_CONFIG` | Same as `--config`. The flag wins when both are set. |

## Exit status

| Code | Meaning |
|---|---|
| 0 | Success. For `search`, at least one match. For `check`, no problems. |
| 1 | The operation could not be applied: host not found, name taken, invalid URI or pattern, nothing matched, problems found, confirmation declined. |
| 2 | Usage error: unknown command, missing or extra arguments, bad flag. |
| 3 | The config file could not be read, parsed or written. |
| 4 | A key named in `--filter` is not set on a host that was printed (D27). The output is complete; a warning names each host and key. `--allow-missing` turns this into 0. |

Errors go to stderr prefixed `error: `. Success messages go to stdout and `-q` silences them. When more than one code applies, the lowest nonzero code wins: a missing host (1) is reported before a missing key (4).

## Shell completion

`rustorm completion <shell>` prints the script. Completion of host-name arguments calls `rustorm list -n`, so it reflects the file at the moment you press Tab. The value of `--filter` and the key part of `--where` complete to the ssh_config keywords, the metadata keys and the pseudo-keys `Host`, `section` and `file`; `--format` completes to its four names and `yml`.
