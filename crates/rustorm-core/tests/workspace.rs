//! The Include workspace in rustorm-core: one exact-content test per
//! catalog case inc-1, inc-2, inc-5..inc-12, inc-14, inc-15, inc-20,
//! inc-23 and inc-24 (plan dhole, step 4).
//!
//! Every test builds the workspace of docs/cli.md's Included files chapter
//! under a temporary home directory:
//!
//! ```text
//! ~/.ssh/config               Include ~/.ssh/config.d/*; host github
//! ~/.ssh/config.d/cypress     hosts cypressPro, cypressPro-ext
//! ~/.ssh/config.d/df-austin   section data foundry: db1, dcaustin-pfsense
//! ~/.ssh/config.d/ranch       Include ranch.d/*; hosts dcevant, ranch-nas
//! ~/.ssh/ranch.d/lab          host lab-1
//! ```

use std::fs;
use std::path::{Path, PathBuf};

use rustorm_core::banner::banner_text;
use rustorm_core::*;

const ROOT: &str = "Include ~/.ssh/config.d/*\n\nHost *\n    ServerAliveInterval 60\n\nHost github\n    HostName github.com\n    User git\n";
const CYPRESS: &str = "Host cypressPro\n    HostName 10.10.0.2\n    User travis\n\nHost cypressPro-ext\n    HostName cypress.example.com\n    User travis\n";
const RANCH: &str = "Include ranch.d/*\n\nHost dcevant\n    HostName dcevant.ranch.lan\n\nHost ranch-nas\n    HostName nas.ranch.lan\n";
const LAB: &str = "Host lab-1\n    HostName 10.30.0.5\n";
const BAK: &str = "df-austin.bak.20260628232757";

fn df_austin() -> String {
    format!(
        "{}\nHost db1\n    HostName db1.example.com\n    User postgres\n\nHost dcaustin-pfsense\n    HostName 10.20.0.1\n    User admin\n",
        banner_text("data foundry")
    )
}

fn env() -> Env {
    Env {
        user: Some("travis".into()),
        home: None,
    }
}

fn s(v: &str) -> String {
    v.to_string()
}

struct Home {
    dir: tempfile::TempDir,
}

impl Home {
    /// The chapter's workspace.
    fn new() -> Home {
        let home = Home {
            dir: tempfile::tempdir().unwrap(),
        };
        home.write(".ssh/config", ROOT);
        home.write(".ssh/config.d/cypress", CYPRESS);
        home.write(".ssh/config.d/df-austin", &df_austin());
        home.write(".ssh/config.d/ranch", RANCH);
        home.write(".ssh/ranch.d/lab", LAB);
        home
    }

    fn path(&self, rel: &str) -> PathBuf {
        self.dir.path().join(rel)
    }

    fn write(&self, rel: &str, text: &str) {
        let p = self.path(rel);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, text).unwrap();
    }

    fn read(&self, rel: &str) -> String {
        fs::read_to_string(self.path(rel)).unwrap()
    }

    /// The backup a save wrote for `rel`: `<file>~` for the root, the dot
    /// backup `<dir>/.<name>~` for an included file, whose `<file>~` the
    /// Include glob would match. Panics if both exist.
    fn backup(&self, rel: &str) -> Option<String> {
        let plain = fs::read_to_string(backup_path(&self.path(rel))).ok();
        let dot = fs::read_to_string(dot_backup_path(&self.path(rel))).ok();
        assert!(plain.is_none() || dot.is_none(), "{rel} has both backups");
        if rel == ".ssh/config" {
            assert!(dot.is_none(), "the root got a dot backup");
            plain
        } else {
            assert!(
                plain.is_none(),
                "{rel} got a <file>~ backup inside the glob"
            );
            dot
        }
    }

    fn load(&self) -> Workspace {
        Workspace::load_with_home(self.path(".ssh/config"), Some(self.dir.path())).unwrap()
    }

    /// Every file of the chapter's workspace with its text on disk.
    fn snapshot(&self) -> Vec<(&'static str, String)> {
        [
            ".ssh/config",
            ".ssh/config.d/cypress",
            ".ssh/config.d/df-austin",
            ".ssh/config.d/ranch",
            ".ssh/ranch.d/lab",
        ]
        .into_iter()
        .map(|r| (r, self.read(r)))
        .collect()
    }

    /// Asserts every file except `changed` is as the fixture wrote it and
    /// has no backup.
    fn assert_untouched_except(&self, before: &[(&str, String)], changed: &[&str]) {
        for (rel, text) in before {
            if changed.contains(rel) {
                continue;
            }
            assert_eq!(&self.read(rel), text, "{rel} changed");
            assert_eq!(self.backup(rel), None, "{rel} was backed up");
        }
    }
}

/// `list` as docs/cli.md prints it: a heading per file on a workspace of
/// several files, a heading per section in a file with sections, arrows
/// aligned across the listing.
fn render_list(ws: &Workspace, rows: &[WorkspaceRow]) -> String {
    let width = rows.iter().map(|r| r.row.name.len()).max().unwrap_or(0);
    let mut out = String::new();
    let mut file = None;
    let mut section = None;
    for r in rows {
        if ws.is_multi() && file != Some(r.file) {
            file = Some(r.file);
            section = None;
            out.push_str(&format!("{}\n", ws.display(r.file)));
        }
        if ws.files[r.file].config.has_sections() && section != Some(r.row.section.clone()) {
            section = Some(r.row.section.clone());
            if let Some(s) = &r.row.section {
                out.push_str(&format!("[{s}]\n"));
            }
        }
        out.push_str(&format!("{:<width$} -> {}\n", r.row.name, r.row.target()));
    }
    out
}

#[test]
fn inc_1_list_shows_every_host_under_a_heading_per_file_in_load_order() {
    let home = Home::new();
    let ws = home.load();
    assert!(ws.is_multi());
    assert!(ws.load_warnings().is_empty());
    let rows = ws.list(&env());
    assert_eq!(
        render_list(&ws, &rows),
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
    let json = serde_json::to_string(&rows[0]).unwrap();
    assert_eq!(
        json,
        format!(
            "{{\"name\":\"github\",\"file\":\"{}\",\"section\":null,\"aliases\":[],\"hostname\":\"github.com\",\"user\":\"git\",\"port\":22,\"options\":{{}}}}",
            home.path(".ssh/config").display()
        )
    );
    let json = serde_json::to_string(&rows[3]).unwrap();
    assert!(json.starts_with(&format!(
        "{{\"name\":\"db1\",\"file\":\"{}\",\"section\":\"data foundry\",",
        home.path(".ssh/config.d/df-austin").display()
    )));
}

const BASE: &str = "\
# main box
Host vps
    HostName vps.example.com
    User root
    Port 2222

Host rails01
    HostName rails01.example.com
    User deploy

Host *
    User emre
    ServerAliveInterval 60
";

#[test]
fn inc_2_no_include_config_is_byte_for_byte_the_single_file_behavior() {
    let home = Home {
        dir: tempfile::tempdir().unwrap(),
    };
    home.write(".ssh/config", BASE);
    let add = AddSpec {
        name: s("web"),
        uri: s("web@web.example.com"),
        ..AddSpec::default()
    };
    let pairs = vec![(s("User"), s("deploy")), (s("Port"), s("22"))];

    // The pre-plan path: ConfigFile plus the per-Config operations.
    let mut file = ConfigFile::load(home.path(".ssh/config")).unwrap();
    let placed = file.config.add(&add, &env()).unwrap();
    assert_eq!(placed.name, "web");
    file.config
        .set(&HostSelector::Name(s("vps")), &pairs, false)
        .unwrap();
    file.config.move_host("rails01", None, Some("bob")).unwrap();
    let golden_check = file.config.check(&env());
    let golden = file.config.render();

    // The workspace path.
    let mut ws = home.load();
    assert!(!ws.is_multi());
    assert_eq!(ws.files.len(), 1);
    let added = ws.add(&add, None, &env()).unwrap();
    let set = ws
        .set(&HostSelector::Name(s("vps")), &pairs, false, None)
        .unwrap();
    let moved = ws.move_host("rails01", None, Some("bob"), None).unwrap();
    let check = ws.check(&env());
    let messages: Vec<String> = [added.messages, set.messages, moved.messages].concat();
    assert_eq!(
        messages,
        vec![
            "web added. Connect with: ssh web",
            "vps updated.",
            "rails01 moved to section bob."
        ]
    );
    assert!([&added.warnings, &set.warnings, &moved.warnings]
        .iter()
        .all(|w| w.is_empty()));
    assert_eq!(check.summary(), "no problems in 3 hosts.");
    assert_eq!(check.summary(), golden_check.summary());
    assert_eq!(check.problems.len(), golden_check.problems.len());

    let expected = format!(
        "{}\nHost rails01\n    HostName rails01.example.com\n    User deploy\n\n{}\n# main box\nHost vps\n    HostName vps.example.com\n    User deploy\n    Port 22\n\nHost web\n    HostName web.example.com\n    User web\n    Port 22\n\nHost *\n    User emre\n    ServerAliveInterval 60\n",
        banner_text("bob"),
        banner_text("other")
    );
    assert_eq!(golden, expected);
    assert_eq!(ws.save(WriteOptions::default()).unwrap(), vec![0]);
    assert_eq!(home.read(".ssh/config"), expected);
    assert_eq!(home.backup(".ssh/config").unwrap(), BASE);

    // Single-file errors keep their single-file text.
    let err = ws.add(&add, None, &env()).unwrap_err();
    assert_eq!(
        err.to_string(),
        "web already exists. Use rustorm edit or rustorm set to modify it."
    );
    // A problem found on a single file prints as before, file carried in JSON.
    home.write(".ssh/config", "Host a\n    HostName a\n    Bogus 1\n");
    let ws = home.load();
    let report = ws.check(&env());
    assert_eq!(report.problems[0].to_string(), "a: unknown key Bogus");
    assert_eq!(
        report.problems[0].file.as_deref(),
        Some(home.path(".ssh/config").as_path())
    );
    // The list rows are the single-file rows.
    let rows: Vec<ListRow> = ws.list(&env()).into_iter().map(|r| r.row).collect();
    assert_eq!(rows, ws.files[0].config.list(&env()));
}

#[test]
fn inc_5_add_with_section_goes_to_the_file_holding_the_section() {
    let home = Home::new();
    let before = home.snapshot();
    let mut ws = home.load();
    let change = ws
        .add(
            &AddSpec {
                name: s("db2"),
                uri: s("postgres@db2.example.com"),
                section: Some(s("data foundry")),
                ..AddSpec::default()
            },
            None,
            &env(),
        )
        .unwrap();
    assert_eq!(
        change.messages,
        vec![
            "db2 added to section data foundry in ~/.ssh/config.d/df-austin. Connect with: ssh db2"
        ]
    );
    assert_eq!(ws.save(WriteOptions::default()).unwrap(), vec![2]);
    assert_eq!(
        home.read(".ssh/config.d/df-austin"),
        format!(
            "{}\nHost db1\n    HostName db1.example.com\n    User postgres\n\nHost db2\n    HostName db2.example.com\n    User postgres\n    Port 22\n\nHost dcaustin-pfsense\n    HostName 10.20.0.1\n    User admin\n",
            banner_text("data foundry")
        )
    );
    assert_eq!(home.backup(".ssh/config.d/df-austin").unwrap(), df_austin());
    home.assert_untouched_except(&before, &[".ssh/config.d/df-austin"]);
}

#[test]
fn inc_6_add_without_section_goes_to_the_root() {
    let home = Home::new();
    let before = home.snapshot();
    let mut ws = home.load();
    let change = ws
        .add(
            &AddSpec {
                name: s("scratch"),
                uri: s("root@h"),
                ..AddSpec::default()
            },
            None,
            &env(),
        )
        .unwrap();
    assert_eq!(
        change.messages,
        vec!["scratch added in ~/.ssh/config. Connect with: ssh scratch"]
    );
    assert_eq!(ws.save(WriteOptions::default()).unwrap(), vec![0]);
    assert_eq!(
        home.read(".ssh/config"),
        format!("{ROOT}\nHost scratch\n    HostName h\n    User root\n    Port 22\n")
    );
    assert_eq!(home.backup(".ssh/config").unwrap(), ROOT);
    home.assert_untouched_except(&before, &[".ssh/config"]);
    // A name any file holds is taken, and the error names the file.
    let err = ws
        .add(
            &AddSpec {
                name: s("db1"),
                uri: s("postgres@db1.example.com"),
                ..AddSpec::default()
            },
            None,
            &env(),
        )
        .unwrap_err();
    assert_eq!(
        err.to_string(),
        "db1 already exists in ~/.ssh/config.d/df-austin. Use rustorm edit or rustorm set to modify it."
    );
    assert_eq!(err.exit_code(), 1);
}

#[test]
fn inc_7_section_in_no_file_is_created_in_the_root() {
    let home = Home::new();
    let before = home.snapshot();
    let mut ws = home.load();
    let change = ws
        .add(
            &AddSpec {
                name: s("x"),
                uri: s("root@h"),
                section: Some(s("lab")),
                ..AddSpec::default()
            },
            None,
            &env(),
        )
        .unwrap();
    assert_eq!(
        change.messages,
        vec!["x added to section lab in ~/.ssh/config. Connect with: ssh x"]
    );
    ws.save(WriteOptions::default()).unwrap();
    assert_eq!(
        home.read(".ssh/config"),
        format!(
            "Include ~/.ssh/config.d/*\n\nHost *\n    ServerAliveInterval 60\n\n{}\nHost x\n    HostName h\n    User root\n    Port 22\n\n{}\nHost github\n    HostName github.com\n    User git\n",
            banner_text("lab"),
            banner_text("other")
        )
    );
    home.assert_untouched_except(&before, &[".ssh/config"]);
}

#[test]
fn inc_8_file_override_creates_the_file_and_its_section() {
    let home = Home::new();
    let before = home.snapshot();
    let mut ws = home.load();
    let change = ws
        .add(
            &AddSpec {
                name: s("CE2"),
                uri: s("izadmin@h"),
                section: Some(s("evant")),
                ..AddSpec::default()
            },
            Some("~/.ssh/config.d/df-evant"),
            &env(),
        )
        .unwrap();
    assert_eq!(
        change.messages,
        vec!["CE2 added to section evant in ~/.ssh/config.d/df-evant. Connect with: ssh CE2"]
    );
    assert_eq!(ws.save(WriteOptions::default()).unwrap(), vec![5]);
    let rel = ".ssh/config.d/df-evant";
    assert_eq!(
        home.read(rel),
        format!(
            "{}\nHost CE2\n    HostName h\n    User izadmin\n    Port 22\n\n{}\n",
            banner_text("evant"),
            banner_text("other")
        )
    );
    assert_eq!(home.backup(rel), None);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::metadata(home.path(rel)).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }
    home.assert_untouched_except(&before, &[]);
    // add-section --file names the new file the same way.
    let mut ws = home.load();
    let change = ws
        .add_section("lab", None, Some("~/.ssh/config.d/df-lab"))
        .unwrap();
    assert_eq!(
        change.messages,
        vec!["section lab added to ~/.ssh/config.d/df-lab."]
    );
}

#[test]
fn inc_9_section_in_two_files_is_ambiguous_and_nothing_is_written() {
    let home = Home::new();
    home.write(
        ".ssh/config.d/cypress",
        &format!("{}\n{CYPRESS}", banner_text("lab")),
    );
    home.write(
        ".ssh/config.d/gke",
        &format!(
            "{}\nHost gke-1\n    HostName 10.40.0.1\n",
            banner_text("lab")
        ),
    );
    let before = home.snapshot();
    let mut ws = home.load();
    let err = ws
        .add(
            &AddSpec {
                name: s("x"),
                uri: s("root@h"),
                section: Some(s("lab")),
                ..AddSpec::default()
            },
            None,
            &env(),
        )
        .unwrap_err();
    assert_eq!(
        err.to_string(),
        "section lab exists in ~/.ssh/config.d/cypress and ~/.ssh/config.d/gke. Say which with --file."
    );
    assert_eq!(err.exit_code(), 1);
    assert!(ws.modified().is_empty());
    assert!(ws.save(WriteOptions::default()).unwrap().is_empty());
    home.assert_untouched_except(&before, &[]);
    assert_eq!(home.backup(".ssh/config.d/gke"), None);
    // The same for move and rename-section; --file settles it.
    let err = ws
        .move_host("dcevant", None, Some("lab"), None)
        .unwrap_err();
    assert_eq!(err.exit_code(), 1);
    let err = ws.rename_section("lab", "labs", None).unwrap_err();
    assert!(err.to_string().starts_with("section lab exists in"));
    let change = ws.rename_section("lab", "labs", Some("gke")).unwrap();
    assert_eq!(
        change.messages,
        vec!["section lab renamed to labs in ~/.ssh/config.d/gke."]
    );
}

#[test]
fn inc_10_set_edits_the_file_holding_the_host() {
    let home = Home::new();
    let before = home.snapshot();
    let mut ws = home.load();
    let change = ws
        .set(
            &HostSelector::Name(s("cypressPro-ext")),
            &[(s("Port"), s("22"))],
            false,
            None,
        )
        .unwrap();
    assert_eq!(
        change.messages,
        vec!["cypressPro-ext updated in ~/.ssh/config.d/cypress."]
    );
    assert_eq!(ws.save(WriteOptions::default()).unwrap(), vec![1]);
    assert_eq!(
        home.read(".ssh/config.d/cypress"),
        format!("{CYPRESS}    Port 22\n")
    );
    assert_eq!(home.backup(".ssh/config.d/cypress").unwrap(), CYPRESS);
    home.assert_untouched_except(&before, &[".ssh/config.d/cypress"]);

    // The backup is the dot file, which the Include glob never matches, so
    // the next load has the same five files and check finds no backup.
    let ws = home.load();
    assert!(home.path(".ssh/config.d/.cypress~").exists());
    assert!(!home.path(".ssh/config.d/cypress~").exists());
    assert_eq!(ws.files.len(), 5);
    assert_eq!(ws.display(2), "~/.ssh/config.d/df-austin");
    assert!(ws.check(&env()).is_clean());

    // The other host-routed edits name the file the same way.
    let mut ws = home.load();
    let msgs = |c: Vec<String>| c;
    assert_eq!(
        msgs(ws.alias("cypressPro", &[s("cp")], None).unwrap().messages),
        vec!["cypressPro in ~/.ssh/config.d/cypress now answers to: cypressPro cp"]
    );
    assert_eq!(
        msgs(ws.unalias(None, &[s("cp")], None).unwrap().messages),
        vec!["cypressPro in ~/.ssh/config.d/cypress now answers to: cypressPro"]
    );
    assert_eq!(
        msgs(
            ws.move_host("ranch-nas", Some("nas"), None, None)
                .unwrap()
                .messages
        ),
        vec!["ranch-nas renamed to nas in ~/.ssh/config.d/ranch. Connect with: ssh nas"]
    );
    assert_eq!(
        msgs(ws.delete(&[s("lab-1")], None).unwrap().messages),
        vec!["lab-1 deleted from ~/.ssh/ranch.d/lab."]
    );
    assert_eq!(
        msgs(
            ws.unset(&HostSelector::Name(s("github")), &[s("User")], None)
                .unwrap()
                .messages
        ),
        vec!["github updated in ~/.ssh/config."]
    );
    assert_eq!(
        msgs(
            ws.set(
                &HostSelector::Regex(s("cypressPro.*|dcevant")),
                &[(s("User"), s("deploy"))],
                false,
                None
            )
            .unwrap()
            .messages
        ),
        vec!["3 hosts updated in 2 files: cypressPro, cypressPro-ext, dcevant"]
    );
    assert_eq!(
        msgs(
            ws.rename_section("data foundry", "dfa", None)
                .unwrap()
                .messages
        ),
        vec!["section data foundry renamed to dfa in ~/.ssh/config.d/df-austin."]
    );
    let err = ws.alias("github", &[s("dcevant")], None).unwrap_err();
    assert_eq!(err.to_string(), "dcevant is already a name of dcevant.");
    let change = ws
        .clone_host(
            &CloneSpec {
                source: s("dcevant"),
                new_name: s("dcevant2"),
                ..CloneSpec::default()
            },
            None,
        )
        .unwrap();
    assert_eq!(
        change.messages,
        vec!["dcevant2 added in ~/.ssh/config.d/ranch. Connect with: ssh dcevant2"]
    );
    let written = ws.save(WriteOptions::default()).unwrap();
    assert_eq!(written, vec![1, 3, 4, 0, 2]);
    assert_eq!(
        home.read(".ssh/config.d/ranch"),
        "Include ranch.d/*\n\nHost dcevant\n    HostName dcevant.ranch.lan\n    User deploy\n\nHost nas\n    HostName nas.ranch.lan\n\nHost dcevant2\n    HostName dcevant2.ranch.lan\n    User deploy\n"
    );
}

#[test]
fn inc_11_move_between_files_writes_both_with_both_backups() {
    let home = Home::new();
    let before = home.snapshot();
    let mut ws = home.load();
    let change = ws
        .move_host("dcevant", None, Some("data foundry"), None)
        .unwrap();
    assert_eq!(
        change.messages,
        vec!["dcevant moved from ~/.ssh/config.d/ranch to section data foundry in ~/.ssh/config.d/df-austin."]
    );
    assert_eq!(change.files, vec![2, 3]);
    assert_eq!(change.value.section.as_deref(), Some("data foundry"));
    // The destination is written first.
    assert_eq!(ws.save(WriteOptions::default()).unwrap(), vec![2, 3]);
    assert_eq!(
        home.read(".ssh/config.d/df-austin"),
        format!(
            "{}\nHost db1\n    HostName db1.example.com\n    User postgres\n\nHost dcaustin-pfsense\n    HostName 10.20.0.1\n    User admin\n\nHost dcevant\n    HostName dcevant.ranch.lan\n",
            banner_text("data foundry")
        )
    );
    assert_eq!(
        home.read(".ssh/config.d/ranch"),
        "Include ranch.d/*\n\nHost ranch-nas\n    HostName nas.ranch.lan\n"
    );
    assert_eq!(home.backup(".ssh/config.d/df-austin").unwrap(), df_austin());
    assert_eq!(home.backup(".ssh/config.d/ranch").unwrap(), RANCH);
    home.assert_untouched_except(&before, &[".ssh/config.d/df-austin", ".ssh/config.d/ranch"]);

    // Comments above the entry travel with it.
    let home = Home::new();
    home.write(
        ".ssh/config.d/ranch",
        "Include ranch.d/*\n\n# the ranch box\nHost dcevant\n    HostName dcevant.ranch.lan\n",
    );
    let mut ws = home.load();
    let change = ws
        .move_host("dcevant", Some("evant"), Some("data foundry"), None)
        .unwrap();
    assert_eq!(
        change.messages,
        vec!["dcevant renamed to evant and moved from ~/.ssh/config.d/ranch to section data foundry in ~/.ssh/config.d/df-austin. Connect with: ssh evant"]
    );
    ws.save(WriteOptions::default()).unwrap();
    assert!(home
        .read(".ssh/config.d/df-austin")
        .ends_with("\n# the ranch box\nHost evant\n    HostName dcevant.ranch.lan\n"));
    assert_eq!(home.read(".ssh/config.d/ranch"), "Include ranch.d/*\n");
}

#[test]
fn inc_12_host_in_two_files_edits_the_first_and_warns() {
    let home = Home::new();
    home.write(
        &format!(".ssh/config.d/{BAK}"),
        "Host dcaustin-pfsense\n    HostName 10.20.0.99\n",
    );
    let mut ws = home.load();
    let warning =
        format!("dcaustin-pfsense is also defined in ~/.ssh/config.d/{BAK}; ssh uses the first.");
    let (shown, warnings) = ws.show(&[s("dcaustin-pfsense")]).unwrap();
    assert_eq!(
        shown[0].text,
        "Host dcaustin-pfsense\n    HostName 10.20.0.1\n    User admin\n"
    );
    assert_eq!(shown[0].file, home.path(".ssh/config.d/df-austin"));
    assert_eq!(warnings, vec![warning.clone()]);
    let change = ws
        .set(
            &HostSelector::Name(s("dcaustin-pfsense")),
            &[(s("Port"), s("22"))],
            false,
            None,
        )
        .unwrap();
    assert_eq!(change.warnings, vec![warning]);
    assert_eq!(
        change.messages,
        vec!["dcaustin-pfsense updated in ~/.ssh/config.d/df-austin."]
    );
    // --file picks the other definition, without a warning.
    let change = ws
        .set(
            &HostSelector::Name(s("dcaustin-pfsense")),
            &[(s("Port"), s("2222"))],
            false,
            Some(BAK),
        )
        .unwrap();
    assert!(change.warnings.is_empty());
    assert_eq!(
        change.messages,
        vec![format!(
            "dcaustin-pfsense updated in ~/.ssh/config.d/{BAK}."
        )]
    );
    ws.save(WriteOptions::default()).unwrap();
    assert_eq!(
        home.read(&format!(".ssh/config.d/{BAK}")),
        "Host dcaustin-pfsense\n    HostName 10.20.0.99\n    Port 2222\n"
    );
    assert!(home
        .read(".ssh/config.d/df-austin")
        .ends_with("Host dcaustin-pfsense\n    HostName 10.20.0.1\n    User admin\n    Port 22\n"));
}

#[cfg(unix)]
fn make_unreadable(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o000)).unwrap();
}

#[cfg(unix)]
#[test]
fn inc_14_unreadable_include_is_skipped_by_reads_and_refused_for_writes() {
    let home = Home::new();
    home.write(".ssh/config.d/private", "Host secret\n    HostName s\n");
    make_unreadable(&home.path(".ssh/config.d/private"));
    let mut ws = home.load();
    assert_eq!(
        ws.load_warnings(),
        vec!["cannot read ~/.ssh/config.d/private (permission denied); skipped."]
    );
    assert_eq!(ws.list(&env()).len(), 8);
    let err = ws
        .add(
            &AddSpec {
                name: s("x"),
                uri: s("h"),
                ..AddSpec::default()
            },
            Some("private"),
            &env(),
        )
        .unwrap_err();
    assert_eq!(
        err.to_string(),
        "cannot read ~/.ssh/config.d/private (permission denied)."
    );
    assert_eq!(err.exit_code(), 3);
    assert_eq!(ws.delete_all(None).unwrap_err().exit_code(), 3);
    assert!(ws.modified().is_empty());
}

#[cfg(unix)]
#[test]
fn inc_15_check_reports_cross_file_duplicate_backup_include_and_host_star() {
    let home = Home::new();
    home.write(
        ".ssh/config",
        "Include ~/.ssh/config.d/private\n\nHost *\n    ServerAliveInterval 60\n    Include ~/.ssh/config.d/*\n\nHost github\n    HostName github.com\n    User git\n",
    );
    home.write(
        &format!(".ssh/config.d/{BAK}"),
        "Host dcaustin-pfsense\n    HostName 10.20.0.1\n    User admin\n",
    );
    home.write(".ssh/config.d/private", "Host secret\n");
    make_unreadable(&home.path(".ssh/config.d/private"));
    let before = home.snapshot();
    let ws = home.load();
    let report = ws.check(&env());
    let text: String = report
        .problems
        .iter()
        .map(|p| format!("{p}\n"))
        .chain(std::iter::once(format!("{}\n", report.summary())))
        .collect();
    assert_eq!(
        text,
        format!(
            "\
dcaustin-pfsense: defined in ~/.ssh/config.d/df-austin and ~/.ssh/config.d/{BAK}; ssh uses the first
Include ~/.ssh/config.d/*: loads ~/.ssh/config.d/{BAK}, which looks like a backup
Include ~/.ssh/config.d/*: inside Host * in ~/.ssh/config; treated as global
Include ~/.ssh/config.d/private: cannot read (permission denied)
4 problems in 9 hosts.
"
        )
    );
    assert!(!report.is_clean());
    let kinds: Vec<ProblemKind> = report.problems.iter().map(|p| p.kind).collect();
    assert_eq!(
        kinds,
        vec![
            ProblemKind::DuplicateAcrossFiles,
            ProblemKind::IncludeLoadsBackup,
            ProblemKind::IncludeInsideHostStar,
            ProblemKind::IncludeUnreadable
        ]
    );
    let json = serde_json::to_value(&report.problems[0]).unwrap();
    assert_eq!(json["kind"], "duplicate_across_files");
    assert_eq!(
        json["file"],
        home.path(".ssh/config.d/df-austin").display().to_string()
    );
    // check writes nothing.
    home.assert_untouched_except(&before, &[]);
}

#[test]
fn inc_20_combine_ignores_the_workspace() {
    let home = Home::new();
    let ws = home.load();
    assert!(ws.is_multi());
    let inputs = vec![
        CombineInput {
            path: home.path(".ssh/config"),
            config: ConfigFile::load(home.path(".ssh/config")).unwrap().config,
        },
        CombineInput {
            path: home.path(".ssh/config.d/cypress"),
            config: ConfigFile::load(home.path(".ssh/config.d/cypress"))
                .unwrap()
                .config,
        },
    ];
    let (result, report) = combine(inputs, OnConflict::Fail, Some(home.dir.path())).unwrap();
    assert_eq!(result.render(), format!("{ROOT}\n{CYPRESS}"));
    assert_eq!(
        report.summary(None),
        "combined 2 files: 3 hosts, 0 sections, 0 conflicts."
    );
    assert_eq!(report.includes.len(), 1);
    assert_eq!(report.includes[0].pattern, "~/.ssh/config.d/*");
    assert_eq!(report.includes[0].loads, home.path(".ssh/config.d/cypress"));
    // A file combine is not given is not read into the result.
    assert!(!result.render().contains("dcevant"));
}

#[test]
fn inc_23_delete_all_sweeps_every_file_and_backs_each_up() {
    let home = Home::new();
    let before = home.snapshot();
    let mut ws = home.load();
    assert_eq!(ws.delete_all_count(None).unwrap(), (8, 5));
    assert_eq!(
        ws.delete_all_prompt(None).unwrap(),
        "Delete 8 hosts from 5 files? [y/N] "
    );
    let change = ws.delete_all(None).unwrap();
    assert_eq!(change.value, 8);
    assert_eq!(change.messages, vec!["8 hosts deleted from 5 files."]);
    assert_eq!(
        ws.save(WriteOptions::default()).unwrap(),
        vec![0, 1, 2, 3, 4]
    );
    assert_eq!(
        home.read(".ssh/config"),
        "Include ~/.ssh/config.d/*\n\nHost *\n    ServerAliveInterval 60\n"
    );
    assert_eq!(home.read(".ssh/config.d/cypress"), "");
    assert_eq!(
        home.read(".ssh/config.d/df-austin"),
        banner_text("data foundry")
    );
    assert_eq!(home.read(".ssh/config.d/ranch"), "Include ranch.d/*\n");
    assert_eq!(home.read(".ssh/ranch.d/lab"), "");
    for (rel, text) in &before {
        assert_eq!(&home.backup(rel).unwrap(), text, "{rel} backup");
    }
    // --file sweeps one file and names it as on a single file.
    let home = Home::new();
    let mut ws = home.load();
    assert_eq!(
        ws.delete_all_prompt(Some("cypress")).unwrap(),
        format!(
            "Delete 2 hosts from {}? [y/N] ",
            home.path(".ssh/config.d/cypress").display()
        )
    );
    let change = ws.delete_all(Some("cypress")).unwrap();
    assert_eq!(change.messages, vec!["2 hosts deleted."]);
    assert_eq!(ws.modified(), vec![1]);
}

#[test]
fn inc_24_sections_carry_their_file_in_load_order() {
    let home = Home::new();
    home.write(
        ".ssh/config.d/cypress",
        &format!("{}\n{CYPRESS}", banner_text("lab")),
    );
    let ws = home.load();
    let rows = ws.sections();
    let got: Vec<(String, String, usize, bool)> = rows
        .iter()
        .map(|r| (ws.display(r.index), r.name.clone(), r.hosts, r.catch_all))
        .collect();
    assert_eq!(
        got,
        vec![
            (s("~/.ssh/config.d/cypress"), s("lab"), 2, true),
            (s("~/.ssh/config.d/df-austin"), s("data foundry"), 2, true),
        ]
    );
    assert_eq!(
        serde_json::to_string(&rows[1]).unwrap(),
        format!(
            "{{\"name\":\"data foundry\",\"file\":\"{}\",\"hosts\":2,\"catch_all\":true}}",
            home.path(".ssh/config.d/df-austin").display()
        )
    );
    assert_eq!(
        rows[1].summary(),
        SectionSummary {
            name: s("data foundry"),
            hosts: 2,
            catch_all: true
        }
    );
    // A file without sections contributes no rows; none anywhere is empty.
    let home = Home::new();
    home.write(".ssh/config.d/df-austin", "Host db1\n    HostName d\n");
    assert!(home.load().sections().is_empty());
}

#[test]
fn includes_json_lists_each_matched_file_with_nested_includes() {
    let home = Home::new();
    let ws = home.load();
    let json = ws.includes_json();
    let rows = json.as_array().unwrap();
    assert_eq!(rows.len(), 3);
    assert_eq!(rows[0]["pattern"], "~/.ssh/config.d/*");
    assert_eq!(
        rows[0]["from"],
        home.path(".ssh/config").display().to_string()
    );
    assert_eq!(
        rows[0]["file"],
        home.path(".ssh/config.d/cypress").display().to_string()
    );
    assert_eq!(rows[0]["hosts"], 2);
    assert_eq!(rows[2]["nested"][0]["pattern"], "ranch.d/*");
    assert_eq!(rows[2]["nested"][0]["hosts"], 1);
    assert_eq!(ws.include_summary(), (5, 8));
}
