//! The keyword schema behind the settings forms (catalog ks-1 .. ks-3).

use rustorm_core::{
    key_spec, key_specs, validate_setting, Config, Error, KeyGroup, KeyType, SettingChange,
    Workspace, WriteOptions, KNOWN_KEYS,
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
