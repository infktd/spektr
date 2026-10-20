//! `config.toml` (ARCHITECTURE §5). Secrets come from `[secrets]` or
//! `SPEKTR_SECRET_<NAME>`; the environment wins. Secrets are never written to
//! a case file or a log.

use std::collections::BTreeMap;
use std::net::IpAddr;
use std::path::Path;

use anyhow::{Context, Result};
use serde::Deserialize;

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    #[serde(default)]
    pub secrets: BTreeMap<String, String>,
    #[serde(default)]
    pub dns: DnsSection,
    /// `[modules.<name>]`: per-module config, merged over manifest defaults.
    #[serde(default)]
    pub modules: BTreeMap<String, toml::Table>,
    /// TUI settings, read by the TUI (Phase 3).
    #[allow(dead_code)]
    #[serde(default)]
    pub mouse: Option<bool>,
    #[serde(default)]
    #[allow(dead_code)]
    pub images: Option<String>,
    #[serde(default)]
    #[allow(dead_code)]
    pub glyphs: Option<String>,
    #[serde(default)]
    #[allow(dead_code)]
    pub colors: Option<String>,
    #[serde(default)]
    #[allow(dead_code)]
    pub keys: BTreeMap<String, toml::Value>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DnsSection {
    #[serde(default)]
    pub nameservers: Vec<IpAddr>,
}

impl Config {
    /// Load `path` if it exists; a missing file is an empty config.
    pub fn load(path: &Path) -> Result<Self> {
        match std::fs::read_to_string(path) {
            Ok(text) => {
                toml::from_str(&text).with_context(|| format!("reading {}", path.display()))
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(e).with_context(|| format!("reading {}", path.display())),
        }
    }

    /// `[secrets]` overlaid with `SPEKTR_SECRET_<NAME>` from `env`.
    pub fn secrets(&self, env: impl Iterator<Item = (String, String)>) -> BTreeMap<String, String> {
        let mut out = self.secrets.clone();
        for (k, v) in env {
            if let Some(name) = k.strip_prefix("SPEKTR_SECRET_")
                && !name.is_empty()
                && !v.is_empty()
            {
                out.insert(name.to_string(), v);
            }
        }
        out.retain(|_, v| !v.is_empty());
        out
    }

    pub fn module_config(&self) -> BTreeMap<String, serde_json::Value> {
        self.modules
            .iter()
            .filter_map(|(k, t)| serde_json::to_value(t).ok().map(|v| (k.clone(), v)))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn env_secrets_win() {
        let c: Config = toml::from_str("[secrets]\nGITHUB_TOKEN = \"from-file\"\nHIBP_KEY = \"k\"\n[dns]\nnameservers = [\"9.9.9.9\"]\n[modules.hibp]\ndaily_budget = 100\n").unwrap();
        let s = c.secrets(
            vec![
                (
                    "SPEKTR_SECRET_GITHUB_TOKEN".to_string(),
                    "from-env".to_string(),
                ),
                ("OTHER".into(), "x".into()),
            ]
            .into_iter(),
        );
        assert_eq!(s["GITHUB_TOKEN"], "from-env");
        assert_eq!(s["HIBP_KEY"], "k");
        assert_eq!(c.dns.nameservers.len(), 1);
        assert_eq!(c.module_config()["hibp"]["daily_budget"], 100);
        assert!(toml::from_str::<Config>("unknown = 1").is_err());
    }
}
