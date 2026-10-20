//! `recent.toml` in the state dir (ARCHITECTURE §5): the launcher's recent
//! cases, newest first.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::paths::Paths;

/// Entries kept.
const KEEP: usize = 20;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct RecentFile {
    #[serde(default, rename = "case")]
    cases: Vec<Entry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    pub path: PathBuf,
    pub opened: jiff::Timestamp,
}

fn file(paths: &Paths) -> PathBuf {
    paths.state.join("recent.toml")
}

pub fn load(paths: &Paths) -> Vec<Entry> {
    std::fs::read_to_string(file(paths))
        .ok()
        .and_then(|s| toml::from_str::<RecentFile>(&s).ok())
        .map(|f| f.cases)
        .unwrap_or_default()
}

/// Move `case` to the top. Failures are logged, never fatal.
pub fn touch(paths: &Paths, case: &Path, now: jiff::Timestamp) {
    let path = std::fs::canonicalize(case).unwrap_or_else(|_| case.to_path_buf());
    let mut cases = load(paths);
    cases.retain(|e| e.path != path);
    cases.insert(0, Entry { path, opened: now });
    cases.truncate(KEEP);
    let write = || -> std::io::Result<()> {
        std::fs::create_dir_all(&paths.state)?;
        let text = toml::to_string(&RecentFile { cases }).map_err(std::io::Error::other)?;
        let tmp = file(paths).with_extension("toml.tmp");
        std::fs::write(&tmp, text)?;
        std::fs::rename(tmp, file(paths))
    };
    if let Err(e) = write() {
        tracing::warn!("recent.toml: {e}");
    }
}
