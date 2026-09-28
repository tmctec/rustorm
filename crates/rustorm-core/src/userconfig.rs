//! rustorm's own configuration: command aliases and startup defaults.
//!
//! The file is TOML at `<OS config dir>/rustorm/config.toml`: Linux
//! `~/.config/rustorm` (or `$XDG_CONFIG_HOME/rustorm`), macOS
//! `~/Library/Application Support/rustorm`, Windows `%AppData%\rustorm`. It
//! never holds host data.
//!
//! ```toml
//! [aliases]
//! delete = ["rm", "del"]
//!
//! [defaults]
//! backup = true
//! color = "auto"
//! ```

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::{Error, Result};

/// When to color output.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ColorMode {
    /// Color on a terminal, unless `NO_COLOR` is set.
    #[default]
    Auto,
    /// Always color.
    Always,
    /// Never color.
    Never,
}

/// The `[defaults]` table.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct UserDefaults {
    /// Write `<config>~` before every change. `--no-backup` overrides it.
    pub backup: bool,
    /// Color mode.
    pub color: ColorMode,
}

impl Default for UserDefaults {
    fn default() -> Self {
        UserDefaults {
            backup: true,
            color: ColorMode::Auto,
        }
    }
}

/// rustorm's configuration file.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct UserConfig {
    /// Canonical command name to extra spellings accepted on the command line.
    pub aliases: BTreeMap<String, Vec<String>>,
    /// Startup defaults.
    pub defaults: UserDefaults,
}

impl UserConfig {
    /// `<OS config dir>/rustorm/config.toml`, or `None` when the OS has no
    /// config directory.
    pub fn default_path() -> Option<PathBuf> {
        dirs::config_dir().map(|d| d.join("rustorm").join("config.toml"))
    }

    /// Parses TOML text. `path` names the file in the error.
    pub fn parse(text: &str, path: &Path) -> Result<UserConfig> {
        toml::from_str(text).map_err(|e| Error::UserConfig {
            path: path.to_path_buf(),
            reason: e.message().to_string(),
        })
    }

    /// Loads `path`; a missing file gives the defaults.
    pub fn load_from(path: &Path) -> Result<UserConfig> {
        match std::fs::read_to_string(path) {
            Ok(text) => UserConfig::parse(&text, path),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(UserConfig::default()),
            Err(source) => Err(Error::Read {
                path: path.to_path_buf(),
                source,
            }),
        }
    }

    /// Loads the file at [`UserConfig::default_path`]; defaults when absent.
    pub fn load() -> Result<UserConfig> {
        match UserConfig::default_path() {
            Some(p) => UserConfig::load_from(&p),
            None => Ok(UserConfig::default()),
        }
    }

    /// The canonical command for `word`: the key of the `[aliases]` entry
    /// whose list contains `word`, or `None`.
    pub fn resolve_alias(&self, word: &str) -> Option<&str> {
        self.aliases
            .iter()
            .find(|(_, spellings)| spellings.iter().any(|s| s == word))
            .map(|(command, _)| command.as_str())
    }
}
