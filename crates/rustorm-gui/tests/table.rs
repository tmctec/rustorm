//! Host table: rendering, sort and filter (ui-2, ui-3, web-gui-1, sort-*,
//! filter-*).

mod common;

use common::*;
use egui_kittest::kittest::Queryable;

fn names(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

/// ui-2: the table shows the 3 fixture hosts with their sections, the
/// title names rustorm, and closing a clean window closes it.
#[test]
fn ui_2_table_renders_three_hosts_with_sections() {
    let f = Fixture::new(&three_hosts_sectioned());
    let mut h = harness(&f.path);
    h.run();
    for host in ["vps", "github", "web-prod"] {
        assert!(shown(&h, host), "row {host} missing");
    }
    // Section cells in the table plus sidebar entries with counts.
    for section in ["personal", "work", "other"] {
        assert!(shown(&h, section), "section cell {section} missing");
    }
    assert!(shown(&h, "personal  1"));
    assert!(shown(&h, "work  1"));
    assert!(shown(&h, "other  1  (catch-all)"));
    assert!(shown(&h, "All hosts  3"));
    let s = h.state();
    assert!(s.title().starts_with("rustorm — "));
    let rows = s.visible_rows();
    let pairs: Vec<(String, Option<String>)> = rows
        .iter()
        .map(|r| (r.name.clone(), r.section.clone()))
        .collect();
    assert_eq!(
        pairs,
        vec![
            ("vps".to_string(), Some("personal".to_string())),
            ("web-prod".to_string(), Some("work".to_string())),
            ("github".to_string(), Some("other".to_string())),
        ]
    );
    // The catch-all is last in the sidebar model too.
    assert_eq!(s.sections().last().unwrap().name, "other");
    assert!(s.sections().last().unwrap().catch_all);
    h.state_mut().request_close();
    h.run();
    assert!(h.state().is_closing());
}

/// ui-3: a missing config shows an empty table without an error and
/// creates nothing on launch.
#[test]
fn ui_3_missing_config_is_empty_and_creates_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("ssh").join("config");
    let mut h = harness(&path);
    h.run();
    assert!(shown(&h, "no hosts yet"));
    assert!(h.state().rows().is_empty());
    assert!(!h.state().status().starts_with("error"));
    assert!(h.state().dialog().is_none());
    assert!(!path.exists());
    assert!(!path.parent().unwrap().exists());
}

/// web-gui-1: hosts listed sorted by name, `Host *` not a row.
#[test]
fn web_gui_1_hosts_sorted_without_defaults() {
    let f = Fixture::new(
        "Host *\n    User me\n\nHost b\n    HostName b.example\n\nHost a\n    HostName a.example\n",
    );
    let mut h = harness(&f.path);
    h.run();
    assert_eq!(h.state().visible_names(), names(&["a", "b"]));
    assert!(!shown(&h, "*"));
    let ya = h.get_by_label("a").rect().top();
    let yb = h.get_by_label("b").rect().top();
    assert!(ya < yb, "a renders above b");
}

fn sort_case(header: &str, asc: &[&str], desc: &[&str]) {
    let f = Fixture::new(&six_hosts());
    let mut h = harness(&f.path);
    h.run();
    assert_eq!(h.state().rows().len(), 6);
    click(&mut h, header);
    assert_eq!(h.state().visible_names(), names(asc), "{header} ascending");
    // The rendered order matches: each row sits below the previous one.
    let tops: Vec<f32> = asc.iter().map(|n| h.get_by_label(n).rect().top()).collect();
    assert!(
        tops.windows(2).all(|w| w[0] < w[1]),
        "{header} rendered order"
    );
    click(&mut h, &format!("{header} ▲"));
    assert_eq!(
        h.state().visible_names(),
        names(desc),
        "{header} descending"
    );
    assert!(shown(&h, &format!("{header} ▼")));
}

/// sort-section: ascending by section, reversed on the second click, the
/// sectionless preamble host last both times.
#[test]
fn sort_section() {
    sort_case(
        "section",
        &["alpha", "bravo", "delta", "charlie", "echo", "loose"],
        &["charlie", "echo", "delta", "alpha", "bravo", "loose"],
    );
}

/// sort-host.
#[test]
fn sort_host() {
    sort_case(
        "host",
        &["alpha", "bravo", "charlie", "delta", "echo", "loose"],
        &["loose", "echo", "delta", "charlie", "bravo", "alpha"],
    );
}

/// sort-user: charlie has no User and sorts last both ways.
#[test]
fn sort_user() {
    sort_case(
        "user",
        &["echo", "alpha", "delta", "bravo", "loose", "charlie"],
        &["loose", "bravo", "alpha", "delta", "echo", "charlie"],
    );
}

/// sort-proxy: hosts without ProxyCommand sort last both ways.
#[test]
fn sort_proxy() {
    sort_case(
        "proxy",
        &["echo", "charlie", "alpha", "loose", "bravo", "delta"],
        &["alpha", "charlie", "echo", "loose", "bravo", "delta"],
    );
}

/// sort-jump: hosts without ProxyJump sort last both ways.
#[test]
fn sort_jump() {
    sort_case(
        "jump",
        &["charlie", "bravo", "echo", "loose", "alpha", "delta"],
        &["echo", "bravo", "charlie", "loose", "alpha", "delta"],
    );
}

fn filter_case(filter: &str, text: &str, expect: &[&str]) {
    let f = Fixture::new(&six_hosts());
    let mut h = harness(&f.path);
    h.run();
    type_into(&mut h, filter, text);
    assert_eq!(h.state().visible_names(), names(expect), "{filter}={text}");
    for n in ["alpha", "bravo", "charlie", "delta", "echo", "loose"] {
        assert_eq!(
            shown(&h, n),
            expect.contains(&n),
            "{n} rendered under {filter}={text}"
        );
    }
    click(&mut h, "Clear filters");
    assert_eq!(
        h.state().visible_names().len(),
        6,
        "clearing restores all rows"
    );
    assert!(shown(&h, "loose") && shown(&h, "alpha"));
}

/// filter-section.
#[test]
fn filter_section() {
    filter_case("filter section", "wo", &["charlie", "echo"]);
}

/// filter-host.
#[test]
fn filter_host() {
    filter_case("filter host", "ha", &["alpha", "charlie"]);
}

/// filter-user.
#[test]
fn filter_user() {
    filter_case("filter user", "dep", &["alpha", "delta"]);
}

/// filter-proxy.
#[test]
fn filter_proxy() {
    filter_case("filter proxy", "%h", &["alpha", "charlie"]);
}

/// filter-jump.
#[test]
fn filter_jump() {
    filter_case("filter jump", "j", &["bravo", "charlie", "echo"]);
}

/// filter-none: nothing matches, the table says so, nothing crashes.
#[test]
fn filter_none() {
    let f = Fixture::new(&six_hosts());
    let mut h = harness(&f.path);
    h.run();
    type_into(&mut h, "filter all", "zzz-nothing");
    assert!(h.state().visible_names().is_empty());
    assert!(shown(&h, "no hosts match"));
    click(&mut h, "Clear all filters");
    assert_eq!(h.state().visible_names().len(), 6);
}

/// filter-combo: section=bob AND user=deploy.
#[test]
fn filter_combo() {
    let f = Fixture::new(&six_hosts());
    let mut h = harness(&f.path);
    h.run();
    type_into(&mut h, "filter section", "bob");
    assert_eq!(h.state().visible_names(), names(&["alpha", "bravo"]));
    type_into(&mut h, "filter user", "deploy");
    assert_eq!(h.state().visible_names(), names(&["alpha"]));
    assert!(
        !shown(&h, "delta"),
        "delta has user deploy but section other"
    );
}

/// The sidebar picks one section exactly.
#[test]
fn sidebar_selects_a_section() {
    let f = Fixture::new(&six_hosts());
    let mut h = harness(&f.path);
    h.run();
    click(&mut h, "work  2");
    assert_eq!(h.state().visible_names(), names(&["charlie", "echo"]));
    click(&mut h, "All hosts  6");
    assert_eq!(h.state().visible_names().len(), 6);
}
