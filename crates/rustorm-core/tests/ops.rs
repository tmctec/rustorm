//! One test per rustorm operation, asserting the resulting file text
//! exactly (step 4 of plan basilisk; catalog cases cli-<cmd>-1).

use rustorm_core::banner::banner_text;
use rustorm_core::*;

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

fn env() -> Env {
    Env {
        user: Some("tester".into()),
        home: None,
    }
}

fn cfg(text: &str) -> Config {
    Config::parse(text).unwrap()
}

fn s(v: &str) -> String {
    v.to_string()
}

fn pairs(v: &[(&str, &str)]) -> Vec<(String, String)> {
    v.iter().map(|(k, v)| (s(k), s(v))).collect()
}

/// preamble, "data foundry" (db1), "bob" (box), "other" (github, vps).
fn sectioned() -> String {
    format!(
        "# preamble\n\nHost *\n    User emre\n\n{}\nHost db1\n    HostName db1.example.com\n\n{}\nHost box\n    HostName box.x\n\n{}\nHost github\n    HostName github.com\n    User git\n\nHost vps\n    HostName vps.example.com\n",
        banner_text("data foundry"),
        banner_text("bob"),
        banner_text("other")
    )
}

const RAILS01: &str = "Host rails01\n    HostName rails01.example.com\n    User deploy\n\n";

#[test]
fn add_appends_before_trailing_defaults_with_uri_identity_and_options() {
    let mut c = cfg(BASE);
    let placed = c
        .add(
            &AddSpec {
                name: s("web-prod"),
                uri: s("web@webprod.example.com"),
                identity: Some(s("~/.ssh/prod.pem")),
                options: vec![
                    parse_option("StrictHostKeyChecking=no").unwrap(),
                    parse_option("ProxyCommand=ssh -W %h:%p bastion").unwrap(),
                ],
                section: None,
            },
            &env(),
        )
        .unwrap();
    assert_eq!(
        placed,
        Placed {
            name: s("web-prod"),
            section: None
        }
    );
    let expected = BASE.replace(
        RAILS01,
        &format!("{RAILS01}Host web-prod\n    HostName webprod.example.com\n    User web\n    Port 22\n    IdentityFile ~/.ssh/prod.pem\n    StrictHostKeyChecking no\n    ProxyCommand ssh -W %h:%p bastion\n\n"),
    );
    assert_eq!(c.render(), expected);
}

#[test]
fn add_resolves_user_and_port_from_defaults_then_env() {
    let mut c = cfg(BASE);
    c.add(
        &AddSpec {
            name: s("x"),
            uri: s("x.example.com"),
            ..Default::default()
        },
        &env(),
    )
    .unwrap();
    let x = c.host(c.find_host("x").unwrap());
    assert_eq!(x.get("User").as_deref(), Some("emre"));
    assert_eq!(x.get("Port").as_deref(), Some("22"));
    let mut c = cfg("Host *\n    Port 2200\n");
    c.add(
        &AddSpec {
            name: s("y"),
            uri: s("y.example.com"),
            ..Default::default()
        },
        &env(),
    )
    .unwrap();
    assert_eq!(
        c.render(),
        "Host y\n    HostName y.example.com\n    User tester\n    Port 2200\n\nHost *\n    Port 2200\n"
    );
    let mut empty = Config::default();
    empty
        .add(
            &AddSpec {
                name: s("z"),
                uri: s("root@z:2222"),
                ..Default::default()
            },
            &env(),
        )
        .unwrap();
    assert_eq!(
        empty.render(),
        "Host z\n    HostName z\n    User root\n    Port 2222\n"
    );
}

#[test]
fn add_refuses_existing_name_and_bad_input() {
    let mut c = cfg(BASE);
    let err = c
        .add(
            &AddSpec {
                name: s("vps"),
                uri: s("root@vps.example.com"),
                ..Default::default()
            },
            &env(),
        )
        .unwrap_err();
    assert_eq!(
        err.to_string(),
        "vps already exists. Use rustorm edit or rustorm set to modify it."
    );
    assert_eq!(err.exit_code(), 1);
    for bad in ["a b", "a@b", "*", ""] {
        let e = c
            .add(
                &AddSpec {
                    name: s(bad),
                    uri: s("h"),
                    ..Default::default()
                },
                &env(),
            )
            .unwrap_err();
        assert!(matches!(e, Error::InvalidName(_)), "{bad}");
    }
    let e = c
        .add(
            &AddSpec {
                name: s("n"),
                uri: s("h:abc"),
                ..Default::default()
            },
            &env(),
        )
        .unwrap_err();
    assert!(matches!(e, Error::InvalidUri { .. }));
    assert_eq!(c.render(), BASE, "refused adds change nothing");
}

#[test]
fn add_with_section_on_unsectioned_file_creates_section_and_catch_all() {
    let mut c = cfg(BASE);
    let placed = c
        .add(
            &AddSpec {
                name: s("db1"),
                uri: s("postgres@db1.example.com"),
                section: Some(s("data foundry")),
                ..Default::default()
            },
            &env(),
        )
        .unwrap();
    assert_eq!(placed.section.as_deref(), Some("data foundry"));
    let expected = format!(
        "{}\nHost db1\n    HostName db1.example.com\n    User postgres\n    Port 22\n\n{}\n{RAILS01}# main box\nHost vps\n    HostName vps.example.com\n    User root\n    Port 2222\n\nHost *\n    User emre\n    ServerAliveInterval 60\n",
        banner_text("data foundry"),
        banner_text("other"),
    );
    assert_eq!(c.render(), expected);
    // Without --section the next host goes to the catch-all, sorted.
    let placed = c
        .add(
            &AddSpec {
                name: s("alpha"),
                uri: s("a@alpha"),
                ..Default::default()
            },
            &env(),
        )
        .unwrap();
    assert_eq!(placed.section.as_deref(), Some("other"));
    let names: Vec<String> = c.sections[1].hosts().map(HostBlock::primary).collect();
    assert_eq!(names, ["alpha", "rails01", "vps", "*"]);
}

#[test]
fn edit_replaces_uri_keys_in_place() {
    let mut c = cfg(BASE);
    c.edit(
        &EditSpec {
            name: s("vps"),
            uri: s("deploy@vps.example.com:2400"),
            ..Default::default()
        },
        &env(),
    )
    .unwrap();
    assert_eq!(
        c.render(),
        BASE.replace(
            "    User root\n    Port 2222\n",
            "    User deploy\n    Port 2400\n"
        )
    );
    let err = c
        .edit(
            &EditSpec {
                name: s("nope"),
                uri: s("emre@vps.example.com"),
                ..Default::default()
            },
            &env(),
        )
        .unwrap_err();
    assert_eq!(
        err.to_string(),
        "nope does not exist. Use rustorm add to create it."
    );
    assert_eq!(err.exit_code(), 1);
}

fn vps_farm() -> String {
    (1..=10)
        .map(|i| format!("Host vps-{i}\n    HostName 10.0.0.{i}\n"))
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn set_regex_is_anchored_to_the_whole_name() {
    let text = vps_farm();
    let mut c = cfg(&text);
    let names = c
        .set(
            &HostSelector::Regex(s("vps-[1-5]")),
            &pairs(&[("user", "emre")]),
            false,
        )
        .unwrap();
    assert_eq!(names, ["vps-1", "vps-2", "vps-3", "vps-4", "vps-5"]);
    let mut expected = text.clone();
    for i in 1..=5 {
        expected = expected.replace(
            &format!("    HostName 10.0.0.{i}\n"),
            &format!("    HostName 10.0.0.{i}\n    User emre\n"),
        );
    }
    assert_eq!(c.render(), expected);
    assert!(!c.render().contains("HostName 10.0.0.10\n    User"));
    let err = c
        .set(
            &HostSelector::Regex(s("nomatch-.*")),
            &pairs(&[("User", "x")]),
            false,
        )
        .unwrap_err();
    assert_eq!(err.to_string(), "no host matches nomatch-.*");
    assert_eq!(err.exit_code(), 1);
    let err = c
        .set(
            &HostSelector::Regex(s("(")),
            &pairs(&[("User", "x")]),
            false,
        )
        .unwrap_err();
    assert!(matches!(err, Error::InvalidPattern { .. }));
}

#[test]
fn set_replaces_and_appends_multi_valued_keys() {
    let mut c = cfg("Host vps\n    HostName v\n    IdentityFile ~/.ssh/a\n    IdentityFile ~/.ssh/b\n    User root\n");
    c.set(
        &HostSelector::Name(s("vps")),
        &pairs(&[("User", "deploy"), ("Port", "22")]),
        false,
    )
    .unwrap();
    assert_eq!(
        c.render(),
        "Host vps\n    HostName v\n    IdentityFile ~/.ssh/a\n    IdentityFile ~/.ssh/b\n    User deploy\n    Port 22\n"
    );
    c.set(
        &HostSelector::Name(s("vps")),
        &pairs(&[("IdentityFile", "~/.ssh/second.pem")]),
        true,
    )
    .unwrap();
    assert_eq!(
        c.render(),
        "Host vps\n    HostName v\n    IdentityFile ~/.ssh/a\n    IdentityFile ~/.ssh/b\n    IdentityFile ~/.ssh/second.pem\n    User deploy\n    Port 22\n"
    );
    c.set(
        &HostSelector::Name(s("vps")),
        &pairs(&[("identityfile", "~/.ssh/only")]),
        false,
    )
    .unwrap();
    assert_eq!(
        c.render(),
        "Host vps\n    HostName v\n    IdentityFile ~/.ssh/only\n    User deploy\n    Port 22\n"
    );
    assert_eq!(pair_up(&[s("User")]).unwrap_err().exit_code(), 2);
    let e = c
        .set(
            &HostSelector::Name(s("nope")),
            &pairs(&[("User", "x")]),
            false,
        )
        .unwrap_err();
    assert_eq!(e.to_string(), "nope does not exist.");
}

#[test]
fn unset_removes_keys_and_ignores_absent_ones() {
    let mut c = cfg(
        "Host vps\n    HostName v\n    IdentityFile a\n    ProxyCommand p\n    IdentityFile b\n",
    );
    let names = c
        .unset(
            &HostSelector::Name(s("vps")),
            &[s("IdentityFile"), s("proxycommand"), s("Port")],
        )
        .unwrap();
    assert_eq!(names, ["vps"]);
    assert_eq!(c.render(), "Host vps\n    HostName v\n");
}

#[test]
fn clone_rewrites_hostname_by_default() {
    let mut c = cfg(BASE);
    let placed = c
        .clone_host(&CloneSpec {
            source: s("rails01"),
            new_name: s("rails02"),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(placed.name, "rails02");
    let expected = BASE.replace(
        RAILS01,
        &format!("{RAILS01}Host rails02\n    HostName rails02.example.com\n    User deploy\n\n"),
    );
    assert_eq!(c.render(), expected);
    let err = c
        .clone_host(&CloneSpec {
            source: s("rails01"),
            new_name: s("rails02"),
            ..Default::default()
        })
        .unwrap_err();
    assert_eq!(err.to_string(), "rails02 already exists.");
    assert_eq!(err.exit_code(), 1);
}

#[test]
fn clone_keep_hostname_and_overrides() {
    let mut c = cfg(BASE);
    c.clone_host(&CloneSpec {
        source: s("rails01"),
        new_name: s("rails02"),
        keep_hostname: true,
        ..Default::default()
    })
    .unwrap();
    c.clone_host(&CloneSpec {
        source: s("rails01"),
        new_name: s("rails03"),
        overrides: pairs(&[("HostName", "rails-03.example.com"), ("User", "dbrady")]),
        ..Default::default()
    })
    .unwrap();
    let expected = BASE.replace(
        RAILS01,
        &format!("{RAILS01}Host rails02\n    HostName rails01.example.com\n    User deploy\n\nHost rails03\n    HostName rails-03.example.com\n    User dbrady\n\n"),
    );
    assert_eq!(c.render(), expected);
}

#[test]
fn clone_goes_to_source_section_sorted() {
    let mut c = cfg(&sectioned());
    let placed = c
        .clone_host(&CloneSpec {
            source: s("db1"),
            new_name: s("db0"),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(placed.section.as_deref(), Some("data foundry"));
    let expected = sectioned().replace(
        "Host db1\n",
        "Host db0\n    HostName db0.example.com\n\nHost db1\n",
    );
    assert_eq!(c.render(), expected);
}

#[test]
fn move_renames_and_changes_section() {
    let mut c = cfg(BASE);
    let m = c.move_host("rails01", Some("staging"), None).unwrap();
    assert_eq!(
        (m.old_name.as_str(), m.new_name.as_str(), m.section),
        ("rails01", "staging", None)
    );
    assert_eq!(c.render(), BASE.replace("Host rails01\n", "Host staging\n"));
    assert_eq!(c.move_host("vps", None, None).unwrap_err().exit_code(), 2);
    assert_eq!(
        c.move_host("vps", None, None).unwrap_err().to_string(),
        "give a new name, a --section, or both."
    );
    assert_eq!(
        c.move_host("vps", Some("staging"), None)
            .unwrap_err()
            .to_string(),
        "staging already exists."
    );

    let mut c = cfg(&sectioned());
    let m = c.move_host("vps", Some("vps2"), Some("bob")).unwrap();
    assert_eq!(m.section.as_deref(), Some("bob"));
    let expected = sectioned()
        .replace(
            "Host box\n    HostName box.x\n\n",
            "Host box\n    HostName box.x\n\nHost vps2\n    HostName vps.example.com\n\n",
        )
        .replace(
            "    User git\n\nHost vps\n    HostName vps.example.com\n",
            "    User git\n",
        );
    assert_eq!(c.render(), expected);
}

#[test]
fn delete_removes_entries_with_their_comments() {
    let mut c = cfg(BASE);
    assert_eq!(
        c.delete(&[s("vps"), s("rails01")]).unwrap(),
        ["vps", "rails01"]
    );
    assert_eq!(
        c.render(),
        "Host *\n    User emre\n    ServerAliveInterval 60\n"
    );
}

#[test]
fn delete_refuses_on_any_missing_name_without_writing() {
    let mut c = cfg(BASE);
    let err = c.delete(&[s("vps"), s("nope")]).unwrap_err();
    assert_eq!(err.to_string(), "nope does not exist.");
    assert_eq!(err.exit_code(), 1);
    assert_eq!(c.render(), BASE);
}

#[test]
fn delete_all_keeps_banners_comments_and_defaults() {
    let mut c = cfg(&sectioned());
    assert_eq!(c.host_count(), 4);
    assert_eq!(c.delete_all(), 4);
    assert_eq!(
        c.render(),
        format!(
            "# preamble\n\nHost *\n    User emre\n\n{}\n{}\n{}",
            banner_text("data foundry"),
            banner_text("bob"),
            banner_text("other")
        )
    );
    let mut c = cfg(BASE);
    assert_eq!(c.delete_all(), 2);
    assert_eq!(
        c.render(),
        "# main box\nHost *\n    User emre\n    ServerAliveInterval 60\n"
    );
    assert_eq!(
        Error::RefuseDeleteAll(14).to_string(),
        "refusing to delete 14 hosts without --yes on a non-interactive terminal."
    );
}

#[test]
fn list_resolves_defaults_and_groups_by_section() {
    let c = cfg(&format!(
        "{}Host bare\n    ProxyJump bastion\n",
        sectioned()
    ));
    let rows = c.list(&env());
    let lines: Vec<(Option<&str>, String)> = rows
        .iter()
        .map(|r| (r.section.as_deref(), r.line()))
        .collect();
    assert_eq!(
        lines,
        vec![
            (Some("data foundry"), s("db1 -> emre@db1.example.com:22")),
            (Some("bob"), s("box -> emre@box.x:22")),
            (Some("other"), s("bare -> [no hostname]")),
            (Some("other"), s("github -> git@github.com:22")),
            (Some("other"), s("vps -> emre@vps.example.com:22")),
        ]
    );
    assert_eq!(rows[2].proxy_jump.as_deref(), Some("bastion"));
    let unsectioned =
        cfg("Host b\n    HostName b\nHost a\n    HostName a\n    Port 2222\n").list(&env());
    assert_eq!(unsectioned[0].line(), "a -> tester@a:2222");
    assert_eq!(unsectioned[1].section, None);
}

#[test]
fn list_row_serializes_with_documented_field_names() {
    let c = cfg("Host db1 d\n    HostName db1.example.com\n    User postgres\n    IdentityFile a\n    IdentityFile b\n    ProxyCommand ssh -W %h:%p x\n");
    let json = serde_json::to_string(&c.list(&env())[0]).unwrap();
    assert_eq!(
        json,
        r#"{"name":"db1","section":null,"aliases":["d"],"hostname":"db1.example.com","user":"postgres","port":22,"options":{"IdentityFile":["a","b"],"ProxyCommand":"ssh -W %h:%p x"},"proxy_command":"ssh -W %h:%p x","proxy_jump":null,"meta":{}}"#
    );
    let docs_row = cfg(&sectioned()).list(&env())[0].clone();
    let v: serde_json::Value = serde_json::to_value(&docs_row).unwrap();
    for field in [
        "name", "section", "aliases", "hostname", "user", "port", "options",
    ] {
        assert!(v.get(field).is_some(), "missing {field}");
    }
    assert_eq!(v["section"], "data foundry");
    assert_eq!(v["options"], serde_json::json!({}));
}

#[test]
fn show_prints_entries_verbatim_by_name_or_alias() {
    let c = cfg("# main box\nHost vps v\n    HostName vps.example.com\n\n# other\n\nHost x\n    HostName x\n");
    let shown = c.show(&[s("v"), s("x")]).unwrap();
    assert_eq!(
        shown[0].text,
        "# main box\nHost vps v\n    HostName vps.example.com\n"
    );
    assert_eq!(shown[1].text, "Host x\n    HostName x\n");
    assert_eq!(
        c.show(&[s("nope")]).unwrap_err().to_string(),
        "nope does not exist."
    );
}

#[test]
fn dump_is_the_file_verbatim() {
    let text = include_str!("fixtures/basic.conf");
    assert_eq!(cfg(text).dump(), text);
}

#[test]
fn search_regex_and_fixed() {
    let c = cfg("Host github\n    HostName github.com\n    User git\n\nHost vps\n    HostName vps.example.com\n    User root\n    Port 2222\n\nHost web\n    HostName web.example.com\n    ProxyCommand ssh -W %h:%p bastion\n");
    let names = |rows: Vec<ListRow>| rows.into_iter().map(|r| r.name).collect::<Vec<_>>();
    assert_eq!(names(c.search("git", false, &env()).unwrap()), ["github"]);
    assert_eq!(
        names(c.search(r"example\.com:2[0-9]{3}", false, &env()).unwrap()),
        ["vps"]
    );
    // docs/cli.md's example pattern also matches port 22 ("2" then "2").
    assert_eq!(
        names(c.search(r"example\.com:2[0-9]+", false, &env()).unwrap()),
        ["vps", "web"]
    );
    assert_eq!(names(c.search("bastion", false, &env()).unwrap()), ["web"]);
    assert_eq!(names(c.search("%h:%p", true, &env()).unwrap()), ["web"]);
    assert!(c.search("zzz", false, &env()).unwrap().is_empty());
    assert_eq!(
        names(c.search("ex.mple", true, &env()).unwrap()),
        Vec::<String>::new()
    );
    assert_eq!(
        names(c.search("ex.mple", false, &env()).unwrap()),
        ["vps", "web"]
    );
    let e = c.search("(", false, &env()).unwrap_err();
    assert_eq!(e.exit_code(), 2);
    let m = Matcher::new("git", false).unwrap();
    assert_eq!(
        m.find_ranges("github -> git@github.com:22"),
        vec![0..3, 10..13, 14..17]
    );
}

#[test]
fn alias_adds_names_and_refuses_taken_ones() {
    let mut c = cfg("Host vps\n    HostName vps.example.com\n\nHost web\n    HostName w\n");
    assert_eq!(
        c.alias("vps", &[s("v"), s("box"), s("v")]).unwrap(),
        ["vps", "v", "box"]
    );
    assert_eq!(
        c.render(),
        "Host vps v box\n    HostName vps.example.com\n\nHost web\n    HostName w\n"
    );
    let e = c.alias("web", &[s("box")]).unwrap_err();
    assert_eq!(e.to_string(), "box is already a name of vps.");
    assert_eq!(c.show(&[s("v")]).unwrap()[0].name, "vps");
}

#[test]
fn unalias_two_and_one_argument_forms() {
    let mut c = cfg("Host vps v box\n    HostName vps.example.com\n");
    let u = c.unalias(Some("vps"), &[s("box")]).unwrap();
    assert_eq!(u.names, ["vps", "v"]);
    assert_eq!(c.render(), "Host vps v\n    HostName vps.example.com\n");
    let u = c.unalias(None, &[s("v")]).unwrap();
    assert_eq!((u.host.as_str(), u.names.clone()), ("vps", vec![s("vps")]));
    assert_eq!(c.render(), "Host vps\n    HostName vps.example.com\n");
    assert!(matches!(
        c.unalias(None, &[s("vps")]).unwrap_err(),
        Error::PrimaryName(_)
    ));
    assert!(matches!(
        c.unalias(Some("vps"), &[s("vps")]).unwrap_err(),
        Error::PrimaryName(_)
    ));
    assert!(matches!(
        c.unalias(None, &[s("zz")]).unwrap_err(),
        Error::AliasNotFound(_)
    ));
    assert!(matches!(
        c.unalias(Some("vps"), &[s("zz")]).unwrap_err(),
        Error::NotAnAliasOf { .. }
    ));
}

#[test]
fn sections_lists_names_and_counts_in_file_order() {
    let summary = cfg(&sectioned()).sections();
    let rows: Vec<(&str, usize, bool)> = summary
        .iter()
        .map(|s| (s.name.as_str(), s.hosts, s.catch_all))
        .collect();
    assert_eq!(
        rows,
        [
            ("data foundry", 1, false),
            ("bob", 1, false),
            ("other", 2, true)
        ]
    );
    assert!(cfg(BASE).sections().is_empty());
}

#[test]
fn rename_section_regenerates_banner() {
    let mut c = cfg(&sectioned());
    let r = c.rename_section("BOB", "cypresspt").unwrap();
    assert_eq!(
        r,
        SectionRename::Renamed {
            from: s("bob"),
            to: s("cypresspt")
        }
    );
    assert_eq!(
        c.render(),
        sectioned().replace(&banner_text("bob"), &banner_text("cypresspt"))
    );
    let e = c.rename_section("nope", "x").unwrap_err();
    assert_eq!(e.to_string(), "section nope does not exist.");
    assert_eq!(e.exit_code(), 1);
}

#[test]
fn rename_section_onto_existing_name_merges() {
    let mut c = cfg(&sectioned());
    let r = c.rename_section("bob", "Data Foundry").unwrap();
    assert_eq!(
        r,
        SectionRename::Merged {
            from: s("bob"),
            into: s("data foundry")
        }
    );
    let expected = format!(
        "# preamble\n\nHost *\n    User emre\n\n{}\nHost box\n    HostName box.x\n\nHost db1\n    HostName db1.example.com\n\n{}\nHost github\n    HostName github.com\n    User git\n\nHost vps\n    HostName vps.example.com\n",
        banner_text("data foundry"),
        banner_text("other")
    );
    assert_eq!(c.render(), expected);
}

#[test]
fn catch_all_rename_keeps_it_last_and_catching() {
    let mut c = cfg(&sectioned());
    c.rename_section("other", "personal").unwrap();
    assert_eq!(
        c.render(),
        sectioned().replace(&banner_text("other"), &banner_text("personal"))
    );
    c.add(
        &AddSpec {
            name: s("new"),
            uri: s("u@new.x"),
            section: Some(s("fresh")),
            ..Default::default()
        },
        &env(),
    )
    .unwrap();
    let names: Vec<&str> = c.sections.iter().map(|s| s.name()).collect();
    assert_eq!(names, ["data foundry", "bob", "fresh", "personal"]);
    let placed = c
        .add(
            &AddSpec {
                name: s("aaa"),
                uri: s("u@aaa.x"),
                ..Default::default()
            },
            &env(),
        )
        .unwrap();
    assert_eq!(placed.section.as_deref(), Some("personal"));
    assert!(c.sections().last().unwrap().catch_all);
}

#[test]
fn check_reports_unknown_key_missing_identity_and_missing_hostname() {
    let home = tempfile::tempdir().unwrap();
    std::fs::create_dir(home.path().join(".ssh")).unwrap();
    std::fs::write(home.path().join(".ssh/ok.pem"), "").unwrap();
    let c = cfg("Host vps\n    HostName vps.example.com\n    IdentityFile ~/.ssh/old.pem\n    IdentityFile ~/.ssh/ok.pem\n\nHost web-prod\n    HostName w\n    StrictHostKeyChekcing no\n    User a\n    user b\n\nHost nohost\n    User x\n\nHost *.example.com\n    User y\n\nHost vps\n    HostName dup\nBrokenLine\n");
    let report = c.check(&Env {
        user: None,
        home: Some(home.path().to_path_buf()),
    });
    let lines: Vec<String> = report.problems.iter().map(ToString::to_string).collect();
    assert_eq!(
        lines,
        [
            "vps: IdentityFile ~/.ssh/old.pem does not exist",
            "web-prod: unknown key StrictHostKeyChekcing",
            "web-prod: duplicate key User",
            "nohost: no HostName",
            "vps: name used by 2 entries",
            "line 20: cannot parse: BrokenLine",
        ]
    );
    assert_eq!(report.summary(), "6 problems in 5 hosts.");
    let clean = cfg("Host a\n    HostName a\n").check(&env());
    assert!(clean.is_clean());
    assert_eq!(clean.summary(), "no problems in 1 host.");
}

#[test]
fn connection_uri_forms() {
    let p = |u: &str| ConnectionUri::parse(u).unwrap();
    assert_eq!(
        p("root@vps.example.com:2222"),
        ConnectionUri {
            user: Some(s("root")),
            host: s("vps.example.com"),
            port: Some(2222)
        }
    );
    assert_eq!(
        p("vps.example.com:2222"),
        ConnectionUri {
            user: None,
            host: s("vps.example.com"),
            port: Some(2222)
        }
    );
    assert_eq!(
        p("vps.example.com"),
        ConnectionUri {
            user: None,
            host: s("vps.example.com"),
            port: None
        }
    );
    assert_eq!(
        p("[2001:db8::1]:22"),
        ConnectionUri {
            user: None,
            host: s("2001:db8::1"),
            port: Some(22)
        }
    );
    assert_eq!(
        p("me@[2001:db8::1]"),
        ConnectionUri {
            user: Some(s("me")),
            host: s("2001:db8::1"),
            port: None
        }
    );
    assert_eq!(
        p("2001:db8::1"),
        ConnectionUri {
            user: None,
            host: s("2001:db8::1"),
            port: None
        }
    );
    for bad in [
        "vps:abc",
        "vps:",
        "vps:70000",
        "vps:0",
        "@vps",
        "",
        "[::1",
        "a b",
    ] {
        let e = ConnectionUri::parse(bad).unwrap_err();
        assert!(matches!(e, Error::InvalidUri { .. }), "{bad}");
        assert_eq!(e.exit_code(), 1);
    }
    assert_eq!(
        ConnectionUri::parse("vps:abc").unwrap_err().to_string(),
        "vps:abc is not a valid connection URI: port abc is not a number from 1 to 65535"
    );
}

#[test]
fn exit_code_for_every_error_variant() {
    let io = || std::io::Error::other("x");
    let table: Vec<(Error, i32)> = vec![
        (Error::HostExists(s("a")), 1),
        (Error::EditTargetMissing(s("a")), 1),
        (Error::HostNotFound(s("a")), 1),
        (Error::TargetExists(s("a")), 1),
        (Error::NoMatch(s("a")), 1),
        (Error::MoveNeedsTarget, 2),
        (Error::RefuseDeleteAll(3), 1),
        (Error::Declined, 1),
        (Error::SectionNotFound(s("a")), 1),
        (
            Error::InvalidUri {
                uri: s("a"),
                reason: s("b"),
            },
            1,
        ),
        (Error::InvalidName(s("a")), 1),
        (
            Error::InvalidPattern {
                pattern: s("a"),
                reason: s("b"),
            },
            2,
        ),
        (
            Error::AliasTaken {
                alias: s("a"),
                owner: s("b"),
            },
            1,
        ),
        (Error::AliasNotFound(s("a")), 1),
        (
            Error::NotAnAliasOf {
                host: s("a"),
                alias: s("b"),
            },
            1,
        ),
        (Error::PrimaryName(s("a")), 1),
        (Error::OddKeyValues, 2),
        (Error::Usage(s("a")), 2),
        (Error::InvalidOption(s("a")), 2),
        (Error::ForbiddenKey(s("Host")), 1),
        (
            Error::Read {
                path: "p".into(),
                source: io(),
            },
            3,
        ),
        (
            Error::Write {
                path: "p".into(),
                source: io(),
            },
            3,
        ),
        (
            Error::UserConfig {
                path: "p".into(),
                reason: s("b"),
            },
            3,
        ),
    ];
    for (err, code) in &table {
        assert_eq!(err.exit_code(), *code, "{err:?}");
    }
    // The documented messages, verbatim (docs/functionality.md error table).
    let docs = [
        (
            Error::HostExists(s("vps")),
            "vps already exists. Use rustorm edit or rustorm set to modify it.",
        ),
        (
            Error::EditTargetMissing(s("nope")),
            "nope does not exist. Use rustorm add to create it.",
        ),
        (
            Error::NoMatch(s("nomatch-.*")),
            "no host matches nomatch-.*",
        ),
        (Error::TargetExists(s("rails02")), "rails02 already exists."),
        (
            Error::MoveNeedsTarget,
            "give a new name, a --section, or both.",
        ),
        (Error::HostNotFound(s("nope")), "nope does not exist."),
        (
            Error::RefuseDeleteAll(14),
            "refusing to delete 14 hosts without --yes on a non-interactive terminal.",
        ),
        (
            Error::SectionNotFound(s("nope")),
            "section nope does not exist.",
        ),
    ];
    for (err, msg) in docs {
        assert_eq!(err.to_string(), msg);
    }
}

#[test]
fn user_config_parses_aliases_and_defaults() {
    let p = std::path::Path::new("config.toml");
    let u = UserConfig::parse("[aliases]\ndelete = [\"rm\", \"del\"]\nadd = [\"create\"]\n\n[defaults]\nbackup = false\ncolor = \"never\"\n", p).unwrap();
    assert_eq!(u.resolve_alias("del"), Some("delete"));
    assert_eq!(u.resolve_alias("create"), Some("add"));
    assert_eq!(u.resolve_alias("zzz"), None);
    assert!(!u.defaults.backup);
    assert_eq!(u.defaults.color, ColorMode::Never);
    let d = UserConfig::parse("", p).unwrap();
    assert!(d.defaults.backup);
    assert_eq!(d.defaults.color, ColorMode::Auto);
    assert_eq!(
        UserConfig::parse("[defaults]\ncolor = 3\n", p)
            .unwrap_err()
            .exit_code(),
        3
    );
    let tmp = tempfile::tempdir().unwrap();
    assert_eq!(
        UserConfig::load_from(&tmp.path().join("none.toml")).unwrap(),
        UserConfig::default()
    );
    let path = UserConfig::default_path().unwrap();
    assert!(path.ends_with("rustorm/config.toml"));
    assert!(!path.starts_with(dirs::home_dir().unwrap().join(".rustorm")));
}
