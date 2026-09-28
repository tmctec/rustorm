//! `completion`: shell scripts generated from the clap command tree, plus
//! host-name completion from `rustorm list -n`.

use clap::CommandFactory;

use crate::cli::{Cli, Shell};

/// Command names and aliases whose positional arguments are existing hosts.
const HOST_COMMANDS: &str =
    "edit set update unset clone copy cp move rename mv delete rm del show alias unalias";

/// Every positional whose help starts with this names an existing host.
const HOST_HELP: &str = "Existing host name";

const ZSH_HOSTS: &str = r#"
_rustorm_hosts() {
    local -a hosts
    hosts=(${(f)"$(rustorm list -n 2>/dev/null)"})
    _describe -t hosts 'host' hosts
}
"#;

const BASH_HOSTS: &str = r#"
_rustorm_with_hosts() {
    local cur="${COMP_WORDS[COMP_CWORD]}" cmd="" i
    for ((i = 1; i < COMP_CWORD; i++)); do
        case "${COMP_WORDS[i]}" in
            -c|--config|-s|--section) ((i++)) ;;
            -*) ;;
            *) cmd="${COMP_WORDS[i]}"; break ;;
        esac
    done
    case " __HOST_COMMANDS__ " in
        *" $cmd "*)
            if [[ -n "$cmd" && "$cur" != -* ]]; then
                COMPREPLY=( $(compgen -W "$(rustorm list -n 2>/dev/null)" -- "$cur") )
                return 0
            fi ;;
    esac
    _rustorm "$@"
}
complete -F _rustorm_with_hosts -o bashdefault -o default rustorm
"#;

/// The completion script for `shell`.
pub fn script(shell: Shell) -> String {
    let generator = match shell {
        Shell::Bash => clap_complete::Shell::Bash,
        Shell::Zsh => clap_complete::Shell::Zsh,
        Shell::Fish => clap_complete::Shell::Fish,
        Shell::Powershell => clap_complete::Shell::PowerShell,
    };
    let mut buf = Vec::new();
    clap_complete::generate(generator, &mut Cli::command(), "rustorm", &mut buf);
    let script = String::from_utf8(buf).expect("clap_complete writes UTF-8");
    match shell {
        Shell::Zsh => zsh_hosts(&script),
        Shell::Bash => format!("{script}{}", BASH_HOSTS.replace("__HOST_COMMANDS__", HOST_COMMANDS)),
        Shell::Fish => format!(
            "{script}complete -c rustorm -n \"__fish_rustorm_using_subcommand {HOST_COMMANDS}\" -f -a \"(rustorm list -n 2>/dev/null)\"\n"
        ),
        Shell::Powershell => script,
    }
}

/// Points zsh's host positionals at `_rustorm_hosts`.
fn zsh_hosts(script: &str) -> String {
    let mut out = String::with_capacity(script.len() + ZSH_HOSTS.len());
    let mut inserted = false;
    for line in script.split_inclusive('\n') {
        if line.contains(&format!("-- {HOST_HELP}")) {
            out.push_str(&line.replace(":_default'", ":_rustorm_hosts'"));
        } else {
            out.push_str(line);
        }
        if !inserted && line.starts_with("autoload -U is-at-least") {
            out.push_str(ZSH_HOSTS);
            inserted = true;
        }
    }
    out
}
