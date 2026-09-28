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
    assert_eq!(keys.len(), 7);
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
