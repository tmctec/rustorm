//! `rustorm-gui [--config <FILE>]`: opens the rustorm window.

use std::path::PathBuf;
use std::process::ExitCode;

const USAGE: &str = "usage: rustorm-gui [--config <FILE>] [--no-backup]

Options:
  -c, --config <FILE>  the ssh config to edit (default: $RUSTORM_CONFIG, then ~/.ssh/config)
      --no-backup      write without the <file>~ backup
  -h, --help           print this help
  -V, --version        print the version";

/// The parsed flags: the config path and whether to skip backups.
struct Args {
    config: Option<PathBuf>,
    no_backup: bool,
}

fn parse_args() -> Result<Args, String> {
    let mut args = std::env::args().skip(1);
    let mut config = None;
    let mut no_backup = false;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-h" | "--help" => {
                println!("{USAGE}");
                std::process::exit(0);
            }
            "-V" | "--version" => {
                println!("rustorm-gui {}", rustorm_core::version());
                std::process::exit(0);
            }
            "-c" | "--config" => {
                config = Some(PathBuf::from(args.next().ok_or("--config needs a FILE")?));
            }
            s if s.starts_with("--config=") => {
                config = Some(PathBuf::from(&s["--config=".len()..]));
            }
            "--no-backup" => no_backup = true,
            other => return Err(format!("unexpected argument {other}")),
        }
    }
    Ok(Args { config, no_backup })
}

fn main() -> ExitCode {
    let args = match parse_args() {
        Ok(f) => f,
        Err(e) => {
            eprintln!("error: {e}\n\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    let Some(path) = rustorm_core::resolve_config_path(args.config.as_deref()) else {
        eprintln!("error: no home directory; give --config <FILE>");
        return ExitCode::from(3);
    };
    let app = match rustorm_gui::App::new(path) {
        Ok(a) => a.with_write_options(rustorm_core::WriteOptions {
            no_backup: args.no_backup,
        }),
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::from(3);
        }
    };
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(app.title())
            .with_inner_size([1200.0, 760.0])
            .with_min_inner_size([900.0, 560.0]),
        ..Default::default()
    };
    match eframe::run_native("rustorm", options, Box::new(|_cc| Ok(Box::new(app)))) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::from(1)
        }
    }
}
