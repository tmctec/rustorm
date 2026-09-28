//! `rustorm`: manage the hosts in `~/.ssh/config` from the command line.
//!
//! docs/cli.md is the contract for every command, message and exit status.
//! The operations live in rustorm-core; this binary parses the command
//! line, loads and saves the file, and prints results.

mod cli;
mod completion;
mod out;
mod run;

use std::ffi::OsString;
use std::io::IsTerminal;

use clap::error::ErrorKind;
use clap::{CommandFactory, Parser};
use rustorm_core::{Error, UserConfig};

use cli::Cli;
use out::Style;

fn main() {
    let code = real_main(std::env::args_os().collect());
    std::process::exit(code);
}

/// Replaces a command word from rustorm's own `[aliases]` table with its
/// canonical name. Built-in names and aliases always win.
fn rewrite_user_alias(mut args: Vec<OsString>, user: &UserConfig) -> Vec<OsString> {
    if user.aliases.is_empty() {
        return args;
    }
    let cmd = Cli::command();
    let known: Vec<String> = cmd
        .get_subcommands()
        .flat_map(|s| {
            std::iter::once(s.get_name().to_string()).chain(s.get_all_aliases().map(str::to_string))
        })
        .collect();
    let mut i = 1;
    while i < args.len() {
        let Some(word) = args[i].to_str() else {
            return args;
        };
        if word == "--" {
            return args;
        }
        if word.starts_with('-') {
            if matches!(word, "-c" | "--config" | "-s" | "--section") {
                i += 1;
            }
            i += 1;
            continue;
        }
        if !known.iter().any(|k| k == word) {
            if let Some(canonical) = user.resolve_alias(word) {
                args[i] = OsString::from(canonical);
            }
        }
        return args;
    }
    args
}

fn color_enabled(cli: &Cli, user: &UserConfig) -> bool {
    if cli.no_color || std::env::var_os("NO_COLOR").is_some() {
        return false;
    }
    match user.defaults.color {
        rustorm_core::ColorMode::Never => false,
        rustorm_core::ColorMode::Always => true,
        rustorm_core::ColorMode::Auto => std::io::stdout().is_terminal(),
    }
}

fn print_error(err: &Error, color: bool) -> i32 {
    let color = color && std::io::stderr().is_terminal();
    out::stderr(&format!(
        "{} {err}\n",
        out::paint("error:", Style::Error, color)
    ));
    err.exit_code()
}

fn real_main(args: Vec<OsString>) -> i32 {
    let user = match UserConfig::load() {
        Ok(u) => u,
        Err(e) => return print_error(&e, false),
    };
    let args = rewrite_user_alias(args, &user);
    let cli = match Cli::try_parse_from(args) {
        Ok(cli) => cli,
        Err(e) => {
            let _ = e.print();
            return match e.kind() {
                ErrorKind::DisplayHelp | ErrorKind::DisplayVersion => 0,
                _ => 2,
            };
        }
    };
    if cli.version {
        out::line(&format!("rustorm {}", rustorm_core::version()));
        return 0;
    }
    let color = color_enabled(&cli, &user);
    let Some(command) = cli.command.as_ref() else {
        let _ = Cli::command().print_help();
        return 2;
    };
    if cli.section.is_some() && !command.takes_section() {
        let err = Error::Usage(format!("--section does not apply to {}.", command.name()));
        return print_error(&err, color);
    }
    match run::run(&cli, command, &user, color) {
        Ok(code) => code,
        Err(e) => print_error(&e, color),
    }
}
