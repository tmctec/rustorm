//! Behavior the docs/cli.md examples do not show: option placement,
//! config-path precedence, color, JSON, usage errors and backups.

use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

const MOVE_FIXTURE: &str = "\
Host github
    HostName github.com
    User git

Host vps
    HostName vps.example.com
    User root
    Port 2222
";

struct Home {
    dir: tempfile::TempDir,
}

impl Home {
    fn new() -> Home {
        let dir = tempfile::tempdir().expect("temp home");
        std::fs::create_dir_all(dir.path().join(".ssh")).unwrap();
        Home { dir }
    }

    fn path(&self) -> &Path {
        self.dir.path()
    }

    fn file(&self, name: &str, text: &str) -> PathBuf {
        let p = self.path().join(name);
        std::fs::write(&p, text).unwrap();
        p
    }

    /// Writes rustorm's own config.toml where `dirs::config_dir` looks.
    fn user_config(&self, text: &str) {
        for dir in [
            self.path().join("Library/Application Support/rustorm"),
            self.path().join(".config/rustorm"),
        ] {
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("config.toml"), text).unwrap();
        }
    }

    fn cmd(&self) -> Command {
        let mut c = Command::new(env!("CARGO_BIN_EXE_rustorm"));
        c.env("HOME", self.path())
            .env("XDG_CONFIG_HOME", self.path().join(".config"))
            .env("USER", "travis")
            .env_remove("RUSTORM_CONFIG")
            .env_remove("RUSTORM_ASSUME_TTY")
            .env_remove("NO_COLOR")
            .stdin(Stdio::null());
        c
    }

    fn run(&self, args: &[&str]) -> Output {
        self.cmd().args(args).output().expect("run rustorm")
    }
}

fn text(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn err(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

#[test]
fn four_move_forms_write_identical_files() {
    let forms: [&[&str]; 4] = [
        &["move", "vps", "vps2", "--section", "bob"],
        &["move", "--section", "bob", "vps", "vps2"],
        &["move", "vps", "--section", "bob", "vps2"],
        &["--section", "bob", "move", "vps", "vps2"],
    ];
    let mut results = Vec::new();
    for form in forms {
        let home = Home::new();
        let config = home.file("config", MOVE_FIXTURE);
        let mut args = vec!["--config", config.to_str().unwrap()];
        args.extend_from_slice(form);
        let out = home.run(&args);
        assert_eq!(out.status.code(), Some(0), "{form:?}: {}", err(&out));
        assert_eq!(
            text(&out),
            "vps renamed to vps2 and moved to section bob. Connect with: ssh vps2\n"
        );
        results.push(std::fs::read(&config).unwrap());
    }
    assert!(
        results.windows(2).all(|w| w[0] == w[1]),
        "move forms differ"
    );
    let file = String::from_utf8(results.remove(0)).unwrap();
    assert!(file.contains("section: bob") && file.contains("Host vps2"));
}

#[test]
fn config_flag_wins_over_rustorm_config() {
    let home = Home::new();
    let from_env = home.file("env.conf", "Host envhost\n    HostName e.example.com\n");
    let from_flag = home.file("flag.conf", "Host flaghost\n    HostName f.example.com\n");
    let out = home
        .cmd()
        .env("RUSTORM_CONFIG", &from_env)
        .args(["list", "-n"])
        .output()
        .unwrap();
    assert_eq!(text(&out), "envhost\n");
    let out = home
        .cmd()
        .env("RUSTORM_CONFIG", &from_env)
        .args(["list", "-n", "--config"])
        .arg(&from_flag)
        .output()
        .unwrap();
    assert_eq!(text(&out), "flaghost\n");
    // Without either, ~/.ssh/config under HOME.
    home.file(".ssh/config", "Host homehost\n    HostName h.example.com\n");
    assert_eq!(text(&home.run(&["ls", "-n"])), "homehost\n");
}

#[test]
fn global_options_go_before_or_after_the_command() {
    let home = Home::new();
    let config = home.file("config", MOVE_FIXTURE);
    let c = config.to_str().unwrap();
    let a = home.run(&["--json", "-c", c, "list"]);
    let b = home.run(&["list", "--config", c, "--json"]);
    assert_eq!(text(&a), text(&b));
    assert_eq!(home.run(&["-V"]).stdout, home.run(&["dump", "-V"]).stdout);
    assert_eq!(
        text(&home.run(&["version"])),
        format!("rustorm {}\n", env!("CARGO_PKG_VERSION"))
    );
}

#[test]
fn read_commands_emit_no_ansi_without_a_terminal_or_with_no_color() {
    let home = Home::new();
    let config = home.file("config", MOVE_FIXTURE);
    let c = config.to_str().unwrap();
    let reads: [&[&str]; 5] = [
        &["list"],
        &["show", "vps"],
        &["dump"],
        &["search", "vps"],
        &["check"],
    ];
    for args in reads {
        let out = home.cmd().arg("-c").arg(c).args(args).output().unwrap();
        assert!(
            !text(&out).contains('\x1b'),
            "{args:?} colored off a terminal"
        );
    }
    // color = "always" in rustorm's config colors a pipe; --no-color and
    // NO_COLOR still win.
    home.user_config("[defaults]\ncolor = \"always\"\n");
    let forced = home.cmd().args(["-c", c, "list"]).output().unwrap();
    assert!(text(&forced).contains('\x1b'), "color = always");
    for args in reads {
        let flag = home
            .cmd()
            .args(["--no-color", "-c", c])
            .args(args)
            .output()
            .unwrap();
        assert!(!text(&flag).contains('\x1b'), "{args:?} with --no-color");
        let env = home
            .cmd()
            .env("NO_COLOR", "1")
            .args(["-c", c])
            .args(args)
            .output()
            .unwrap();
        assert!(!text(&env).contains('\x1b'), "{args:?} with NO_COLOR");
    }
}

#[test]
fn json_list_rows_carry_section() {
    let home = Home::new();
    let config = home.file("config", MOVE_FIXTURE);
    let c = config.to_str().unwrap();
    assert_eq!(
        home.run(&[
            "-c",
            c,
            "add",
            "db1",
            "postgres@db1.example.com",
            "-s",
            "data foundry"
        ])
        .status
        .code(),
        Some(0)
    );
    let out = home.run(&["--json", "list", "--config", c]);
    let rows: serde_json::Value = serde_json::from_slice(&out.stdout).expect("JSON");
    let rows = rows.as_array().unwrap();
    assert_eq!(rows.len(), 3);
    assert!(rows.iter().all(|r| r.get("section").is_some()));
    assert_eq!(rows[0]["section"], "data foundry");
    let keys: Vec<&String> = rows[0].as_object().unwrap().keys().collect();
    assert_eq!(keys.len(), 8);
    // D23: every row names its file, on a workspace of one file too.
    assert!(rows.iter().all(|r| r["file"] == c));
    for cmd in [
        &["show", "vps"][..],
        &["dump"],
        &["search", "git"],
        &["check"],
    ] {
        let out = home
            .cmd()
            .args(["--json", "-c", c])
            .args(cmd)
            .output()
            .unwrap();
        serde_json::from_slice::<serde_json::Value>(&out.stdout)
            .unwrap_or_else(|e| panic!("{cmd:?}: {e}"));
    }
}

#[test]
fn usage_errors_exit_2() {
    let home = Home::new();
    let config = home.file("config", MOVE_FIXTURE);
    let c = config.to_str().unwrap();
    for args in [
        &["frobnicate"][..],
        &["list", "--bogus"],
        &["add", "only-name"],
        &["completion", "ksh"],
        &["set", "vps", "User"],
        &["move", "vps"],
        &["search", "("],
        &["delete", "vps", "--section", "x"],
    ] {
        let out = home.cmd().args(["-c", c]).args(args).output().unwrap();
        assert_eq!(out.status.code(), Some(2), "{args:?}: {}", err(&out));
        assert!(!err(&out).is_empty(), "{args:?} printed nothing");
    }
    let out = home.run(&["-c", c, "set", "vps", "User"]);
    assert_eq!(err(&out), "error: keys and values must come in pairs.\n");
    assert_eq!(std::fs::read_to_string(&config).unwrap(), MOVE_FIXTURE);
}

#[test]
fn quiet_backup_and_no_backup() {
    let home = Home::new();
    let config = home.file("config", MOVE_FIXTURE);
    let c = config.to_str().unwrap();
    let backup = home.path().join("config~");
    let out = home.run(&["-q", "-c", c, "set", "vps", "User", "deploy"]);
    assert_eq!((out.status.code(), text(&out)), (Some(0), String::new()));
    assert_eq!(std::fs::read_to_string(&backup).unwrap(), MOVE_FIXTURE);
    std::fs::remove_file(&backup).unwrap();
    home.run(&["--no-backup", "-c", c, "set", "vps", "User", "root"]);
    assert!(!backup.exists());
    // Errors still print under -q.
    let out = home.run(&["-q", "-c", c, "show", "nope"]);
    assert_eq!(
        (out.status.code(), err(&out)),
        (Some(1), "error: nope does not exist.\n".into())
    );
}

#[test]
fn delete_all_prompt_declined_leaves_file() {
    let home = Home::new();
    let config = home.file("config", MOVE_FIXTURE);
    let mut child = home
        .cmd()
        .env("RUSTORM_ASSUME_TTY", "1")
        .args(["-c", config.to_str().unwrap(), "delete-all"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    use std::io::Write;
    child.stdin.take().unwrap().write_all(b"n\n").unwrap();
    let out = child.wait_with_output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(
        err(&out).ends_with("[y/N] error: nothing deleted.\n"),
        "{}",
        err(&out)
    );
    assert_eq!(std::fs::read_to_string(&config).unwrap(), MOVE_FIXTURE);
    let out = home.run(&["-c", config.to_str().unwrap(), "delete-all", "--yes"]);
    assert_eq!(text(&out), "2 hosts deleted.\n");
}

#[test]
fn user_config_command_aliases() {
    let home = Home::new();
    let config = home.file("config", MOVE_FIXTURE);
    home.user_config("[aliases]\nlist = [\"hosts\"]\n");
    let out = home.run(&["-c", config.to_str().unwrap(), "hosts", "-n"]);
    assert_eq!(text(&out), "github\nvps\n", "{}", err(&out));
}

#[test]
fn every_documented_alias_parses() {
    let home = Home::new();
    let config = home.file("config", MOVE_FIXTURE);
    let c = config.to_str().unwrap();
    for (alias, rest) in [
        ("update", &["vps", "User", "x"][..]),
        ("copy", &["vps", "vps-a"]),
        ("cp", &["vps", "vps-b"]),
        ("rename", &["vps-a", "vps-c"]),
        ("mv", &["vps-c", "vps-d"]),
        ("rm", &["vps-d"]),
        ("del", &["vps-b"]),
        ("ls", &[]),
        ("cat", &[]),
        ("find", &["git"]),
        ("grep", &["git"]),
        ("lint", &[]),
        ("delete_all", &["-y"]),
    ] {
        let out = home
            .cmd()
            .args(["-c", c, alias])
            .args(rest)
            .output()
            .unwrap();
        assert_eq!(out.status.code(), Some(0), "{alias}: {}", err(&out));
    }
}

#[test]
fn completion_scripts() {
    let home = Home::new();
    for shell in ["bash", "zsh", "fish", "powershell"] {
        let out = home.run(&["completion", shell]);
        assert_eq!(out.status.code(), Some(0));
        assert!(!out.stdout.is_empty());
    }
    let zsh = text(&home.run(&["completion", "zsh"]));
    assert!(zsh.starts_with("#compdef rustorm"));
    assert!(zsh.contains("rustorm list -n") && zsh.contains(":_rustorm_hosts'"));
    assert!(text(&home.run(&["completion", "bash"])).contains("rustorm list -n"));
    assert!(text(&home.run(&["completion", "fish"])).contains("rustorm list -n"));
}

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(format!("{name}.conf"))
}

/// combine-1..4, combine-8, combine-9, combine-11 end to end: a duplicate
/// name fails and lists it, `keep` writes with a backup, `--stdout` and
/// `-o` leave the inputs alone, a missing input exits 3.
#[test]
fn combine_end_to_end_on_fixtures() {
    let home = Home::new();
    let base = home.path().join(".ssh/config");
    let second = home.path().join(".ssh/legacy");
    std::fs::copy(fixture("combine-base"), &base).unwrap();
    std::fs::copy(fixture("combine-legacy"), &second).unwrap();
    let before = std::fs::read_to_string(&base).unwrap();
    let b = base.to_str().unwrap();
    let s = second.to_str().unwrap();

    let out = home.run(&["combine", b, s]);
    assert_eq!(out.status.code(), Some(1));
    let e = err(&out);
    assert!(
        e.starts_with("error: 2 hosts are defined more than once; nothing written."),
        "{e}"
    );
    assert!(
        e.contains(&format!("\n  nas: {b}, {s}\n")) && e.contains(&format!("\n  vps: {b}, {s}\n")),
        "{e}"
    );
    assert_eq!(std::fs::read_to_string(&base).unwrap(), before);
    assert!(!base.with_file_name("config~").exists());

    let out = home.run(&["combine", b, s, "--stdout"]);
    assert_eq!(out.status.code(), Some(1), "fail applies to --stdout too");

    let out = home.run(&["combine", "--on-conflict", "keep", b, s]);
    assert_eq!(out.status.code(), Some(0), "{}", err(&out));
    assert_eq!(
        text(&out),
        format!("combined 2 files into {b}: 4 hosts, 2 sections, 2 conflicts.\n")
    );
    assert_eq!(
        std::fs::read_to_string(base.with_file_name("config~")).unwrap(),
        before
    );
    let merged = std::fs::read_to_string(&base).unwrap();
    assert!(
        merged.contains("Host printer\n    HostName 192.168.1.20\n"),
        "{merged}"
    );
    assert!(
        merged.contains("HostName 192.168.1.10") && !merged.contains("nas.local"),
        "keep holds the base's nas"
    );
    assert_eq!(
        std::fs::read_to_string(&second).unwrap(),
        std::fs::read_to_string(fixture("combine-legacy")).unwrap()
    );

    let out = home.run(&[
        "--json",
        "combine",
        b,
        s,
        "--on-conflict",
        "replace",
        "--stdout",
    ]);
    assert_eq!(out.status.code(), Some(0), "{}", err(&out));
    let v: serde_json::Value = serde_json::from_str(&text(&out)).unwrap();
    assert_eq!(v["files"], 2);
    assert_eq!(v["hosts"], 4);
    assert_eq!(
        v["conflicts"].as_array().unwrap().len(),
        3,
        "printer is now in the base too"
    );
    assert!(v["output"].is_null());
    assert!(
        v["text"].as_str().unwrap().contains("HostName nas.local"),
        "replace takes legacy's nas"
    );
    assert_eq!(
        std::fs::read_to_string(&base).unwrap(),
        merged,
        "--stdout writes nothing"
    );

    let other = home.path().join("other.conf");
    let out = home.run(&[
        "combine",
        s,
        b,
        "-o",
        other.to_str().unwrap(),
        "--on-conflict",
        "keep",
    ]);
    assert_eq!(out.status.code(), Some(0), "{}", err(&out));
    assert!(other.exists() && !home.path().join("other.conf~").exists());
    assert_eq!(
        std::fs::read_to_string(&base).unwrap(),
        merged,
        "-o leaves the inputs alone"
    );

    let out = home.run(&["combine", b, home.path().join("missing").to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(3));
    assert!(
        err(&out).starts_with("error: cannot read "),
        "{}",
        err(&out)
    );

    let out = home.run(&["--section", "x", "combine", b, s]);
    assert_eq!(out.status.code(), Some(2));
    assert_eq!(err(&out), "error: --section does not apply to combine.\n");
    let out = home.run(&["combine", b, s, "--on-conflict", "drop"]);
    assert_eq!(out.status.code(), Some(2));
    let out = home.run(&["merge", s, b, "--stdout", "--on-conflict", "keep"]);
    assert_eq!(out.status.code(), Some(0), "merge is an alias of combine");
}

/// addsec-1 and addsec-2 end to end: the first section on an unsectioned
/// file creates the catch-all, and `sections` lists a later one above it.
#[test]
fn add_section_on_an_unsectioned_fixture_then_sections_lists_it_above_the_catch_all() {
    let home = Home::new();
    let config = home.path().join("config");
    std::fs::copy(fixture("list-flat"), &config).unwrap();
    let c = config.to_str().unwrap();
    let out = home.run(&["--config", c, "add-section", "work"]);
    assert_eq!(out.status.code(), Some(0), "{}", err(&out));
    assert_eq!(
        text(&out),
        "section work added; other created with 3 hosts.\n"
    );
    assert!(home.path().join("config~").exists());
    let out = home.run(&["add-section", "lab", "--config", c]);
    assert_eq!(text(&out), "section lab added.\n");
    let out = home.run(&["--config", c, "sections"]);
    assert_eq!(text(&out), "work    0\nlab     0\nother   3\n");
    let out = home.run(&[
        "--config",
        c,
        "--json",
        "add-section",
        "home",
        "--before",
        "lab",
    ]);
    assert_eq!(out.status.code(), Some(0));
    let v: serde_json::Value = serde_json::from_str(&text(&out)).unwrap();
    assert_eq!(v["before"], "lab");
    let out = home.run(&["--config", c, "add-section", "LAB"]);
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(err(&out), "error: section LAB already exists.\n");
    let out = home.run(&["--config", c, "sections"]);
    assert_eq!(text(&out), "work    0\nhome    0\nlab     0\nother   3\n");
}

// ----- Included files (docs/cli.md "Included files"): a temp HOME whose
// ~/.ssh/config includes ~/.ssh/config.d/*, run without --config -----

/// The docs' workspace under a temp HOME: `~/.ssh/config` (Include
/// `~/.ssh/config.d/*`, `Host *`, github), `config.d/cypress`,
/// `config.d/df-austin` (section `data foundry`), `config.d/ranch`
/// (Include `ranch.d/*`) and `ranch.d/lab`.
fn workspace_home() -> Home {
    let home = Home::new();
    for (name, rel) in [
        ("includes-root", "config"),
        ("includes-cypress", "config.d/cypress"),
        ("includes-df-austin", "config.d/df-austin"),
        ("includes-ranch", "config.d/ranch"),
        ("includes-lab", "ranch.d/lab"),
    ] {
        let dest = home.path().join(".ssh").join(rel);
        std::fs::create_dir_all(dest.parent().unwrap()).unwrap();
        std::fs::copy(fixture(name), dest).unwrap();
    }
    home
}

/// Every file under `~/.ssh`, relative path to bytes.
fn snapshot(home: &Home) -> std::collections::BTreeMap<String, Vec<u8>> {
    fn walk(dir: &Path, base: &Path, out: &mut std::collections::BTreeMap<String, Vec<u8>>) {
        for e in std::fs::read_dir(dir).unwrap() {
            let p = e.unwrap().path();
            if p.is_dir() {
                walk(&p, base, out);
            } else {
                let rel = p.strip_prefix(base).unwrap().display().to_string();
                out.insert(rel, std::fs::read(&p).unwrap());
            }
        }
    }
    let base = home.path().join(".ssh");
    let mut out = std::collections::BTreeMap::new();
    walk(&base, &base, &mut out);
    out
}

/// The paths whose bytes differ between two snapshots, or that exist in
/// only one of them.
fn changed(
    before: &std::collections::BTreeMap<String, Vec<u8>>,
    after: &std::collections::BTreeMap<String, Vec<u8>>,
) -> Vec<String> {
    let mut keys: Vec<&String> = before.keys().chain(after.keys()).collect();
    keys.sort();
    keys.dedup();
    keys.into_iter()
        .filter(|k| before.get(*k) != after.get(*k))
        .cloned()
        .collect()
}

fn ssh_text(home: &Home, rel: &str) -> String {
    std::fs::read_to_string(home.path().join(".ssh").join(rel)).unwrap()
}

#[test]
fn inc_1_list_prints_every_host_under_a_heading_per_file() {
    let home = workspace_home();
    let out = home.run(&["list"]);
    assert_eq!(out.status.code(), Some(0), "{}", err(&out));
    assert_eq!(
        text(&out),
        "\
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
"
    );
    // Shell completion reads `list -n`: every host of the workspace.
    assert_eq!(
        text(&home.run(&["list", "-n"])),
        "github\ncypressPro\ncypressPro-ext\ndb1\ndcaustin-pfsense\ndcevant\nranch-nas\nlab-1\n"
    );
    // --section keeps the file that holds it, under its heading.
    assert_eq!(
        text(&home.run(&["list", "--section", "data foundry"])),
        "~/.ssh/config.d/df-austin\n[data foundry]\ndb1              -> postgres@db1.example.com:22\ndcaustin-pfsense -> admin@10.20.0.1:22\n"
    );
    assert_eq!(
        text(&home.run(&["sections"])),
        "~/.ssh/config.d/df-austin\ndata foundry   2\n"
    );
}

#[test]
fn inc_5_add_with_section_writes_only_the_file_holding_the_section() {
    let home = workspace_home();
    let before = snapshot(&home);
    let out = home.run(&["add", "db2", "postgres@h", "--section", "data foundry"]);
    assert_eq!(out.status.code(), Some(0), "{}", err(&out));
    assert_eq!(
        text(&out),
        "db2 added to section data foundry in ~/.ssh/config.d/df-austin. Connect with: ssh db2\n"
    );
    let after = snapshot(&home);
    // The backup of a file inside the Include glob is a dot file, so the
    // glob never loads it (core's backup rule).
    assert_eq!(
        changed(&before, &after),
        ["config.d/.df-austin~", "config.d/df-austin"]
    );
    assert_eq!(after["config.d/.df-austin~"], before["config.d/df-austin"]);
    let df = ssh_text(&home, "config.d/df-austin");
    assert!(df.contains("section: data foundry") && df.contains("Host db2\n"));
    assert!(df.find("Host db2").unwrap() > df.find("section: data foundry").unwrap());
}

#[test]
fn inc_6_add_without_section_goes_to_the_root() {
    let home = workspace_home();
    let before = snapshot(&home);
    let out = home.run(&["add", "scratch", "root@h"]);
    assert_eq!(out.status.code(), Some(0), "{}", err(&out));
    assert_eq!(
        text(&out),
        "scratch added in ~/.ssh/config. Connect with: ssh scratch\n"
    );
    let after = snapshot(&home);
    assert_eq!(changed(&before, &after), ["config", "config~"]);
    let root = ssh_text(&home, "config");
    // An unsectioned root: the new host is its last entry.
    assert_eq!(root.rfind("Host "), root.find("Host scratch\n"), "{root}");
}

/// `text` with the sections of `rustorm add-section NAME` applied, made by
/// running rustorm on a scratch copy outside the workspace.
fn with_section(home: &Home, text: &str, name: &str) -> String {
    let scratch = home.file("scratch.conf", text);
    let out = home.run(&["-c", scratch.to_str().unwrap(), "add-section", name]);
    assert_eq!(out.status.code(), Some(0), "{}", err(&out));
    let result = std::fs::read_to_string(&scratch).unwrap();
    std::fs::remove_file(&scratch).unwrap();
    let _ = std::fs::remove_file(home.path().join("scratch.conf~"));
    result
}

#[test]
fn inc_9_a_section_in_two_files_is_ambiguous_and_writes_nothing() {
    let home = workspace_home();
    for rel in ["config.d/cypress", "config.d/ranch"] {
        let sectioned = with_section(&home, &ssh_text(&home, rel), "lab");
        std::fs::write(home.path().join(".ssh").join(rel), sectioned).unwrap();
    }
    let before = snapshot(&home);
    let out = home.run(&["add", "x", "root@h", "--section", "lab"]);
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(
        err(&out),
        "error: section lab exists in ~/.ssh/config.d/cypress and ~/.ssh/config.d/ranch. Say which with --file.\n"
    );
    assert_eq!(text(&out), "");
    assert!(changed(&before, &snapshot(&home)).is_empty());
    // --file settles it.
    let out = home.run(&["add", "x", "root@h", "--section", "lab", "--file", "ranch"]);
    assert_eq!(out.status.code(), Some(0), "{}", err(&out));
    assert!(ssh_text(&home, "config.d/ranch").contains("Host x\n"));
    assert!(!ssh_text(&home, "config.d/cypress").contains("Host x\n"));
}

#[test]
fn inc_10_set_writes_the_file_holding_the_host() {
    let home = workspace_home();
    let before = snapshot(&home);
    let out = home.run(&["set", "cypressPro-ext", "Port", "22"]);
    assert_eq!(out.status.code(), Some(0), "{}", err(&out));
    assert_eq!(
        text(&out),
        "cypressPro-ext updated in ~/.ssh/config.d/cypress.\n"
    );
    assert_eq!(
        changed(&before, &snapshot(&home)),
        ["config.d/.cypress~", "config.d/cypress"]
    );
    assert!(ssh_text(&home, "config.d/cypress").contains("    Port 22\n"));
    // The dot backup is not loaded on the next run: no duplicate warning.
    let out = home.run(&["set", "cypressPro-ext", "Port", "23"]);
    assert_eq!(err(&out), "");
    assert_eq!(
        text(&home.run(&["includes"])).lines().last(),
        Some("5 files, 8 hosts.")
    );
}

#[test]
fn inc_11_move_between_files_backs_up_both_and_names_both() {
    let home = workspace_home();
    let before = snapshot(&home);
    let out = home.run(&["move", "dcevant", "--section", "data foundry"]);
    assert_eq!(out.status.code(), Some(0), "{}", err(&out));
    assert_eq!(
        text(&out),
        "dcevant moved from ~/.ssh/config.d/ranch to section data foundry in ~/.ssh/config.d/df-austin.\n"
    );
    assert_eq!(
        changed(&before, &snapshot(&home)),
        [
            "config.d/.df-austin~",
            "config.d/.ranch~",
            "config.d/df-austin",
            "config.d/ranch"
        ]
    );
    assert!(!ssh_text(&home, "config.d/ranch").contains("dcevant"));
    assert!(ssh_text(&home, "config.d/df-austin")
        .contains("Host dcevant\n    HostName dcevant.ranch.lan"));
}

#[test]
fn inc_13_file_resolves_a_bare_name_a_path_and_refuses_an_unknown_name() {
    let home = workspace_home();
    // Bare name: the loaded file with that name.
    let out = home.run(&["add", "x1", "root@h", "--file", "cypress"]);
    assert_eq!(out.status.code(), Some(0), "{}", err(&out));
    assert!(ssh_text(&home, "config.d/cypress").contains("Host x1\n"));
    assert!(
        text(&out).contains("~/.ssh/config.d/cypress"),
        "{}",
        text(&out)
    );
    // A path, used as given.
    let ranch = home.path().join(".ssh/config.d/ranch");
    let out = home.run(&["-f", ranch.to_str().unwrap(), "add", "x2", "root@h"]);
    assert_eq!(out.status.code(), Some(0), "{}", err(&out));
    assert!(ssh_text(&home, "config.d/ranch").contains("Host x2\n"));
    // `config` names the root.
    let out = home.run(&["add", "x3", "root@h", "--file", "config"]);
    assert_eq!(out.status.code(), Some(0), "{}", err(&out));
    assert!(ssh_text(&home, "config").contains("Host x3\n"));
    // Unknown name: exit 1, nothing written.
    let before = snapshot(&home);
    let out = home.run(&["add", "x4", "root@h", "--file", "nas"]);
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(err(&out), "error: no such file nas in the workspace.\n");
    assert!(changed(&before, &snapshot(&home)).is_empty());
    // dump --file prints that file; --file on list is a usage error.
    let out = home.run(&["dump", "--file", "df-austin"]);
    assert_eq!(
        out.stdout,
        std::fs::read(home.path().join(".ssh/config.d/df-austin")).unwrap()
    );
    assert_eq!(
        home.run(&["list", "--file", "cypress"]).status.code(),
        Some(2)
    );
}

#[test]
fn inc_15_check_reports_the_duplicate_the_backup_and_the_include_in_host_star() {
    let home = workspace_home();
    std::fs::write(
        home.path().join(".ssh/config"),
        "Host *\n    ServerAliveInterval 60\n    Include ~/.ssh/config.d/*\n\nHost github\n    HostName github.com\n    User git\n",
    )
    .unwrap();
    std::fs::write(
        home.path()
            .join(".ssh/config.d/df-austin.bak.20260628232757"),
        "Host dcaustin-pfsense\n    HostName 10.20.0.1\n    User admin\n",
    )
    .unwrap();
    let before = snapshot(&home);
    let out = home.run(&["check"]);
    assert_eq!(out.status.code(), Some(1), "{}", err(&out));
    assert_eq!(
        text(&out),
        "\
dcaustin-pfsense: defined in ~/.ssh/config.d/df-austin and ~/.ssh/config.d/df-austin.bak.20260628232757; ssh uses the first
Include ~/.ssh/config.d/*: loads ~/.ssh/config.d/df-austin.bak.20260628232757, which looks like a backup
Include ~/.ssh/config.d/*: inside Host * in ~/.ssh/config; treated as global
3 problems in 9 hosts.
"
    );
    assert!(changed(&before, &snapshot(&home)).is_empty());
}

#[test]
fn inc_16_includes_lists_the_workspace_in_text_and_json() {
    let home = workspace_home();
    let out = home.run(&["includes"]);
    assert_eq!(out.status.code(), Some(0), "{}", err(&out));
    assert_eq!(
        text(&out),
        "\
~/.ssh/config: Include ~/.ssh/config.d/*
  ~/.ssh/config.d/cypress    2 hosts
  ~/.ssh/config.d/df-austin  2 hosts
  ~/.ssh/config.d/ranch      2 hosts
    ~/.ssh/config.d/ranch: Include ranch.d/*
      ~/.ssh/ranch.d/lab     1 host
5 files, 8 hosts.
"
    );
    let ssh = home.path().join(".ssh").display().to_string();
    let out = home.run(&["--json", "includes"]);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(
        text(&out),
        format!(
            "[{{\"pattern\":\"~/.ssh/config.d/*\",\"from\":\"{ssh}/config\",\"file\":\"{ssh}/config.d/cypress\",\"hosts\":2,\"nested\":[]}},\
{{\"pattern\":\"~/.ssh/config.d/*\",\"from\":\"{ssh}/config\",\"file\":\"{ssh}/config.d/df-austin\",\"hosts\":2,\"nested\":[]}},\
{{\"pattern\":\"~/.ssh/config.d/*\",\"from\":\"{ssh}/config\",\"file\":\"{ssh}/config.d/ranch\",\"hosts\":2,\"nested\":[\
{{\"pattern\":\"ranch.d/*\",\"from\":\"{ssh}/config.d/ranch\",\"file\":\"{ssh}/ranch.d/lab\",\"hosts\":1,\"nested\":[]}}]}}]\n"
        )
    );
    // A root without Include.
    std::fs::write(home.path().join(".ssh/config"), MOVE_FIXTURE).unwrap();
    let out = home.run(&["includes"]);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(text(&out), "no Include lines in ~/.ssh/config\n");
    assert_eq!(text(&home.run(&["--json", "includes"])), "[]\n");
}

#[test]
fn inc_17_every_json_row_carries_its_file() {
    let home = workspace_home();
    std::fs::write(
        home.path()
            .join(".ssh/config.d/df-austin.bak.20260628232757"),
        "Host dcaustin-pfsense\n    HostName 10.20.0.1\n",
    )
    .unwrap();
    let ssh = home.path().join(".ssh");
    let abs = |rel: &str| ssh.join(rel).display().to_string();
    let json = |args: &[&str]| -> serde_json::Value {
        let out = home.run(args);
        serde_json::from_slice(&out.stdout).unwrap_or_else(|e| panic!("{args:?}: {e}"))
    };
    let rows = json(&["--json", "list"]);
    let rows = rows.as_array().unwrap();
    assert_eq!(rows.len(), 9);
    let file_of = |rows: &[serde_json::Value], name: &str| -> String {
        rows.iter().find(|r| r["name"] == name).unwrap()["file"]
            .as_str()
            .unwrap()
            .to_string()
    };
    assert_eq!(file_of(rows, "github"), abs("config"));
    assert_eq!(file_of(rows, "cypressPro"), abs("config.d/cypress"));
    assert_eq!(file_of(rows, "lab-1"), abs("ranch.d/lab"));
    let rows = json(&["--json", "search", "^d"]);
    let rows = rows.as_array().unwrap();
    assert!(rows.iter().all(|r| r["file"].is_string()));
    assert_eq!(file_of(rows, "db1"), abs("config.d/df-austin"));
    assert_eq!(file_of(rows, "dcevant"), abs("config.d/ranch"));
    let report = json(&["--json", "check"]);
    let problems = report["problems"].as_array().expect("problems array");
    assert!(!problems.is_empty());
    assert!(problems.iter().all(|p| p["file"].is_string()), "{report}");
    let sections = json(&["--json", "sections"]);
    assert_eq!(sections[0]["file"], abs("config.d/df-austin"));
    let shown = json(&["--json", "show", "lab-1"]);
    assert_eq!(shown[0]["file"], abs("ranch.d/lab"));
}
