//! `rustorm-tui` binary: argument parsing, terminal setup and the event loop.

use std::io::{self, IsTerminal, Stdout};
use std::path::PathBuf;
use std::process::ExitCode;

use clap::Parser;
use crossterm::event::{self, Event, KeyEventKind};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;
use rustorm_tui::{App, Options};

/// Terminal UI for rustorm: list, add, edit, clone, move and delete ssh hosts,
/// and edit the config file with highlighting.
#[derive(Parser, Debug)]
#[command(name = "rustorm-tui", version)]
struct Args {
    /// Operate on FILE instead of $RUSTORM_CONFIG or ~/.ssh/config.
    #[arg(short, long, value_name = "FILE")]
    config: Option<PathBuf>,
    /// Do not write <config>~ before a change.
    #[arg(long)]
    no_backup: bool,
}

/// Leaves raw mode and the alternate screen when dropped.
struct TerminalGuard;

fn restore() {
    let _ = disable_raw_mode();
    let _ = execute!(io::stdout(), LeaveAlternateScreen, crossterm::cursor::Show);
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        restore();
    }
}

fn run(app: &mut App) -> anyhow::Result<()> {
    enable_raw_mode()?;
    let _guard = TerminalGuard;
    execute!(io::stdout(), EnterAlternateScreen)?;
    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        restore();
        hook(info);
    }));
    let mut terminal: Terminal<CrosstermBackend<Stdout>> =
        Terminal::new(CrosstermBackend::new(io::stdout()))?;
    terminal.clear()?;
    while !app.should_quit() {
        terminal.draw(|f| app.render(f))?;
        match event::read()? {
            Event::Key(k) if k.kind == KeyEventKind::Press => app.handle(k),
            _ => {}
        }
    }
    Ok(())
}

fn main() -> ExitCode {
    let args = Args::parse();
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        eprintln!(
            "error: rustorm-tui needs an interactive terminal. Use rustorm list or rustorm dump in scripts."
        );
        return ExitCode::from(2);
    }
    let Some(path) = rustorm_core::resolve_config_path(args.config.as_deref()) else {
        eprintln!("error: cannot find the home directory. Pass --config <FILE>.");
        return ExitCode::from(2);
    };
    let mut options = Options::detect();
    options.no_backup = args.no_backup;
    let mut app = match App::with_options(&path, options) {
        Ok(app) => app,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::from(e.exit_code() as u8);
        }
    };
    if let Err(e) = run(&mut app) {
        restore();
        eprintln!("error: {e}");
        return ExitCode::from(3);
    }
    ExitCode::SUCCESS
}
