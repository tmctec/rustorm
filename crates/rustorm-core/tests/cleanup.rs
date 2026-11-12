//! `cleanup` in rustorm-core: one test per catalog case cln-1..cln-25
//! (plan coati, step 4).
//!
//! Every workspace lives under a temporary home. Probes are real: TCP
//! listeners on `127.0.0.1` ephemeral ports (works), a port bound and
//! released (refused), a `.invalid` name (dns failure) and the
//! non-routable `10.255.255.1` with a 1 s timeout (timeout). `--auth` runs
//! a stub ssh script written into the temp home, since a real sshd login
//! is proven separately (plan step 9).

use std::fs;
use std::net::TcpListener;
use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime};

use rustorm_core::*;

struct Home {
    dir: tempfile::TempDir,
}

impl Home {
    fn new() -> Home {
        Home {
            dir: tempfile::tempdir().unwrap(),
        }
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

    fn exists(&self, rel: &str) -> bool {
        self.path(rel).exists()
    }

    fn load(&self) -> Workspace {
        Workspace::load_with_home(self.path(".ssh/config"), Some(self.dir.path())).unwrap()
    }

    fn env(&self) -> Env {
        Env {
            user: Some("tester".into()),
            home: Some(self.dir.path().to_path_buf()),
        }
    }

    fn report(&self) -> (Workspace, CleanupReport) {
        let ws = self.load();
        let r = ws.cleanup_report(None, &self.env());
        (ws, r)
    }

    /// Every file under the home with its mtime, backups included.
    fn snapshot(&self) -> Vec<(PathBuf, SystemTime)> {
        fn walk(dir: &std::path::Path, out: &mut Vec<(PathBuf, SystemTime)>) {
            for e in fs::read_dir(dir).unwrap() {
                let e = e.unwrap();
                let p = e.path();
                if p.is_dir() {
                    walk(&p, out);
                } else {
                    out.push((p, e.metadata().unwrap().modified().unwrap()));
                }
            }
        }
        let mut out = Vec::new();
        walk(self.dir.path(), &mut out);
        out.sort();
        out
    }
}

/// A port something listens on, kept open while the listener lives.
fn listening() -> (TcpListener, u16) {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    (l, port)
}

/// A port nothing listens on: bound, then released.
fn closed_port() -> u16 {
    let (l, port) = listening();
    drop(l);
    port
}

fn host(name: &str, hostname: &str, port: u16) -> String {
    format!("Host {name}\n    HostName {hostname}\n    Port {port}\n")
}

const ROOT: &str = "Include ~/.ssh/config.d/*\n\nHost *\n    ServerAliveInterval 30\n";

/// A same-name workspace: `vps` live in `config.d/vps`, its copy in
/// `config.d/vps.bak`.
fn pair_home(live: &str, copy: &str) -> Home {
    let h = Home::new();
    h.write(".ssh/config", ROOT);
    h.write(".ssh/config.d/vps", live);
    h.write(".ssh/config.d/vps.bak", copy);
    h
}

fn probe(r: &mut CleanupReport, prober: &dyn Prober, secs: u64) {
    r.probe(prober, Duration::from_secs(secs), DEFAULT_JOBS, None, &mut |_, _| {});
}

fn statuses(f: &Finding) -> Vec<Option<ProbeStatus>> {
    f.definitions.iter().map(|d| d.status).collect()
}

// TESTMASTER: id=rustorm-core::cln_1_same_name_in_root_and_include_names_live_and_copy tier=? parallel=no
#[test]
fn cln_1_same_name_in_root_and_include_names_live_and_copy() {
    let h = Home::new();
    h.write(
        ".ssh/config",
        "Include ~/.ssh/config.d/*\n\nHost vps\n    HostName vps.example.com\n    User root\n",
    );
    h.write(".ssh/config.d/vps", "Host vps\n    HostName vps.example.com\n    User admin\n");
    let before = h.snapshot();
    let (_, r) = h.report();
    assert_eq!(r.findings.len(), 1);
    let f = &r.findings[0];
    assert_eq!(f.kind, FindingKind::SameName);
    assert_eq!(f.names, ["vps"]);
    assert_eq!(f.definitions[0].role, Role::Live);
    assert_eq!(f.definitions[0].block.label, "~/.ssh/config.d/vps");
    assert_eq!(f.definitions[1].role, Role::Copy);
    assert_eq!(f.definitions[1].block.label, "~/.ssh/config");
    // No probe ran: every status is unset and nothing changed on disk.
    assert_eq!(statuses(f), [None, None]);
    assert_eq!(f.recommendation.text, "probe to decide");
    assert_eq!(h.snapshot(), before);
}

// TESTMASTER: id=rustorm-core::cln_2_name_twice_in_one_file_marks_the_later_block_dead tier=? parallel=no
#[test]
fn cln_2_name_twice_in_one_file_marks_the_later_block_dead() {
    let h = Home::new();
    h.write(
        ".ssh/config",
        "Host nas\n    HostName 192.168.1.10\n    User travis\n\nHost nas\n    HostName 192.168.1.10\n",
    );
    let (_, r) = h.report();
    assert_eq!(r.findings.len(), 1);
    let f = &r.findings[0];
    assert_eq!((f.kind, f.names.clone()), (FindingKind::Dead, vec!["nas".to_string()]));
    assert_eq!(f.definitions[0].role, Role::First);
    assert_eq!((f.definitions[1].role, f.definitions[1].block.line), (Role::Dead, 5));
    assert_eq!(f.recommendation.action, Some(Action::DropDead));
    assert_eq!(f.recommendation.text, "drop the dead block at ~/.ssh/config:5");
}

// TESTMASTER: id=rustorm-core::cln_3_two_names_one_endpoint_are_same_target tier=? parallel=no
#[test]
fn cln_3_two_names_one_endpoint_are_same_target() {
    let h = Home::new();
    h.write(
        ".ssh/config",
        "Host web\n    HostName web.example.com\n    User deploy\n\nHost web-prod\n    HostName WEB.example.com\n    User deploy\n    Port 22\n",
    );
    let (_, r) = h.report();
    assert_eq!(r.findings.len(), 1);
    let f = &r.findings[0];
    assert_eq!(f.kind, FindingKind::SameTarget);
    assert_eq!(f.names, ["web", "web-prod"]);
    assert_eq!(f.target.as_deref(), Some("deploy@web.example.com:22"));
    assert_eq!(f.heading(), "same target: web, web-prod (deploy@web.example.com:22)");
    // The HostName case and Port 22 differ, so the names do not fold.
    assert_eq!(f.recommendation.action, None);
    assert_eq!(f.recommendation.text, "settings differ (HostName, Port); left as is");
}

// TESTMASTER: id=rustorm-core::cln_4_no_duplicates_reports_none_and_probes_nothing tier=? parallel=no
#[test]
fn cln_4_no_duplicates_reports_none_and_probes_nothing() {
    let h = Home::new();
    h.write(
        ".ssh/config",
        "Host *\n    User me\n\nHost a\n    HostName a.example.com\n\nHost b\n    HostName b.example.com\n",
    );
    let (_, mut r) = h.report();
    assert!(r.findings.is_empty());
    assert_eq!(r.text(), "no duplicates in 2 hosts.\n");
    assert!(r.probe_targets(DEFAULT_TIMEOUT).is_empty());
    struct Panics;
    impl Prober for Panics {
        fn probe(&self, _: &ProbeTarget) -> ProbeResult {
            panic!("no probe may run without a finding");
        }
    }
    probe(&mut r, &Panics, 1);
}

// TESTMASTER: id=rustorm-core::cln_5_same_hostname_other_port_or_user_is_not_same_target tier=? parallel=no
#[test]
fn cln_5_same_hostname_other_port_or_user_is_not_same_target() {
    let h = Home::new();
    h.write(
        ".ssh/config",
        "Host a\n    HostName box.example.com\n\nHost b\n    HostName box.example.com\n    Port 2222\n\nHost c\n    HostName box.example.com\n    User other\n",
    );
    let (_, r) = h.report();
    assert!(r.findings.is_empty(), "{}", r.text());
}

// TESTMASTER: id=rustorm-core::cln_6_listening_port_works tier=? parallel=no
#[test]
fn cln_6_listening_port_works() {
    let (_l, port) = listening();
    let h = pair_home(&host("vps", "127.0.0.1", port), &host("vps", "127.0.0.1", closed_port()));
    let (_, mut r) = h.report();
    let started = Instant::now();
    probe(&mut r, &TcpProber, 2);
    assert!(started.elapsed() < Duration::from_secs(2));
    assert_eq!(r.findings[0].definitions[0].status, Some(ProbeStatus::Works));
}

// TESTMASTER: id=rustorm-core::cln_7_closed_port_is_refused tier=? parallel=no
#[test]
fn cln_7_closed_port_is_refused() {
    let (_l, port) = listening();
    let h = pair_home(&host("vps", "127.0.0.1", closed_port()), &host("vps", "127.0.0.1", port));
    let (_, mut r) = h.report();
    let started = Instant::now();
    probe(&mut r, &TcpProber, 2);
    assert!(started.elapsed() < Duration::from_secs(2));
    let live = &r.findings[0].definitions[0];
    assert_eq!(live.status, Some(ProbeStatus::Refused), "{:?}", live.detail);
    assert_eq!(live.status_label(), "refused");
}

// TESTMASTER: id=rustorm-core::cln_8_invalid_name_is_dns_failure tier=? parallel=no
#[test]
fn cln_8_invalid_name_is_dns_failure() {
    let h = pair_home(&host("vps", "old-vps.invalid", 22), &host("vps", "older-vps.invalid", 22));
    let (_, mut r) = h.report();
    let started = Instant::now();
    probe(&mut r, &TcpProber, 2);
    assert!(started.elapsed() < Duration::from_secs(2));
    assert_eq!(
        statuses(&r.findings[0]),
        [Some(ProbeStatus::DnsFailure), Some(ProbeStatus::DnsFailure)]
    );
    assert_eq!(r.findings[0].definitions[0].status_label(), "dns failure");
}

// TESTMASTER: id=rustorm-core::cln_9_non_routable_address_times_out_within_the_bound tier=? parallel=no
#[test]
fn cln_9_non_routable_address_times_out_within_the_bound() {
    let h = pair_home(&host("vps", "10.255.255.1", 22), &host("vps", "10.255.255.1", 2222));
    let (_, mut r) = h.report();
    let started = Instant::now();
    probe(&mut r, &TcpProber, 1);
    let took = started.elapsed();
    assert!(took < Duration::from_secs(2), "probes took {took:?}");
    assert_eq!(
        statuses(&r.findings[0]),
        [Some(ProbeStatus::Timeout), Some(ProbeStatus::Timeout)]
    );
}

// TESTMASTER: id=rustorm-core::cln_10_each_definition_is_probed_alone_through_its_block_and_host_star tier=? parallel=no
#[test]
fn cln_10_each_definition_is_probed_alone_through_its_block_and_host_star() {
    let (_l, port) = listening();
    let h = pair_home(&host("vps", "gone.invalid", port), &host("vps", "127.0.0.1", port));
    let (_, mut r) = h.report();
    let f = &r.findings[0];
    let (live, copy) = (&f.definitions[0], &f.definitions[1]);
    assert_eq!(
        live.probe_config,
        format!("{}\nHost *\n    ServerAliveInterval 30\n", host("vps", "gone.invalid", port))
    );
    assert_eq!(
        copy.probe_config,
        format!("{}\nHost *\n    ServerAliveInterval 30\n", host("vps", "127.0.0.1", port))
    );
    assert!(!copy.probe_config.contains("Include"));
    probe(&mut r, &TcpProber, 2);
    let f = &r.findings[0];
    // ssh reads the live block first, yet the copy is tested too.
    assert_eq!(
        statuses(f),
        [Some(ProbeStatus::DnsFailure), Some(ProbeStatus::Works)]
    );
    assert_eq!(f.recommendation.action, Some(Action::TakeCopy));
}

// TESTMASTER: id=rustorm-core::cln_11_proxyjump_needs_auth_without_a_tcp_probe tier=? parallel=no
#[test]
fn cln_11_proxyjump_needs_auth_without_a_tcp_probe() {
    // The HostName would be a dns failure if a TCP probe ran.
    let h = pair_home(
        "Host vps\n    HostName inner.invalid\n    ProxyJump bastion\n",
        "Host vps\n    HostName inner.invalid\n    ProxyCommand ssh -W %h:%p bastion\n",
    );
    let (_, mut r) = h.report();
    probe(&mut r, &TcpProber, 1);
    let f = &r.findings[0];
    assert_eq!(
        statuses(f),
        [Some(ProbeStatus::NeedsAuth), Some(ProbeStatus::NeedsAuth)]
    );
    assert_eq!(f.definitions[0].detail.as_deref(), Some("ProxyJump bastion"));
    assert_eq!(f.definitions[0].status_label(), "needs --auth");
    assert_eq!(f.recommendation.text, "probe with --auth to decide");
}

/// A stub ssh: logs its arguments and the temp config's mode and text,
/// then exits 0 unless the config says `User admin`, where it prints a
/// login failure and exits 255.
fn stub_ssh(h: &Home) -> (PathBuf, PathBuf) {
    let log = h.path("ssh.log");
    let stub = h.path("bin/ssh");
    h.write(
        "bin/ssh",
        &format!(
            "#!/bin/sh\nlog={}\necho \"ARGS $*\" >> \"$log\"\nls -l \"$2\" | cut -c1-10 >> \"$log\"\ncat \"$2\" >> \"$log\"\nif grep -q 'User admin' \"$2\"; then echo \"$7: Permission denied (publickey).\" >&2; exit 255; fi\nexit 0\n",
            log.display()
        ),
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&stub, fs::Permissions::from_mode(0o755)).unwrap();
    }
    (stub, log)
}

// TESTMASTER: id=rustorm-core::cln_12_auth_runs_batchmode_ssh_through_a_private_temp_config tier=? parallel=no
#[test]
fn cln_12_auth_runs_batchmode_ssh_through_a_private_temp_config() {
    let h = pair_home(
        "Host vps\n    HostName vps.example.com\n    User admin\n",
        "Host vps\n    HostName vps.example.com\n    User travis\n",
    );
    let (stub, log) = stub_ssh(&h);
    let (_, mut r) = h.report();
    let prober = SshProber { program: stub };
    let started = Instant::now();
    probe(&mut r, &prober, 3);
    assert!(started.elapsed() < Duration::from_secs(2));
    let f = &r.findings[0];
    assert_eq!(
        statuses(f),
        [Some(ProbeStatus::AuthFailure), Some(ProbeStatus::Works)]
    );
    assert_eq!(
        f.definitions[0].detail.as_deref(),
        Some("vps: Permission denied (publickey).")
    );
    let text = fs::read_to_string(&log).unwrap();
    let args: Vec<&str> = text.lines().filter(|l| l.starts_with("ARGS ")).collect();
    assert_eq!(args.len(), 2);
    for a in &args {
        let words: Vec<&str> = a.split(' ').collect();
        assert_eq!(words[1], "-F");
        assert_eq!(
            &words[3..],
            ["-o", "BatchMode=yes", "-o", "ConnectTimeout=3", "vps", "true"]
        );
        // The temp config is gone once the probe is done.
        assert!(!std::path::Path::new(words[2]).exists());
    }
    assert_eq!(text.matches("-rw-------").count(), 2, "{text}");
    assert!(text.contains("User admin") && text.contains("User travis"));
    assert!(!text.contains("Include"));
}

// TESTMASTER: id=rustorm-core::cln_13_live_fails_copy_works_takes_the_copy_with_a_backup tier=? parallel=no
#[test]
fn cln_13_live_fails_copy_works_takes_the_copy_with_a_backup() {
    let (_l, port) = listening();
    let live = format!("# note: old\n{}", host("vps", "127.0.0.1", closed_port()));
    let copy = host("vps", "127.0.0.1", port);
    let h = pair_home(&live, &copy);
    let (_, mut r) = h.report();
    probe(&mut r, &TcpProber, 2);
    assert_eq!(r.applicable(), [0]);
    let mut ws = h.load();
    let change = ws.apply_cleanup(&r, &r.applicable()).unwrap();
    assert_eq!(
        change.messages,
        ["vps: moved the copy from ~/.ssh/config.d/vps.bak into ~/.ssh/config.d/vps; only the copy works."]
    );
    let backups: Vec<PathBuf> = change.files.iter().map(|&i| ws.backup_path_for(i)).collect();
    ws.save(WriteOptions::default()).unwrap();
    assert_eq!(h.read(".ssh/config.d/vps"), copy);
    assert_eq!(h.read(".ssh/config.d/vps.bak"), "");
    for b in backups {
        assert!(b.exists(), "backup {} missing", b.display());
    }
    assert_eq!(fs::read_to_string(h.path(".ssh/config.d/.vps~")).unwrap(), live);
}

// TESTMASTER: id=rustorm-core::cln_14_live_works_copy_fails_drops_the_copy_block tier=? parallel=no
#[test]
fn cln_14_live_works_copy_fails_drops_the_copy_block() {
    let (_l, port) = listening();
    let live = host("vps", "127.0.0.1", port);
    let copy = format!(
        "{}\nHost printer\n    HostName 192.168.1.20\n",
        host("vps", "127.0.0.1", closed_port())
    );
    let h = pair_home(&live, &copy);
    let (_, mut r) = h.report();
    probe(&mut r, &TcpProber, 2);
    assert_eq!(r.findings[0].recommendation.action, Some(Action::DropCopy));
    let mut ws = h.load();
    let change = ws.apply_cleanup(&r, &[0]).unwrap();
    assert_eq!(
        change.messages,
        ["vps: kept ~/.ssh/config.d/vps and dropped the copy from ~/.ssh/config.d/vps.bak; only live works."]
    );
    ws.save(WriteOptions::default()).unwrap();
    assert_eq!(h.read(".ssh/config.d/vps"), live);
    assert_eq!(h.read(".ssh/config.d/vps.bak"), "Host printer\n    HostName 192.168.1.20\n");
}

// TESTMASTER: id=rustorm-core::cln_15_both_work_and_differ_stays_undecided_for_reconcile tier=? parallel=no
#[test]
fn cln_15_both_work_and_differ_stays_undecided_for_reconcile() {
    let (_a, pa) = listening();
    let (_b, pb) = listening();
    let h = pair_home(&host("vps", "127.0.0.1", pa), &host("vps", "127.0.0.1", pb));
    let before = h.snapshot();
    let (_, mut r) = h.report();
    probe(&mut r, &TcpProber, 2);
    let f = &r.findings[0];
    assert_eq!(statuses(f), [Some(ProbeStatus::Works), Some(ProbeStatus::Works)]);
    assert_eq!(f.recommendation.action, None);
    assert_eq!(
        f.recommendation.text,
        "both work and differ; decide with rustorm reconcile"
    );
    assert!(r.applicable().is_empty());
    let mut ws = h.load();
    assert_eq!(ws.apply_cleanup(&r, &[0]).unwrap_err().exit_code(), 2);
    assert!(ws.modified().is_empty());
    assert_eq!(h.snapshot(), before);
}

// TESTMASTER: id=rustorm-core::cln_16_both_fail_gets_no_recommendation tier=? parallel=no
#[test]
fn cln_16_both_fail_gets_no_recommendation() {
    let h = pair_home(
        &host("vps", "127.0.0.1", closed_port()),
        &host("vps", "gone.invalid", 22),
    );
    let (_, mut r) = h.report();
    probe(&mut r, &TcpProber, 2);
    let f = &r.findings[0];
    assert_eq!(
        statuses(f),
        [Some(ProbeStatus::Refused), Some(ProbeStatus::DnsFailure)]
    );
    assert_eq!(f.recommendation.action, None);
    assert_eq!(f.recommendation.text, "both fail; nothing to recommend");
    assert!(r.applicable().is_empty());
}

// TESTMASTER: id=rustorm-core::cln_17_identical_copy_and_dead_block_are_dropped_live_untouched tier=? parallel=no
#[test]
fn cln_17_identical_copy_and_dead_block_are_dropped_live_untouched() {
    let live = "Host vps\n    HostName vps.example.com\n\nHost nas\n    HostName nas.local\n    User me\n\nHost nas\n    HostName nas.local\n";
    let copy = "# stray\nHost vps\n    hostname   vps.example.com\n";
    let h = pair_home(live, copy);
    let (_, r) = h.report();
    let kinds: Vec<FindingKind> = r.findings.iter().map(|f| f.kind).collect();
    assert_eq!(kinds, [FindingKind::SameName, FindingKind::Dead]);
    assert_eq!(r.applicable(), [0, 1]);
    let mut ws = h.load();
    let change = ws.apply_cleanup(&r, &r.applicable()).unwrap();
    assert_eq!(
        change.messages,
        [
            "vps: dropped the identical copy from ~/.ssh/config.d/vps.bak.",
            "nas: dropped the dead block at ~/.ssh/config.d/vps:8.",
        ]
    );
    ws.save(WriteOptions::default()).unwrap();
    assert_eq!(
        h.read(".ssh/config.d/vps"),
        "Host vps\n    HostName vps.example.com\n\nHost nas\n    HostName nas.local\n    User me\n"
    );
    assert_eq!(h.read(".ssh/config.d/vps.bak"), "");
    assert!(h.load().cleanup_report(None, &h.env()).findings.is_empty());
}

// TESTMASTER: id=rustorm-core::cln_18_report_and_probe_write_nothing tier=? parallel=no
#[test]
fn cln_18_report_and_probe_write_nothing() {
    let (_l, port) = listening();
    let h = pair_home(
        &format!("{}\nHost nas\n    HostName n\n\nHost nas\n    HostName n\n", host("vps", "127.0.0.1", closed_port())),
        &host("vps", "127.0.0.1", port),
    );
    let before = h.snapshot();
    let (_, mut r) = h.report();
    probe(&mut r, &TcpProber, 2);
    assert_eq!(r.applicable().len(), 2);
    assert_eq!(h.snapshot(), before, "a file or backup changed");
    assert!(!h.exists(".ssh/config~") && !h.exists(".ssh/config.d/.vps~"));
}

const TWINS: &str = "Host web\n    HostName web.example.com\n    User deploy\n\n# prod alias\nHost web-prod\n    HostName web.example.com\n    User deploy\n";

// TESTMASTER: id=rustorm-core::cln_19_fold_makes_the_second_name_an_alias_and_removes_its_block tier=? parallel=no
#[test]
fn cln_19_fold_makes_the_second_name_an_alias_and_removes_its_block() {
    let h = Home::new();
    h.write(".ssh/config", TWINS);
    let (mut ws, r) = h.report();
    assert_eq!(r.findings[0].recommendation.action, Some(Action::Fold));
    assert_eq!(
        r.findings[0].recommendation.text,
        "fold web-prod into web with --fold web-prod"
    );
    let change = ws.fold_host(&r, "web-prod").unwrap();
    assert_eq!(change.value, "web");
    assert_eq!(change.messages, ["web-prod folded into web in ~/.ssh/config."]);
    ws.save(WriteOptions::default()).unwrap();
    assert_eq!(
        h.read(".ssh/config"),
        "Host web web-prod\n    HostName web.example.com\n    User deploy\n"
    );
    assert_eq!(h.read(".ssh/config~"), TWINS);
}

// TESTMASTER: id=rustorm-core::cln_20_fold_refuses_when_directives_differ tier=? parallel=no
#[test]
fn cln_20_fold_refuses_when_directives_differ() {
    let h = Home::new();
    let text = "Host web\n    HostName web.example.com\n    User deploy\n\nHost web-prod\n    HostName web.example.com\n    User deploy\n    ForwardAgent yes\n";
    h.write(".ssh/config", text);
    let (mut ws, r) = h.report();
    let err = ws.fold_host(&r, "web-prod").unwrap_err();
    assert_eq!(err.exit_code(), 2);
    assert_eq!(
        err.to_string(),
        "web-prod differs from web in ForwardAgent; not folded."
    );
    assert!(ws.modified().is_empty());
    assert_eq!(h.read(".ssh/config"), text);
}

// TESTMASTER: id=rustorm-core::cln_21_fold_refuses_when_metadata_differs tier=? parallel=no
#[test]
fn cln_21_fold_refuses_when_metadata_differs() {
    let h = Home::new();
    let text = "# location: rack 1\nHost web\n    HostName web.example.com\n\n# location: rack 2\nHost web-prod\n    HostName web.example.com\n";
    h.write(".ssh/config", text);
    let (mut ws, r) = h.report();
    assert_eq!(r.findings[0].recommendation.action, None);
    let err = ws.fold_host(&r, "web-prod").unwrap_err();
    assert_eq!(err.exit_code(), 2);
    assert_eq!(err.to_string(), "web-prod differs from web in location; not folded.");
    assert!(ws.modified().is_empty());
    assert_eq!(h.read(".ssh/config"), text);
}

// TESTMASTER: id=rustorm-core::cln_22_a_file_changed_since_the_report_is_refused_as_stale tier=? parallel=no
#[test]
fn cln_22_a_file_changed_since_the_report_is_refused_as_stale() {
    let copy = "Host vps\n    HostName vps.example.com\n";
    let h = pair_home(copy, copy);
    let (_, r) = h.report();
    let edited = "Host vps\n    HostName vps.example.com\n    User edited\n";
    h.write(".ssh/config.d/vps.bak", edited);
    let mut ws = h.load();
    let err = ws.apply_cleanup(&r, &[0]).unwrap_err();
    assert_eq!(err.exit_code(), 1);
    assert_eq!(err.to_string(), "vps changed since the report; run cleanup again.");
    assert!(ws.modified().is_empty());
    assert_eq!(h.read(".ssh/config.d/vps.bak"), edited);
    // A fold of a block edited since the report is refused alike.
    let t = Home::new();
    t.write(".ssh/config", TWINS);
    let (_, r) = t.report();
    t.write(".ssh/config", &TWINS.replace("deploy", "ops"));
    let mut ws = t.load();
    assert_eq!(ws.fold_host(&r, "web-prod").unwrap_err().exit_code(), 1);
}

// TESTMASTER: id=rustorm-core::cln_23_an_unwritable_target_writes_no_file tier=? parallel=no
#[test]
#[cfg(unix)]
fn cln_23_an_unwritable_target_writes_no_file() {
    use std::os::unix::fs::PermissionsExt;
    let (_l, port) = listening();
    let live = host("vps", "127.0.0.1", closed_port());
    let copy = host("vps", "127.0.0.1", port);
    let h = pair_home(&live, &copy);
    let (_, mut r) = h.report();
    probe(&mut r, &TcpProber, 2);
    let bak = h.path(".ssh/config.d/vps.bak");
    fs::set_permissions(&bak, fs::Permissions::from_mode(0o444)).unwrap();
    let mut ws = h.load();
    ws.apply_cleanup(&r, &r.applicable()).unwrap();
    let err = ws.save(WriteOptions::default()).unwrap_err();
    fs::set_permissions(&bak, fs::Permissions::from_mode(0o644)).unwrap();
    assert_eq!(err.exit_code(), 3);
    assert_eq!(h.read(".ssh/config.d/vps"), live);
    assert_eq!(h.read(".ssh/config.d/vps.bak"), copy);
    assert!(!h.exists(".ssh/config.d/.vps~") && !h.exists(".ssh/config.d/.vps.bak~"));
}

// TESTMASTER: id=rustorm-core::cln_24_json_carries_kind_names_files_and_statuses tier=? parallel=no
#[test]
fn cln_24_json_carries_kind_names_files_and_statuses() {
    let (_l, port) = listening();
    let h = pair_home(&host("vps", "127.0.0.1", port), &host("vps", "gone.invalid", port));
    h.write(".ssh/config", &format!("{ROOT}\n{TWINS}"));
    let (_, mut r) = h.report();
    probe(&mut r, &TcpProber, 2);
    let v = serde_json::to_value(&r).unwrap();
    assert_eq!(v["hosts"], 4);
    let f = v["findings"].as_array().unwrap();
    assert_eq!(f.len(), 2);
    assert_eq!(f[0]["kind"], "same_name");
    assert_eq!(f[0]["names"], serde_json::json!(["vps"]));
    assert_eq!(f[0]["target"], serde_json::Value::Null);
    let d = &f[0]["definitions"];
    assert_eq!(d[0]["role"], "live");
    assert_eq!(
        d[0]["file"],
        h.path(".ssh/config.d/vps").display().to_string()
    );
    assert_eq!(d[0]["line"], 1);
    assert_eq!(d[0]["status"], "works");
    assert_eq!(d[1]["status"], "dns_failure");
    assert!(d[1]["detail"].is_string());
    assert_eq!(d[0]["target"], format!("tester@127.0.0.1:{port}"));
    assert_eq!(f[0]["recommendation"]["action"], "drop_copy");
    assert_eq!(f[1]["kind"], "same_target");
    assert_eq!(f[1]["target"], "deploy@web.example.com:22");
    assert_eq!(f[1]["recommendation"]["action"], "fold");
    let keys: Vec<&String> = d[0].as_object().unwrap().keys().collect();
    let mut want = vec!["role", "name", "file", "line", "section", "text", "target", "status", "detail"];
    want.sort_unstable();
    let mut got: Vec<&str> = keys.iter().map(|k| k.as_str()).collect();
    got.sort_unstable();
    assert_eq!(got, want);
}

// TESTMASTER: id=rustorm-core::cln_25_findings_remain_until_applied_then_none tier=? parallel=no
#[test]
fn cln_25_findings_remain_until_applied_then_none() {
    // The exit codes themselves (1, 0, and 2 for --no-probe --auth) are
    // asserted against the binary in rustorm's cli_cases (cln_25_*).
    let h = pair_home(
        "Host vps\n    HostName v\n",
        "Host vps\n    HostName v\n\nHost nas\n    HostName n\n\nHost nas\n    HostName n\n",
    );
    let (mut ws, r) = h.report();
    assert_eq!(r.findings.len(), 2);
    assert_eq!(r.summary(), "2 findings in 4 hosts.");
    let change = ws.apply_cleanup(&r, &r.applicable()).unwrap();
    assert_eq!(change.value, [0, 1]);
    ws.save(WriteOptions::default()).unwrap();
    let after = h.load().cleanup_report(None, &h.env());
    assert!(after.findings.is_empty());
    assert_eq!(remaining_findings(after.findings.len()), "no findings remain.");
    assert_eq!(remaining_findings(1), "1 finding remains.");
    assert_eq!(remaining_findings(3), "3 findings remain.");
}

// TESTMASTER: id=rustorm-core::cln_probes_stream_and_cancel tier=? parallel=no
#[test]
fn cln_probes_stream_each_result_and_stop_on_cancel() {
    use std::sync::atomic::AtomicBool;
    struct Fixed;
    impl Prober for Fixed {
        fn probe(&self, t: &ProbeTarget) -> ProbeResult {
            let status = if t.endpoint.port == 1 {
                ProbeStatus::Works
            } else {
                ProbeStatus::Refused
            };
            ProbeResult { status, detail: None }
        }
    }
    let h = pair_home(&host("vps", "a", 1), &host("vps", "a", 2));
    let (_, mut r) = h.report();
    let mut seen = Vec::new();
    r.probe(&Fixed, DEFAULT_TIMEOUT, 1, None, &mut |t, res| {
        seen.push((t.endpoint.port, res.status))
    });
    seen.sort_by_key(|(p, _)| *p);
    assert_eq!(seen, [(1, ProbeStatus::Works), (2, ProbeStatus::Refused)]);
    assert_eq!(r.findings[0].recommendation.action, Some(Action::DropCopy));
    let cancel = AtomicBool::new(true);
    let (_, mut r) = h.report();
    let mut n = 0;
    r.probe(&Fixed, DEFAULT_TIMEOUT, 2, Some(&cancel), &mut |_, _| n += 1);
    assert_eq!(n, 0);
}
