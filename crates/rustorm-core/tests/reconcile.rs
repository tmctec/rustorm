//! `reconcile` in rustorm-core: one test per catalog case rec-1..rec-15
//! (plan gannet, step 3).
//!
//! Most tests build the workspace of docs/cli.md's reconcile examples under
//! a temporary home directory:
//!
//! ```text
//! ~/.ssh/config                Include ~/.ssh/config.d/*; host github
//! ~/.ssh/config.d/cypress      hosts cypressPro, cypressPro-ext, lab-1
//! ~/.ssh/config.d/cypress.bak  the same three, then printer
//! ```

use std::fs;
use std::path::PathBuf;

use rustorm_core::banner::banner_text;
use rustorm_core::*;

const ROOT: &str =
    "Include ~/.ssh/config.d/*\n\nHost github\n    HostName github.com\n    User git\n";
const CYPRESS: &str = "Host cypressPro\n    HostName 10.10.0.2\n    User travis\n\nHost cypressPro-ext\n    HostName cypress.example.com\n    User travis\n    Port 2222\n\n# location: Austin DC, rack 4\nHost lab-1\n    HostName 10.10.0.30\n    User travis\n";
const BAK: &str = "Host cypressPro\n    HostName 10.10.0.9\n    User travis\n\n# old box\nHost cypressPro-ext\n    hostname   cypress.example.com\n    User travis\n    Port 2222\n\nHost lab-1\n    HostName 10.10.0.30\n    User travis\n\nHost printer\n    HostName 192.168.1.20\n";

struct Home {
    dir: tempfile::TempDir,
}

impl Home {
    fn empty() -> Home {
        Home {
            dir: tempfile::tempdir().unwrap(),
        }
    }

    /// The docs example workspace.
    fn new() -> Home {
        let home = Home::empty();
        home.write(".ssh/config", ROOT);
        home.write(".ssh/config.d/cypress", CYPRESS);
        home.write(".ssh/config.d/cypress.bak", BAK);
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

    fn load(&self) -> Workspace {
        Workspace::load_with_home(self.path(".ssh/config"), Some(self.dir.path())).unwrap()
    }
}

fn kinds(r: &ReconcileReport) -> Vec<(String, PairKind)> {
    r.items.iter().map(|p| (p.name.clone(), p.kind)).collect()
}

fn decide(r: &ReconcileReport, host: &str, d: Decision) -> (usize, Decision) {
    (r.lookup(host).unwrap(), d)
}

// rec-1: identical vs conflict vs orphan; orphans only when FILE names the copy
#[test]
fn rec_1_classifies_identical_conflict_and_orphan() {
    let home = Home::new();
    let mut ws = home.load();
    let all = ws.reconcile_report(None);
    assert_eq!(
        kinds(&all),
        vec![
            ("cypressPro".to_string(), PairKind::Conflict),
            ("cypressPro-ext".to_string(), PairKind::Identical),
            ("lab-1".to_string(), PairKind::Conflict),
        ]
    );
    assert_eq!(
        all.summary(),
        "2 conflicts, 1 identical, 0 orphans across 2 files"
    );
    let live = all.items[0].live.as_ref().unwrap();
    assert_eq!(live.label, "~/.ssh/config.d/cypress");
    assert_eq!(live.line, 1);
    assert_eq!(all.items[0].copy.label, "~/.ssh/config.d/cypress.bak");
    let scope = ws.reconcile_scope(&["cypress.bak".to_string()]).unwrap();
    let named = ws.reconcile_report(Some(&scope));
    assert_eq!(named.orphans, 1);
    assert_eq!(named.items[3].name, "printer");
    assert!(named.items[3].live.is_none());
    assert_eq!(
        named.summary(),
        "2 conflicts, 1 identical, 1 orphan across 2 files"
    );
    // Naming the live file leaves no copy in scope.
    let scope = ws.reconcile_scope(&["cypress".to_string()]).unwrap();
    let none = ws.reconcile_report(Some(&scope));
    assert_eq!(
        none.summary(),
        "0 conflicts, 0 identical, 0 orphans across 0 files"
    );
}

// rec-2: a difference in plain comments, key case, indentation or spacing is identical
#[test]
fn rec_2_comment_only_difference_is_identical() {
    let home = Home::empty();
    home.write(".ssh/config", "Include ~/.ssh/config.d/*\n");
    home.write(
        ".ssh/config.d/a",
        "# main box\nHost vps\n    HostName vps.example.com\n    User root\n",
    );
    home.write(
        ".ssh/config.d/b",
        "# the old notes\n# TODO: retire\nHost vps\n\thostname   vps.example.com\n  # inline remark\n\n  USER = root\n",
    );
    let r = home.load().reconcile_report(None);
    assert_eq!(kinds(&r), vec![("vps".to_string(), PairKind::Identical)]);
    assert!(r.items[0].diff.is_empty());
    assert!(r.items[0].keys.is_empty());
}

// rec-3: a metadata (`# location:`) difference is a conflict, with the key and a diff
#[test]
fn rec_3_metadata_difference_is_a_conflict() {
    let home = Home::new();
    let r = home.load().reconcile_report(None);
    let lab = &r.items[r.lookup("lab-1").unwrap()];
    assert_eq!(lab.kind, PairKind::Conflict);
    assert_eq!(
        lab.keys,
        vec![KeyDiff {
            key: "location".to_string(),
            live: vec!["Austin DC, rack 4".to_string()],
            copy: Vec::new(),
        }]
    );
    assert_eq!(
        lab.diff,
        "--- ~/.ssh/config.d/cypress (live)\n+++ ~/.ssh/config.d/cypress.bak (copy)\n@@ -1,4 +1,3 @@ lab-1\n-# location: Austin DC, rack 4\n Host lab-1\n     HostName 10.10.0.30\n     User travis\n"
    );
}

// rec-4: take-copy replaces body and labels, keeps the live Host line, place and section
#[test]
fn rec_4_take_copy_keeps_host_line_position_and_section() {
    let home = Home::empty();
    home.write(".ssh/config", "Include ~/.ssh/config.d/*\n");
    let live = format!(
        "{}\n# hand note\n# location: rack 1\nHost db1 db1-alias\n    HostName 10.0.0.1\n    User admin\n\nHost db2\n    HostName 10.0.0.2\n",
        banner_text("data foundry")
    );
    home.write(".ssh/config.d/df-austin", &live);
    home.write(
        ".ssh/config.d/old",
        "# tags: prod, db\n# location: rack 9\nHost db1\n    HostName 10.0.0.99\n    # moved in May\n    User postgres\n    Port 5432\n",
    );
    let mut ws = home.load();
    let r = ws.reconcile_report(None);
    let ch = ws
        .apply_decisions(&r, &[decide(&r, "db1", Decision::TakeCopy)], None)
        .unwrap();
    assert_eq!(
        ch.messages,
        vec![
            "db1: took the copy from ~/.ssh/config.d/old into ~/.ssh/config.d/df-austin.",
            "no conflicts remain."
        ]
    );
    assert_eq!(ch.value.taken, vec!["db1"]);
    ws.save(WriteOptions::default()).unwrap();
    assert_eq!(
        home.read(".ssh/config.d/df-austin"),
        format!(
            "{}\n# hand note\n# tags: prod, db\n# location: rack 9\nHost db1 db1-alias\n    HostName 10.0.0.99\n    # moved in May\n    User postgres\n    Port 5432\n\nHost db2\n    HostName 10.0.0.2\n",
            banner_text("data foundry")
        )
    );
    let again = home.load();
    let wl = again.find_host("db1-alias").unwrap();
    assert_eq!(
        again.files[wl.file].config.section_name(wl.loc),
        Some("data foundry")
    );
    // The pair is identical now; the copy is untouched.
    assert_eq!(again.reconcile_report(None).identical, 1);
    assert!(home.read(".ssh/config.d/old").contains("10.0.0.99"));
}

// rec-5: keep-live writes nothing; both files stay byte-identical
#[test]
fn rec_5_keep_live_leaves_both_files_byte_identical() {
    let home = Home::new();
    let mut ws = home.load();
    let r = ws.reconcile_report(None);
    let ch = ws
        .apply_decisions(
            &r,
            &[
                decide(&r, "cypressPro", Decision::KeepLive),
                decide(&r, "lab-1", Decision::KeepLive),
            ],
            None,
        )
        .unwrap();
    assert_eq!(
        ch.messages,
        vec![
            "cypressPro: kept ~/.ssh/config.d/cypress.",
            "lab-1: kept ~/.ssh/config.d/cypress.",
            "no conflicts remain."
        ]
    );
    assert!(ch.files.is_empty());
    assert_eq!(
        ws.save(WriteOptions::default()).unwrap(),
        Vec::<usize>::new()
    );
    assert_eq!(home.read(".ssh/config.d/cypress"), CYPRESS);
    assert_eq!(home.read(".ssh/config.d/cypress.bak"), BAK);
    assert!(!home.path(".ssh/config.d/.cypress~").exists());
}

// rec-6: drop-identical removes only the copy's block, leading comments included
#[test]
fn rec_6_drop_identical_removes_only_the_copy_block() {
    let home = Home::new();
    let mut ws = home.load();
    let r = ws.reconcile_report(None);
    let decisions = r.bulk(PairKind::Identical, &Decision::DropIdentical, &[]);
    let ch = ws.apply_decisions(&r, &decisions, None).unwrap();
    assert_eq!(
        ch.messages,
        vec![
            "1 identical copy dropped from ~/.ssh/config.d/cypress.bak.",
            "2 conflicts remain."
        ]
    );
    assert_eq!(ch.value.remaining, 2);
    assert_eq!(ws.save(WriteOptions::default()).unwrap(), vec![2]);
    assert_eq!(home.read(".ssh/config.d/cypress"), CYPRESS);
    assert_eq!(
        home.read(".ssh/config.d/cypress.bak"),
        "Host cypressPro\n    HostName 10.10.0.9\n    User travis\n\nHost lab-1\n    HostName 10.10.0.30\n    User travis\n\nHost printer\n    HostName 192.168.1.20\n"
    );
    // The backup of the copy is the dot backup, outside the glob.
    assert_eq!(home.read(".ssh/config.d/.cypress.bak~"), BAK);
}

// rec-7: add moves the orphan into the root, or into the --file target's same-named section
#[test]
fn rec_7_add_places_the_orphan() {
    let home = Home::new();
    let mut ws = home.load();
    let scope = ws.reconcile_scope(&["cypress.bak".to_string()]).unwrap();
    let r = ws.reconcile_report(Some(&scope));
    let ch = ws
        .apply_decisions(&r, &[decide(&r, "printer", Decision::Add)], None)
        .unwrap();
    assert_eq!(
        ch.messages,
        vec![
            "printer added to ~/.ssh/config from ~/.ssh/config.d/cypress.bak.",
            "2 conflicts remain."
        ]
    );
    ws.save(WriteOptions::default()).unwrap();
    assert_eq!(
        home.read(".ssh/config"),
        format!("{ROOT}\nHost printer\n    HostName 192.168.1.20\n")
    );
    assert!(!home.read(".ssh/config.d/cypress.bak").contains("printer"));
    // Into a --file target that has the orphan's section.
    let home = Home::empty();
    home.write(".ssh/config", "Include ~/.ssh/config.d/*\n");
    let lab = format!(
        "{}\nHost lab-1\n    HostName 10.0.0.1\n",
        banner_text("lab")
    );
    home.write(".ssh/config.d/lab", &lab);
    home.write(
        ".ssh/config.d/old",
        &format!(
            "{}\nHost lab-2\n    HostName 10.0.0.2\n",
            banner_text("lab")
        ),
    );
    let mut ws = home.load();
    let scope = ws.reconcile_scope(&["old".to_string()]).unwrap();
    let target = ws.resolve_file("lab").unwrap();
    let r = ws.reconcile_report(Some(&scope));
    ws.apply_decisions(&r, &[decide(&r, "lab-2", Decision::Add)], Some(target))
        .unwrap();
    ws.save(WriteOptions::default()).unwrap();
    assert_eq!(
        home.read(".ssh/config.d/lab"),
        format!("{lab}\nHost lab-2\n    HostName 10.0.0.2\n")
    );
}

// rec-8: retire refuses while a conflict or an orphan is undecided, and names them
#[test]
fn rec_8_retire_refuses_while_a_conflict_remains() {
    let home = Home::new();
    let mut ws = home.load();
    let bak = ws.resolve_file("cypress.bak").unwrap();
    let err = ws.retire(bak, &[], WriteOptions::default()).unwrap_err();
    assert_eq!(
        err.to_string(),
        "~/.ssh/config.d/cypress.bak not retired; undecided: cypressPro (conflict), lab-1 (conflict), printer (orphan)."
    );
    assert_eq!(err.exit_code(), 1);
    // One conflict decided, one left: still refused.
    let scope = vec![bak];
    let r = ws.reconcile_report(Some(&scope));
    let ch = ws
        .apply_decisions(
            &r,
            &[
                decide(&r, "cypressPro", Decision::TakeCopy),
                decide(&r, "printer", Decision::Add),
            ],
            None,
        )
        .unwrap();
    let err = ws
        .retire(bak, &ch.value.decided, WriteOptions::default())
        .unwrap_err();
    assert_eq!(
        err.to_string(),
        "~/.ssh/config.d/cypress.bak not retired; undecided: lab-1 (conflict)."
    );
    assert!(home.path(".ssh/config.d/cypress.bak").exists());
    assert!(!home.path(".ssh/retired").exists());
}

// rec-9: a fully resolved copy moves to ~/.ssh/retired/<name>; a taken name gets .1; nothing is deleted
#[test]
fn rec_9_retire_moves_a_resolved_copy() {
    let home = Home::new();
    home.write(".ssh/retired/cypress.bak", "older retired copy\n");
    let mut ws = home.load();
    let scope = ws.reconcile_scope(&["cypress.bak".to_string()]).unwrap();
    let r = ws.reconcile_report(Some(&scope));
    let ch = ws
        .apply_decisions(
            &r,
            &[
                decide(&r, "cypressPro", Decision::KeepLive),
                decide(&r, "lab-1", Decision::KeepLive),
                decide(&r, "printer", Decision::Add),
            ],
            None,
        )
        .unwrap();
    assert_eq!(
        ch.messages,
        vec![
            "cypressPro: kept ~/.ssh/config.d/cypress.",
            "lab-1: kept ~/.ssh/config.d/cypress.",
            "printer added to ~/.ssh/config from ~/.ssh/config.d/cypress.bak.",
            "no conflicts remain."
        ]
    );
    ws.save(WriteOptions::default()).unwrap();
    let retired = ws
        .retire(scope[0], &ch.value.decided, WriteOptions::default())
        .unwrap();
    assert_eq!(
        retired.messages,
        vec!["~/.ssh/config.d/cypress.bak retired to ~/.ssh/retired/cypress.bak.1."]
    );
    assert!(!home.path(".ssh/config.d/cypress.bak").exists());
    assert_eq!(
        home.read(".ssh/retired/cypress.bak"),
        "older retired copy\n"
    );
    assert!(home
        .read(".ssh/retired/cypress.bak.1")
        .starts_with("Host cypressPro\n    HostName 10.10.0.9\n"));
    // The workspace no longer loads it; the live hosts and printer remain.
    let again = home.load();
    assert_eq!(again.files.len(), 2);
    assert_eq!(again.reconcile_report(None).items.len(), 0);
    assert!(again.find_host("printer").is_some());
}

// rec-10: the root never retires
#[test]
fn rec_10_root_never_retires() {
    let home = Home::new();
    let mut ws = home.load();
    let err = ws.retire(0, &[], WriteOptions::default()).unwrap_err();
    assert_eq!(
        err.to_string(),
        "~/.ssh/config is the root config and never retires."
    );
    assert_eq!(err.exit_code(), 1);
    assert_eq!(home.read(".ssh/config"), ROOT);
    assert!(!home.path(".ssh/retired").exists());
}

// rec-11: the definition ssh reads first is live: an included file over the root below its Include, a backup that sorts first
#[test]
fn rec_11_copy_read_first_is_live() {
    let home = Home::empty();
    // Include inside Host * on line 2: config.d is read before the root's hosts.
    home.write(
        ".ssh/config",
        "Host *\n    Include ~/.ssh/config.d/*\n\nHost vps\n    HostName old.example.com\n",
    );
    home.write(
        ".ssh/config.d/a.bak",
        "Host vps\n    HostName new.example.com\n",
    );
    home.write(
        ".ssh/config.d/b",
        "Host vps\n    HostName mid.example.com\n",
    );
    let mut ws = home.load();
    let r = ws.reconcile_report(None);
    assert_eq!(r.items.len(), 2);
    for p in &r.items {
        assert_eq!(p.live.as_ref().unwrap().label, "~/.ssh/config.d/a.bak");
        assert!(p.live_is_backup);
        assert_eq!(
            p.note.as_deref(),
            Some("vps: ssh reads ~/.ssh/config.d/a.bak first, so its definition is the live one.")
        );
    }
    let copies: Vec<&str> = r.items.iter().map(|p| p.copy.label.as_str()).collect();
    assert_eq!(copies, vec!["~/.ssh/config.d/b", "~/.ssh/config"]);
    assert!(r.list_text().contains(
        "\nnote: vps: ssh reads ~/.ssh/config.d/a.bak first, so its definition is the live one.\n"
    ));
    // The root as a copy is in scope when named, but never retires.
    let scope = ws.reconcile_scope(&["config".to_string()]).unwrap();
    assert_eq!(ws.reconcile_report(Some(&scope)).conflicts, 1);
    // An Include below the root's host makes the root live.
    home.write(
        ".ssh/config",
        "Host vps\n    HostName old.example.com\n\nInclude ~/.ssh/config.d/*\n",
    );
    let r = home.load().reconcile_report(None);
    assert_eq!(r.items[0].live.as_ref().unwrap().label, "~/.ssh/config");
    assert!(!r.items[0].live_is_backup);
    assert!(r.items[0].note.is_none());
}

// rec-12: every line a decision does not touch stays byte-identical
#[test]
fn rec_12_untouched_lines_stay_byte_identical() {
    let home = Home::empty();
    home.write(".ssh/config", "Include ~/.ssh/config.d/*\n");
    // Odd spacing, tabs, CRLF and an unterminated last line around the pair.
    let live = "#  header   comment\r\nHost zeta\n\tHostName   z.example.com\n\n\n# about a\nHost alpha\n    HostName a.example.com\n\nHost omega\n  User   x\n  Port=22";
    let copy = "Host alpha\n    HostName a2.example.com\n\nHost omega\n  user x\n  port 22\n\n# keep me\nHost spare\n    HostName s\n";
    home.write(".ssh/config.d/a", live);
    home.write(".ssh/config.d/b", copy);
    let mut ws = home.load();
    let r = ws.reconcile_report(None);
    let mut d = vec![decide(&r, "alpha", Decision::TakeCopy)];
    let identical = r.bulk(PairKind::Identical, &Decision::DropIdentical, &d);
    d.extend(identical);
    ws.apply_decisions(&r, &d, None).unwrap();
    ws.save(WriteOptions::default()).unwrap();
    assert_eq!(
        home.read(".ssh/config.d/a"),
        "#  header   comment\r\nHost zeta\n\tHostName   z.example.com\n\n\n# about a\nHost alpha\n    HostName a2.example.com\n\nHost omega\n  User   x\n  Port=22"
    );
    assert_eq!(
        home.read(".ssh/config.d/b"),
        "Host alpha\n    HostName a2.example.com\n\n# keep me\nHost spare\n    HostName s\n"
    );
}

// rec-13: errors carry the contract's exit codes: 2 usage, 1 refused
#[test]
fn rec_13_error_exit_codes() {
    let home = Home::new();
    let mut ws = home.load();
    let r = ws.reconcile_report(None);
    let err = r.lookup("nas").unwrap_err();
    assert_eq!(
        err.to_string(),
        "nas is not defined in two workspace files."
    );
    assert_eq!(err.exit_code(), 2);
    let ext = r.lookup("cypressPro-ext").unwrap();
    let err = ws
        .apply_decisions(&r, &[(0, Decision::DropIdentical)], None)
        .unwrap_err();
    assert_eq!(
        err.to_string(),
        "cypressPro differs from its live definition; take the copy or keep the live one."
    );
    assert_eq!(err.exit_code(), 2);
    let err = ws
        .apply_decisions(&r, &[(ext, Decision::Add)], None)
        .unwrap_err();
    assert_eq!(err.exit_code(), 2);
    let pick = Decision::Keys(vec![KeyPick {
        key: "Port".into(),
        pick: Pick::Copy,
    }]);
    let err = ws.apply_decisions(&r, &[(0, pick)], None).unwrap_err();
    assert_eq!(err.to_string(), "cypressPro has no differing key Port.");
    assert_eq!(err.exit_code(), 2);
    // A copy in two files needs FILE.
    home.write(
        ".ssh/config.d/old",
        "Host cypressPro\n    HostName 10.10.0.1\n",
    );
    let ws2 = home.load();
    let err = ws2.reconcile_report(None).lookup("cypressPro").unwrap_err();
    assert_eq!(
        err.to_string(),
        "cypressPro has copies in ~/.ssh/config.d/cypress.bak and ~/.ssh/config.d/old. Name the copy as FILE."
    );
    assert_eq!(err.exit_code(), 2);
    // A report older than the workspace is refused.
    ws.set(
        &HostSelector::Name("lab-1".into()),
        &[("User".into(), "root".into())],
        false,
        None,
    )
    .unwrap();
    let lab = r.lookup("lab-1").unwrap();
    let err = ws
        .apply_decisions(&r, &[(lab, Decision::TakeCopy)], None)
        .unwrap_err();
    assert_eq!(
        err.to_string(),
        "lab-1 changed since the report; reconcile again."
    );
    assert_eq!(err.exit_code(), 1);
    // Refusals: root and unresolved retire are 1 (rec-8, rec-10); nothing was written.
    assert_eq!(home.read(".ssh/config.d/cypress"), CYPRESS);
}

// rec-14: the report renders docs/cli.md's --list example and JSON fields exactly
#[test]
fn rec_14_list_text_and_json_match_the_contract() {
    let home = Home::new();
    let r = home.load().reconcile_report(None);
    assert_eq!(
        r.list_text(),
        "2 conflicts, 1 identical, 0 orphans across 2 files
--- ~/.ssh/config.d/cypress (live)
+++ ~/.ssh/config.d/cypress.bak (copy)
@@ -1,3 +1,3 @@ cypressPro
 Host cypressPro
-    HostName 10.10.0.2
+    HostName 10.10.0.9
     User travis

--- ~/.ssh/config.d/cypress (live)
+++ ~/.ssh/config.d/cypress.bak (copy)
@@ -1,4 +1,3 @@ lab-1
-# location: Austin DC, rack 4
 Host lab-1
     HostName 10.10.0.30
     User travis
"
    );
    let json = serde_json::to_value(&r).unwrap();
    assert_eq!(json["conflicts"], 2);
    assert_eq!(json["identical"], 1);
    assert_eq!(json["orphans"], 0);
    assert_eq!(json["files"], 2);
    let item = &json["items"][0];
    let mut fields: Vec<&str> = item
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    fields.sort_unstable();
    assert_eq!(
        fields,
        vec![
            "copy",
            "diff",
            "keys",
            "kind",
            "live",
            "live_is_backup",
            "name",
            "names",
            "note"
        ]
    );
    assert_eq!(item["kind"], "conflict");
    assert_eq!(
        item["live"]["file"],
        home.path(".ssh/config.d/cypress").display().to_string()
    );
    assert_eq!(item["live"]["line"], 1);
    let d = serde_json::to_value(Decision::Keys(vec![KeyPick {
        key: "HostName".into(),
        pick: Pick::Copy,
    }]))
    .unwrap();
    assert_eq!(
        d,
        serde_json::json!({"keys": [{"key": "HostName", "pick": "copy"}]})
    );
    assert_eq!(
        serde_json::to_value(Decision::TakeCopy).unwrap(),
        "take_copy"
    );
}

// rec-15: per-key picks take only the chosen keys from the copy
#[test]
fn rec_15_key_picks_take_only_chosen_keys() {
    let home = Home::empty();
    home.write(".ssh/config", "Include ~/.ssh/config.d/*\n");
    home.write(
        ".ssh/config.d/a",
        "# location: rack 1\nHost vps\n    HostName 10.0.0.1\n    User root\n    IdentityFile ~/.ssh/a\n",
    );
    home.write(
        ".ssh/config.d/b",
        "# location: rack 2\nHost vps\n    HostName 10.0.0.2\n    User admin\n    IdentityFile ~/.ssh/b1\n    IdentityFile ~/.ssh/b2\n",
    );
    let mut ws = home.load();
    let r = ws.reconcile_report(None);
    let keys: Vec<&str> = r.items[0].keys.iter().map(|k| k.key.as_str()).collect();
    assert_eq!(keys, vec!["location", "HostName", "User", "IdentityFile"]);
    let picks = Decision::Keys(vec![
        KeyPick {
            key: "location".into(),
            pick: Pick::Copy,
        },
        KeyPick {
            key: "HostName".into(),
            pick: Pick::Live,
        },
        KeyPick {
            key: "identityfile".into(),
            pick: Pick::Copy,
        },
    ]);
    let ch = ws.apply_decisions(&r, &[(0, picks)], None).unwrap();
    assert_eq!(
        ch.messages,
        vec![
            "vps: took location, IdentityFile from ~/.ssh/config.d/b into ~/.ssh/config.d/a.",
            "no conflicts remain."
        ]
    );
    ws.save(WriteOptions::default()).unwrap();
    assert_eq!(
        home.read(".ssh/config.d/a"),
        "# location: rack 2\nHost vps\n    HostName 10.0.0.1\n    User root\n    IdentityFile ~/.ssh/b1\n    IdentityFile ~/.ssh/b2\n"
    );
}
