//! The keyword schema behind the settings forms (catalog ks-1 .. ks-3).

use rustorm_core::{
    complete_line, complete_setting, key_spec, key_specs, next_choice, premade_value, swap_value,
    validate_setting, Config, Error, KeyGroup, KeyType, SettingChange, SettingsDraft, Workspace,
    WriteOptions, KNOWN_KEYS,
};

/// ks-1: every keyword a host can set has a spec; Host, Match and Include
/// have none.
#[test]
fn ks_1_every_known_key_has_a_spec() {
    for k in KNOWN_KEYS {
        let spec = key_spec(k);
        if ["Host", "Match", "Include"].contains(k) {
            assert!(spec.is_none(), "{k}");
        } else {
            let spec = spec.unwrap_or_else(|| panic!("{k} has no spec"));
            assert_eq!(spec.key, *k);
        }
    }
    assert_eq!(key_specs().len(), KNOWN_KEYS.len() - 3);
    for g in KeyGroup::ALL {
        assert!(
            key_specs().iter().any(|s| s.group == g),
            "{} is empty",
            g.title()
        );
    }
    let multi: Vec<&str> = key_specs()
        .iter()
        .filter(|s| s.multi)
        .map(|s| s.key)
        .collect();
    assert_eq!(multi.len(), rustorm_core::MULTI_VALUED_KEYS.len());
}

/// ks-2: fixed choices match ssh_config(5).
#[test]
fn ks_2_choices_match_the_man_page() {
    let values = |k: &str| match key_spec(k).unwrap().kind {
        KeyType::Choice { values, .. } => values.to_vec(),
        other => panic!("{k} is {other:?}"),
    };
    assert_eq!(
        values("StrictHostKeyChecking"),
        ["yes", "no", "ask", "accept-new", "off"]
    );
    assert_eq!(
        values("ControlMaster"),
        ["no", "yes", "ask", "auto", "autoask"]
    );
    assert_eq!(values("RequestTTY"), ["no", "yes", "force", "auto"]);
    assert_eq!(values("AddressFamily"), ["any", "inet", "inet6"]);
    assert_eq!(key_spec("Compression").unwrap().kind, KeyType::Flag);
    assert_eq!(key_spec("ProxyCommand").unwrap().group, KeyGroup::Proxy);
    assert_eq!(
        key_spec("ControlPath").unwrap().group,
        KeyGroup::Multiplexing
    );
    assert!(validate_setting("StrictHostKeyChecking", "accept-new").is_ok());
    assert!(validate_setting("StrictHostKeyChecking", "sometimes").is_err());
    assert!(validate_setting("ControlPersist", "10m").is_ok());
    assert!(validate_setting("LocalForward", "8080").is_err());
    assert!(validate_setting("LocalForward", "8080 localhost:80").is_ok());
    assert!(validate_setting("DynamicForward", "1080").is_ok());
    assert!(validate_setting("ServerAliveInterval", "-1").is_err());
    assert!(validate_setting("ProxyCommand", "ssh -W %h:%p jump").is_ok());
    assert!(validate_setting("MyOwnKey", "anything").is_ok());
}

const RANCH: &str = "\
# ranch machines
Host dcevant
    # the old box
    HostName dcevant.ranch.lan
    Compression yes
    User admin
";

/// ks-3: one batch of sets and unsets on a host in an included file writes
/// that file once with one backup; a bad value is refused naming the key.
#[test]
fn ks_3_batch_writes_once_and_refuses_bad_values() {
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path().join("config.d");
    std::fs::create_dir(&d).unwrap();
    let root = dir.path().join("config");
    let root_text = format!("Include {}/*\n\nHost github\n    User git\n", d.display());
    std::fs::write(&root, &root_text).unwrap();
    let ranch = d.join("ranch");
    std::fs::write(&ranch, RANCH).unwrap();

    let mut ws = Workspace::load_with_home(&root, None).unwrap();
    let bad = ws
        .clone()
        .apply_settings("dcevant", &[SettingChange::set("Port", "abc")], None)
        .unwrap_err();
    assert!(matches!(bad, Error::InvalidSetting { ref key, .. } if key == "Port"));
    assert_eq!(bad.to_string(), "Port must be a port from 1 to 65535.");

    let changes = [
        SettingChange::set("forwardagent", "no"),
        SettingChange::set("ControlMaster", "auto"),
        SettingChange {
            key: "LocalForward".into(),
            values: vec!["8080 localhost:80".into(), "8443 localhost:443".into()],
        },
        SettingChange::unset("Compression"),
    ];
    ws.apply_settings("dcevant", &changes, None).unwrap();
    let written = ws.save(WriteOptions::default()).unwrap();
    assert_eq!(written.len(), 1);
    let text = std::fs::read_to_string(&ranch).unwrap();
    assert_eq!(
        text,
        "\
# ranch machines
Host dcevant
    # the old box
    HostName dcevant.ranch.lan
    User admin
    ForwardAgent no
    ControlMaster auto
    LocalForward 8080 localhost:80
    LocalForward 8443 localhost:443
"
    );
    let i = ws.files.iter().position(|f| f.path == ranch).unwrap();
    assert_eq!(
        std::fs::read_to_string(ws.backup_path_for(i)).unwrap(),
        RANCH
    );
    assert_eq!(std::fs::read_to_string(&root).unwrap(), root_text);

    // A one-value key given two values is refused.
    let mut c = Config::parse(RANCH).unwrap();
    let e = c
        .apply_settings(
            "dcevant",
            &[SettingChange {
                key: "User".into(),
                values: vec!["a".into(), "b".into()],
            }],
        )
        .unwrap_err();
    assert_eq!(e.to_string(), "User takes one value.");
}

const FIVE: &str = "\
Host *
    User fallback

Host five
    HostName five.example.com
    User deploy
    Port 2222
    IdentityFile ~/.ssh/five
    Compression yes
";

fn five_draft() -> SettingsDraft {
    let c = Config::parse(FIVE).unwrap();
    let block = c.host(c.find_primary("five").unwrap());
    SettingsDraft::new("five", block, c.defaults())
}

fn filled_keys(d: &SettingsDraft) -> Vec<&'static str> {
    d.filled_rows()
        .iter()
        .map(|&i| d.rows[i].spec.key)
        .collect()
}

/// fv-1 (core): the filled view of a 5-key host is exactly those 5 rows,
/// in form order; the inherited User fallback does not fill a row.
#[test]
fn fv_1_filled_rows_are_the_hosts_keys() {
    let d = five_draft();
    let mut keys = filled_keys(&d);
    assert_eq!(keys.len(), 5, "{keys:?}");
    keys.sort();
    assert_eq!(
        keys,
        ["Compression", "HostName", "IdentityFile", "Port", "User"]
    );
}

/// fv-10 (core): a cleared loaded value stays filled; an edited empty row
/// becomes filled.
#[test]
fn fv_10_cleared_row_stays_filled() {
    let mut d = five_draft();
    let port = d.rows.iter().position(|r| r.spec.key == "Port").unwrap();
    d.rows[port].value.clear();
    assert!(d.rows[port].filled() && d.rows[port].changed());
    assert!(d.filled_rows().contains(&port));
    let tag = d.rows.iter().position(|r| r.spec.key == "Tag").unwrap();
    assert!(!d.rows[tag].filled());
    d.rows[tag].value = "x".into();
    assert!(d.filled_rows().contains(&tag));
}

/// fv-6 (core): completion lists prefix matches in KNOWN_KEYS order, or
/// substring matches when no keyword starts with the text; unknown text
/// matches nothing.
#[test]
fn fv_6_complete_orders_prefix_then_substring() {
    let d = five_draft();
    assert_eq!(d.complete("hostk"), ["HostKeyAlias"]);
    assert_eq!(complete_setting("HOSTK"), ["HostKeyAlias"]);
    assert_eq!(
        d.complete("forward"),
        [
            "ForwardAgent",
            "ForwardX11",
            "ForwardX11Timeout",
            "ForwardX11Trusted"
        ]
    );
    assert_eq!(
        d.complete("forwardx11t"),
        ["ForwardX11Timeout", "ForwardX11Trusted"]
    );
    let sub = d.complete("forwarding");
    assert!(sub.contains(&"ClearAllForwardings"), "{sub:?}");
    assert_eq!(d.complete("keyal"), ["HostKeyAlias"]);
    assert!(d.complete("zzz").is_empty());
    assert!(d.complete("").is_empty());
    assert!(d.complete("hos").iter().all(|k| *k != "Host"));
    assert_eq!(d.complete("hostkeyalgorithms"), ["HostKeyAlgorithms"]);
}

/// fv-8 / fv-9 / fv-4 (core): adding a set single-value key returns its
/// row; a repeatable key gains an empty row after its values; an unset
/// key's row joins the filled view.
#[test]
fn fv_8_9_add_key_focuses_or_adds_rows() {
    let mut d = five_draft();
    let rows = d.rows.len();
    let user = d.add_key("user").unwrap();
    assert_eq!(d.rows[user].spec.key, "User");
    assert_eq!(d.rows[user].value, "deploy");
    assert_eq!(d.rows.len(), rows);
    assert_eq!(d.filled_rows().len(), 5);

    let id = d.add_key("IdentityFile").unwrap();
    assert_eq!(d.rows[id].spec.key, "IdentityFile");
    assert_eq!(d.rows[id].value, "");
    assert_eq!(d.rows[id - 1].value, "~/.ssh/five");
    assert!(d.filled_rows().contains(&id));
    assert_eq!(d.filled_rows().len(), 6);

    let alias = d.add_key("HostKeyAlias").unwrap();
    assert_eq!(d.rows[alias].value, "");
    assert!(d.rows[alias].added);
    assert_eq!(d.filled_rows().len(), 7);
    d.rows[alias].value = "alias1".into();
    assert_eq!(
        d.changes().unwrap(),
        [SettingChange::set("HostKeyAlias", "alias1")]
    );
    assert_eq!(d.add_key("HostKeyAlias"), Some(alias));
    assert_eq!(d.add_key("Host"), None);
    assert_eq!(d.add_key("NoSuchKey"), None);
}

fn keyword(line: &str) -> Option<&'static str> {
    complete_line(line, line.chars().count()).map(|c| c.keyword)
}

/// ed-1 / ed-2 / ed-8 / ed-9 (core): the first word of a line completes to
/// a keyword; comments and values do not.
#[test]
fn ed_completion_in_the_keyword_position() {
    assert_eq!(keyword("    hostk"), Some("HostKeyAlias"));
    assert_eq!(keyword("    por"), Some("Port"));
    assert_eq!(keyword("\tcompr"), Some("Compression"));
    assert_eq!(keyword("ho"), Some("Host"));
    assert_eq!(keyword("inc"), Some("Include"));
    assert_eq!(keyword("    ho").map(|k| k != "Host"), Some(true));
    assert_eq!(keyword("    port"), Some("Port"));
    assert_eq!(keyword("# por"), None);
    assert_eq!(keyword("    # por"), None);
    assert_eq!(keyword("    User tra"), None);
    assert_eq!(keyword("    Port 22"), None);
    assert_eq!(keyword("    zzz"), None);
    assert_eq!(keyword("    "), None);
    assert_eq!(keyword(""), None);
    // The cursor must end the word.
    assert_eq!(complete_line("    port", 6), None);
    assert_eq!(
        complete_line("    por 1", 7).map(|c| c.keyword),
        Some("Port")
    );
}

/// ed-1 / ed-2 / ed-6 / ed-8 (core): the ghost text and the accepted line.
#[test]
fn ed_accept_inserts_keyword_space_and_premade_value() {
    let c = complete_line("    hostk", 9).unwrap();
    assert_eq!(c.ghost("    hostk"), "eyAlias");
    let a = c.accept("    hostk");
    assert_eq!(a.line, "    HostKeyAlias ");
    assert_eq!((a.cursor, a.select), (17, None));

    let a = complete_line("    por", 7).unwrap().accept("    por");
    assert_eq!(a.line, "    Port 22");
    assert_eq!((a.cursor, a.select), (11, Some(9..11)));

    let c = complete_line("    port", 8).unwrap();
    assert_eq!(c.ghost("    port"), "");
    assert_eq!(c.accept("    port").line, "    Port 22");

    let a = complete_line("ho", 2).unwrap().accept("ho");
    assert_eq!((a.line.as_str(), a.select), ("Host ", None));

    let c = complete_line("    keyal", 9).unwrap();
    assert_eq!(c.ghost("    keyal"), " → HostKeyAlias");
    // Text after the cursor is kept.
    let a = complete_line("    por  # web", 7)
        .unwrap()
        .accept("    por  # web");
    assert_eq!(a.line, "    Port 22  # web");
}

/// ed-2 / ed-3 / fv-14 (core): premade values.
#[test]
fn ed_premade_values() {
    for (k, v) in [
        ("Port", Some("22")),
        ("Compression", Some("yes")),
        ("StrictHostKeyChecking", Some("yes")),
        ("ServerAliveInterval", Some("60")),
        ("ConnectTimeout", Some("10")),
        ("ControlPersist", Some("10m")),
        ("ControlPath", Some("~/.ssh/cm-%r@%h:%p")),
        ("IdentityFile", Some("~/.ssh/id_ed25519")),
        ("LocalForward", Some("8080 localhost:80")),
        ("ControlMaster", Some("no")),
        ("HostKeyAlias", None),
        ("ProxyCommand", None),
        ("User", None),
        ("Host", None),
    ] {
        assert_eq!(premade_value(k), v, "{k}");
    }
    for k in key_specs() {
        if let Some(v) = premade_value(k.key) {
            assert!(validate_setting(k.key, v).is_ok(), "{} {v}", k.key);
        }
    }
}

/// ed-3 / ed-4 / ed-10 (core): swapping flag and choice values.
#[test]
fn ed_swap_values() {
    assert_eq!(next_choice("Compression", "yes"), Some("no"));
    assert_eq!(next_choice("Compression", "no"), Some("yes"));
    assert_eq!(
        next_choice("StrictHostKeyChecking", "ask"),
        Some("accept-new")
    );
    assert_eq!(next_choice("StrictHostKeyChecking", "off"), Some("yes"));
    assert_eq!(next_choice("User", "travis"), None);
    assert_eq!(next_choice("ControlPersist", "10m"), None);
    let line = "    StrictHostKeyChecking ask";
    assert_eq!(swap_value(line, 29), Some((26..29, "accept-new")));
    assert_eq!(swap_value(line, 26), Some((26..29, "accept-new")));
    assert_eq!(swap_value(line, 10), None);
    assert_eq!(swap_value("    User travis", 15), None);
    assert_eq!(swap_value("    Compression=yes", 19), Some((16..19, "no")));
    assert_eq!(swap_value("    LocalForward 80 x:80", 20), None);
}
