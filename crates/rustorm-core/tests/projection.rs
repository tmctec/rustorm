//! Reading output: `--where`, `--filter`, `--format`, `--just-value`
//! (docs/cli.md, Reading output; plan anemone step 5; catalog cases
//! read-1..read-24). The parse-back tests shell out to `jq` and python3
//! (`csv`, `yaml`) so every format is read by an independent parser.

use std::io::Write;
use std::process::{Command, Stdio};

use rustorm_core::banner::banner_text;
use rustorm_core::*;

fn s(v: &str) -> String {
    v.to_string()
}

fn keys(v: &[&str]) -> Vec<String> {
    v.iter().map(|k| s(k)).collect()
}

struct Fx {
    _dir: tempfile::TempDir,
    ws: Workspace,
}

/// A root with `Host *`, a sectioned host with metadata, a plain host, and
/// an included file with one host.
fn fixture() -> Fx {
    let dir = tempfile::tempdir().unwrap();
    let ssh = dir.path().join(".ssh");
    std::fs::create_dir_all(ssh.join("config.d")).unwrap();
    let root = format!(
        "Include config.d/*\n\nHost *\n    User fallback\n    Port 2200\n\n{}\n# hand comment\n# note: Primary build box\n# note: Reboot only after 18:00\n# location: Austin DC, rack 4, U12\n# privateKeyLocation: keepassxc\n# other: owner alice\n# tags: prod, austin, db\nHost buildbox bb\n    hostname 10.0.4.12\n    User deploy\n    IdentityFile ~/.ssh/a\n    IdentityFile ~/.ssh/b\n    Compression yes\n\n{}\nHost plain\n    HostName plain.example.com\n    Port 22\n",
        banner_text("df austin"),
        banner_text("other")
    );
    std::fs::write(ssh.join("config"), root).unwrap();
    std::fs::write(
        ssh.join("config.d/lab"),
        "# tags: lab\nHost lab-1\n    HostName 10.30.0.5\n    User travis\n",
    )
    .unwrap();
    let ws = Workspace::load_with_home(ssh.join("config"), Some(dir.path())).unwrap();
    Fx { _dir: dir, ws }
}

fn view<'a>(fx: &'a Fx, name: &str) -> HostView<'a> {
    let wl = fx.ws.find_host(name).unwrap();
    fx.ws.view(wl)
}

fn rows(fx: &Fx, names: &[&str], filter: &[&str]) -> Vec<Projected> {
    names
        .iter()
        .map(|n| project(&view(fx, n), &keys(filter)))
        .collect()
}

fn python(code: &str, input: &str) -> String {
    let mut child = Command::new("python3")
        .arg("-c")
        .arg(code)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("python3");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(input.as_bytes())
        .unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(out.status.success(), "python failed on:\n{input}");
    String::from_utf8(out.stdout).unwrap()
}

// read-1: only the named keys print, in order; no Host line without Host in the filter.
#[test]
fn read_1_only_named_keys_print() {
    let fx = fixture();
    let r = rows(&fx, &["buildbox"], &["hostname", "User"]);
    assert_eq!(
        render(&r, Format::Txt, false),
        "    hostname 10.0.4.12\n    User deploy\n"
    );
    let r = rows(&fx, &["buildbox"], &["Host", "hostname"]);
    assert_eq!(
        render(&r, Format::Txt, false),
        "Host buildbox bb\n    hostname 10.0.4.12\n"
    );
}

// read-2: keys resolve case-insensitively; txt keeps the file's spelling, other formats the typed one.
#[test]
fn read_2_key_case() {
    let fx = fixture();
    for k in ["Host", "host", "HOST"] {
        assert_eq!(resolve(&view(&fx, "buildbox"), k).value, Value::One(s("buildbox")));
    }
    let r = rows(&fx, &["buildbox"], &["HostName", "USER"]);
    assert_eq!(
        render(&r, Format::Txt, false),
        "    hostname 10.0.4.12\n    User deploy\n"
    );
    assert_eq!(
        render(&r, Format::Csv, false),
        "HostName,USER\n10.0.4.12,deploy\n"
    );
    assert_eq!(
        render(&r, Format::Json, false),
        "[{\"HostName\":\"10.0.4.12\",\"USER\":\"deploy\"}]\n"
    );
}

// read-3: metadata keys and tags filter like keywords.
#[test]
fn read_3_metadata_keys_filter() {
    let fx = fixture();
    let r = rows(&fx, &["buildbox"], &["Host", "note", "location", "privateKeyLocation", "other", "tags"]);
    assert_eq!(
        render(&r, Format::Json, false),
        "[{\"Host\":\"buildbox\",\"note\":[\"Primary build box\",\"Reboot only after 18:00\"],\"location\":\"Austin DC, rack 4, U12\",\"privateKeyLocation\":\"keepassxc\",\"other\":\"owner alice\",\"tags\":[\"prod\",\"austin\",\"db\"]}]\n"
    );
    assert_eq!(
        render(&r, Format::Txt, false),
        "Host buildbox bb\n# note: Primary build box\n# note: Reboot only after 18:00\n# location: Austin DC, rack 4, U12\n# privateKeyLocation: keepassxc\n# other: owner alice\n# tags: prod, austin, db\n"
    );
}

// read-4: an unset key prints an empty cell, null or an empty line and is reported missing.
#[test]
fn read_4_unset_key_is_missing() {
    let fx = fixture();
    let r = rows(&fx, &["plain"], &["Host", "proxyjump"]);
    assert_eq!(missing(&r), [(s("plain"), s("proxyjump"))]);
    assert_eq!(render(&r, Format::Json, false), "[{\"Host\":\"plain\",\"proxyjump\":null}]\n");
    assert_eq!(render(&r, Format::Csv, false), "Host,proxyjump\nplain,\n");
    assert_eq!(render(&r, Format::Txt, false), "Host plain\n");
    assert_eq!(render(&r, Format::Txt, true), "plain\n\n");
    assert_eq!(render(&r, Format::Yaml, false), "- Host: plain\n  proxyjump: null\n");
}

// read-5: a key the host lacks falls back to Host * and is not missing.
#[test]
fn read_5_defaults_fallback() {
    let fx = fixture();
    let r = rows(&fx, &["buildbox", "plain"], &["Host", "Port", "User"]);
    assert!(missing(&r).is_empty());
    assert_eq!(
        render(&r, Format::Csv, false),
        "Host,Port,User\nbuildbox,2200,deploy\nplain,22,fallback\n"
    );
    assert_eq!(
        render(&r, Format::Txt, false),
        "Host buildbox bb\n    Port 2200\n    User deploy\nHost plain\n    Port 22\n    User fallback\n"
    );
}

// read-6: pseudo-keys are never missing; section is empty outside every section.
#[test]
fn read_6_pseudo_keys_never_missing() {
    let fx = fixture();
    let r = rows(&fx, &["buildbox", "lab-1"], &["Host", "section", "file", "tags"]);
    assert!(missing(&r).is_empty());
    let lab = fx.ws.abs(1).display().to_string();
    let root = fx.ws.abs(0).display().to_string();
    assert_eq!(
        render(&r, Format::Csv, false),
        format!("Host,section,file,tags\nbuildbox,df austin,{root},prod;austin;db\nlab-1,,{lab},lab\n")
    );
    assert_eq!(
        render(&r, Format::Txt, false),
        format!("Host buildbox bb\n    section df austin\n    file {root}\n# tags: prod, austin, db\nHost lab-1\n    section \n    file {lab}\n# tags: lab\n")
    );
    let r = rows(&fx, &["plain"], &["tags"]);
    assert!(missing(&r).is_empty());
    assert_eq!(render(&r, Format::Json, false), "[{\"tags\":[]}]\n");
}

// read-7: --just-value drops the keys and the Host line; one value per line in txt.
#[test]
fn read_7_just_value() {
    let fx = fixture();
    let r = rows(&fx, &["buildbox", "plain"], &["Host", "hostname", "IdentityFile"]);
    assert_eq!(
        render(&r, Format::Txt, true),
        "buildbox\n10.0.4.12\n~/.ssh/a\n~/.ssh/b\nplain\nplain.example.com\n\n"
    );
    assert_eq!(
        render(&r, Format::Json, true),
        "[[\"buildbox\",\"10.0.4.12\",[\"~/.ssh/a\",\"~/.ssh/b\"]],[\"plain\",\"plain.example.com\",null]]\n"
    );
    assert_eq!(
        render(&r, Format::Csv, true),
        "buildbox,10.0.4.12,~/.ssh/a;~/.ssh/b\nplain,plain.example.com,\n"
    );
    assert_eq!(
        render(&r, Format::Yaml, true),
        "- [buildbox, 10.0.4.12, [~/.ssh/a, ~/.ssh/b]]\n- [plain, plain.example.com, null]\n"
    );
}

// read-8: yml is yaml; format names parse case-insensitively; an unknown name is a usage error.
#[test]
fn read_8_format_names() {
    assert_eq!(Format::parse("yml").unwrap(), Format::Yaml);
    assert_eq!(Format::parse("YAML").unwrap(), Format::Yaml);
    assert_eq!(Format::parse("Json").unwrap(), Format::Json);
    assert_eq!(Format::parse("csv").unwrap(), Format::Csv);
    assert_eq!(Format::parse("txt").unwrap(), Format::Txt);
    let err = Format::parse("xml").unwrap_err();
    assert_eq!(err.to_string(), "xml is not a format; use txt, json, csv or yaml.");
    assert_eq!(err.exit_code(), 2);
}

// read-9: yaml quotes yes/no/22/~ and leaves an IP bare.
#[test]
fn read_9_yaml_quoting() {
    assert_eq!(yaml_scalar("yes"), "\"yes\"");
    assert_eq!(yaml_scalar("No"), "\"No\"");
    assert_eq!(yaml_scalar("22"), "\"22\"");
    assert_eq!(yaml_scalar("~"), "\"~\"");
    assert_eq!(yaml_scalar("null"), "\"null\"");
    assert_eq!(yaml_scalar("1e3"), "\"1e3\"");
    assert_eq!(yaml_scalar(""), "\"\"");
    assert_eq!(yaml_scalar("- x"), "\"- x\"");
    assert_eq!(yaml_scalar("a: b"), "\"a: b\"");
    assert_eq!(yaml_scalar("a #b"), "\"a #b\"");
    assert_eq!(yaml_scalar("10.7.112.72"), "10.7.112.72");
    assert_eq!(yaml_scalar("Austin DC, rack 4, U12"), "Austin DC, rack 4, U12");
    assert_eq!(yaml_scalar("~/.ssh/a"), "~/.ssh/a", "only a bare ~ is null");
    assert_eq!(yaml_scalar("1_000"), "\"1_000\"");
    assert_eq!(yaml_scalar("1:30"), "\"1:30\"");
    assert_eq!(yaml_scalar("0x1f"), "\"0x1f\"");
    assert_eq!(yaml_scalar("say \"hi\""), "say \"hi\"", "a quote inside a plain scalar is text");
    assert_eq!(yaml_scalar("\"quoted\""), "\"\\\"quoted\\\"\"");
}

// read-10: csv quotes commas, quotes and newlines; a quote doubles.
#[test]
fn read_10_csv_quoting() {
    assert_eq!(csv_field("plain"), "plain");
    assert_eq!(csv_field("Austin DC, rack 4"), "\"Austin DC, rack 4\"");
    assert_eq!(csv_field("say \"hi\""), "\"say \"\"hi\"\"\"");
    assert_eq!(csv_field("two\nlines"), "\"two\nlines\"");
    let fx = fixture();
    let r = rows(&fx, &["buildbox"], &["Host", "location"]);
    assert_eq!(
        render(&r, Format::Csv, false),
        "Host,location\nbuildbox,\"Austin DC, rack 4, U12\"\n"
    );
}

// read-11: multi-value keys: json array, `;` csv cell, yaml list, one line each in txt.
#[test]
fn read_11_multi_value_keys() {
    let fx = fixture();
    let r = rows(&fx, &["buildbox"], &["IdentityFile", "note"]);
    assert_eq!(
        render(&r, Format::Json, false),
        "[{\"IdentityFile\":[\"~/.ssh/a\",\"~/.ssh/b\"],\"note\":[\"Primary build box\",\"Reboot only after 18:00\"]}]\n"
    );
    assert_eq!(
        render(&r, Format::Csv, false),
        "IdentityFile,note\n~/.ssh/a;~/.ssh/b,Primary build box;Reboot only after 18:00\n"
    );
    assert_eq!(
        render(&r, Format::Yaml, false),
        "- IdentityFile: [~/.ssh/a, ~/.ssh/b]\n  note: [Primary build box, Reboot only after 18:00]\n"
    );
    assert_eq!(
        render(&r, Format::Txt, false),
        "    IdentityFile ~/.ssh/a\n    IdentityFile ~/.ssh/b\n# note: Primary build box\n# note: Reboot only after 18:00\n"
    );
    assert_eq!(
        render(&r, Format::Txt, true),
        "~/.ssh/a\n~/.ssh/b\nPrimary build box\nReboot only after 18:00\n"
    );
}

// read-12: --where KEY=VALUE is exact and case-insensitive; contains on a list.
#[test]
fn read_12_where_equals() {
    let fx = fixture();
    let w = Where::parse("user=DEPLOY").unwrap();
    assert!(w.holds(&view(&fx, "buildbox")));
    assert!(!w.holds(&view(&fx, "plain")));
    let w = Where::parse("tags=Austin").unwrap();
    assert!(w.holds(&view(&fx, "buildbox")));
    assert!(!w.holds(&view(&fx, "lab-1")));
    let w = Where::parse("IdentityFile=~/.ssh/b").unwrap();
    assert!(w.holds(&view(&fx, "buildbox")));
    let w = Where::parse("section=DF AUSTIN").unwrap();
    assert!(w.holds(&view(&fx, "buildbox")));
    assert!(!w.holds(&view(&fx, "plain")));
    let w = Where::parse("Port=22").unwrap();
    assert!(w.holds(&view(&fx, "plain")));
    assert!(!w.holds(&view(&fx, "buildbox")), "buildbox inherits 2200 from Host *");
}

// read-13: a comma in the value means OR; repeated clauses AND; != negates and is true when unset.
#[test]
fn read_13_where_or_and_not() {
    let fx = fixture();
    let w = Where::parse("tags=lab,db").unwrap();
    assert!(w.holds(&view(&fx, "buildbox")));
    assert!(w.holds(&view(&fx, "lab-1")));
    assert!(!w.holds(&view(&fx, "plain")));
    let both = [
        Where::parse("tags=prod").unwrap(),
        Where::parse("user=deploy").unwrap(),
    ];
    assert!(selected(&both, &view(&fx, "buildbox")));
    let both = [
        Where::parse("tags=prod").unwrap(),
        Where::parse("user=travis").unwrap(),
    ];
    assert!(!selected(&both, &view(&fx, "buildbox")));
    let w = Where::parse("tags!=retired").unwrap();
    assert!(w.holds(&view(&fx, "buildbox")));
    assert!(w.holds(&view(&fx, "plain")), "a host without the key is != anything");
    let w = Where::parse("location!=Austin DC, rack 4, U12").unwrap();
    assert!(w.holds(&view(&fx, "buildbox")), "comma splits the alternatives: none equals the whole text");
    let w = Where::parse("proxyjump=x").unwrap();
    assert!(!w.holds(&view(&fx, "plain")), "an unset key equals nothing");
}

// read-14: ~ is a regex over any value; a bad pattern is exit 2; a clause without an operator is usage.
#[test]
fn read_14_where_regex_and_usage() {
    let fx = fixture();
    let w = Where::parse("location~^Austin").unwrap();
    assert!(w.holds(&view(&fx, "buildbox")));
    let w = Where::parse("IdentityFile~/b$").unwrap();
    assert!(w.holds(&view(&fx, "buildbox")));
    let w = Where::parse("hostname~example").unwrap();
    assert!(w.holds(&view(&fx, "plain")));
    assert!(!w.holds(&view(&fx, "buildbox")));
    let err = Where::parse("location~(").unwrap_err();
    assert_eq!(err.exit_code(), 2);
    let err = Where::parse("nonsense").unwrap_err();
    assert_eq!(
        err.to_string(),
        "nonsense is not KEY=VALUE, KEY!=VALUE or KEY~PATTERN."
    );
    assert_eq!(err.exit_code(), 2);
    assert_eq!(Where::parse("=x").unwrap_err().exit_code(), 2);
    let w = Where::parse("Host=plain").unwrap();
    assert_eq!(w.op, WhereOp::Eq);
    assert!(w.holds(&view(&fx, "plain")));
}

// read-15: `other` is a key under --filter and a section under --where section=.
#[test]
fn read_15_other_is_both_a_key_and_a_section() {
    let fx = fixture();
    let r = rows(&fx, &["buildbox"], &["other"]);
    assert_eq!(render(&r, Format::Txt, true), "owner alice\n");
    let w = Where::parse("section=other").unwrap();
    assert!(w.holds(&view(&fx, "plain")));
    assert!(!w.holds(&view(&fx, "buildbox")));
}

// read-16: --filter parses commas, trims, refuses an empty list.
#[test]
fn read_16_filter_parsing() {
    assert_eq!(parse_filter(" Host , hostname,user ").unwrap(), keys(&["Host", "hostname", "user"]));
    let err = parse_filter(" , ").unwrap_err();
    assert_eq!(err.to_string(), "--filter needs at least one key.");
    assert_eq!(err.exit_code(), 2);
}

// read-17: completion offers the pseudo-keys, metadata keys and ssh keywords, never Match or Include.
#[test]
fn read_17_completion_keys() {
    let ks = completion_keys();
    assert_eq!(&ks[..3], &["Host", "section", "file"]);
    assert!(ks.contains(&"note") && ks.contains(&"tags") && ks.contains(&"privateKeyLocation"));
    assert!(ks.contains(&"HostName") && ks.contains(&"ProxyJump"));
    assert!(!ks.contains(&"Match") && !ks.contains(&"Include"));
    assert_eq!(ks.iter().filter(|k| **k == "Host").count(), 1);
}

// read-18: json parses back with jq to the same values.
#[test]
fn read_18_json_parses_back_with_jq() {
    let fx = fixture();
    let r = rows(&fx, &["buildbox", "plain"], &["Host", "hostname", "tags", "proxyjump"]);
    let json = render(&r, Format::Json, false);
    let mut child = Command::new("jq")
        .args(["-r", ".[] | [.Host, .hostname, (.tags | join(\";\")), (.proxyjump // \"NULL\")] | @tsv"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("jq");
    child.stdin.take().unwrap().write_all(json.as_bytes()).unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(out.status.success());
    assert_eq!(
        String::from_utf8(out.stdout).unwrap(),
        "buildbox\t10.0.4.12\tprod;austin;db\tNULL\nplain\tplain.example.com\t\tNULL\n"
    );
}

// read-19: csv parses back with Python's csv module, quoting included.
#[test]
fn read_19_csv_parses_back_with_python() {
    let fx = fixture();
    let r = rows(&fx, &["buildbox", "plain"], &["Host", "location", "IdentityFile", "other"]);
    let csv = render(&r, Format::Csv, false);
    let out = python(
        "import csv,sys,json; rows=list(csv.reader(sys.stdin)); print(json.dumps(rows))",
        &csv,
    );
    assert_eq!(
        out.trim(),
        "[[\"Host\", \"location\", \"IdentityFile\", \"other\"], [\"buildbox\", \"Austin DC, rack 4, U12\", \"~/.ssh/a;~/.ssh/b\", \"owner alice\"], [\"plain\", \"\", \"\", \"\"]]"
    );
}

// read-20: yaml parses back with PyYAML to the same typed values, strings kept as strings.
#[test]
fn read_20_yaml_parses_back_with_pyyaml() {
    let fx = fixture();
    let r = rows(&fx, &["buildbox", "plain"], &["Host", "hostname", "Compression", "Port", "tags", "proxyjump", "IdentityFile"]);
    let yaml = render(&r, Format::Yaml, false);
    let out = python(
        "import yaml,sys,json; print(json.dumps(yaml.safe_load(sys.stdin), sort_keys=True))",
        &yaml,
    );
    assert_eq!(
        out.trim(),
        "[{\"Compression\": \"yes\", \"Host\": \"buildbox\", \"IdentityFile\": [\"~/.ssh/a\", \"~/.ssh/b\"], \"Port\": \"2200\", \"hostname\": \"10.0.4.12\", \"proxyjump\": null, \"tags\": [\"prod\", \"austin\", \"db\"]}, {\"Compression\": null, \"Host\": \"plain\", \"IdentityFile\": null, \"Port\": \"22\", \"hostname\": \"plain.example.com\", \"proxyjump\": null, \"tags\": []}]"
    );
    let yaml = render(&r, Format::Yaml, true);
    let out = python(
        "import yaml,sys,json; print(json.dumps(yaml.safe_load(sys.stdin)))",
        &yaml,
    );
    assert!(out.starts_with("[[\"buildbox\", \"10.0.4.12\", \"yes\", \"2200\""));
}

// read-21: the same keys give the same values through every format.
#[test]
fn read_21_formats_agree() {
    let fx = fixture();
    let r = rows(&fx, &["buildbox", "plain", "lab-1"], &["Host", "hostname", "user", "section"]);
    let json: serde_json::Value = serde_json::from_str(&render(&r, Format::Json, false)).unwrap();
    let csv = render(&r, Format::Csv, false);
    let yaml = python(
        "import yaml,sys,json; print(json.dumps(yaml.safe_load(sys.stdin)))",
        &render(&r, Format::Yaml, false),
    );
    let yaml: serde_json::Value = serde_json::from_str(&yaml).unwrap();
    assert_eq!(json, yaml);
    let csv_rows: Vec<Vec<&str>> = csv.lines().map(|l| l.split(',').collect()).collect();
    for (i, host) in json.as_array().unwrap().iter().enumerate() {
        assert_eq!(csv_rows[i + 1][0], host["Host"].as_str().unwrap());
        assert_eq!(csv_rows[i + 1][1], host["hostname"].as_str().unwrap());
        assert_eq!(csv_rows[i + 1][2], host["user"].as_str().unwrap());
        assert_eq!(csv_rows[i + 1][3], host["section"].as_str().unwrap());
    }
    let txt = render(&r, Format::Txt, true);
    assert_eq!(txt.lines().count(), 12);
}

// read-22: Host resolves to the primary name, not the aliases; the txt line is the file's Host line.
#[test]
fn read_22_host_key() {
    let fx = fixture();
    let c = resolve(&view(&fx, "bb"), "Host");
    assert_eq!(c.value, Value::One(s("buildbox")));
    assert_eq!(c.lines, ["Host buildbox bb"]);
    assert!(!c.missing);
}

// read-23: txt output has no separator between hosts and the exact file lines.
#[test]
fn read_23_txt_is_the_files_lines() {
    let fx = fixture();
    let r = rows(&fx, &["plain", "lab-1"], &["HostName", "User"]);
    assert_eq!(
        render(&r, Format::Txt, false),
        "    HostName plain.example.com\n    User fallback\n    HostName 10.30.0.5\n    User travis\n"
    );
}

// read-24: the view of an included file's host reports that file's absolute path.
#[test]
fn read_24_file_key_is_the_holding_file() {
    let fx = fixture();
    let c = resolve(&view(&fx, "lab-1"), "file");
    assert_eq!(c.value, Value::One(fx.ws.abs(1).display().to_string()));
    assert!(fx.ws.abs(1).ends_with("config.d/lab"));
    let c = resolve(&view(&fx, "plain"), "FILE");
    assert_eq!(c.value, Value::One(fx.ws.abs(0).display().to_string()));
}
