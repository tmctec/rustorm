//! One function per command: load the config, run the core operation,
//! save when it changed, print the docs/cli.md message.

use std::io::{BufRead, IsTerminal};
use std::path::PathBuf;

use rustorm_core::{
    combine as core_combine, missing, pair_up, parse_filter, parse_option, project, render,
    resolve_config_path, selected, write_text, AddSpec, Change, CloneSpec, CombineInput,
    CombineReport, ConfigFile, Decision, EditSpec, Env, Error, Format, HostSelector, IncludeMatch,
    IncludeStatus, KeyPick, ListRow, OnConflict, Pair, PairKind, Pick, Projected, ReconcileReport,
    Result, UserConfig, Where, Workspace, WorkspaceRow, WriteOptions,
};
use serde::Serialize;

use crate::cli::{Cli, Cmd, FormatName, ReadArgs};
use crate::out::{self, paint, Style};

/// The keys `csv` and `yaml` print without `--filter` (docs/cli.md, Reading
/// output).
const DEFAULT_KEYS: &[&str] = &["Host", "hostname", "user", "port", "section", "file"];

/// The parsed read options of `list`, `show` and `search`.
struct ReadOpts {
    wheres: Vec<Where>,
    filter: Option<Vec<String>>,
    format: Format,
    just_value: bool,
    allow_missing: bool,
}

impl ReadOpts {
    fn parse(ctx: &Ctx, args: &ReadArgs) -> Result<ReadOpts> {
        let named = args.format.map(|f| match f {
            FormatName::Txt => Format::Txt,
            FormatName::Json => Format::Json,
            FormatName::Csv => Format::Csv,
            FormatName::Yaml | FormatName::Yml => Format::Yaml,
        });
        let format = match (ctx.json, named) {
            (true, Some(f)) if f != Format::Json => {
                return Err(Error::Usage(format!(
                    "--json and --format {} conflict.",
                    f.name()
                )));
            }
            (true, _) => Format::Json,
            (false, Some(f)) => f,
            (false, None) => Format::Txt,
        };
        let wheres = args
            .where_
            .iter()
            .map(|w| Where::parse(w))
            .collect::<Result<Vec<_>>>()?;
        let filter = match &args.filter {
            Some(f) => Some(parse_filter(f)?),
            None => None,
        };
        Ok(ReadOpts {
            wheres,
            filter,
            format,
            just_value: args.just_value,
            allow_missing: args.allow_missing,
        })
    }

    /// The keys to project: `--filter`, else the default set for `csv`,
    /// `yaml` and `--just-value`; `None` keeps the command's usual output.
    fn keys(&self) -> Option<Vec<String>> {
        match (&self.filter, self.format, self.just_value) {
            (Some(f), _, _) => Some(f.clone()),
            (None, Format::Csv | Format::Yaml, _) | (None, _, true) => {
                Some(DEFAULT_KEYS.iter().map(|k| k.to_string()).collect())
            }
            _ => None,
        }
    }
}

/// Keeps the rows `--section` and `--where` select.
fn keep_rows(
    ctx: &Ctx,
    ws: &Workspace,
    mut rows: Vec<WorkspaceRow>,
    opts: &ReadOpts,
) -> Result<Vec<WorkspaceRow>> {
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
    if !opts.wheres.is_empty() {
        rows.retain(|r| {
            ws.view_of(r.file, &r.row.name)
                .is_some_and(|v| selected(&opts.wheres, &v))
        });
    }
    Ok(rows)
}

/// Prints the projection of `rows` over `keys`. Returns 4 when a key is
/// missing on a printed host and `--allow-missing` is off (D27), else 0.
fn print_projection(ctx: &Ctx, ws: &Workspace, rows: &[WorkspaceRow], keys: &[String], opts: &ReadOpts) -> i32 {
    let projected: Vec<Projected> = rows
        .iter()
        .filter_map(|r| ws.view_of(r.file, &r.row.name).map(|v| project(&v, keys)))
        .collect();
    let miss = missing(&projected);
    let code = if miss.is_empty() || opts.allow_missing {
        0
    } else {
        for (host, key) in &miss {
            ctx.warn(&format!("{host} has no '{key}'"));
        }
        4
    };
    out::stdout(&render(&projected, opts.format, opts.just_value));
    code
}

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
            tag,
            untag,
            name,
            pairs,
        } => set(&ctx, *regex, *append, tag, untag, name, pairs),
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
        Cmd::List { long, names, read } => list(&ctx, *long, *names, read),
        Cmd::Show { names, read } => show(&ctx, names, read),
        Cmd::Dump => dump(&ctx),
        Cmd::Search {
            fixed_strings,
            pattern,
            read,
        } => search(&ctx, pattern, *fixed_strings, read),
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
        Cmd::Reconcile {
            files,
            list,
            take_copy,
            keep_live,
            add,
            all_live,
            all_copy,
            drop_identical,
            retire,
        } => reconcile(
            &ctx,
            files,
            &ReconcileFlags {
                list: *list,
                take_copy,
                keep_live,
                add,
                all_live: *all_live,
                all_copy: *all_copy,
                drop_identical: *drop_identical,
                retire: *retire,
            },
        ),
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

fn set(
    ctx: &Ctx,
    regex: bool,
    append: bool,
    tag: &[String],
    untag: &[String],
    name: &str,
    pairs: &[String],
) -> Result<i32> {
    if pairs.is_empty() && tag.is_empty() && untag.is_empty() {
        return Err(Error::Usage(
            "set needs KEY VALUE pairs, --tag or --untag.".to_string(),
        ));
    }
    let pairs = pair_up(pairs)?;
    let mut ws = ctx.load()?;
    let change = ws.set_with_tags(&selector(regex, name), &pairs, append, tag, untag, ctx.file())?;
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

fn list(ctx: &Ctx, long: bool, names_only: bool, read: &ReadArgs) -> Result<i32> {
    let opts = ReadOpts::parse(ctx, read)?;
    let ws = ctx.load_read()?;
    let rows = keep_rows(ctx, &ws, ws.list(&ctx.env), &opts)?;
    if let Some(keys) = opts.keys() {
        return Ok(print_projection(ctx, &ws, &rows, &keys, &opts));
    }
    if opts.format == Format::Json {
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

fn show(ctx: &Ctx, names: &[String], read: &ReadArgs) -> Result<i32> {
    let opts = ReadOpts::parse(ctx, read)?;
    if names.is_empty() && opts.wheres.is_empty() && ctx.section.is_none() {
        return Err(Error::Usage(
            "show needs a host name, --where or --section.".to_string(),
        ));
    }
    let ws = ctx.load_read()?;
    // The named hosts (first definition in load order, with the duplicate
    // warnings), or every host when only --where/--section select.
    let rows: Vec<WorkspaceRow> = if names.is_empty() {
        ws.list(&ctx.env)
    } else {
        let (shown, warnings) = ws.show(names)?;
        for w in &warnings {
            ctx.warn(w);
        }
        let all = ws.list(&ctx.env);
        shown
            .iter()
            .filter_map(|h| {
                all.iter()
                    .find(|r| r.file == h.index && r.row.name == h.name)
                    .cloned()
            })
            .collect()
    };
    let rows = keep_rows(ctx, &ws, rows, &opts)?;
    let code = if rows.is_empty() { 1 } else { 0 };
    if let Some(keys) = opts.keys() {
        let missing_code = print_projection(ctx, &ws, &rows, &keys, &opts);
        return Ok(if code != 0 { code } else { missing_code });
    }
    let shown: Vec<_> = rows
        .iter()
        .filter_map(|r| ws.shown_at(r.file, &r.row.name))
        .collect();
    if opts.format == Format::Json {
        out::line(&to_json(&shown));
        return Ok(code);
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
    Ok(code)
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

fn search(ctx: &Ctx, pattern: &str, fixed: bool, read: &ReadArgs) -> Result<i32> {
    let opts = ReadOpts::parse(ctx, read)?;
    let ws = ctx.load_read()?;
    let rows = keep_rows(ctx, &ws, ws.search(pattern, fixed, &ctx.env)?, &opts)?;
    let code = if rows.is_empty() { 1 } else { 0 };
    if let Some(keys) = opts.keys() {
        let missing_code = print_projection(ctx, &ws, &rows, &keys, &opts);
        return Ok(if code != 0 { code } else { missing_code });
    }
    if opts.format == Format::Json {
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
        // A match only in the metadata prints the matching line under the
        // host, so the output names the field (docs/cli.md search).
        let elsewhere = matcher.is_match(&r.row.line())
            || r.row.aliases.iter().any(|a| matcher.is_match(a))
            || r
                .row
                .options
                .iter()
                .any(|(k, v)| matcher.is_match(k) || matcher.is_match(v));
        if !elsewhere {
            for line in r.row.meta.lines() {
                if matcher.is_match(&line) {
                    text.push_str("    ");
                    text.push_str(&out::highlight(
                        &line,
                        &matcher.find_ranges(&line),
                        ctx.color,
                    ));
                    text.push('\n');
                }
            }
        }
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

/// The decision flags of `reconcile`.
struct ReconcileFlags<'a> {
    list: bool,
    take_copy: &'a [String],
    keep_live: &'a [String],
    add: &'a [String],
    all_live: bool,
    all_copy: bool,
    drop_identical: bool,
    retire: bool,
}

impl ReconcileFlags<'_> {
    /// True when a flag decides something (`--retire` included).
    fn decides(&self) -> bool {
        self.has_decision() || self.retire
    }

    /// True when a host flag or a bulk flag is given.
    fn has_decision(&self) -> bool {
        !self.take_copy.is_empty()
            || !self.keep_live.is_empty()
            || !self.add.is_empty()
            || self.all_live
            || self.all_copy
            || self.drop_identical
    }
}

/// The per-host and bulk flags as one decision per report item, in item
/// order. A host decided twice is a usage error.
fn flag_decisions(
    report: &ReconcileReport,
    flags: &ReconcileFlags,
) -> Result<Vec<(usize, Decision)>> {
    let mut decisions: Vec<(usize, Decision)> = Vec::new();
    let named = [
        (flags.take_copy, Decision::TakeCopy),
        (flags.keep_live, Decision::KeepLive),
        (flags.add, Decision::Add),
    ];
    for (hosts, decision) in named {
        for host in hosts {
            let i = report.lookup(host)?;
            if decisions.iter().any(|(d, _)| *d == i) {
                return Err(Error::Usage(format!(
                    "{} is decided twice.",
                    report.items[i].name
                )));
            }
            decisions.push((i, decision.clone()));
        }
    }
    let bulk = [
        (flags.all_live, PairKind::Conflict, Decision::KeepLive),
        (flags.all_copy, PairKind::Conflict, Decision::TakeCopy),
        (
            flags.drop_identical,
            PairKind::Identical,
            Decision::DropIdentical,
        ),
    ];
    for (on, kind, decision) in bulk {
        if on {
            let more = report.bulk(kind, &decision, &decisions);
            decisions.extend(more);
        }
    }
    decisions.sort_by_key(|(i, _)| *i);
    Ok(decisions)
}

/// One answer read from stdin, lowercased; `None` at end of input.
fn read_answer() -> Result<Option<String>> {
    let mut answer = String::new();
    let n = std::io::stdin()
        .lock()
        .read_line(&mut answer)
        .map_err(|e| Error::Usage(format!("cannot read the answer: {e}")))?;
    Ok((n > 0).then(|| answer.trim().to_ascii_lowercase()))
}

/// Asks `prompt` until the answer's first letter is one of `choices`.
/// End of input answers `q`.
fn ask(prompt: &str, choices: &[char]) -> Result<char> {
    loop {
        out::stderr(&format!("{prompt} "));
        let Some(answer) = read_answer()? else {
            out::stderr("\n");
            return Ok('q');
        };
        if let Some(c) = answer.chars().next().filter(|c| choices.contains(c)) {
            return Ok(c);
        }
    }
}

/// The live block on the left, the copy on the right, each under its
/// file's label.
fn side_by_side(pair: &Pair) -> String {
    let live = pair.live.as_ref().expect("a conflict has a live side");
    let left: Vec<String> = std::iter::once(format!("{} (live)", live.label))
        .chain(live.text.lines().map(str::to_string))
        .collect();
    let right: Vec<String> = std::iter::once(format!("{} (copy)", pair.copy.label))
        .chain(pair.copy.text.lines().map(str::to_string))
        .collect();
    let width = left.iter().map(|l| l.chars().count()).max().unwrap_or(0);
    let mut text = String::new();
    for row in 0..left.len().max(right.len()) {
        let l = left.get(row).map_or("", String::as_str);
        let r = right.get(row).map_or("", String::as_str);
        let line = format!("{l:<width$} | {r}");
        text.push_str(line.trim_end());
        text.push('\n');
    }
    text
}

fn values(v: &[String]) -> String {
    if v.is_empty() {
        "(unset)".to_string()
    } else {
        v.join(", ")
    }
}

/// Asks for every conflict, then every orphan, of `report` on the
/// terminal. `q` stops and keeps the decisions made so far.
fn ask_decisions(report: &ReconcileReport) -> Result<Vec<(usize, Decision)>> {
    out::stderr(&format!("{}\n", report.summary()));
    let mut decisions = Vec::new();
    for (i, pair) in report.items.iter().enumerate() {
        let decision = match pair.kind {
            PairKind::Identical => continue,
            PairKind::Conflict => {
                let mut text = format!("\n{}\n", side_by_side(pair));
                if let Some(note) = &pair.note {
                    text.push_str(&format!("note: {note}\n"));
                }
                out::stderr(&text);
                let prompt = format!("{}: [l]ive / [c]opy / [k]eys / [s]kip / [q]uit?", pair.name);
                match ask(&prompt, &['l', 'c', 'k', 's', 'q'])? {
                    'l' => Decision::KeepLive,
                    'c' => Decision::TakeCopy,
                    'k' => {
                        let mut picks = Vec::new();
                        for k in &pair.keys {
                            let prompt = format!(
                                "  {}: {} (live) / {} (copy)  [l]ive / [c]opy?",
                                k.key,
                                values(&k.live),
                                values(&k.copy)
                            );
                            let pick = match ask(&prompt, &['l', 'c', 'q'])? {
                                'l' => Pick::Live,
                                'c' => Pick::Copy,
                                _ => return Ok(decisions),
                            };
                            picks.push(KeyPick {
                                key: k.key.clone(),
                                pick,
                            });
                        }
                        Decision::Keys(picks)
                    }
                    's' => continue,
                    _ => break,
                }
            }
            PairKind::Orphan => {
                out::stderr(&format!(
                    "\norphan: {} in {}\n{}",
                    pair.name, pair.copy.label, pair.copy.text
                ));
                if !pair.copy.text.ends_with('\n') {
                    out::stderr("\n");
                }
                let prompt = format!("{}: [a]dd / [s]kip / [q]uit?", pair.name);
                match ask(&prompt, &['a', 's', 'q'])? {
                    'a' => Decision::Add,
                    's' => continue,
                    _ => break,
                }
            }
        };
        decisions.push((i, decision));
    }
    Ok(decisions)
}

/// `reconcile`: report, decide (from flags or on a terminal), save, then
/// retire each named file.
fn reconcile(ctx: &Ctx, files: &[String], flags: &ReconcileFlags) -> Result<i32> {
    if flags.retire && files.is_empty() {
        return Err(Error::Usage("--retire needs FILE.".to_string()));
    }
    if ctx.json && flags.decides() {
        return Err(Error::Usage(
            "--json prints the report only; drop it to decide.".to_string(),
        ));
    }
    if flags.list && flags.decides() {
        return Err(Error::Usage(
            "--list prints the report only; drop it to decide.".to_string(),
        ));
    }
    if flags.all_live && flags.all_copy {
        return Err(Error::Usage(
            "--all-live and --all-copy conflict.".to_string(),
        ));
    }
    let mut ws = ctx.load_read()?;
    let scope = if files.is_empty() {
        None
    } else {
        Some(ws.reconcile_scope(files)?)
    };
    let add_to = match ctx.file() {
        Some(f) => Some(ws.resolve_file(f)?),
        None => None,
    };
    let report = ws.reconcile_report(scope.as_deref());
    let listed_code = if report.conflicts > 0 { 1 } else { 0 };
    if ctx.json {
        out::line(&to_json(&report));
        return Ok(listed_code);
    }
    let interactive = !flags.list && !flags.has_decision() && stdin_is_terminal();
    if flags.list || (!flags.decides() && !interactive) {
        out::stdout(&report.list_text());
        return Ok(listed_code);
    }
    let decisions = if interactive {
        ask_decisions(&report)?
    } else {
        flag_decisions(&report, flags)?
    };
    let mut decided = Vec::new();
    if interactive || !decisions.is_empty() {
        let change = ws.apply_decisions(&report, &decisions, add_to)?;
        decided = ctx.finish(&mut ws, change)?.decided;
    }
    if flags.retire {
        let targets: Vec<PathBuf> = scope
            .as_deref()
            .unwrap_or_default()
            .iter()
            .map(|&i| ws.abs(i))
            .collect();
        for (n, path) in targets.iter().enumerate() {
            if n > 0 {
                ws = ctx.load()?;
            }
            let Some(i) = (0..ws.files.len()).find(|&i| ws.abs(i) == *path) else {
                continue;
            };
            let change = ws.retire(i, &decided, ctx.write)?;
            for m in &change.messages {
                ctx.say(m);
            }
        }
    }
    Ok(if report.remaining(&decisions) > 0 {
        1
    } else {
        0
    })
}
