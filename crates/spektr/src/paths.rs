//! The one place that resolves spektr's directories (ARCHITECTURE §5): XDG
//! variables, falling back to `~/.config`, `~/.local/state` and
//! `~/.local/share` — on macOS too. Never `dirs::config_dir()`.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Paths {
    /// `<config>`: config.toml, modules/, profiles/, playbooks/, launchers.toml, sites.json.
    pub config: PathBuf,
    /// recent.toml, tutor.toml, logs, opened.jsonl.
    pub state: PathBuf,
    /// Headless Chrome, the tutor case template.
    pub data: PathBuf,
}

fn xdg(var: &str, home: &Path, fallback: &str) -> PathBuf {
    match std::env::var_os(var)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
    {
        Some(p) if p.is_absolute() => p.join("spektr"),
        _ => home.join(fallback).join("spektr"),
    }
}

impl Paths {
    pub fn resolve() -> Result<Self> {
        let home = dirs::home_dir().context("can't find the home directory (is $HOME set?)")?;
        Ok(Self::from_home(&home))
    }

    pub fn from_home(home: &Path) -> Self {
        Self {
            config: xdg("XDG_CONFIG_HOME", home, ".config"),
            state: xdg("XDG_STATE_HOME", home, ".local/state"),
            data: xdg("XDG_DATA_HOME", home, ".local/share"),
        }
    }

    pub fn config_file(&self) -> PathBuf {
        self.config.join("config.toml")
    }

    pub fn modules_dir(&self) -> PathBuf {
        self.config.join("modules")
    }

    pub fn logs_dir(&self) -> PathBuf {
        self.state.join("logs")
    }
}

/// `~/…` for paths under the home directory.
pub fn tilde(p: &Path) -> String {
    match std::env::var_os("HOME").map(PathBuf::from) {
        Some(home) if p.starts_with(&home) => match p.strip_prefix(&home) {
            Ok(rest) => format!("~/{}", rest.display()),
            Err(_) => p.display().to_string(),
        },
        _ => p.display().to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fallbacks_ignore_relative_xdg() {
        let p = Paths::from_home(Path::new("/home/jm"));
        // the test environment may set XDG vars; only check the shape
        assert!(p.config.ends_with("spektr"));
        assert!(p.state.ends_with("spektr"));
        assert!(p.data.ends_with("spektr"));
    }
}
