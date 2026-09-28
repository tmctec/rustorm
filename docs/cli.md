# rustorm — command-line reference

`rustorm` (rust storm) manages the hosts in your `~/.ssh/config`. It adds, edits, clones, deletes, lists, searches and groups host entries into sections, and it never trashes the file: comments, blank lines and hand-written formatting survive every write. It is written in Rust.

This document is the contract the implementation is built against. Each command carries an `Origin` line (which reference tool it comes from) and a `Status` line (`v1` after the feature review).

## Decisions

| # | Decision | Alternative | Status |
|---|---|---|---|
| D1 | The binary and crate are `rustorm`, short for rust storm. The repository is `rs-storm`. The implementation is Rust. | `storm`, `sshc` | agreed |
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

## Synopsis

```
rustorm [GLOBAL OPTIONS] <COMMAND> [ARGS]
```

## Global options

| Option | Effect |
|---|---|
| `-c, --config <FILE>` | Operate on `FILE` instead of `~/.ssh/config`. |
| `--no-backup` | Do not write `<config>~` before changing the file. |
| `--json` | Emit JSON instead of text on `list`, `show`, `dump`, `search`, `check`. This is the machine interface; there is no web API. |
| `--no-color` | Disable ANSI color. `NO_COLOR` in the environment does the same. |
| `-q, --quiet` | Suppress success messages. Errors still print. |
| `-s, --section <NAME>` | Section for `add`, `edit`, `clone`, `move` and `list`; accepted before or after the command. |
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
| `completion` | contrib script | — | — | F-17 |
| `sections` | — | — | — | F-20 |
| `rename-section` | — | — | — | F-21 |
| `add-section` | — | — | — | F-53 |
| `combine` | — | — | `merge` | F-52 |
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
| `-o, --option KEY=VALUE` | Writes any ssh_config directive. Repeatable. Splits on the first `=` only. |
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

Sets one or more keys on a host. The host must exist. Keys match case-insensitively and are written in canonical case (D6). On a multi-valued key `set` replaces every existing value; `--append` adds one more line (D13).

**Options**

| Option | Effect |
|---|---|
| `-r, --regex` | Treat the first argument as a regular expression anchored to the whole host name and apply the change to every matching host. Replaces stormssh's `update`. |
| `-a, --append` | Add a value to a multi-valued key instead of replacing. |

**Examples**

```
$ rustorm set vps User deploy Port 22
vps updated.

$ rustorm set -r 'vps-[1-5]' User emre
5 hosts updated: vps-1, vps-2, vps-3, vps-4, vps-5

$ rustorm set -a vps IdentityFile ~/.ssh/second.pem
vps updated.

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

Removes the named keys from a host. Removing every key leaves an empty `Host` line; use `delete` to remove the entry. Removing a key the host does not have is not an error.

**Examples**

```
$ rustorm unset vps IdentityFile ProxyCommand
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

Removes every host entry. Comments, blank lines, section banners and the `Host *` defaults section stay. On a terminal the command asks `Delete 14 hosts from ~/.ssh/config? [y/N]`; without a terminal it refuses unless `-y, --yes` is given (D8).

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
rustorm list [-l] [-n]
```

**Description**

Prints one line per host, sorted by name, in the form `name -> user@hostname:port`. User and port fall back to the `Host *` defaults, then to `$USER` and `22`. A host without `HostName` shows `[no hostname]`.

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
[{"name":"db1","section":"data foundry","aliases":[],"hostname":"db1.example.com","user":"postgres","port":22,"options":{}}, ...]
```

The section headings appear only when the file has sections.

**Exit status**

0 · 3 config file unreadable.

### show

Origin: ssh-config · Status: v1

**Synopsis**

```
rustorm show <NAME>...
```

**Description**

Prints the entries verbatim as they appear in the file, including their comments. `NAME` may be a primary name or an alias.

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

Prints the whole config file as rustorm parsed it, with `Host` lines and keys colored on a terminal. Useful to confirm the parser preserves the file byte for byte.

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
rustorm search [-F] <PATTERN>
```

**Description**

Prints every host whose name, alias, key or value matches `PATTERN`, a regular expression (D7), in `list` format with matches highlighted. `-F, --fixed-strings` searches the literal text.

**Examples**

```
$ rustorm search git
github -> git@github.com:22

$ rustorm search 'example\.com:2[0-9]+'
vps -> root@vps.example.com:2222

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

Lists sections in file order with their host counts; the last one is the catch-all. Prints `no sections` on an unsectioned file.

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

Errors go to stderr prefixed `error: `. Success messages go to stdout and `-q` silences them.

## Shell completion

`rustorm completion <shell>` prints the script. Completion of host-name arguments calls `rustorm list -n`, so it reflects the file at the moment you press Tab.
