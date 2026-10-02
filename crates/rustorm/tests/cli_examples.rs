//! Runs every `$ rustorm …` line under every `**Examples**` block of
//! docs/cli.md against the built binary and compares merged stdout+stderr
//! and the exit status with the transcript.
//!
//! Each command runs with `HOME` set to a fresh temp directory and
//! `--config <home>/.ssh/config`. `~` in the command line expands to that
//! home, and `/home/me` in the expected text stands for it. The table
//! in [`plan`] names, for every command of every block, the fixture the
//! config is reset to before it (or `None` to continue with the state the
//! previous command left) and the exit status it must return.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const DOC: &str = include_str!("../../../docs/cli.md");
const DOC_HOME: &str = "/home/me";

/// One step of a block: reset fixture (or continue) and expected exit status.
type Step = (Option<&'static str>, i32);

/// The fixture and exit status of every command, per command section.
fn plan(heading: &str) -> Option<Vec<Step>> {
    let f = |name| Some(name);
    let steps: Vec<Step> = match heading {
        "add" => vec![(f("empty"), 0), (None, 0), (None, 0), (None, 1)],
        "edit" => vec![(f("edit"), 0), (None, 1)],
        "set" => vec![
            (f("set"), 0),
            (None, 0),
            (None, 0),
            (None, 0),
            (None, 1),
            (None, 1),
        ],
        "unset" => vec![(f("unset"), 0), (None, 0)],
        "clone" => vec![(f("clone"), 0), (None, 0), (None, 0), (None, 0), (None, 1)],
        "move" => vec![(f("move"), 0), (None, 0), (None, 0), (None, 2)],
        "delete" => vec![(f("delete"), 0), (None, 1)],
        // Two independent scenarios on the same 14-host file.
        "delete-all" => vec![(f("fourteen"), 0), (f("fourteen"), 1)],
        // An unsectioned file, then a sectioned one.
        "list" => vec![
            (f("list-flat"), 0),
            (None, 0),
            (f("list-sections"), 0),
            (None, 0),
        ],
        "show" => vec![(f("show"), 0), (None, 1)],
        "dump" => vec![(f("dump"), 0), (None, 0)],
        "search" => vec![(f("search"), 0), (None, 0), (None, 0), (None, 1)],
        // Reading output (a `##` section): a sectioned file with metadata,
        // then the Include workspace (config.d/df-austin seeded by
        // [`extra_files`]) for the `file` example, then the sectioned file
        // again. `echo $?` lines are steps too.
        "Reading output" => vec![
            (f("read"), 0),
            (None, 0),
            (None, 0),
            (None, 0),
            (None, 0),
            (None, 0),
            (None, 0),
            (None, 0),
            (f("read-root"), 0),
            (f("read"), 0),
            (None, 4),
            (None, 0),
            (None, 0),
            (None, 0),
            (None, 0),
            (None, 2),
            (None, 0),
        ],
        "alias" => vec![(f("alias"), 0), (None, 0)],
        "unalias" => vec![(f("unalias"), 0), (None, 0)],
        "sections" => vec![(f("sections"), 0)],
        // An unsectioned file first, then the sectioned one.
        "add-section" => vec![
            (f("list-flat"), 0),
            (f("sections"), 0),
            (None, 0),
            (None, 0),
            (None, 1),
        ],
        // The base; config.d/cypress and config.d/legacy are seeded by [`extra_files`].
        "combine" => vec![
            (f("combine-base"), 0),
            (None, 1),
            (None, 0),
            (None, 0),
            (None, 2),
        ],
        "rename-section" => vec![(f("sections"), 0), (None, 0), (None, 0), (None, 1)],
        "backup" => vec![(f("show"), 0)],
        // A file with two problems, then a clean one.
        "check" => vec![(f("check-problems"), 1), (f("fourteen"), 0)],
        // The Included files workspace (config.d and ranch.d seeded by
        // [`extra_files`]), a root whose only Include matches nothing, and
        // a root without Include.
        "includes" => vec![
            (f("includes-root"), 0),
            (None, 0),
            (f("includes-empty"), 0),
            (f("list-flat"), 0),
        ],
        "completion" => vec![(f("empty"), 0)],
        "version" => vec![(f("empty"), 0)],
        _ => return None,
    };
    Some(steps)
}

/// Files beside the config that a block needs: (fixture, path under `~/.ssh`).
fn extra_files(heading: &str) -> Vec<(&'static str, &'static str)> {
    match heading {
        "combine" => vec![
            ("combine-cypress", "config.d/cypress"),
            ("combine-legacy", "config.d/legacy"),
        ],
        "includes" => vec![
            ("includes-cypress", "config.d/cypress"),
            ("includes-df-austin", "config.d/df-austin"),
            ("includes-ranch", "config.d/ranch"),
            ("includes-lab", "ranch.d/lab"),
        ],
        "Reading output" => vec![("read-df-austin", "config.d/df-austin")],
        _ => Vec::new(),
    }
}

struct Example {
    command: String,
    expected: String,
}

struct Block {
    heading: String,
    examples: Vec<Example>,
}

/// Every fenced block that follows a `**Examples**` line, with the `###`
/// (or, outside the command sections, `##`) heading it sits under.
fn blocks() -> Vec<Block> {
    let mut out = Vec::new();
    let mut heading = String::new();
    let mut want_block = false;
    let mut lines = DOC.lines();
    while let Some(line) = lines.next() {
        if let Some(h) = line.strip_prefix("### ").or_else(|| line.strip_prefix("## ")) {
            heading = h.trim().to_string();
            want_block = false;
        } else if line.trim() == "**Examples**" {
            want_block = true;
        } else if want_block && line.starts_with("```") {
            want_block = false;
            let mut body = Vec::new();
            for l in lines.by_ref() {
                if l.starts_with("```") {
                    break;
                }
                body.push(l);
            }
            out.push(Block {
                heading: heading.clone(),
                examples: parse_examples(&body),
            });
        }
    }
    out
}

fn parse_examples(body: &[&str]) -> Vec<Example> {
    let mut out: Vec<Example> = Vec::new();
    let mut expected: Vec<&str> = Vec::new();
    let flush = |out: &mut Vec<Example>, expected: &mut Vec<&str>| {
        if let Some(last) = out.last_mut() {
            while expected.last().is_some_and(|l| l.trim().is_empty()) {
                expected.pop();
            }
            last.expected = expected.join("\n");
        }
        expected.clear();
    };
    for line in body {
        if let Some(cmd) = line.strip_prefix("$ ") {
            flush(&mut out, &mut expected);
            out.push(Example {
                command: cmd.to_string(),
                expected: String::new(),
            });
        } else {
            expected.push(line);
        }
    }
    flush(&mut out, &mut expected);
    out
}

/// A word of the shell line; `quoted` words never expand `~`.
#[derive(Debug, Clone, PartialEq)]
struct Word {
    text: String,
    operator: bool,
}

/// Splits a shell line into words with single quotes, double quotes and
/// backslash escapes, expands a leading unquoted `~` to `home`, and keeps
/// `|`, `<` and `>` as operator words.
fn split(line: &str, home: &Path) -> Vec<Word> {
    let mut words = Vec::new();
    let mut cur = String::new();
    let mut in_word = false;
    let mut chars = line.chars().peekable();
    let home = home.display().to_string();
    while let Some(c) = chars.next() {
        match c {
            ' ' | '\t' => {
                if in_word {
                    words.push(Word {
                        text: std::mem::take(&mut cur),
                        operator: false,
                    });
                    in_word = false;
                }
            }
            '|' | '<' | '>' if !in_word => {
                words.push(Word {
                    text: c.to_string(),
                    operator: true,
                });
            }
            '\'' => {
                in_word = true;
                for q in chars.by_ref() {
                    if q == '\'' {
                        break;
                    }
                    cur.push(q);
                }
            }
            '"' => {
                in_word = true;
                while let Some(q) = chars.next() {
                    match q {
                        '"' => break,
                        '\\' if matches!(chars.peek(), Some('"' | '\\' | '$')) => {
                            cur.push(chars.next().unwrap());
                        }
                        _ => cur.push(q),
                    }
                }
            }
            '\\' => {
                in_word = true;
                if let Some(n) = chars.next() {
                    cur.push(n);
                }
            }
            '~' if !in_word && matches!(chars.peek(), None | Some('/' | ' ')) => {
                in_word = true;
                cur.push_str(&home);
            }
            _ => {
                in_word = true;
                cur.push(c);
            }
        }
    }
    if in_word {
        words.push(Word {
            text: cur,
            operator: false,
        });
    }
    words
}

struct Outcome {
    output: String,
    status: i32,
}

struct Shell {
    home: PathBuf,
    config: PathBuf,
    last_status: i32,
}

impl Shell {
    fn command(&self, args: &[String]) -> Command {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_rustorm"));
        cmd.arg("--config").arg(&self.config).args(args);
        cmd.env("HOME", &self.home)
            .env("USER", "travis")
            .env_remove("RUSTORM_CONFIG")
            .env_remove("RUSTORM_ASSUME_TTY")
            .env_remove("NO_COLOR");
        cmd
    }

    /// Runs one transcript line. `answer` feeds a prompt through a pipe.
    fn run(&mut self, line: &str, answer: Option<&str>) -> Outcome {
        let words = split(line, &self.home);
        let outcome = if words.len() == 2 && words[0].text == "echo" && words[1].text == "$?" {
            Outcome {
                output: format!("{}\n", self.last_status),
                status: 0,
            }
        } else {
            assert_eq!(words[0].text, "rustorm", "unsupported example line: {line}");
            let op = words.iter().position(|w| w.operator);
            let args: Vec<String> = words[1..op.unwrap_or(words.len())]
                .iter()
                .map(|w| w.text.clone())
                .collect();
            match op.map(|i| (words[i].text.as_str(), &words[i + 1..])) {
                None => self.merged(&args, answer.map(|a| format!("{a}\n")), answer.is_some()),
                Some(("<", rest)) => {
                    let input = std::fs::read_to_string(&rest[0].text).unwrap_or_default();
                    self.merged(&args, Some(input), false)
                }
                Some((">", rest)) => self.to_file(&args, Path::new(&rest[0].text)),
                Some(("|", rest)) => self.pipe_to_diff(&args, rest, line),
                Some((other, _)) => panic!("unsupported operator {other} in {line}"),
            }
        };
        self.last_status = outcome.status;
        outcome
    }

    /// stdout and stderr through one pipe, so their order is kept.
    fn merged(&self, args: &[String], stdin: Option<String>, assume_tty: bool) -> Outcome {
        let (mut reader, writer) = std::io::pipe().expect("pipe");
        let mut cmd = self.command(args);
        if assume_tty {
            cmd.env("RUSTORM_ASSUME_TTY", "1");
        }
        cmd.stdout(writer.try_clone().expect("clone pipe"))
            .stderr(writer)
            .stdin(Stdio::piped());
        let mut child = cmd.spawn().expect("spawn rustorm");
        drop(cmd);
        {
            let mut input = child.stdin.take().expect("stdin");
            if let Some(text) = stdin {
                input.write_all(text.as_bytes()).expect("write stdin");
            }
        }
        let mut output = String::new();
        reader.read_to_string(&mut output).expect("read output");
        let status = child.wait().expect("wait").code().expect("exit code");
        Outcome { output, status }
    }

    fn to_file(&self, args: &[String], path: &Path) -> Outcome {
        std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
        let out = self
            .command(args)
            .stdin(Stdio::null())
            .output()
            .expect("run rustorm");
        std::fs::write(path, &out.stdout).expect("write redirect target");
        Outcome {
            output: String::from_utf8(out.stderr).expect("utf8"),
            status: out.status.code().expect("exit code"),
        }
    }

    /// `rustorm … | diff - FILE`: empty output and status 0 when stdout
    /// equals the file.
    fn pipe_to_diff(&self, args: &[String], rest: &[Word], line: &str) -> Outcome {
        assert!(
            rest.len() == 3 && rest[0].text == "diff" && rest[1].text == "-",
            "unsupported pipeline: {line}"
        );
        let out = self
            .command(args)
            .stdin(Stdio::null())
            .output()
            .expect("run rustorm");
        let file = std::fs::read(&rest[2].text).expect("read diff target");
        if out.stdout == file && out.status.success() {
            Outcome {
                output: String::from_utf8(out.stderr).unwrap(),
                status: 0,
            }
        } else {
            Outcome {
                output: "stdout differs from the file\n".to_string(),
                status: 1,
            }
        }
    }
}

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(format!("{name}.conf"))
}

/// Compares line by line. An expected line ending in `, ...]` (the JSON
/// example's elision) matches any actual line that starts with the text
/// before it and ends with `]`.
fn matches(expected: &str, actual: &str) -> bool {
    let e: Vec<&str> = expected.lines().collect();
    let a: Vec<&str> = actual.lines().collect();
    e.len() == a.len()
        && e.iter()
            .zip(&a)
            .all(|(e, a)| match e.strip_suffix(", ...]") {
                Some(prefix) => a.starts_with(prefix) && a.ends_with(']'),
                None => e == a,
            })
}

#[test]
fn every_cli_md_example_matches() {
    let blocks = blocks();
    let mut commands = 0;
    let mut failures = Vec::new();
    for block in &blocks {
        let steps = plan(&block.heading)
            .unwrap_or_else(|| panic!("no fixture plan for examples under ### {}", block.heading));
        assert_eq!(
            steps.len(),
            block.examples.len(),
            "fixture plan for {} lists {} commands, docs/cli.md has {}",
            block.heading,
            steps.len(),
            block.examples.len()
        );
        let home = tempfile::tempdir().expect("temp home");
        let ssh = home.path().join(".ssh");
        std::fs::create_dir_all(&ssh).unwrap();
        let mut shell = Shell {
            home: home.path().to_path_buf(),
            config: ssh.join("config"),
            last_status: 0,
        };
        let home_text = home.path().display().to_string();
        for (name, rel) in extra_files(&block.heading) {
            let dest = ssh.join(rel);
            std::fs::create_dir_all(dest.parent().unwrap()).unwrap();
            std::fs::copy(fixture(name), dest).expect("seed extra file");
        }
        for (example, (reset, status)) in block.examples.iter().zip(steps) {
            if let Some(name) = reset {
                std::fs::copy(fixture(name), &shell.config).expect("seed fixture");
                let _ = std::fs::remove_file(ssh.join("config~"));
            }
            let mut expected = example.expected.replace(DOC_HOME, &home_text);
            // A prompt line `… [y/N] y` is the terminal echoing the answer;
            // from a pipe the answer is input and is not echoed.
            let mut answer = None;
            if let Some(pos) = expected.find("[y/N] ") {
                let end = expected[pos..]
                    .find('\n')
                    .map_or(expected.len(), |n| pos + n);
                answer = Some(expected[pos + 6..end].to_string());
                expected.replace_range(pos + 6..(end + 1).min(expected.len()), "");
            }
            let got = shell.run(&example.command, answer.as_deref());
            commands += 1;
            let actual = got.output.trim_end_matches('\n');
            if !matches(&expected, actual) || got.status != status {
                failures.push(format!(
                    "### {} $ {}\n--- expected (exit {status}):\n{expected}\n--- actual (exit {}):\n{actual}\n",
                    block.heading, example.command, got.status
                ));
            }
            if example.command.contains("completion zsh >") {
                let script = std::fs::read_to_string(home.path().join(".zfunc/_storm")).unwrap();
                assert!(script.starts_with("#compdef"), "zsh script header");
            }
        }
    }
    println!(
        "cli_examples: {} blocks, {} commands executed",
        blocks.len(),
        commands
    );
    assert_eq!(
        blocks.len(),
        24,
        "docs/cli.md has one Examples block per command, plus Reading output"
    );
    assert!(
        failures.is_empty(),
        "{} example(s) failed:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

#[test]
fn splitter_handles_quotes_tilde_and_operators() {
    let home = Path::new("/h");
    let w: Vec<String> = split(r#"add x "a b" 'c\d' e\ f ~/k "~/q" < /dev/null"#, home)
        .into_iter()
        .map(|w| w.text)
        .collect();
    assert_eq!(
        w,
        [
            "add",
            "x",
            "a b",
            r"c\d",
            "e f",
            "/h/k",
            "~/q",
            "<",
            "/dev/null"
        ]
    );
}
