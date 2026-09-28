//! Parser, writer, sections, banners and key handling (step 3 of plan basilisk).

use rustorm_core::banner::banner_lines;
use rustorm_core::{Config, Entry, HostBlock};

const BASIC: &str = include_str!("fixtures/basic.conf");
const SECTIONED: &str = include_str!("fixtures/sectioned.conf");
const CRLF_NOEOL: &str = include_str!("fixtures/crlf-noeol.conf");
const DATA_FOUNDRY: &str = include_str!("fixtures/banner-data-foundry.txt");

#[test]
fn round_trip_is_byte_identical_on_every_fixture() {
    for (name, text) in [
        ("basic.conf", BASIC),
        ("sectioned.conf", SECTIONED),
        ("crlf-noeol.conf", CRLF_NOEOL),
        ("banner", DATA_FOUNDRY),
        ("empty", ""),
        ("no newline", "Host a"),
    ] {
        let c = Config::parse(text).unwrap();
        assert_eq!(c.render(), text, "{name} did not round-trip");
    }
}

#[test]
fn basic_fixture_structure() {
    let c = Config::parse(BASIC).unwrap();
    assert!(!c.has_sections());
    let names: Vec<String> = c.hosts().iter().map(|h| h.primary()).collect();
    assert_eq!(names, ["vps", "*", "github.com", "web-prod"]);
    let vps = c.host(c.find_host("v").unwrap());
    assert_eq!(vps.aliases(), ["v"]);
    assert_eq!(vps.get_all("identityfile").len(), 2);
    assert_eq!(
        vps.leading.len(),
        1,
        "comment directly above belongs to the host"
    );
    assert_eq!(c.defaults().unwrap().get("user").as_deref(), Some("emre"));
    let matches = c
        .preamble
        .iter()
        .filter(|e| matches!(e, Entry::Match(_)))
        .count();
    assert_eq!(matches, 1, "Match block is kept opaque");
}

#[test]
fn banner_for_data_foundry_matches_docs_line_for_line() {
    let expected: Vec<&str> = DATA_FOUNDRY.lines().collect();
    let actual = banner_lines("data foundry");
    assert_eq!(actual.len(), expected.len());
    for (i, (a, e)) in actual.iter().zip(expected.iter()).enumerate() {
        assert_eq!(a, e, "banner line {i} differs");
    }
}

#[test]
fn parsing_a_banner_recovers_the_name() {
    let c = Config::parse(SECTIONED).unwrap();
    let names: Vec<&str> = c.sections.iter().map(|s| s.name()).collect();
    assert_eq!(names, ["work", "personal stuff"]);
    assert_eq!(c.sections[0].banner.lines.len(), 4);
    let generated = Config::parse(DATA_FOUNDRY).unwrap();
    assert_eq!(generated.sections[0].name(), "data foundry");
    assert_eq!(generated.sections[0].banner.lines.len(), 9);
}

fn host(name: &str, hostname: &str) -> HostBlock {
    let mut h = HostBlock::new(&[name.to_string()]);
    h.set("HostName", hostname);
    h
}

#[test]
fn first_section_creates_catch_all_last_and_new_sections_go_before_it() {
    let mut c = Config::parse(
        "# mine\n\nHost *\n    User me\n\nHost b\n    HostName b.x\n\nHost a\n    HostName a.x\n",
    )
    .unwrap();
    let work = c.ensure_section("work");
    assert_eq!(work, 0);
    let names: Vec<&str> = c.sections.iter().map(|s| s.name()).collect();
    assert_eq!(names, ["work", "other"]);
    let home = c.ensure_section("home");
    assert_eq!(home, 1);
    let names: Vec<&str> = c.sections.iter().map(|s| s.name()).collect();
    assert_eq!(names, ["work", "home", "other"]);
    assert_eq!(c.find_section("OTHER"), Some(2));
    // Preamble keeps the comment and Host *; the catch-all holds a and b, sorted.
    let expected = format!(
        "# mine\n\nHost *\n    User me\n\n{}\n{}\n{}\nHost a\n    HostName a.x\n\nHost b\n    HostName b.x\n",
        banner_lines("work").join("\n") + "\n",
        banner_lines("home").join("\n") + "\n",
        banner_lines("other").join("\n") + "\n",
    );
    assert_eq!(c.render(), expected);
}

#[test]
fn hosts_sort_alphabetically_within_a_section() {
    let mut c = Config::default();
    let s = c.ensure_section("lab");
    for n in ["delta", "Bravo", "alpha", "charlie"] {
        c.insert_host(Some(s), host(n, &format!("{n}.lab")));
    }
    c.sort_sections();
    let names: Vec<String> = c.sections[s].hosts().map(HostBlock::primary).collect();
    assert_eq!(names, ["alpha", "Bravo", "charlie", "delta"]);
    let reparsed = Config::parse(&c.render()).unwrap();
    assert_eq!(reparsed.render(), c.render());
    assert_eq!(reparsed.sections.last().unwrap().name(), "other");
}

#[test]
fn multi_valued_keys_accumulate_and_set_replaces_all() {
    let mut h = host("vps", "vps.x");
    h.append("IdentityFile", "~/.ssh/a");
    h.append("identityfile", "~/.ssh/b");
    h.append("SendEnv", "LANG");
    assert_eq!(h.get_all("IdentityFile"), ["~/.ssh/a", "~/.ssh/b"]);
    h.set("IdentityFile", "~/.ssh/c");
    assert_eq!(h.get_all("IdentityFile"), ["~/.ssh/c"]);
    assert_eq!(
        h.text(),
        "Host vps\n    HostName vps.x\n    IdentityFile ~/.ssh/c\n    SendEnv LANG\n"
    );
}

#[test]
fn keys_are_written_in_canonical_case_and_unknown_keys_keep_their_spelling() {
    let mut c =
        Config::parse("Host vps\n  hostname old.x\n  fooBarBaz keep\n  identityfile ~/.ssh/k\n")
            .unwrap();
    let loc = c.find_host("vps").unwrap();
    let h = c.host_mut(loc);
    h.set("HOSTNAME", "new.x");
    h.set("proxyjump", "bastion");
    h.set("MyCustomKey", "v");
    assert_eq!(
        c.render(),
        "Host vps\n  HostName new.x\n  fooBarBaz keep\n  identityfile ~/.ssh/k\n  ProxyJump bastion\n  MyCustomKey v\n"
    );
}

#[test]
fn unterminated_last_line_gets_a_newline_before_new_content() {
    let mut c = Config::parse(CRLF_NOEOL).unwrap();
    c.insert_host(None, host("new", "new.x"));
    let out = c.render();
    assert!(out.starts_with(CRLF_NOEOL));
    assert!(out.ends_with("\tUser\tops\n\nHost new\n    HostName new.x\n"));
}

#[test]
fn include_and_match_lines_survive_edits() {
    let mut c = Config::parse(BASIC).unwrap();
    let loc = c.find_host("web-prod").unwrap();
    c.host_mut(loc).set("User", "web");
    let out = c.render();
    assert!(out.contains("Include ~/.ssh/config.d/*\n"));
    assert!(
        out.contains("Match host *.internal exec \"test -f /tmp/vpn\"\n    ProxyJump bastion\n")
    );
    assert_eq!(
        out,
        BASIC.replace(
            "    FooBarUnknown yes\n",
            "    FooBarUnknown yes\n    User web\n"
        )
    );
}
