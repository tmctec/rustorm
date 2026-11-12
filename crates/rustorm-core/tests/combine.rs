//! `combine` and `add-section` in rustorm-core: one exact-text test per
//! catalog case combine-1..12 and addsec-1..5 (plan carp, step 2), and
//! cmb-1..9 for per-host decisions and `--retire` (plan coati, step 4).

use std::path::{Path, PathBuf};

use rustorm_core::banner::banner_text;
use rustorm_core::*;

fn cfg(text: &str) -> Config {
    Config::parse(text).unwrap()
}

fn input(path: &str, text: &str) -> CombineInput {
    CombineInput {
        path: PathBuf::from(path),
        config: cfg(text),
    }
}

fn banner(name: &str) -> String {
    format!("{}\n", banner_text(name).trim_end_matches('\n'))
}

fn env() -> Env {
    Env {
        user: Some("tester".into()),
        home: None,
    }
}

fn merge(files: &[(&str, &str)], policy: OnConflict) -> Result<(Config, CombineReport)> {
    combine(
        files.iter().map(|(p, t)| input(p, t)).collect(),
        policy,
        Some(Path::new("/h")),
    )
}

#[test]
fn combine_1_unsectioned_second_joins_the_catch_all_and_everything_is_kept() {
    let base = format!(
        "# defaults\nHost *\n    ServerAliveInterval 30\n\n{}Host a\n    HostName a.example.com\n\nHost b\n    HostName b.example.com\n\n{}# the rest\nHost z\n    HostName z.example.com\n",
        banner("work"),
        banner("other")
    );
    let second =
        "# c box\nHost c\n    HostName c.example.com\n\nHost d\n    HostName d.example.com\n";
    let (result, report) = merge(&[("base", &base), ("second", second)], OnConflict::Fail).unwrap();
    let expected = format!(
        "# defaults\nHost *\n    ServerAliveInterval 30\n\n{}Host a\n    HostName a.example.com\n\nHost b\n    HostName b.example.com\n\n{}# c box\nHost c\n    HostName c.example.com\n\nHost d\n    HostName d.example.com\n\n# the rest\nHost z\n    HostName z.example.com\n",
        banner("work"),
        banner("other")
    );
    assert_eq!(result.render(), expected);
    assert_eq!((report.files, report.hosts, report.sections), (2, 5, 2));
    assert!(report.conflicts.is_empty());
    assert_eq!(
        report.summary(Some(Path::new("/h/.ssh/config"))),
        "combined 2 files into /h/.ssh/config: 5 hosts, 2 sections, 0 conflicts."
    );
    assert_eq!(
        report.summary(None),
        "combined 2 files: 5 hosts, 2 sections, 0 conflicts."
    );
}

#[test]
fn combine_2_duplicate_name_fails_by_default_and_lists_every_duplicate() {
    let base = "Host vps\n    HostName vps.example.com\n\nHost nas\n    HostName nas\n";
    let second = "Host nas\n    HostName nas2\n\nHost vps\n    HostName other.example.com\n";
    let err = merge(&[("base", base), ("second", second)], OnConflict::Fail).unwrap_err();
    assert_eq!(err.exit_code(), 1);
    assert_eq!(
        err.to_string(),
        "2 hosts are defined more than once; nothing written. Use --on-conflict keep or replace.\n  nas: base, second\n  vps: base, second"
    );
    let one = merge(
        &[("base", "Host vps\n"), ("second", "Host vps\n")],
        OnConflict::Fail,
    )
    .unwrap_err();
    assert_eq!(
        one.to_string(),
        "1 host is defined more than once; nothing written. Use --on-conflict keep or replace.\n  vps: base, second"
    );
}

#[test]
fn combine_3_keep_holds_the_base_block_byte_for_byte() {
    let base = "Host vps\n  HostName   vps.example.com\n  User root\n";
    let second = "Host vps\n    HostName other.example.com\n\nHost extra\n    HostName e\n";
    let (result, report) = merge(&[("base", base), ("second", second)], OnConflict::Keep).unwrap();
    assert_eq!(
        result.render(),
        "Host vps\n  HostName   vps.example.com\n  User root\n\nHost extra\n    HostName e\n"
    );
    assert_eq!(report.conflicts.len(), 1);
    assert_eq!(report.conflicts[0].name, "vps");
    assert_eq!(report.conflicts[0].first, PathBuf::from("base"));
    assert_eq!(report.conflicts[0].second, PathBuf::from("second"));
    assert_eq!(
        report.summary(Some(Path::new("base"))),
        "combined 2 files into base: 2 hosts, 0 sections, 1 conflict."
    );
}

#[test]
fn combine_4_replace_puts_the_later_block_in_the_earlier_place() {
    let base = "Host a\n    HostName a\n\nHost vps\n    HostName vps.example.com\n\nHost b\n    HostName b\n";
    let second = "Host vps\n    HostName new.example.com\n    User root\n";
    let (result, report) =
        merge(&[("base", base), ("second", second)], OnConflict::Replace).unwrap();
    assert_eq!(
        result.render(),
        "Host a\n    HostName a\n\nHost vps\n    HostName new.example.com\n    User root\n\nHost b\n    HostName b\n"
    );
    assert_eq!(report.conflicts.len(), 1);
    // In a section the replaced block keeps the section too.
    let base = format!(
        "{}Host vps\n    HostName old\n\n{}Host z\n    HostName z\n",
        banner("work"),
        banner("other")
    );
    let (result, _) = merge(&[("base", &base), ("second", second)], OnConflict::Replace).unwrap();
    assert_eq!(
        result.render(),
        format!(
            "{}Host vps\n    HostName new.example.com\n    User root\n\n{}Host z\n    HostName z\n",
            banner("work"),
            banner("other")
        )
    );
}

#[test]
fn combine_5_same_named_sections_merge_under_one_banner_alphabetically() {
    let base = format!(
        "{}Host b\n    HostName b\n\n{}Host z\n    HostName z\n",
        banner("work"),
        banner("other")
    );
    let second = format!(
        "{}Host a\n    HostName a\n\n{}",
        banner("Work"),
        banner("other")
    );
    let (result, report) =
        merge(&[("base", &base), ("second", &second)], OnConflict::Fail).unwrap();
    assert_eq!(
        result.render(),
        format!(
            "{}Host a\n    HostName a\n\nHost b\n    HostName b\n\n{}Host z\n    HostName z\n",
            banner("work"),
            banner("other")
        )
    );
    assert_eq!(report.sections, 2);
    assert_eq!(result.sections().len(), 2);
}

#[test]
fn combine_6_defaults_keep_base_keys_gain_absent_ones_and_report_both() {
    let base = "Host *\n    ServerAliveInterval 30\n\nHost a\n    HostName a\n";
    let second =
        "Host *\n    ServerAliveInterval 60\n    Compression yes\n\nHost b\n    HostName b\n";
    let (result, report) = merge(&[("base", base), ("second", second)], OnConflict::Fail).unwrap();
    assert_eq!(
        result.render(),
        "Host *\n    ServerAliveInterval 30\n    Compression yes\n\nHost a\n    HostName a\n\nHost b\n    HostName b\n"
    );
    assert_eq!(
        report.added,
        vec![DefaultsAdded {
            key: "Compression".into(),
            value: "yes".into(),
            from: PathBuf::from("second"),
        }]
    );
    assert_eq!(
        report.skipped,
        vec![DefaultsSkipped {
            key: "ServerAliveInterval".into(),
            value: "60".into(),
            from: PathBuf::from("second"),
            kept: "30".into(),
        }]
    );
    // A base without `Host *` takes the later block whole.
    let (result, report) = merge(
        &[("base", "Host a\n    HostName a\n"), ("second", second)],
        OnConflict::Fail,
    )
    .unwrap();
    assert_eq!(
        result.render(),
        "Host a\n    HostName a\n\nHost b\n    HostName b\n\nHost *\n    ServerAliveInterval 60\n    Compression yes\n"
    );
    assert!(report.added.is_empty() && report.skipped.is_empty());
}

#[test]
fn combine_7_include_lines_stay_and_a_matching_one_is_warned_about() {
    let base = "Include config.d/*\n\nHost a\n    HostName a\n";
    let second = "Host cy\n    HostName cy\n";
    let (result, report) = merge(
        &[
            ("/h/.ssh/config", base),
            ("/h/.ssh/config.d/cypress", second),
        ],
        OnConflict::Fail,
    )
    .unwrap();
    assert_eq!(
        result.render(),
        "Include config.d/*\n\nHost a\n    HostName a\n\nHost cy\n    HostName cy\n"
    );
    assert_eq!(
        report.includes,
        vec![IncludeWarning {
            pattern: "config.d/*".into(),
            file: PathBuf::from("/h/.ssh/config"),
            loads: PathBuf::from("/h/.ssh/config.d/cypress"),
        }]
    );
    // No warning when the pattern loads none of the inputs.
    let (_, report) = merge(
        &[
            ("/h/.ssh/config", base),
            ("/h/.ssh/elsewhere/cypress", second),
        ],
        OnConflict::Fail,
    )
    .unwrap();
    assert!(report.includes.is_empty());
}

#[test]
fn combine_8_merging_touches_no_input_file() {
    let dir = tempfile::tempdir().unwrap();
    let a = dir.path().join("a");
    let b = dir.path().join("b");
    std::fs::write(&a, "Host a\n    HostName a\n").unwrap();
    std::fs::write(&b, "Host b\n    HostName b\n").unwrap();
    let load = |p: &Path| CombineInput {
        path: p.to_path_buf(),
        config: ConfigFile::load(p).unwrap().config,
    };
    let (result, _) = combine(vec![load(&a), load(&b)], OnConflict::Fail, None).unwrap();
    assert_eq!(
        result.render(),
        "Host a\n    HostName a\n\nHost b\n    HostName b\n"
    );
    assert_eq!(
        std::fs::read_to_string(&a).unwrap(),
        "Host a\n    HostName a\n"
    );
    assert_eq!(
        std::fs::read_to_string(&b).unwrap(),
        "Host b\n    HostName b\n"
    );
    assert!(!backup_path(&a).exists() && !backup_path(&b).exists());
}

#[test]
fn combine_9_writing_elsewhere_backs_up_only_an_existing_target() {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("other.conf");
    let (result, _) = merge(&[("a", "Host a\n"), ("b", "Host b\n")], OnConflict::Fail).unwrap();
    write_text(&out, &result.render(), WriteOptions::default()).unwrap();
    assert_eq!(std::fs::read_to_string(&out).unwrap(), "Host a\n\nHost b\n");
    assert!(!backup_path(&out).exists());
    write_text(&out, "Host c\n", WriteOptions::default()).unwrap();
    assert_eq!(
        std::fs::read_to_string(backup_path(&out)).unwrap(),
        "Host a\n\nHost b\n"
    );
}

#[test]
fn combine_10_one_file_is_a_usage_error() {
    let err = merge(&[("only", "Host a\n")], OnConflict::Fail).unwrap_err();
    assert_eq!(err.to_string(), "combine needs at least two files.");
    assert_eq!(err.exit_code(), 2);
    assert_eq!("fail".parse::<OnConflict>().unwrap(), OnConflict::Fail);
    assert_eq!("keep".parse::<OnConflict>().unwrap(), OnConflict::Keep);
    assert_eq!(
        "replace".parse::<OnConflict>().unwrap(),
        OnConflict::Replace
    );
    assert_eq!("drop".parse::<OnConflict>().unwrap_err().exit_code(), 2);
}

#[test]
fn combine_11_an_unreadable_input_is_a_read_error_with_exit_3() {
    let dir = tempfile::tempdir().unwrap();
    let err = ConfigFile::load(dir.path()).unwrap_err();
    assert!(matches!(err, Error::Read { .. }));
    assert_eq!(err.exit_code(), 3);
    assert!(err
        .to_string()
        .starts_with(&format!("cannot read {}: ", dir.path().display())));
}

#[test]
fn combine_12_three_files_merge_in_order_and_a_late_duplicate_is_reported() {
    let files = [
        ("f1", "Host a\n    HostName a\n"),
        ("f2", "Host b\n    HostName b\n"),
        ("f3", "Host b\n    HostName b3\n\nHost c\n    HostName c\n"),
    ];
    let err = merge(&files, OnConflict::Fail).unwrap_err();
    assert_eq!(
        err.to_string(),
        "1 host is defined more than once; nothing written. Use --on-conflict keep or replace.\n  b: f2, f3"
    );
    let (result, report) = merge(&files, OnConflict::Keep).unwrap();
    assert_eq!(
        result.render(),
        "Host a\n    HostName a\n\nHost b\n    HostName b\n\nHost c\n    HostName c\n"
    );
    assert_eq!(report.files, 3);
    assert_eq!(report.hosts, 3);
    assert_eq!(report.conflicts[0].first, PathBuf::from("f2"));
    let (result, _) = merge(&files, OnConflict::Replace).unwrap();
    assert_eq!(
        result.render(),
        "Host a\n    HostName a\n\nHost b\n    HostName b3\n\nHost c\n    HostName c\n"
    );
}

#[test]
fn addsec_1_first_section_on_an_unsectioned_file_creates_the_catch_all() {
    let mut c = cfg("Host a\n    HostName a\n\nHost b\n    HostName b\n");
    let added = c.add_section("work", None).unwrap();
    assert_eq!(
        added,
        SectionAdded {
            name: "work".into(),
            before: None,
            catch_all: Some(("other".into(), 2)),
        }
    );
    assert_eq!(
        c.render(),
        format!(
            "{}\n{}\nHost a\n    HostName a\n\nHost b\n    HostName b\n",
            banner("work"),
            banner("other")
        )
    );
    let summary: Vec<(String, usize, bool)> = c
        .sections()
        .into_iter()
        .map(|s| (s.name, s.hosts, s.catch_all))
        .collect();
    assert_eq!(
        summary,
        vec![("work".into(), 0, false), ("other".into(), 2, true)]
    );
}

#[test]
fn addsec_2_new_section_goes_before_the_catch_all_empty() {
    let mut c = cfg(&format!(
        "{}Host a\n    HostName a\n\n{}Host z\n    HostName z\n",
        banner("work"),
        banner("other")
    ));
    let added = c.add_section("lab", None).unwrap();
    assert_eq!(added.catch_all, None);
    assert_eq!(added.before, None);
    assert_eq!(
        c.render(),
        format!(
            "{}Host a\n    HostName a\n\n{}\n{}Host z\n    HostName z\n",
            banner("work"),
            banner("lab"),
            banner("other")
        )
    );
}

#[test]
fn addsec_3_existing_name_is_refused_and_the_file_is_unchanged() {
    let text = format!(
        "{}Host a\n    HostName a\n\n{}Host z\n    HostName z\n",
        banner("work"),
        banner("other")
    );
    let mut c = cfg(&text);
    let err = c.add_section("Work", None).unwrap_err();
    assert_eq!(err.to_string(), "section Work already exists.");
    assert_eq!(err.exit_code(), 1);
    assert_eq!(c.render(), text);
    assert_eq!(
        Error::SectionExists("lab".into()).to_string(),
        "section lab already exists."
    );
}

#[test]
fn addsec_4_before_puts_the_section_in_front_of_the_named_one() {
    let mut c = cfg(&format!(
        "{}Host a\n    HostName a\n\n{}Host l\n    HostName l\n\n{}Host z\n    HostName z\n",
        banner("work"),
        banner("lab"),
        banner("other")
    ));
    let added = c.add_section("home", Some("LAB")).unwrap();
    assert_eq!(added.before.as_deref(), Some("lab"));
    assert_eq!(
        c.render(),
        format!(
            "{}Host a\n    HostName a\n\n{}\n{}Host l\n    HostName l\n\n{}Host z\n    HostName z\n",
            banner("work"),
            banner("home"),
            banner("lab"),
            banner("other")
        )
    );
    let names: Vec<String> = c.sections().into_iter().map(|s| s.name).collect();
    assert_eq!(names, ["work", "home", "lab", "other"]);
    let err = c.add_section("x", Some("nope")).unwrap_err();
    assert_eq!(err.to_string(), "section nope does not exist.");
}

#[test]
fn addsec_5_an_empty_section_survives_a_write_and_takes_its_first_host() {
    let mut c = cfg("Host z\n    HostName z\n");
    c.add_section("work", None).unwrap();
    // The intermediate write: render, then parse what was written.
    let mut c = cfg(&c.render());
    let placed = c
        .add(
            &AddSpec {
                name: "x".into(),
                uri: "u@h".into(),
                identity: None,
                options: vec![],
                section: Some("work".into()),
            },
            &env(),
        )
        .unwrap();
    assert_eq!(placed.section.as_deref(), Some("work"));
    assert_eq!(
        c.render(),
        format!(
            "{}\nHost x\n    HostName h\n    User u\n    Port 22\n\n{}\nHost z\n    HostName z\n",
            banner("work"),
            banner("other")
        )
    );
}

// ----- per-host decisions and --retire: cmb-1..cmb-9 (plan coati, step 4) -----

fn decisions(keep: &[&str], replace: &[&str]) -> HostDecisions {
    let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
    host_decisions(&s(keep), &s(replace)).unwrap()
}

/// A home with `~/.ssh/config` (`Include config.d/*`, host `base`) and
/// `~/.ssh/config.d/extra` (host `extra`).
struct Home {
    dir: tempfile::TempDir,
}

const HOME_BASE: &str = "Include config.d/*\n\nHost base\n    HostName base.example.com\n";
const HOME_EXTRA: &str = "Host extra\n    HostName extra.example.com\n";

impl Home {
    fn new() -> Home {
        let h = Home {
            dir: tempfile::tempdir().unwrap(),
        };
        h.write(".ssh/config", HOME_BASE);
        h.write(".ssh/config.d/extra", HOME_EXTRA);
        h
    }

    fn path(&self, rel: &str) -> PathBuf {
        self.dir.path().join(rel)
    }

    fn write(&self, rel: &str, text: &str) {
        let p = self.path(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }

    fn read(&self, rel: &str) -> String {
        std::fs::read_to_string(self.path(rel)).unwrap()
    }

    fn request(&self, inputs: &[&str], retire: bool) -> CombineRequest {
        CombineRequest {
            inputs: inputs.iter().map(|r| self.path(r)).collect(),
            retire,
            ..CombineRequest::default()
        }
    }

    fn commit(&self, req: &CombineRequest) -> Result<CombineOutcome> {
        req.commit(None, WriteOptions::default(), Some(self.dir.path()))
    }
}

const TWO_A: &str = "Host a\n    HostName a1\n\nHost b\n    HostName b1\n";
const TWO_B: &str = "Host a\n    HostName a2\n\nHost b\n    HostName b2\n";

// TESTMASTER: id=cmb-1 tier=? parallel=yes
#[test]
fn cmb_1_keep_and_replace_override_the_policy_per_host() {
    let inputs = vec![input("one", TWO_A), input("two", TWO_B)];
    let (result, report) =
        combine_with(inputs, OnConflict::Fail, &decisions(&["a"], &["b"]), None).unwrap();
    assert_eq!(
        result.render(),
        "Host a\n    HostName a1\n\nHost b\n    HostName b2\n"
    );
    assert_eq!(report.conflicts.len(), 2);
    // A per-host decision wins over --on-conflict replace too.
    let inputs = vec![input("one", TWO_A), input("two", TWO_B)];
    let (result, _) =
        combine_with(inputs, OnConflict::Replace, &decisions(&["a"], &[]), None).unwrap();
    assert_eq!(
        result.render(),
        "Host a\n    HostName a1\n\nHost b\n    HostName b2\n"
    );
    // The preview shows both blocks and each decision.
    let h = Home::new();
    h.write("one", TWO_A);
    h.write("two", TWO_B);
    let req = CombineRequest {
        decisions: decisions(&["a"], &[]),
        ..h.request(&["one", "two"], false)
    };
    let p = req.preview(None).unwrap();
    assert_eq!(p.conflicts.len(), 2);
    assert_eq!(
        (
            p.conflicts[0].name.as_str(),
            p.conflicts[0].decision,
            p.conflicts[0].explicit
        ),
        ("a", Some(HostDecision::Keep), true)
    );
    assert_eq!(p.conflicts[0].second_text, "Host a\n    HostName a2\n");
    assert_eq!(p.undecided().len(), 1);
    assert!(!p.is_ready());
    assert_eq!(p.fingerprints.len(), 2);
    // A changed input refuses the commit as stale; nothing is written.
    h.write("two", "Host a\n    HostName a3\n");
    let full = CombineRequest {
        decisions: decisions(&["a"], &["b"]),
        ..h.request(&["one", "two"], false)
    };
    let err = full
        .commit(Some(&p.fingerprints), WriteOptions::default(), None)
        .unwrap_err();
    assert_eq!(err.exit_code(), 1);
    assert!(err
        .to_string()
        .ends_with("two changed since the preview; preview again."));
    assert_eq!(h.read("one"), TWO_A);
}

// TESTMASTER: id=cmb-2 tier=? parallel=yes
#[test]
fn cmb_2_an_unnamed_conflict_under_fail_writes_nothing() {
    let h = Home::new();
    h.write("one", TWO_A);
    h.write("two", TWO_B);
    let req = CombineRequest {
        decisions: decisions(&["a"], &[]),
        ..h.request(&["one", "two"], false)
    };
    let err = h.commit(&req).unwrap_err();
    assert_eq!(err.exit_code(), 1);
    assert_eq!(
        err.to_string(),
        format!(
            "1 conflict is left undecided; nothing written. Name it with --keep or --replace, or use --on-conflict keep or replace.\n  b: {}, {}",
            h.path("one").display(),
            h.path("two").display()
        )
    );
    assert_eq!(h.read("one"), TWO_A);
    assert!(!h.path("one~").exists());
}

// TESTMASTER: id=cmb-3 tier=? parallel=yes
#[test]
fn cmb_3_a_named_host_that_is_no_conflict_is_a_usage_error() {
    let inputs = vec![input("one", "Host nas\n"), input("two", "Host vps\n")];
    let err = combine_with(inputs, OnConflict::Fail, &decisions(&["nas"], &[]), None).unwrap_err();
    assert_eq!(err.exit_code(), 2);
    assert_eq!(
        err.to_string(),
        "nas is not a conflict; --keep and --replace name a host two files define."
    );
    let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
    let twice = host_decisions(&s(&["a"]), &s(&["a"])).unwrap_err();
    assert_eq!(
        (twice.exit_code(), twice.to_string()),
        (2, "a is decided twice.".to_string())
    );
}

// TESTMASTER: id=cmb-4 tier=? parallel=yes
#[test]
#[cfg(unix)]
fn cmb_4_retire_moves_an_included_input_into_a_private_retired_dir() {
    use std::os::unix::fs::PermissionsExt;
    let h = Home::new();
    let out = h
        .commit(&h.request(&[".ssh/config", ".ssh/config.d/extra"], true))
        .unwrap();
    assert_eq!(h.read(".ssh/config"), format!("{HOME_BASE}\n{HOME_EXTRA}"));
    let to = h.path(".ssh/retired/extra");
    assert_eq!(
        out.retired,
        [CombineRetired {
            from: h.path(".ssh/config.d/extra"),
            to: Some(to.clone())
        }]
    );
    assert_eq!(
        out.retired[0].message(),
        format!(
            "{} retired to {}.",
            h.path(".ssh/config.d/extra").display(),
            to.display()
        )
    );
    assert!(!h.path(".ssh/config.d/extra").exists());
    assert_eq!(std::fs::read_to_string(&to).unwrap(), HOME_EXTRA);
    let mode = std::fs::metadata(h.path(".ssh/retired"))
        .unwrap()
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o700);
    assert_eq!(h.read(".ssh/config~"), HOME_BASE);
}

// TESTMASTER: id=cmb-5 tier=? parallel=yes
#[test]
fn cmb_5_without_retire_no_file_moves() {
    let h = Home::new();
    let out = h
        .commit(&h.request(&[".ssh/config", ".ssh/config.d/extra"], false))
        .unwrap();
    assert!(out.retired.is_empty());
    assert_eq!(h.read(".ssh/config.d/extra"), HOME_EXTRA);
    assert!(!h.path(".ssh/retired").exists());
}

// TESTMASTER: id=cmb-6 tier=? parallel=yes
#[test]
fn cmb_6_an_input_no_include_loads_stays_and_says_so() {
    let h = Home::new();
    h.write("old/hosts", "Host old\n    HostName old.example.com\n");
    let out = h
        .commit(&h.request(&[".ssh/config", "old/hosts", ".ssh/config.d/extra"], true))
        .unwrap();
    assert_eq!(out.retired.len(), 2);
    assert_eq!(out.retired[0].to, None);
    assert_eq!(
        out.retired[0].message(),
        format!(
            "{}: no Include loads it; not retired.",
            h.path("old/hosts").display()
        )
    );
    assert!(h.path("old/hosts").exists());
    assert_eq!(out.retired[1].to, Some(h.path(".ssh/retired/extra")));
}

// TESTMASTER: id=cmb-7 tier=? parallel=no
#[test]
#[cfg(unix)]
fn cmb_7_a_failed_write_retires_nothing() {
    use std::os::unix::fs::PermissionsExt;
    let h = Home::new();
    std::fs::create_dir_all(h.path("ro")).unwrap();
    std::fs::set_permissions(h.path("ro"), std::fs::Permissions::from_mode(0o555)).unwrap();
    let req = CombineRequest {
        output: Some(h.path("ro/out")),
        ..h.request(&[".ssh/config", ".ssh/config.d/extra"], true)
    };
    let err = h.commit(&req).unwrap_err();
    std::fs::set_permissions(h.path("ro"), std::fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(err.exit_code(), 3);
    assert_eq!(h.read(".ssh/config.d/extra"), HOME_EXTRA);
    assert!(!h.path(".ssh/retired").exists());
    assert!(!h.path("ro/out").exists());
}

// TESTMASTER: id=cmb-8 tier=? parallel=yes
#[test]
fn cmb_8_retire_with_stdout_is_a_usage_error() {
    let h = Home::new();
    let err = h
        .request(&[".ssh/config", ".ssh/config.d/extra"], true)
        .render(Some(h.dir.path()))
        .unwrap_err();
    assert_eq!(err.exit_code(), 2);
    assert_eq!(
        err.to_string(),
        "--retire moves files after a write; drop --stdout."
    );
    assert_eq!(h.read(".ssh/config.d/extra"), HOME_EXTRA);
}

// TESTMASTER: id=cmb-9 tier=? parallel=yes
#[test]
fn cmb_9_a_taken_retired_name_gets_a_numeric_suffix() {
    let h = Home::new();
    h.write(".ssh/retired/extra", "older\n");
    let out = h
        .commit(&h.request(&[".ssh/config", ".ssh/config.d/extra"], true))
        .unwrap();
    assert_eq!(out.retired[0].to, Some(h.path(".ssh/retired/extra.1")));
    assert_eq!(h.read(".ssh/retired/extra"), "older\n");
    assert_eq!(h.read(".ssh/retired/extra.1"), HOME_EXTRA);
}
