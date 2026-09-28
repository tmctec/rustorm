//! Shared fixtures and harness helpers for the rustorm-gui kittest suite.
#![allow(dead_code)]

use std::path::{Path, PathBuf};

use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;
use rustorm_core::{Config, Env};
use rustorm_gui::App;

/// The environment every test uses: `$USER` unset so a host without `User`
/// has no user at all, which makes "missing sorts last" observable.
pub fn env() -> Env {
    Env {
        user: None,
        home: None,
    }
}

/// Three hosts (vps, github, web-prod) in three sections, with a
/// preamble comment, `Host *`, comments above hosts, an alias and an
/// identity file.
pub const THREE_HOSTS: &str = "\
# laptop ssh config
Host *
    ServerAliveInterval 60

# the main box
Host vps v
    HostName vps.example.com
    User root
    Port 2222
    IdentityFile ~/.ssh/vps.pem

Host github
    HostName github.com
    User git

# production web
Host web-prod
    HostName webprod.example.com
    User web
    ProxyCommand ssh -W %h:%p bastion
    ProxyJump jumpbox
";

/// Sections the three-host fixture: vps -> personal, web-prod -> work,
/// github stays in the catch-all `other`. Built through the core.
pub fn three_hosts_sectioned() -> String {
    let mut c = Config::parse(THREE_HOSTS).unwrap();
    c.move_host("vps", None, Some("personal")).unwrap();
    c.move_host("web-prod", None, Some("work")).unwrap();
    c.render()
}

/// Six hosts differing in section, user, proxy and jump; `loose` sits in
/// the preamble, so it has no section.
pub fn six_hosts() -> String {
    let base = "\
Host *
    ServerAliveInterval 60

Host alpha
    HostName alpha.example.com
    User deploy
    ProxyCommand ssh -W %h:%p b1

Host bravo
    HostName bravo.example.com
    User root
    ProxyJump j2

Host charlie
    HostName charlie.example.com
    ProxyCommand nc -X 5 %h %p
    ProxyJump j1

Host delta
    HostName delta.example.com
    User deploy

Host echo
    HostName echo.example.com
    User admin
    ProxyCommand corkscrew
    ProxyJump j3
";
    let mut c = Config::parse(base).unwrap();
    c.move_host("alpha", None, Some("bob")).unwrap();
    c.move_host("bravo", None, Some("bob")).unwrap();
    c.move_host("charlie", None, Some("work")).unwrap();
    c.move_host("echo", None, Some("work")).unwrap();
    let text = c.render();
    text.replacen(
        "    ServerAliveInterval 60\n",
        "    ServerAliveInterval 60\n\nHost loose\n    HostName loose.example.com\n    User zed\n",
        1,
    )
}

/// A temp dir holding `config` with `text`.
pub struct Fixture {
    pub dir: tempfile::TempDir,
    pub path: PathBuf,
}

impl Fixture {
    pub fn new(text: &str) -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config");
        std::fs::write(&path, text).unwrap();
        Fixture { dir, path }
    }

    pub fn read(&self) -> String {
        std::fs::read_to_string(&self.path).unwrap()
    }

    pub fn backup(&self) -> PathBuf {
        backup_of(&self.path)
    }
}

pub fn backup_of(path: &Path) -> PathBuf {
    rustorm_core::backup_path(path)
}

/// A harness driving the whole app at desktop size.
pub fn harness(path: &Path) -> Harness<'static, App> {
    let app = App::with_env(path, env()).unwrap();
    Harness::builder()
        .with_size([1500.0, 950.0])
        .build_ui_state(|ui, app: &mut App| app.show(ui), app)
}

/// Clicks the text field labelled `label` (the last one when several
/// match, which is the one in a dialog) and types `text` into it.
pub fn type_into(h: &mut Harness<'static, App>, label: &str, text: &str) {
    h.get_all_by_label(label).last().unwrap().click();
    h.run();
    h.get_all_by_label(label).last().unwrap().type_text(text);
    h.run();
}

/// Replaces the content of the text field labelled `label`.
pub fn replace_in(h: &mut Harness<'static, App>, label: &str, text: &str) {
    h.get_all_by_label(label).last().unwrap().click();
    h.run();
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    h.run();
    if text.is_empty() {
        h.key_press(egui::Key::Backspace);
    } else {
        h.get_all_by_label(label).last().unwrap().type_text(text);
    }
    h.run();
}

/// Clicks the last node labelled `label`.
pub fn click(h: &mut Harness<'static, App>, label: &str) {
    h.get_all_by_label(label).last().unwrap().click();
    h.run();
}

/// True when some node carries exactly `label`.
pub fn shown(h: &Harness<'static, App>, label: &str) -> bool {
    h.query_by_label(label).is_some() || h.query_all_by_label(label).next().is_some()
}

/// True when some node's label contains `text`.
pub fn shown_contains(h: &Harness<'static, App>, text: &str) -> bool {
    h.query_all_by_label_contains(text).next().is_some()
}
