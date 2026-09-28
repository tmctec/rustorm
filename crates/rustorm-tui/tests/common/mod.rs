//! Shared helpers for the rustorm-tui TestBackend tests.
#![allow(dead_code)]

use std::path::{Path, PathBuf};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::Terminal;
use rustorm_core::{AddSpec, Config, Env, HostSelector};
use rustorm_tui::{App, Options, Theme};

pub const W: u16 = 150;
pub const H: u16 = 60;

pub fn env() -> Env {
    Env {
        user: Some("tester".into()),
        home: None,
    }
}

pub fn options() -> Options {
    Options {
        no_backup: false,
        theme: Theme::full(),
        env: env(),
    }
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
    pub fn backup(&self) -> Option<String> {
        std::fs::read_to_string(rustorm_core::backup_path(&self.path)).ok()
    }
    pub fn app(&self) -> App {
        App::with_options(&self.path, options()).unwrap()
    }
}

pub fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

pub fn ctrl(c: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
}

pub fn press(app: &mut App, c: char) {
    app.handle(key(KeyCode::Char(c)));
}

pub fn typ(app: &mut App, s: &str) {
    for c in s.chars() {
        if c == '\n' {
            app.handle(key(KeyCode::Enter));
        } else {
            press(app, c);
        }
    }
}

pub fn backspace(app: &mut App, n: usize) {
    for _ in 0..n {
        app.handle(key(KeyCode::Backspace));
    }
}

pub fn draw(app: &mut App) -> Buffer {
    draw_sized(app, W, H)
}

pub fn draw_sized(app: &mut App, w: u16, h: u16) -> Buffer {
    let mut t = Terminal::new(TestBackend::new(w, h)).unwrap();
    t.draw(|f| app.render(f)).unwrap();
    t.backend().buffer().clone()
}

pub fn lines(buf: &Buffer) -> Vec<String> {
    (0..buf.area.height)
        .map(|y| {
            (0..buf.area.width)
                .map(|x| buf[(x, y)].symbol())
                .collect::<String>()
        })
        .collect()
}

pub fn screen(app: &mut App) -> String {
    lines(&draw(app)).join("\n")
}

/// Lines above the editor pane (sections, table).
pub fn table_lines(buf: &Buffer) -> Vec<String> {
    let all = lines(buf);
    let end = all
        .iter()
        .position(|l| l.contains("Editor "))
        .unwrap_or(all.len());
    all[..end].to_vec()
}

/// Host names in the order the table draws them.
pub fn drawn_order(app: &mut App, names: &[&str]) -> Vec<String> {
    let buf = draw(app);
    let mut out = Vec::new();
    for l in table_lines(&buf) {
        let tokens: Vec<&str> = l
            .split(|c: char| c.is_whitespace() || "│║".contains(c))
            .filter(|t| !t.is_empty())
            .collect();
        if let Some(n) = tokens.iter().find(|t| names.contains(t)) {
            out.push(n.to_string());
        }
    }
    out
}

/// Moves the table selection to `name`.
pub fn select(app: &mut App, name: &str) {
    press(app, 'g');
    for _ in 0..50 {
        if app.selected() == Some(name) {
            return;
        }
        press(app, 'j');
    }
    panic!("{name} not in table");
}

/// The 3-host sectioned fixture: Host *, a comment, sections bob and other.
pub fn three_hosts() -> String {
    let mut c = Config::parse(
        "# my ssh config\nHost *\n    ServerAliveInterval 60\n\n# the web box\nHost web\n    HostName web.example.com\n    User deploy\n    ProxyCommand ssh -W %h:%p bastion\n\nHost db\n    HostName db.internal\n    User postgres\n    ProxyJump bastion\n",
    )
    .unwrap();
    c.add(
        &AddSpec {
            name: "vps".into(),
            uri: "root@vps.example.com:2222".into(),
            identity: Some("~/.ssh/vps.pem".into()),
            section: Some("bob".into()),
            ..Default::default()
        },
        &env(),
    )
    .unwrap();
    c.render()
}

pub const SIX: [&str; 7] = ["gnu", "ant", "fox", "bee", "cat", "dog", "eel"];

/// Seven hosts: gnu in the preamble (no section), the rest in sections
/// alpha, bob and other, with users, ports, proxies and jumps partly missing.
pub fn six_hosts() -> String {
    let env = env();
    let mut c = Config::parse(
        "Host *\n    ServerAliveInterval 60\n\nHost dog\n    HostName dog.example.com\n    ProxyJump alpha-jump\n\nHost eel\n    HostName eel.example.com\n    User admin\n    Port 2222\n",
    )
    .unwrap();
    let add = |c: &mut Config, name: &str, uri: &str, section: &str, opts: &[(&str, &str)]| {
        c.add(
            &AddSpec {
                name: name.into(),
                uri: uri.into(),
                section: Some(section.into()),
                options: opts
                    .iter()
                    .map(|(k, v)| (k.to_string(), v.to_string()))
                    .collect(),
                ..Default::default()
            },
            &env,
        )
        .unwrap();
    };
    add(
        &mut c,
        "ant",
        "deploy@ant.example.com:2201",
        "alpha",
        &[("ProxyCommand", "ssh -W %h:%p gw1")],
    );
    add(
        &mut c,
        "fox",
        "zed@fox.example.com",
        "alpha",
        &[("ProxyCommand", "corkscrew"), ("ProxyJump", "bastion1")],
    );
    add(
        &mut c,
        "bee",
        "deploy@bee.example.com:22",
        "bob",
        &[("ProxyJump", "bastion2")],
    );
    add(
        &mut c,
        "cat",
        "root@cat.example.com",
        "bob",
        &[("ProxyCommand", "nc gw2")],
    );
    let unset = |c: &mut Config, n: &str, k: &str| {
        c.unset(&HostSelector::Name(n.into()), &[k.into()]).unwrap();
    };
    unset(&mut c, "fox", "Port");
    unset(&mut c, "cat", "Port");
    let text = c.render();
    let at = text.find("#---").unwrap();
    format!(
        "{}Host gnu\n    HostName gnu.example.com\n    User carol\n\n{}",
        &text[..at],
        &text[at..]
    )
}

pub fn exists(p: &Path) -> bool {
    p.exists()
}
