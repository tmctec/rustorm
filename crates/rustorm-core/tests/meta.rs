//! Host metadata: `# key: value` comments above the `Host` line (docs/cli.md,
//! Host metadata; plan anemone step 3; catalog cases meta-1..meta-13).

use rustorm_core::banner::banner_text;
use rustorm_core::*;

const BUILDBOX: &str = "\
# Primary build box. Reboot only after 18:00.
# note: Primary build box
# note: Reboot only after 18:00
# location: Austin DC, rack 4, U12
# privateKeyLocation: keepassxc
# other: owner alice
# tags: prod, austin, db
Host buildbox
    HostName 10.0.4.12
    User deploy

Host plain
    HostName plain.example.com
";

fn cfg(text: &str) -> Config {
    Config::parse(text).unwrap()
}

fn s(v: &str) -> String {
    v.to_string()
}

fn pairs(v: &[(&str, &str)]) -> Vec<(String, String)> {
    v.iter().map(|(k, v)| (s(k), s(v))).collect()
}

fn host<'a>(c: &'a Config, name: &str) -> &'a HostBlock {
    c.host(c.find_host(name).unwrap())
}

fn by_name(selector: &str) -> HostSelector {
    HostSelector::Name(s(selector))
}

// meta-1: a file with metadata round-trips byte for byte, and every key reads back.
#[test]
fn metadata_round_trips_and_reads_back() {
    let c = cfg(BUILDBOX);
    assert_eq!(c.render(), BUILDBOX);
    let m = host(&c, "buildbox").meta();
    assert_eq!(m.note, ["Primary build box", "Reboot only after 18:00"]);
    assert_eq!(m.location.as_deref(), Some("Austin DC, rack 4, U12"));
    assert_eq!(m.private_key_location.as_deref(), Some("keepassxc"));
    assert_eq!(m.other.as_deref(), Some("owner alice"));
    assert_eq!(m.tags, ["prod", "austin", "db"]);
    assert!(host(&c, "plain").meta().is_empty());
}

// meta-2: hand-written lines with odd spacing and case read as metadata.
#[test]
fn hand_written_lines_read_case_insensitively() {
    let c = cfg("#Location:   Berlin  \n#   NOTE:one\n# PRIVATEKEYLOCATION: vault/a\n# Tags:A,,b , a\nHost h\n");
    let m = host(&c, "h").meta();
    assert_eq!(m.location.as_deref(), Some("Berlin"));
    assert_eq!(m.note, ["one"]);
    assert_eq!(m.private_key_location.as_deref(), Some("vault/a"));
    assert_eq!(m.tags, ["A", "b"], "tags split on commas, trimmed, deduplicated ignoring case");
    assert_eq!(host(&c, "h").get("tags").as_deref(), Some("A, b"));
    assert_eq!(host(&c, "h").get_all("note"), ["one"]);
}

// meta-3: set rewrites the first line in place, drops duplicates, inserts new keys in write order.
#[test]
fn set_rewrites_in_place_and_inserts_in_write_order() {
    let mut c = cfg("# hand comment\n# location: old\n# location: older\n# tags: a\nHost h\n    HostName h.x\n");
    c.set(&by_name("h"), &pairs(&[("Location", "new"), ("note", "n1"), ("other", "o")]), false)
        .unwrap();
    assert_eq!(
        c.render(),
        "# hand comment\n# note: n1\n# location: new\n# other: o\n# tags: a\nHost h\n    HostName h.x\n"
    );
}

// meta-4: note is multi-line: set replaces every line, append adds one after the last.
#[test]
fn note_replaces_or_appends_lines() {
    let mut c = cfg(BUILDBOX);
    c.set(&by_name("buildbox"), &pairs(&[("note", "only")]), false)
        .unwrap();
    assert_eq!(host(&c, "buildbox").meta().note, ["only"]);
    c.set(&by_name("buildbox"), &pairs(&[("note", "second")]), true)
        .unwrap();
    assert_eq!(host(&c, "buildbox").meta().note, ["only", "second"]);
    let text = c.render();
    assert!(text.contains("# note: only\n# note: second\n# location: Austin DC"));
}

// meta-5: an empty value removes the line; unset removes every line of the key.
#[test]
fn empty_value_and_unset_remove_lines() {
    let mut c = cfg(BUILDBOX);
    c.set(&by_name("buildbox"), &pairs(&[("location", "   ")]), false)
        .unwrap();
    assert!(host(&c, "buildbox").meta().location.is_none());
    c.unset(&by_name("buildbox"), &[s("note"), s("tags"), s("nothere")])
        .unwrap();
    let m = host(&c, "buildbox").meta();
    assert!(m.note.is_empty() && m.tags.is_empty());
    assert_eq!(
        c.render(),
        "# Primary build box. Reboot only after 18:00.\n# privateKeyLocation: keepassxc\n# other: owner alice\nHost buildbox\n    HostName 10.0.4.12\n    User deploy\n\nHost plain\n    HostName plain.example.com\n"
    );
}

// meta-6: comments that are not metadata are never read as it, moved, or rewritten.
#[test]
fn other_comments_are_untouched() {
    let src = "# TODO: fix this\n# section: nope\n# plain words: with colon\n# note: real\nHost h\n";
    let mut c = cfg(src);
    let m = host(&c, "h").meta();
    assert_eq!(m.note, ["real"]);
    assert!(m.other.is_none(), "`# section:` and `# TODO:` are not metadata");
    c.set(&by_name("h"), &pairs(&[("location", "L")]), false).unwrap();
    assert_eq!(
        c.render(),
        "# TODO: fix this\n# section: nope\n# plain words: with colon\n# note: real\n# location: L\nHost h\n"
    );
    assert!(parse_meta_line("# section: data foundry").is_none());
    assert!(parse_meta_line("# TODO: x").is_none());
    assert!(parse_meta_line("not a comment: x").is_none());
    assert_eq!(
        parse_meta_line("#  privateKeyLocation : keepassxc "),
        Some((MetaKey::PrivateKeyLocation, s("keepassxc")))
    );
}

// meta-7: a section banner's label line is never metadata, even inside a sectioned file.
#[test]
fn banner_label_is_not_metadata() {
    let text = format!(
        "{}\n# note: n\nHost a\n    HostName a.x\n\n{}\nHost b\n    HostName b.x\n",
        banner_text("data foundry"),
        banner_text("other")
    );
    let c = cfg(&text);
    assert_eq!(host(&c, "a").meta().note, ["n"]);
    assert!(host(&c, "b").meta().is_empty());
    assert_eq!(c.sections.len(), 2);
    assert_eq!(c.render(), text);
}

// meta-8: privateKeyLocation refuses key material; a tag refuses spaces; values are one line.
#[test]
fn validation_rejects_key_material_spaced_tags_and_newlines() {
    let mut c = cfg(BUILDBOX);
    let err = c
        .set(
            &by_name("buildbox"),
            &pairs(&[("privateKeyLocation", "-----BEGIN OPENSSH PRIVATE KEY-----")]),
            false,
        )
        .unwrap_err();
    assert_eq!(
        err.to_string(),
        "privateKeyLocation holds a reference to a key, not the key itself."
    );
    assert_eq!(err.exit_code(), 1);
    let err = c
        .set(&by_name("buildbox"), &pairs(&[("tags", "prod, two words")]), false)
        .unwrap_err();
    assert_eq!(err.to_string(), "tags has a tag with a space: two words.");
    let err = c
        .set(&by_name("buildbox"), &pairs(&[("note", "a\nb")]), false)
        .unwrap_err();
    assert_eq!(err.to_string(), "note must be one line.");
    assert_eq!(c.render(), BUILDBOX, "a rejected value changes nothing");
    assert!(validate_meta(MetaKey::PrivateKeyLocation, "keepassxc").is_ok());
}

// meta-9: --tag adds (deduplicated, case-insensitive), --untag removes, the last removal drops the line.
#[test]
fn tag_and_untag() {
    let mut c = cfg("Host h\n    HostName h.x\n");
    c.set_with_tags(&by_name("h"), &[], false, &[s("prod"), s("DB"), s("prod")], &[])
        .unwrap();
    assert_eq!(c.render(), "# tags: prod, DB\nHost h\n    HostName h.x\n");
    c.set_with_tags(&by_name("h"), &[], false, &[s("db")], &[s("PROD")])
        .unwrap();
    assert_eq!(c.render(), "# tags: DB\nHost h\n    HostName h.x\n");
    c.set_with_tags(&by_name("h"), &[], false, &[], &[s("db")])
        .unwrap();
    assert_eq!(c.render(), "Host h\n    HostName h.x\n");
    c.set(&by_name("h"), &pairs(&[("tags", "x, y")]), false).unwrap();
    c.set(&by_name("h"), &pairs(&[("tags", "z")]), true).unwrap();
    assert_eq!(host(&c, "h").meta().tags, ["x", "y", "z"]);
    let err = c
        .set_with_tags(&by_name("h"), &[], false, &[s("bad tag")], &[])
        .unwrap_err();
    assert_eq!(err.to_string(), "tags has a tag with a space: bad tag.");
}

// meta-10: clone carries the metadata lines and nothing else from the leading comments.
#[test]
fn clone_carries_metadata_only() {
    let mut c = cfg(BUILDBOX);
    c.clone_host(&CloneSpec {
        source: s("buildbox"),
        new_name: s("buildbox2"),
        keep_hostname: true,
        overrides: pairs(&[("location", "Dallas")]),
        section: None,
    })
    .unwrap();
    let copy = host(&c, "buildbox2");
    assert_eq!(copy.leading.len(), 6, "five metadata lines, the hand comment stays behind");
    let m = copy.meta();
    assert_eq!(m.location.as_deref(), Some("Dallas"));
    assert_eq!(m.tags, ["prod", "austin", "db"]);
    assert_eq!(m.note.len(), 2);
    assert!(!copy.text().contains("Primary build box. Reboot"));
    assert_eq!(host(&c, "buildbox").meta().location.as_deref(), Some("Austin DC, rack 4, U12"));
}

// meta-11: move and combine keep the lines with the host, through leading comments.
#[test]
fn move_and_combine_keep_metadata() {
    let mut c = cfg(BUILDBOX);
    c.move_host("buildbox", Some("bb"), Some("lab")).unwrap();
    let m = host(&c, "bb").meta();
    assert_eq!(m.location.as_deref(), Some("Austin DC, rack 4, U12"));
    assert_eq!(c.sections.len(), 2);

    let base = cfg("Host a\n    HostName a.x\n");
    let extra = cfg("# note: from extra\n# tags: t1\nHost e\n    HostName e.x\n");
    let (merged, _report) = combine(
        vec![
            CombineInput {
                path: "base.conf".into(),
                config: base,
            },
            CombineInput {
                path: "extra.conf".into(),
                config: extra,
            },
        ],
        OnConflict::Fail,
        None,
    )
    .unwrap();
    let m = host(&merged, "e").meta();
    assert_eq!(m.note, ["from extra"]);
    assert_eq!(m.tags, ["t1"]);
}

// meta-12: delete removes the host's metadata lines with it; add -o and the settings form write them.
#[test]
fn delete_add_and_settings_form_handle_metadata() {
    let mut c = cfg(BUILDBOX);
    c.delete(&[s("buildbox")]).unwrap();
    assert_eq!(c.render(), "Host plain\n    HostName plain.example.com\n");
    let env = Env {
        user: Some(s("tester")),
        home: None,
    };
    c.add(
        &AddSpec {
            name: s("n"),
            uri: s("n.example.com"),
            identity: None,
            options: pairs(&[("location", "Oslo"), ("tags", "a, b")]),
            section: None,
        },
        &env,
    )
    .unwrap();
    let n = host(&c, "n");
    assert_eq!(n.meta().location.as_deref(), Some("Oslo"));
    assert_eq!(n.meta().tags, ["a", "b"]);
    assert!(c.render().contains("# location: Oslo\n# tags: a, b\nHost n\n"));
    c.apply_settings(
        "n",
        &[
            SettingChange {
                key: s("note"),
                values: vec![s("l1"), s("l2")],
            },
            SettingChange::unset("location"),
            SettingChange::set("other", "x"),
        ],
    )
    .unwrap();
    let n = host(&c, "n");
    assert_eq!(n.meta().note, ["l1", "l2"]);
    assert!(n.meta().location.is_none());
    assert_eq!(n.meta().other.as_deref(), Some("x"));
    let err = c
        .apply_settings(
            "n",
            &[SettingChange {
                key: s("location"),
                values: vec![s("a"), s("b")],
            }],
        )
        .unwrap_err();
    assert_eq!(err.to_string(), "location takes one value.");
}

// meta-13: list and show rows carry the metadata and serialize it as the "meta" object; search matches it.
#[test]
fn rows_carry_meta_and_search_matches_it() {
    let c = cfg(BUILDBOX);
    let env = Env {
        user: Some(s("tester")),
        home: None,
    };
    let rows = c.list(&env);
    let bb = rows.iter().find(|r| r.name == "buildbox").unwrap();
    assert_eq!(bb.meta.tags, ["prod", "austin", "db"]);
    assert!(bb.options.is_empty(), "metadata is not an option");
    let json = serde_json::to_value(bb).unwrap();
    assert_eq!(
        json["meta"],
        serde_json::json!({
            "note": ["Primary build box", "Reboot only after 18:00"],
            "location": "Austin DC, rack 4, U12",
            "privateKeyLocation": "keepassxc",
            "other": "owner alice",
            "tags": ["prod", "austin", "db"]
        })
    );
    let plain = rows.iter().find(|r| r.name == "plain").unwrap();
    assert_eq!(serde_json::to_value(plain).unwrap()["meta"], serde_json::json!({}));
    let shown = c.show(&[s("buildbox")]).unwrap();
    assert_eq!(shown[0].meta.location.as_deref(), Some("Austin DC, rack 4, U12"));
    assert_eq!(shown[0].text, BUILDBOX.split("\n\n").next().unwrap().to_string() + "\n");

    let hits = c.search("keepassxc", true, &env).unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].name, "buildbox");
    let hits = c.search("rack 4", true, &env).unwrap();
    assert_eq!(hits.len(), 1);
    assert!(c.search("nowhere", true, &env).unwrap().is_empty());
    assert_eq!(
        bb.meta.lines(),
        [
            "# note: Primary build box",
            "# note: Reboot only after 18:00",
            "# location: Austin DC, rack 4, U12",
            "# privateKeyLocation: keepassxc",
            "# other: owner alice",
            "# tags: prod, austin, db"
        ]
    );
}
