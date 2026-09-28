//! Reading and writing the ssh_config file.
//!
//! Writes back up the current file to `<config>~` (unless disabled), then
//! write a temporary file in the same directory, fsync it, and rename it over
//! the original, so a failed write leaves the original intact. A missing file
//! is created with mode 0600 and a missing directory with mode 0700; an
//! existing file keeps its mode.

use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};

use crate::{Config, Error, Result};

/// Environment variable that overrides the default config path.
pub const CONFIG_ENV: &str = "RUSTORM_CONFIG";

/// `~/.ssh/config`, or `None` when the home directory is unknown.
pub fn default_config_path() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(".ssh").join("config"))
}

/// The config path to use: `flag` when given, else `$RUSTORM_CONFIG`, else
/// `~/.ssh/config`.
pub fn resolve_config_path(flag: Option<&Path>) -> Option<PathBuf> {
    if let Some(p) = flag {
        return Some(p.to_path_buf());
    }
    if let Some(p) = std::env::var_os(CONFIG_ENV).filter(|v| !v.is_empty()) {
        return Some(PathBuf::from(p));
    }
    default_config_path()
}

/// The backup path for `path`: the same path with `~` appended.
pub fn backup_path(path: &Path) -> PathBuf {
    let mut s = path.as_os_str().to_os_string();
    s.push("~");
    PathBuf::from(s)
}

/// How [`ConfigFile::save`] writes.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct WriteOptions {
    /// Skip the `<config>~` backup.
    pub no_backup: bool,
}

/// An ssh_config file on disk and its parsed model.
#[derive(Debug, Clone)]
pub struct ConfigFile {
    /// The path given to [`ConfigFile::load`].
    pub path: PathBuf,
    /// The parsed model.
    pub config: Config,
    /// The text read from disk (empty when the file does not exist).
    pub original: String,
    /// True when the file existed at load time.
    pub existed: bool,
}

fn read_error(path: &Path, source: std::io::Error) -> Error {
    Error::Read {
        path: path.to_path_buf(),
        source,
    }
}

fn write_error(path: &Path, source: std::io::Error) -> Error {
    Error::Write {
        path: path.to_path_buf(),
        source,
    }
}

impl ConfigFile {
    /// Reads and parses `path`. A missing file loads as an empty config.
    pub fn load(path: impl Into<PathBuf>) -> Result<ConfigFile> {
        let path = path.into();
        let (original, existed) = match fs::read(&path) {
            Ok(bytes) => {
                let text = String::from_utf8(bytes).map_err(|e| {
                    read_error(
                        &path,
                        std::io::Error::new(std::io::ErrorKind::InvalidData, e),
                    )
                })?;
                (text, true)
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (String::new(), false),
            Err(e) => return Err(read_error(&path, e)),
        };
        let config = Config::parse(&original)?;
        Ok(ConfigFile {
            path,
            config,
            original,
            existed,
        })
    }

    /// True when the model renders differently from the text on disk.
    pub fn is_modified(&self) -> bool {
        self.config.render() != self.original
    }

    /// Writes the model to disk (see [`write_text`]) and records the new
    /// text as the original.
    pub fn save(&mut self, options: WriteOptions) -> Result<()> {
        let text = self.config.render();
        write_text(&self.path, &text, options)?;
        self.original = text;
        self.existed = true;
        Ok(())
    }

    /// Replaces the file with `text` (for editors that edit the raw file),
    /// re-parses it into the model, and writes it like [`ConfigFile::save`].
    pub fn save_text(&mut self, text: &str, options: WriteOptions) -> Result<()> {
        self.config = Config::parse(text)?;
        write_text(&self.path, text, options)?;
        self.original = text.to_string();
        self.existed = true;
        Ok(())
    }

    /// Copies the file on disk to `dest`, or to `<config>~` when `dest` is
    /// `None`. Returns the destination path.
    pub fn backup(&self, dest: Option<&Path>) -> Result<PathBuf> {
        let dest = dest.map_or_else(|| backup_path(&self.path), Path::to_path_buf);
        fs::copy(&self.path, &dest).map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound && !self.path.exists() {
                read_error(&self.path, e)
            } else {
                write_error(&dest, e)
            }
        })?;
        Ok(dest)
    }
}

#[cfg(unix)]
fn set_mode(path: &Path, mode: u32) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(mode))
}

#[cfg(not(unix))]
fn set_mode(_path: &Path, _mode: u32) -> std::io::Result<()> {
    Ok(())
}

#[cfg(unix)]
fn mode_of(meta: &fs::Metadata) -> u32 {
    use std::os::unix::fs::PermissionsExt;
    meta.permissions().mode() & 0o7777
}

#[cfg(not(unix))]
fn mode_of(_meta: &fs::Metadata) -> u32 {
    0o600
}

fn create_dirs(dir: &Path) -> std::io::Result<()> {
    if dir.as_os_str().is_empty() || dir.exists() {
        return Ok(());
    }
    if let Some(parent) = dir.parent() {
        create_dirs(parent)?;
    }
    fs::create_dir(dir)?;
    set_mode(dir, 0o700)
}

/// Writes `text` to `path` atomically.
///
/// A symlinked `path` writes through to its target. The current file is
/// copied to `<path>~` first unless `options.no_backup`. The text goes to a
/// temporary file in the same directory, is fsynced, gets the original
/// file's mode (0600 for a new file) and is renamed over `path`. A missing
/// directory is created with mode 0700.
pub fn write_text(path: &Path, text: &str, options: WriteOptions) -> Result<()> {
    let target = match fs::canonicalize(path) {
        Ok(p) => p,
        Err(_) => path.to_path_buf(),
    };
    let dir = target
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    let dir = if dir.as_os_str().is_empty() {
        PathBuf::from(".")
    } else {
        dir
    };
    create_dirs(&dir).map_err(|e| write_error(&dir, e))?;
    let existing = fs::metadata(&target).ok();
    if existing.is_some() && !options.no_backup {
        let backup = backup_path(path);
        fs::copy(&target, &backup).map_err(|e| write_error(&backup, e))?;
    }
    let mode = existing.as_ref().map_or(0o600, mode_of);
    let mut tmp = tempfile::Builder::new()
        .prefix(".rustorm-")
        .tempfile_in(&dir)
        .map_err(|e| write_error(&target, e))?;
    tmp.write_all(text.as_bytes())
        .map_err(|e| write_error(&target, e))?;
    tmp.as_file()
        .sync_all()
        .map_err(|e| write_error(&target, e))?;
    set_mode(tmp.path(), mode).map_err(|e| write_error(&target, e))?;
    tmp.persist(&target)
        .map_err(|e| write_error(&target, e.error))?;
    Ok(())
}
