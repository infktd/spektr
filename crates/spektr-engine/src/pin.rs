//! Installed exec modules (MODULES §9): tree-hash pinning and the disabled
//! list. A module whose files changed since install stays unloaded until
//! `spektr module install --update` re-approves it.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// The pin file in an installed module's directory.
pub const PIN_FILE: &str = ".spektr-pin";
/// Disabled module names, one per line, in the modules directory.
pub const DISABLED_FILE: &str = ".disabled";

/// Directories and files the tree hash ignores.
fn skipped(name: &str) -> bool {
    matches!(
        name,
        PIN_FILE | ".git" | "target" | "__pycache__" | ".DS_Store" | "node_modules"
    )
}

fn walk(root: &Path, dir: &Path, out: &mut Vec<PathBuf>) -> std::io::Result<()> {
    let mut entries: Vec<_> = std::fs::read_dir(dir)?.filter_map(Result::ok).collect();
    entries.sort_by_key(std::fs::DirEntry::file_name);
    for e in entries {
        let name = e.file_name().to_string_lossy().into_owned();
        if skipped(&name) {
            continue;
        }
        let p = e.path();
        let ft = e.file_type()?;
        if ft.is_dir() {
            walk(root, &p, out)?;
        } else if ft.is_file() {
            out.push(p.strip_prefix(root).unwrap_or(&p).to_path_buf());
        }
    }
    Ok(())
}

/// SHA-256 over every file's relative path and contents (sorted).
pub fn tree_hash(dir: &Path) -> std::io::Result<String> {
    let mut files = Vec::new();
    walk(dir, dir, &mut files)?;
    let mut manifest = String::new();
    for f in files {
        let bytes = std::fs::read(dir.join(&f))?;
        let path = f.to_string_lossy().replace('\\', "/");
        manifest.push_str(&format!("{path}\0{}\n", spektr_store::sha256(&bytes).hex()));
    }
    Ok(spektr_store::sha256(manifest.as_bytes()).hex())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PinState {
    /// Installed and unchanged.
    Pinned,
    /// Dropped in by hand: no pin.
    Unpinned,
    Changed,
}

pub fn pin_state(dir: &Path) -> PinState {
    let Ok(want) = std::fs::read_to_string(dir.join(PIN_FILE)) else {
        return PinState::Unpinned;
    };
    match tree_hash(dir) {
        Ok(h) if h == want.trim() => PinState::Pinned,
        _ => PinState::Changed,
    }
}

pub fn write_pin(dir: &Path) -> std::io::Result<String> {
    let h = tree_hash(dir)?;
    std::fs::write(dir.join(PIN_FILE), format!("{h}\n"))?;
    Ok(h)
}

/// Disabled module names in `modules_dir`.
pub fn disabled(modules_dir: &Path) -> BTreeSet<String> {
    std::fs::read_to_string(modules_dir.join(DISABLED_FILE))
        .unwrap_or_default()
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(str::to_string)
        .collect()
}

pub fn set_disabled(modules_dir: &Path, name: &str, off: bool) -> std::io::Result<()> {
    let mut set = disabled(modules_dir);
    if off {
        set.insert(name.to_string());
    } else {
        set.remove(name);
    }
    std::fs::create_dir_all(modules_dir)?;
    let mut text = String::from("# modules spektr doesn't load (spektr module enable <name>)\n");
    for n in set {
        text.push_str(&n);
        text.push('\n');
    }
    std::fs::write(modules_dir.join(DISABLED_FILE), text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pin_tracks_changes() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("module.toml"), "a").unwrap();
        std::fs::create_dir(d.path().join("src")).unwrap();
        std::fs::write(d.path().join("src/main.py"), "b").unwrap();
        assert_eq!(pin_state(d.path()), PinState::Unpinned);
        write_pin(d.path()).unwrap();
        assert_eq!(pin_state(d.path()), PinState::Pinned);
        std::fs::create_dir(d.path().join("__pycache__")).unwrap();
        std::fs::write(d.path().join("__pycache__/x.pyc"), "ignored").unwrap();
        assert_eq!(pin_state(d.path()), PinState::Pinned);
        std::fs::write(d.path().join("src/main.py"), "changed").unwrap();
        assert_eq!(pin_state(d.path()), PinState::Changed);
    }

    #[test]
    fn disabled_list() {
        let d = tempfile::tempdir().unwrap();
        set_disabled(d.path(), "wayback", true).unwrap();
        set_disabled(d.path(), "lemmy", true).unwrap();
        set_disabled(d.path(), "wayback", false).unwrap();
        assert_eq!(
            disabled(d.path()).into_iter().collect::<Vec<_>>(),
            ["lemmy"]
        );
    }
}
