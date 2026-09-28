//! One function per command: load the config, run the core operation,
//! save when it changed, print the docs/cli.md message.

use std::io::{BufRead, IsTerminal};
use std::path::PathBuf;

use rustorm_core::{
    combine as core_combine, pair_up, parse_option, resolve_config_path, write_text, AddSpec,
    Change, CloneSpec, CombineInput, CombineReport, ConfigFile, EditSpec, Env, Error,
    HostSelector, IncludeMatch, IncludeStatus, ListRow, OnConflict, Result, UserConfig,
    Workspace, WriteOptions,
};
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
    file: Option<String>,
    write: WriteOptions,
    env: Env,
}

impl Ctx {
    fn say(&self, msg: &str) {
        if !self.quiet {
            out::line(msg);
        }
    }

    fn file(&self) -> Option<&str> {
        self.file.as_deref()
    }

    /// Loads the root and every file its `Include` lines load.
    fn load(&self) -> Result<Workspace> {
        Workspace::load(&self.path)
    }

    /// Loads the workspace for a read command and prints its load warnings
    /// (unreadable includes, D24) on stderr.
    fn load_read(&self) -> Result<Workspace> {
        let ws = self.load()?;
        for w in ws.load_warnings() {
            self.warn(&w);
        }
        Ok(ws)
    }

    fn warn(&self, msg: &str) {
        out::stderr(&format!(
            "{} {msg}\n",
            paint(
                "warning:",
                Style::Warning,
                self.color && std::io::stderr().is_terminal()
            )
        ));
    }

    /// Saves every file the change touched, then prints its warnings on
    /// stderr and its messages on stdout.
    fn finish<T>(&self, ws: &mut Workspace, change: Change<T>) -> Result<T> {
        ws.save(self.write)?;
        for w in &change.warnings {
            self.warn(w);
        }
        for m in &change.messages {
            self.say(m);
        }
        Ok(change.value)
    }
}

fn plural(n: usize, word: &str) -> String {
    if n == 1 {
        format!("{n} {word}")
    } else {
        format!("{n} {word}s")
    }
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
        file: cli.file.clone(),
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
        Cmd::Includes => includes(&ctx),
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
    let mut ws = ctx.load()?;
    let change = ws.add(&spec, ctx.file(), &ctx.env)?;
    ctx.finish(&mut ws, change)?;
    Ok(0)
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
    let mut ws = ctx.load()?;
    let change = ws.edit(&spec, ctx.file(), &ctx.env)?;
    ctx.finish(&mut ws, change)?;
    Ok(0)
}

fn selector(regex: bool, name: &str) -> HostSelector {
    if regex {
        HostSelector::Regex(name.to_string())
    } else {
        HostSelector::Name(name.to_string())
    }
}

fn set(ctx: &Ctx, regex: bool, append: bool, name: &str, pairs: &[String]) -> Result<i32> {
    let pairs = pair_up(pairs)?;
    let mut ws = ctx.load()?;
    let change = ws.set(&selector(regex, name), &pairs, append, ctx.file())?;
    ctx.finish(&mut ws, change)?;
    Ok(0)
}

fn unset(ctx: &Ctx, regex: bool, name: &str, keys: &[String]) -> Result<i32> {
    let mut ws = ctx.load()?;
    let change = ws.unset(&selector(regex, name), keys, ctx.file())?;
    ctx.finish(&mut ws, change)?;
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
    let mut ws = ctx.load()?;
    let change = ws.clone_host(&spec, ctx.file())?;
    ctx.finish(&mut ws, change)?;
    Ok(0)
}

fn move_host(ctx: &Ctx, name: &str, new_name: Option<&str>) -> Result<i32> {
    if new_name.is_none() && ctx.section.is_none() {
        return Err(Error::MoveNeedsTarget);
    }
    let mut ws = ctx.load()?;
    let change = ws.move_host(name, new_name, ctx.section.as_deref(), ctx.file())?;
    ctx.finish(&mut ws, change)?;
    Ok(0)
}

fn delete(ctx: &Ctx, names: &[String]) -> Result<i32> {
    let mut ws = ctx.load()?;
    let change = ws.delete(names, ctx.file())?;
    ctx.finish(&mut ws, change)?;
    Ok(0)
}

fn stdin_is_terminal() -> bool {
    std::io::stdin().is_terminal() || std::env::var_os(ASSUME_TTY_ENV).is_some_and(|v| v == "1")
}

fn delete_all(ctx: &Ctx, yes: bool) -> Result<i32> {
    let mut ws = ctx.load()?;
    let (count, _) = ws.delete_all_count(ctx.file())?;
    if count > 0 && !yes {
        if !stdin_is_terminal() {
            return Err(Error::RefuseDeleteAll(count));
        }
        out::stderr(&ws.delete_all_prompt(ctx.file())?);
        let mut answer = String::new();
        std::io::stdin()
            .lock()
            .read_line(&mut answer)
            .map_err(|e| Error::Usage(format!("cannot read the answer: {e}")))?;
        if !matches!(answer.trim().to_ascii_lowercase().as_str(), "y" | "yes") {
            return Err(Error::Declined);
        }
    }
    let change = ws.delete_all(ctx.file())?;
    ctx.finish(&mut ws, change)?;
    Ok(0)
}

fn list(ctx: &Ctx, long: bool, names_only: bool) -> Result<i32> {
    let ws = ctx.load_read()?;
    let mut rows = ws.list(&ctx.env);
    if let Some(wanted) = &ctx.section {
        // Every file holding the section keeps its rows (docs/cli.md list).
        let names: Vec<Option<String>> = ws
            .files
            .iter()
            .map(|f| {
                f.config
                    .find_section(wanted)
                    .map(|idx| f.config.sections[idx].name().to_string())
            })
            .collect();
        if names.iter().all(Option::is_none) {
            return Err(Error::SectionNotFound(wanted.clone()));
        }
        rows.retain(|r| {
            names[r.file].is_some() && r.row.section.as_deref() == names[r.file].as_deref()
        });
    }
    if ctx.json {
        out::line(&to_json(&rows));
        return Ok(0);
    }
    if names_only {
        let mut text = String::new();
        for r in &rows {
            text.push_str(&r.row.name);
            text.push('\n');
        }
        out::stdout(&text);
        return Ok(0);
    }
    let width = rows
        .iter()
        .map(|r| r.row.name.chars().count())
        .max()
        .unwrap_or(0);
    let multi = ws.is_multi();
    let mut text = String::new();
    let mut current_file: Option<usize> = None;
    let mut current: Option<Option<&str>> = None;
    for r in &rows {
        if current_file != Some(r.file) {
            current_file = Some(r.file);
            current = None;
            if multi {
                text.push_str(&paint(&ws.display(r.file), Style::Heading, ctx.color));
                text.push('\n');
            }
        }
        let headings = ws.files[r.file].config.has_sections();
        if headings && current != Some(r.row.section.as_deref()) {
            current = Some(r.row.section.as_deref());
            if let Some(s) = &r.row.section {
                text.push_str(&paint(&format!("[{s}]"), Style::Heading, ctx.color));
                text.push('\n');
            }
        }
        text.push_str(&row_line(&r.row, width, ctx.color));
        text.push('\n');
        if long {
            for (k, v) in &r.row.options {
                text.push_str(&format!("    {} {v}\n", paint(k, Style::Key, ctx.color)));
            }
        }
    }
    if long && ctx.section.is_none() {
        let defaults = ws.defaults_options();
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
    let ws = ctx.load_read()?;
    let (shown, warnings) = ws.show(names)?;
    for w in &warnings {
        ctx.warn(w);
    }
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
    file: std::path::PathBuf,
    text: &'a str,
}

fn dump(ctx: &Ctx) -> Result<i32> {
    let mut ws = ctx.load_read()?;
    let (i, text) = ws.dump(ctx.file())?;
    if ctx.json {
        out::line(&to_json(&JsonDump {
            path: ws.files[i].path.display().to_string(),
            file: ws.abs(i),
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
    let ws = ctx.load_read()?;
    let rows = ws.search(pattern, fixed, &ctx.env)?;
    let code = if rows.is_empty() { 1 } else { 0 };
    if ctx.json {
        out::line(&to_json(&rows));
        return Ok(code);
    }
    if rows.is_empty() {
        out::line("no results found.");
        return Ok(code);
    }
    let matcher = rustorm_core::Matcher::new(pattern, fixed)?;
    let width = rows
        .iter()
        .map(|r| r.row.name.chars().count())
        .max()
        .unwrap_or(0);
    let mut text = String::new();
    for r in &rows {
        let plain = format!("{:<width$} -> {}", r.row.name, r.row.target());
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
    let mut ws = ctx.load()?;
    let change = ws.alias(name, aliases, ctx.file())?;
    ctx.finish(&mut ws, change)?;
    Ok(0)
}

fn unalias(ctx: &Ctx, args: &[String]) -> Result<i32> {
    let (host, aliases) = if args.len() > 1 {
        (Some(args[0].as_str()), &args[1..])
    } else {
        (None, args)
    };
    let mut ws = ctx.load()?;
    let change = ws.unalias(host, aliases, ctx.file())?;
    ctx.finish(&mut ws, change)?;
    Ok(0)
}

fn sections(ctx: &Ctx) -> Result<i32> {
    let ws = ctx.load_read()?;
    let list = ws.sections();
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
    let multi = ws.is_multi();
    let mut text = String::new();
    let mut current: Option<usize> = None;
    for s in &list {
        if multi && current != Some(s.index) {
            current = Some(s.index);
            text.push_str(&paint(&ws.display(s.index), Style::Heading, ctx.color));
            text.push('\n');
        }
        text.push_str(&format!("{:<width$}   {}\n", s.name, s.hosts));
    }
    out::stdout(&text);
    Ok(0)
}

fn add_section(ctx: &Ctx, name: &str, before: Option<&str>) -> Result<i32> {
    let mut ws = ctx.load()?;
    let change = ws.add_section(name, before, ctx.file())?;
    if ctx.json {
        ws.save(ctx.write)?;
        for w in &change.warnings {
            ctx.warn(w);
        }
        out::line(&to_json(&change.value));
        return Ok(0);
    }
    ctx.finish(&mut ws, change)?;
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
    let mut ws = ctx.load()?;
    let change = ws.rename_section(old, new, ctx.file())?;
    ctx.finish(&mut ws, change)?;
    Ok(0)
}

fn backup(ctx: &Ctx, dest: Option<&std::path::Path>) -> Result<i32> {
    let file = ConfigFile::load(&ctx.path)?;
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
    // Unreadable includes are reported as problems, not load warnings.
    let ws = ctx.load()?;
    let report = ws.check(&ctx.env);
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

/// One line of the `includes` listing before the count column is aligned.
enum IncludeLine {
    /// `<file>: Include <patterns>` or `matches no files`: printed as is.
    Plain(String),
    /// A matched file: its indented path and what follows the column.
    File(String, FileCount),
}

enum FileCount {
    Hosts(usize),
    Text(String),
}

/// Appends the lines of `matches` (one level of the Include tree) to
/// `lines`, grouping the patterns of one `Include` line under one heading.
fn include_lines(ws: &Workspace, matches: &[IncludeMatch], lines: &mut Vec<IncludeLine>) {
    let mut last: Option<(&std::path::Path, usize)> = None;
    for m in matches {
        let indent = "    ".repeat(m.depth);
        if last != Some((m.from.as_path(), m.line)) {
            last = Some((m.from.as_path(), m.line));
            lines.push(IncludeLine::Plain(format!(
                "{indent}{}: Include {}",
                ws.display_path(&m.from),
                m.directive
            )));
        }
        if m.matches_nothing || m.files.is_empty() {
            lines.push(IncludeLine::Plain(format!("{indent}  matches no files")));
            continue;
        }
        for f in &m.files {
            let path = format!("{indent}  {}", ws.display_path(&f.path));
            let count = match &f.status {
                IncludeStatus::Loaded { hosts } => FileCount::Hosts(*hosts),
                IncludeStatus::AlreadyLoaded => FileCount::Text("already loaded".to_string()),
                IncludeStatus::Unreadable { reason } => {
                    FileCount::Text(format!("cannot read ({reason})"))
                }
            };
            lines.push(IncludeLine::File(path, count));
            for n in &f.nested {
                include_lines(ws, std::slice::from_ref(n), lines);
            }
        }
    }
}

/// One `--json includes` object, fields in docs/cli.md order.
#[derive(Serialize)]
struct JsonInclude<'a> {
    pattern: &'a str,
    from: std::path::PathBuf,
    file: Option<std::path::PathBuf>,
    hosts: usize,
    nested: Vec<JsonInclude<'a>>,
}

fn json_includes(m: &IncludeMatch) -> Vec<JsonInclude<'_>> {
    let from = rustorm_core::absolute_path(&m.from);
    if m.files.is_empty() {
        return vec![JsonInclude {
            pattern: &m.pattern,
            from,
            file: None,
            hosts: 0,
            nested: Vec::new(),
        }];
    }
    m.files
        .iter()
        .map(|f| JsonInclude {
            pattern: &m.pattern,
            from: from.clone(),
            file: Some(rustorm_core::absolute_path(&f.path)),
            hosts: match f.status {
                IncludeStatus::Loaded { hosts } => hosts,
                _ => 0,
            },
            nested: f.nested.iter().flat_map(json_includes).collect(),
        })
        .collect()
}

fn includes(ctx: &Ctx) -> Result<i32> {
    let ws = ctx.load()?;
    if ctx.json {
        let rows: Vec<JsonInclude> = ws.includes.iter().flat_map(json_includes).collect();
        out::line(&to_json(&rows));
        return Ok(0);
    }
    if ws.includes.is_empty() {
        out::line(&format!("no Include lines in {}", ws.display(0)));
        return Ok(0);
    }
    let mut lines = Vec::new();
    include_lines(&ws, &ws.includes, &mut lines);
    let path_width = lines
        .iter()
        .filter_map(|l| match l {
            IncludeLine::File(p, _) => Some(p.chars().count()),
            IncludeLine::Plain(_) => None,
        })
        .max()
        .unwrap_or(0);
    let digits = lines
        .iter()
        .filter_map(|l| match l {
            IncludeLine::File(_, FileCount::Hosts(n)) => Some(n.to_string().len()),
            _ => None,
        })
        .max()
        .unwrap_or(1);
    let mut text = String::new();
    for l in &lines {
        match l {
            IncludeLine::Plain(s) => text.push_str(s),
            IncludeLine::File(p, count) => {
                let pad = path_width - p.chars().count();
                let tail = match count {
                    FileCount::Hosts(n) => {
                        let word = if *n == 1 { "host" } else { "hosts" };
                        format!("{n:>digits$} {word}")
                    }
                    FileCount::Text(t) => t.clone(),
                };
                text.push_str(&format!("{p}{}  {tail}", " ".repeat(pad)));
            }
        }
        text.push('\n');
    }
    let (files, hosts) = ws.include_summary();
    text.push_str(&format!(
        "{}, {}.\n",
        plural(files, "file"),
        plural(hosts, "host")
    ));
    out::stdout(&text);
    Ok(0)
}
