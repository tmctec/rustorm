//! One function per command: load the config, run the core operation,
//! save when it changed, print the docs/cli.md message.

use std::io::{BufRead, IsTerminal};
use std::path::PathBuf;

use rustorm_core::{
    combine as core_combine, is_multi_valued, pair_up, parse_option, resolve_config_path,
    write_text, AddSpec, CloneSpec, CombineInput, CombineReport, ConfigFile, EditSpec, Env, Error,
    HostSelector, ListRow, OnConflict, Result, SectionRename, UserConfig, WriteOptions,
};
use serde::ser::SerializeMap;
use serde::Serialize;

use crate::cli::{Cli, Cmd};
use crate::out::{self, paint, Style};

/// Environment variable that makes `delete-all` treat stdin as a terminal.
/// The examples test uses it to answer the prompt from a pipe.
const ASSUME_TTY_ENV: &str = "RUSTORM_ASSUME_TTY";

struct Ctx {
    path: PathBuf,
    quiet: bool,
    json: bool,
    color: bool,
    section: Option<String>,
    write: WriteOptions,
    env: Env,
}

impl Ctx {
    fn say(&self, msg: &str) {
        if !self.quiet {
            out::line(msg);
        }
    }

    fn load(&self) -> Result<ConfigFile> {
        ConfigFile::load(&self.path)
    }

    fn save(&self, file: &mut ConfigFile) -> Result<()> {
        if file.is_modified() {
            file.save(self.write)?;
        }
        Ok(())
    }
}

fn plural(n: usize, word: &str) -> String {
    if n == 1 {
        format!("{n} {word}")
    } else {
        format!("{n} {word}s")
    }
}

/// A `--json list` / `--json search` row: the fields docs/cli.md shows.
#[derive(Serialize)]
struct JsonRow<'a> {
    name: &'a str,
    section: Option<&'a str>,
    aliases: &'a [String],
    hostname: Option<&'a str>,
    user: &'a str,
    port: u16,
    options: JsonOptions<'a>,
}

struct JsonOptions<'a>(&'a [(String, String)]);

impl Serialize for JsonOptions<'_> {
    fn serialize<S: serde::Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        let mut keys: Vec<&str> = Vec::new();
        for (k, _) in self.0 {
            if !keys.contains(&k.as_str()) {
                keys.push(k);
            }
        }
        let mut map = s.serialize_map(Some(keys.len()))?;
        for key in keys {
            let values: Vec<&str> = self
                .0
                .iter()
                .filter(|(k, _)| k == key)
                .map(|(_, v)| v.as_str())
                .collect();
            if is_multi_valued(key) {
                map.serialize_entry(key, &values)?;
            } else {
                map.serialize_entry(key, values[0])?;
            }
        }
        map.end()
    }
}

fn json_rows(rows: &[ListRow]) -> String {
    let rows: Vec<JsonRow> = rows
        .iter()
        .map(|r| JsonRow {
            name: &r.name,
            section: r.section.as_deref(),
            aliases: &r.aliases,
            hostname: r.hostname.as_deref(),
            user: &r.user,
            port: r.port,
            options: JsonOptions(&r.options),
        })
        .collect();
    to_json(&rows)
}

fn to_json<T: Serialize + ?Sized>(value: &T) -> String {
    serde_json::to_string(value).expect("serializing plain data cannot fail")
}

/// `name -> user@host:port` with the name padded to `width`.
fn row_line(row: &ListRow, width: usize, color: bool) -> String {
    let padded = format!("{:<width$}", row.name);
    let name_end = row.name.len();
    format!(
        "{}{} -> {}",
        paint(&padded[..name_end], Style::Name, color),
        &padded[name_end..],
        row.target()
    )
}

/// Runs `command`. Returns the exit status on success paths (0, or 1 for
/// `search` without matches and `check` with problems).
pub fn run(cli: &Cli, command: &Cmd, user: &UserConfig, color: bool) -> Result<i32> {
    if let Cmd::Version = command {
        out::line(&format!("rustorm {}", rustorm_core::version()));
        return Ok(0);
    }
    if let Cmd::Completion { shell } = command {
        out::stdout(&crate::completion::script(*shell));
        return Ok(0);
    }
    let path = resolve_config_path(cli.config.as_deref()).ok_or_else(|| {
        Error::Usage("cannot find the home directory; give --config.".to_string())
    })?;
    let ctx = Ctx {
        path,
        quiet: cli.quiet,
        json: cli.json,
        color,
        section: cli.section.clone(),
        write: WriteOptions {
            no_backup: cli.no_backup || !user.defaults.backup,
        },
        env: Env::from_process(),
    };
    match command {
        Cmd::Add {
            name,
            uri,
            identity,
            option,
        } => add(&ctx, name, uri, identity, option),
        Cmd::Edit {
            name,
            uri,
            identity,
            option,
        } => edit(&ctx, name, uri, identity, option),
        Cmd::Set {
            regex,
            append,
            name,
            pairs,
        } => set(&ctx, *regex, *append, name, pairs),
        Cmd::Unset { regex, name, keys } => unset(&ctx, *regex, name, keys),
        Cmd::Clone {
            keep_hostname,
            name,
            new_name,
            pairs,
        } => clone(&ctx, *keep_hostname, name, new_name, pairs),
        Cmd::Move { name, new_name } => move_host(&ctx, name, new_name.as_deref()),
        Cmd::Delete { names } => delete(&ctx, names),
        Cmd::DeleteAll { yes } => delete_all(&ctx, *yes),
        Cmd::List { long, names } => list(&ctx, *long, *names),
        Cmd::Show { names } => show(&ctx, names),
        Cmd::Dump => dump(&ctx),
        Cmd::Search {
            fixed_strings,
            pattern,
        } => search(&ctx, pattern, *fixed_strings),
        Cmd::Alias { name, aliases } => alias(&ctx, name, aliases),
        Cmd::Unalias { args } => unalias(&ctx, args),
        Cmd::Sections => sections(&ctx),
        Cmd::AddSection { name, before } => add_section(&ctx, name, before.as_deref()),
        Cmd::Combine {
            files,
            output,
            on_conflict,
            stdout,
        } => combine(&ctx, files, output.as_deref(), on_conflict, *stdout),
        Cmd::RenameSection { old, new } => rename_section(&ctx, old, new),
        Cmd::Backup { file } => backup(&ctx, file.as_deref()),
        Cmd::Check => check(&ctx),
        Cmd::Completion { .. } | Cmd::Version => unreachable!("handled above"),
    }
}

fn parse_options(options: &[String]) -> Result<Vec<(String, String)>> {
    options.iter().map(|o| parse_option(o)).collect()
}

fn add(
    ctx: &Ctx,
    name: &str,
    uri: &str,
    identity: &Option<String>,
    options: &[String],
) -> Result<i32> {
    let spec = AddSpec {
        name: name.to_string(),
        uri: uri.to_string(),
        identity: identity.clone(),
        options: parse_options(options)?,
        section: ctx.section.clone(),
    };
    let mut file = ctx.load()?;
    let placed = file.config.add(&spec, &ctx.env)?;
    ctx.save(&mut file)?;
    ctx.say(&added_message(
        &placed.name,
        ctx.section.is_some(),
        placed.section.as_deref(),
    ));
    Ok(0)
}

fn added_message(name: &str, explicit: bool, section: Option<&str>) -> String {
    match section {
        Some(s) if explicit => format!("{name} added to section {s}. Connect with: ssh {name}"),
        _ => format!("{name} added. Connect with: ssh {name}"),
    }
}

fn edit(
    ctx: &Ctx,
    name: &str,
    uri: &str,
    identity: &Option<String>,
    options: &[String],
) -> Result<i32> {
    let spec = EditSpec {
        name: name.to_string(),
        uri: uri.to_string(),
        identity: identity.clone(),
        options: parse_options(options)?,
        section: ctx.section.clone(),
    };
    let mut file = ctx.load()?;
    let placed = file.config.edit(&spec, &ctx.env)?;
    ctx.save(&mut file)?;
    match (&ctx.section, placed.section) {
        (Some(_), Some(s)) => ctx.say(&format!(
            "{} updated and moved to section {s}.",
            placed.name
        )),
        _ => ctx.say(&format!("{} updated.", placed.name)),
    }
    Ok(0)
}

fn selector(regex: bool, name: &str) -> HostSelector {
    if regex {
        HostSelector::Regex(name.to_string())
    } else {
        HostSelector::Name(name.to_string())
    }
}

fn updated_message(regex: bool, names: &[String]) -> String {
    if regex {
        format!(
            "{} updated: {}",
            plural(names.len(), "host"),
            names.join(", ")
        )
    } else {
        format!("{} updated.", names.join(", "))
    }
}

fn set(ctx: &Ctx, regex: bool, append: bool, name: &str, pairs: &[String]) -> Result<i32> {
    let pairs = pair_up(pairs)?;
    let mut file = ctx.load()?;
    let names = file.config.set(&selector(regex, name), &pairs, append)?;
    ctx.save(&mut file)?;
    ctx.say(&updated_message(regex, &names));
    Ok(0)
}

fn unset(ctx: &Ctx, regex: bool, name: &str, keys: &[String]) -> Result<i32> {
    let mut file = ctx.load()?;
    let names = file.config.unset(&selector(regex, name), keys)?;
    ctx.save(&mut file)?;
    ctx.say(&updated_message(regex, &names));
    Ok(0)
}

fn clone(
    ctx: &Ctx,
    keep_hostname: bool,
    name: &str,
    new_name: &str,
    pairs: &[String],
) -> Result<i32> {
    let spec = CloneSpec {
        source: name.to_string(),
        new_name: new_name.to_string(),
        keep_hostname,
        overrides: pair_up(pairs)?,
        section: ctx.section.clone(),
    };
    let mut file = ctx.load()?;
    let placed = file.config.clone_host(&spec)?;
    ctx.save(&mut file)?;
    ctx.say(&added_message(
        &placed.name,
        ctx.section.is_some(),
        placed.section.as_deref(),
    ));
    Ok(0)
}

fn move_host(ctx: &Ctx, name: &str, new_name: Option<&str>) -> Result<i32> {
    if new_name.is_none() && ctx.section.is_none() {
        return Err(Error::MoveNeedsTarget);
    }
    let mut file = ctx.load()?;
    let moved = file
        .config
        .move_host(name, new_name, ctx.section.as_deref())?;
    ctx.save(&mut file)?;
    let (old, new) = (&moved.old_name, &moved.new_name);
    let msg = match (new_name, moved.section) {
        (Some(_), Some(s)) => {
            format!("{old} renamed to {new} and moved to section {s}. Connect with: ssh {new}")
        }
        (Some(_), None) => format!("{old} renamed to {new}. Connect with: ssh {new}"),
        (None, Some(s)) => format!("{old} moved to section {s}."),
        (None, None) => format!("{old} unchanged."),
    };
    ctx.say(&msg);
    Ok(0)
}

fn delete(ctx: &Ctx, names: &[String]) -> Result<i32> {
    let mut file = ctx.load()?;
    let deleted = file.config.delete(names)?;
    ctx.save(&mut file)?;
    for n in deleted {
        ctx.say(&format!("{n} deleted."));
    }
    Ok(0)
}

fn stdin_is_terminal() -> bool {
    std::io::stdin().is_terminal() || std::env::var_os(ASSUME_TTY_ENV).is_some_and(|v| v == "1")
}

fn delete_all(ctx: &Ctx, yes: bool) -> Result<i32> {
    let mut file = ctx.load()?;
    let count = file.config.host_count();
    if count > 0 && !yes {
        if !stdin_is_terminal() {
            return Err(Error::RefuseDeleteAll(count));
        }
        out::stderr(&format!(
            "Delete {} from {}? [y/N] ",
            plural(count, "host"),
            ctx.path.display()
        ));
        let mut answer = String::new();
        std::io::stdin()
            .lock()
            .read_line(&mut answer)
            .map_err(|e| Error::Usage(format!("cannot read the answer: {e}")))?;
        if !matches!(answer.trim().to_ascii_lowercase().as_str(), "y" | "yes") {
            return Err(Error::Declined);
        }
    }
    let removed = file.config.delete_all();
    ctx.save(&mut file)?;
    ctx.say(&format!("{} deleted.", plural(removed, "host")));
    Ok(0)
}

fn list(ctx: &Ctx, long: bool, names_only: bool) -> Result<i32> {
    let file = ctx.load()?;
    let config = &file.config;
    let mut rows = config.list(&ctx.env);
    if let Some(wanted) = &ctx.section {
        let idx = config
            .find_section(wanted)
            .ok_or_else(|| Error::SectionNotFound(wanted.clone()))?;
        let name = config.sections[idx].name().to_string();
        rows.retain(|r| r.section.as_deref() == Some(name.as_str()));
    }
    if ctx.json {
        out::line(&json_rows(&rows));
        return Ok(0);
    }
    if names_only {
        let mut text = String::new();
        for r in &rows {
            text.push_str(&r.name);
            text.push('\n');
        }
        out::stdout(&text);
        return Ok(0);
    }
    let width = rows
        .iter()
        .map(|r| r.name.chars().count())
        .max()
        .unwrap_or(0);
    let headings = config.has_sections();
    let mut text = String::new();
    let mut current: Option<Option<&str>> = None;
    for r in &rows {
        if headings && current != Some(r.section.as_deref()) {
            current = Some(r.section.as_deref());
            if let Some(s) = &r.section {
                text.push_str(&paint(&format!("[{s}]"), Style::Heading, ctx.color));
                text.push('\n');
            }
        }
        text.push_str(&row_line(r, width, ctx.color));
        text.push('\n');
        if long {
            for (k, v) in &r.options {
                text.push_str(&format!("    {} {v}\n", paint(k, Style::Key, ctx.color)));
            }
        }
    }
    if long && ctx.section.is_none() {
        let defaults = config.defaults_options();
        if !defaults.is_empty() {
            text.push('\n');
            text.push_str(&paint("(*) defaults", Style::Heading, ctx.color));
            text.push('\n');
            for (k, v) in defaults {
                text.push_str(&format!("    {} {v}\n", paint(&k, Style::Key, ctx.color)));
            }
        }
    }
    out::stdout(&text);
    Ok(0)
}

fn show(ctx: &Ctx, names: &[String]) -> Result<i32> {
    let file = ctx.load()?;
    let shown = file.config.show(names)?;
    if ctx.json {
        out::line(&to_json(&shown));
        return Ok(0);
    }
    let mut text = String::new();
    for h in shown {
        text.push_str(&h.text);
        if !text.ends_with('\n') {
            text.push('\n');
        }
    }
    if ctx.color {
        text = out::color_dump(&text);
    }
    out::stdout(&text);
    Ok(0)
}

#[derive(Serialize)]
struct JsonDump<'a> {
    path: String,
    text: &'a str,
}

fn dump(ctx: &Ctx) -> Result<i32> {
    let file = ctx.load()?;
    let text = file.config.dump();
    if ctx.json {
        out::line(&to_json(&JsonDump {
            path: ctx.path.display().to_string(),
            text: &text,
        }));
    } else if ctx.color {
        out::stdout(&out::color_dump(&text));
    } else {
        out::stdout(&text);
    }
    Ok(0)
}

fn search(ctx: &Ctx, pattern: &str, fixed: bool) -> Result<i32> {
    let file = ctx.load()?;
    let rows = file.config.search(pattern, fixed, &ctx.env)?;
    let code = if rows.is_empty() { 1 } else { 0 };
    if ctx.json {
        out::line(&json_rows(&rows));
        return Ok(code);
    }
    if rows.is_empty() {
        out::line("no results found.");
        return Ok(code);
    }
    let matcher = rustorm_core::Matcher::new(pattern, fixed)?;
    let width = rows
        .iter()
        .map(|r| r.name.chars().count())
        .max()
        .unwrap_or(0);
    let mut text = String::new();
    for r in &rows {
        let plain = format!("{:<width$} -> {}", r.name, r.target());
        text.push_str(&out::highlight(
            &plain,
            &matcher.find_ranges(&plain),
            ctx.color,
        ));
        text.push('\n');
    }
    out::stdout(&text);
    Ok(code)
}

fn alias(ctx: &Ctx, name: &str, aliases: &[String]) -> Result<i32> {
    let mut file = ctx.load()?;
    let names = file.config.alias(name, aliases)?;
    ctx.save(&mut file)?;
    ctx.say(&format!("{} now answers to: {}", names[0], names.join(" ")));
    Ok(0)
}

fn unalias(ctx: &Ctx, args: &[String]) -> Result<i32> {
    let (host, aliases) = if args.len() > 1 {
        (Some(args[0].as_str()), &args[1..])
    } else {
        (None, args)
    };
    let mut file = ctx.load()?;
    let result = file.config.unalias(host, aliases)?;
    ctx.save(&mut file)?;
    ctx.say(&format!(
        "{} now answers to: {}",
        result.host,
        result.names.join(" ")
    ));
    Ok(0)
}

fn sections(ctx: &Ctx) -> Result<i32> {
    let file = ctx.load()?;
    let list = file.config.sections();
    if ctx.json {
        out::line(&to_json(&list));
        return Ok(0);
    }
    if list.is_empty() {
        out::line("no sections");
        return Ok(0);
    }
    let width = list
        .iter()
        .map(|s| s.name.chars().count())
        .max()
        .unwrap_or(0);
    let mut text = String::new();
    for s in list {
        text.push_str(&format!("{:<width$}   {}\n", s.name, s.hosts));
    }
    out::stdout(&text);
    Ok(0)
}

fn add_section(ctx: &Ctx, name: &str, before: Option<&str>) -> Result<i32> {
    let mut file = ctx.load()?;
    let added = file.config.add_section(name, before)?;
    ctx.save(&mut file)?;
    if ctx.json {
        out::line(&to_json(&added));
        return Ok(0);
    }
    match (&added.before, &added.catch_all) {
        (Some(b), _) => ctx.say(&format!("section {name} added before {b}.")),
        (None, Some((catch_all, n))) => ctx.say(&format!(
            "section {name} added; {catch_all} created with {}.",
            plural(*n, "host")
        )),
        (None, None) => ctx.say(&format!("section {name} added.")),
    }
    Ok(0)
}

/// The `--json combine` document: the report plus the output path.
#[derive(Serialize)]
struct CombineJson<'a> {
    #[serde(flatten)]
    report: &'a CombineReport,
    output: Option<&'a std::path::Path>,
    #[serde(skip_serializing_if = "Option::is_none")]
    text: Option<&'a str>,
}

fn combine(
    ctx: &Ctx,
    files: &[PathBuf],
    output: Option<&std::path::Path>,
    on_conflict: &str,
    to_stdout: bool,
) -> Result<i32> {
    if files.len() < 2 {
        return Err(Error::Usage(
            "combine needs at least two files.".to_string(),
        ));
    }
    let policy: OnConflict = on_conflict.parse()?;
    let mut inputs = Vec::with_capacity(files.len());
    for path in files {
        let file = ConfigFile::load(path)?;
        if !file.existed {
            return Err(Error::Read {
                path: path.clone(),
                source: std::io::Error::from(std::io::ErrorKind::NotFound),
            });
        }
        inputs.push(CombineInput {
            path: path.clone(),
            config: file.config,
        });
    }
    let (result, report) = core_combine(inputs, policy, ctx.env.home.as_deref())?;
    let text = result.render();
    let destination = if to_stdout {
        None
    } else {
        Some(output.unwrap_or(&files[0]))
    };
    if let Some(dest) = destination {
        write_text(dest, &text, ctx.write)?;
    }
    if ctx.json {
        out::line(&to_json(&CombineJson {
            report: &report,
            output: destination,
            text: to_stdout.then_some(text.as_str()),
        }));
        return Ok(0);
    }
    if to_stdout {
        out::stdout(&text);
    }
    for a in &report.added {
        ctx.say(&format!(
            "Host *: {} {} added from {}.",
            a.key,
            a.value,
            a.from.display()
        ));
    }
    for k in &report.skipped {
        ctx.say(&format!(
            "Host *: {} {} from {} skipped, keeping {}.",
            k.key,
            k.value,
            k.from.display(),
            k.kept
        ));
    }
    for w in &report.includes {
        out::stderr(&format!(
            "{} Include {} in {} still loads {}.\n",
            paint(
                "warning:",
                Style::Warning,
                ctx.color && std::io::stderr().is_terminal()
            ),
            w.pattern,
            w.file.display(),
            w.loads.display()
        ));
    }
    let summary = report.summary(destination);
    if to_stdout {
        if !ctx.quiet {
            out::stderr(&format!("{summary}\n"));
        }
    } else {
        ctx.say(&summary);
    }
    Ok(0)
}

fn rename_section(ctx: &Ctx, old: &str, new: &str) -> Result<i32> {
    let mut file = ctx.load()?;
    let outcome = file.config.rename_section(old, new)?;
    ctx.save(&mut file)?;
    match outcome {
        SectionRename::Renamed { from, to } => ctx.say(&format!("section {from} renamed to {to}.")),
        SectionRename::Merged { from, into } => {
            ctx.say(&format!("section {from} merged into {into}."))
        }
    }
    Ok(0)
}

fn backup(ctx: &Ctx, dest: Option<&std::path::Path>) -> Result<i32> {
    let file = ctx.load()?;
    if !file.existed {
        return Err(Error::Read {
            path: ctx.path.clone(),
            source: std::io::Error::from(std::io::ErrorKind::NotFound),
        });
    }
    let written = file.backup(dest)?;
    ctx.say(&format!(
        "{} copied to {}",
        ctx.path.display(),
        written.display()
    ));
    Ok(0)
}

fn check(ctx: &Ctx) -> Result<i32> {
    let file = ctx.load()?;
    let report = file.config.check(&ctx.env);
    let code = if report.is_clean() { 0 } else { 1 };
    if ctx.json {
        out::line(&to_json(&report));
        return Ok(code);
    }
    let mut text = String::new();
    for p in &report.problems {
        text.push_str(&format!("{p}\n"));
    }
    text.push_str(&report.summary());
    text.push('\n');
    out::stdout(&text);
    Ok(code)
}
