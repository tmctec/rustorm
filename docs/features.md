# rustorm — feature inventory

Every row below is a capability drawn from `stormssh` (origin `storm`), from `dbrady/ssh-config` (origin `ssh-config`), from both (origin `both`), or proposed new for rustorm (origin `new`). `Port` records the outcome of the user's exclusion review and is `pending` on every row until that review runs. MoSCoW reflects this document's best judgment ahead of that review: core config-file operations are `Must`, storm/ssh-config niceties are `Should`, web UI/library API/ecosystem items are `Could`, and warts not worth carrying forward are `Won't`.

F-01 through F-22 use the same ids as the Command mapping table in `docs/cli.md`, so citations between the two documents stay consistent; F-23 onward numbers the remaining capabilities from the two inventories.

| F-NN | Feature | Origin | Description | MoSCoW (Must/Should/Could/Won't) | Port |
|---|---|---|---|---|---|
| F-01 | add | storm | Creates a new `Host` entry from a connection URI, writing HostName/User/Port; fails if the name already exists (storm's `add`). | Must | yes |
| F-02 | edit | storm | Replaces HostName/User/Port on an existing entry from a URI, preserving other keys (storm's `edit`). | Must | yes |
| F-03 | set | both | Sets one or more keys on a host by name or, with `--regex`, on every host matching a pattern; replaces storm's `update` and covers ssh-config's `set`. | Must | yes |
| F-04 | unset | both | Removes named keys from a host; covers ssh-config's `unset` and the gap left by storm having no dedicated key-removal command. | Must | yes |
| F-05 | clone | both | Copies every line of a host to a new name, rewriting HostName by substituting the old name for the new one, and applies trailing key/value overrides; covers storm's `clone` (verbatim copy, no rewrite) and ssh-config's `copy`/`cp` (rewrite plus overrides). | Must | yes |
| F-06 | move | storm | Renames a host, moves it to another section, or both, in one step; covers storm's `move` (implemented there as clone-then-delete). | Must | yes |
| F-07 | delete | both | Removes whole host entries by name, failing before any write if a name is missing; covers storm's `delete` (exact match, errors on miss) and ssh-config's `rm`/`del`/`delete` (silent no-op on miss). | Must | yes |
| F-08 | delete-all | storm | Removes every host entry while keeping comments, banners and `Host *`; covers storm's `delete_all`, which instead wipes the whole file (comments, blank lines and defaults too) with no confirmation. | Must | yes |
| F-09 | list | both | Prints one line per host sorted by name; covers storm's `list` (detailed, shows defaults) and ssh-config's `list` (names only). | Must | yes |
| F-10 | show | ssh-config | Prints one or more entries verbatim including their comments; covers ssh-config's `show` (storm has no equivalent). | Must | yes |
| F-11 | dump | ssh-config | Prints the whole file as parsed, for confirming byte-for-byte round-trip; covers ssh-config's `dump` (storm has no equivalent). | Should | yes |
| F-12 | search | both | Finds hosts whose name, alias, key or value matches a pattern; covers storm's `search` (case-sensitive substring) and ssh-config's `search` (regex with highlight). | Must | yes |
| F-13 | alias | ssh-config | Adds extra names to a host's `Host` line; covers ssh-config's `alias` (storm has no equivalent). | Should | yes |
| F-14 | unalias | ssh-config | Removes extra names from a host's `Host` line; covers ssh-config's `unalias` (storm has no equivalent). | Should | yes |
| F-15 | backup | both | Copies the config file to a named target on demand; covers storm's `backup` and ssh-config's constructor-level backup toggle. | Should | yes |
| F-16 | check | new | Reports config problems (unknown keys, missing HostName, missing identity files, duplicate names) without changing the file; new, based on ssh-config's TODO idea. | Should | yes |
| F-17 | completion | new | Prints a shell completion script for bash, zsh, fish or PowerShell, replacing storm's single contrib bash-completion script. | Should | yes |
| F-18 | version | storm | Prints the tool's version; covers storm's `version`. | Must | yes |
| F-19 | web | storm | Serves a local browser UI with JSON routes over the same operations; covers storm's `web` (Flask, port 9002, three themes). | Could | no |
| F-20 | sections | new | Lists sections in file order with host counts; new, not in either reference tool. | Should | yes |
| F-21 | rename-section | new | Renames a section and regenerates its banner, merging into an existing section if the new name collides; new. | Should | yes |
| F-22 | move --section | new | Moves a host into a different section as part of `move`, creating the section if needed; new, part of the sections feature. | Should | yes |
| F-23 | Connection-URI grammar | storm | `[user@]host[:port]` parsing for `add`/`edit`, falling back to `Host *` then `$USER`/22 for missing parts (storm's `ssh_uri_parser`). | Must | yes |
| F-24 | `-o`/`--option` custom directive flag | storm | Repeatable flag that writes an arbitrary ssh_config directive, splitting on the first `=` only (storm's `--o`, fixed from its unlimited-split crash on embedded `=`). | Must | yes |
| F-25 | `-i`/`--identity` identity-file flag | storm | Sets IdentityFile on `add`/`edit` (storm's `--id_file`, renamed per D14). | Must | yes |
| F-26 | `-c`/`--config` alternate file | storm | Operates on a config file other than `~/.ssh/config` (storm's `--config`, exposed on every command; ssh-config supports this only in its library, never its CLI). | Must | yes |
| F-27 | Regex bulk update across hosts | storm | Applies a key/value change to every host whose name matches a pattern; storm's `update` used unanchored `re.match` prefix semantics, replaced by `set --regex` with full-pattern matching (D3). | Should | yes |
| F-28 | Multi-name `edit` comma-joining | storm | storm's `edit host1,host2 ...` does not edit two hosts; it collapses the names into one literal `Host host1 host2` key. A wart, not a feature to port. | Won't | no |
| F-29 | Comment and blank-line preservation | both | Every comment and blank line round-trips at its original position on parse and write (storm's `comment`/`empty_line` records; ssh-config's raw per-line `lines` arrays). | Must | yes |
| F-30 | `Host *` defaults | storm | The `Host *` block's `User`/`Port` (and other keys) feed URI-resolution defaults and print as a separate defaults section in `list -l` (storm's `defaults`). | Must | yes |
| F-31 | Multi-valued keys accumulate | storm | IdentityFile, LocalForward, RemoteForward and similar keys accumulate as repeated lines instead of overwriting (storm special-cases three keys; D13 extends this to the full ssh_config(5) list). | Must | yes |
| F-32 | 0600 file creation | storm | Creates a missing config file, and its parent directory, with mode 0600 (storm's `ConfigParser.__init__`). | Must | yes |
| F-33 | Automatic backup on write | ssh-config | Every mutating command backs the file up to `<config>~` before writing, with no flag needed (ssh-config's `ConfigFile#save`; D2 makes this rustorm's default, unlike storm's manual-only `backup`). | Must | yes |
| F-34 | Colored output | both | Host lines, keys and search matches are ANSI-colored on a terminal (storm's `termcolor` prefixes; ssh-config's regex-based `colorize` on `show`/`dump`/`search`). | Should | yes |
| F-35 | Command aliases | storm | User-defined alternate spellings for a command name, read from a config file (storm's `~/.stormssh/config` JSON `aliases` map; rustorm's own config file per D10). | Should | yes |
| F-36 | Host-name shell completion | storm | Tab-completion of host-name arguments reads the live host list at completion time (storm's contrib script shells out to `storm list`; rustorm's `completion` calls `list -n`). | Should | yes |
| F-37 | JSON API via web UI | storm | The web UI's `/list`, `/add`, `/edit`, `/delete` routes return JSON (storm's Flask routes); superseded for scripting by `--json` on the CLI (D9). | Could | no |
| F-38 | Library API | both | An embeddable API for host CRUD without shelling out (storm's `Storm` class; ssh-config's `ConfigFile`/`ConfigSection` classes). | Could | no |
| F-39 | Ecosystem companion apps | storm | A desktop GUI and a tray indicator that drive the same config file (storm's `storm-gui` and `storm-indicator`, separate repos by the same author). | Could | no |
| F-40 | `-` deletes a key | ssh-config | Setting a key's value to a literal `-` deletes that key instead of writing it (ssh-config's `set host key -`, an alternate route to `unset`). | Should | no |
| F-41 | Multi-name `Host` line as alias storage | ssh-config | Aliases are stored as extra names on the same `Host` directive line, not a separate bookkeeping structure, matching native ssh_config's own multi-name `Host` syntax (ssh-config's `header`/`aliases`). | Must | yes |
| F-42 | Search-match highlighting | ssh-config | Matched text in `search` output is wrapped in a highlight color (ssh-config's yellow-background `gsub` pass). | Should | yes |
| F-43 | Hand-edit survival | ssh-config | Formatting, whitespace and unrecognized directives the tool never touches survive a write unchanged, because settings are stored as raw per-line text, not a rebuilt structure (ssh-config's `@lines` arrays). | Must | yes |
| F-44 | Option-name validation | ssh-config | Flags directive keys not found in the real ssh_config(5) keyword list, catching typos like `HostNmae` (ssh-config's TODO idea; implemented as rustorm's `check`). | Should | yes |
| F-45 | Section banner grammar | new | A 103-column banner made of a rule line, a `section: <name>` label line, FIGlet standard-font art of the name, and a closing rule line, so the banner stays a valid comment block (D16). | Must | yes |
| F-46 | Alphabetical order within a section | new | Hosts inside a section are sorted alphabetically by primary name on every write. | Must | yes |
| F-47 | Automatic catch-all section | new | The first `--section` use creates a section named `other` that holds every host not yet under a banner, stays last in the file, and can be renamed without losing that role (D17). | Must | yes |
| F-48 | Options anywhere on the line | new | Flags may appear before, between or after positional arguments on every command (D19), unlike either reference tool's argparse/case-based parsing. | Should | yes |
| F-49 | Config file as the only database | both | No separate cache, index or database file; the config file is parsed fresh on every invocation and written straight back (storm's `config_data`; ssh-config's `@sections`; D15). | Must | yes |
| F-50 | `--json` output | new | Every read command can emit JSON on stdout instead of text, for the planned GUI (D9); neither reference tool offers this from its CLI. | Should | yes |
| F-51 | Canonical key case | new | Keys match case-insensitively but are always written in ssh_config(5)'s canonical case, such as `HostName`/`IdentityFile` (D6), unlike storm's all-lowercase writes. | Must | yes |
| F-52 | combine | new | Merges two or more config files into one: the first file is the base and keeps its order, sections merge by name, unsectioned hosts join the catch-all, `Host *` gains absent keys, `Include` lines are kept and warned about, duplicate names fail unless `--on-conflict keep` or `replace` is given. | Should | yes |
| F-53 | add-section | new | Creates an empty section by name, before the catch-all or before a named section; the first section on an unsectioned file also creates the catch-all `other`. | Should | yes |
