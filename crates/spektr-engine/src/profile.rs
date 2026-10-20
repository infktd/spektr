//! Rules profiles (MODULES §7): techniques allowed, modules blocked,
//! finding categories, submission lints and export format. Built-ins are
//! embedded; user profiles live in `<config>/profiles/`.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};
use spektr_protocol::Technique;

use crate::error::{EngineError, EngineResult};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Category {
    pub id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub points: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub needs_approval: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Lints {
    #[serde(default)]
    pub require_claim: bool,
    #[serde(default)]
    pub require_relevance: bool,
    #[serde(default)]
    pub require_url_or_screenshot: bool,
    #[serde(default)]
    pub no_duplicate_across_categories: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blocked_source_domains_file: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExportCfg {
    #[serde(default)]
    pub format: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProfileHeader {
    name: String,
    #[serde(default)]
    description: String,
    techniques: Vec<Technique>,
    #[serde(default)]
    blocked_modules: Vec<String>,
    #[serde(default = "two")]
    default_depth: u32,
    #[serde(default = "yes")]
    auto: bool,
    #[serde(default)]
    categories: Vec<Category>,
}

fn two() -> u32 {
    2
}
fn yes() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProfileFile {
    profile: ProfileHeader,
    #[serde(default)]
    lints: Lints,
    #[serde(default)]
    export: ExportCfg,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RulesProfile {
    pub name: String,
    pub description: String,
    pub techniques: Vec<Technique>,
    pub blocked_modules: Vec<String>,
    pub default_depth: u32,
    /// Automatic expansion on or off.
    pub auto: bool,
    pub categories: Vec<Category>,
    pub lints: Lints,
    pub export: ExportCfg,
    /// Registrable domains from `blocked_source_domains_file`.
    pub blocked_source_domains: Vec<String>,
    /// `built-in` or the file it came from.
    pub source: String,
}

impl RulesProfile {
    pub fn parse(src: &str, blocked_domains: Option<&str>, source: &str) -> EngineResult<Self> {
        let f: ProfileFile =
            toml::from_str(src).map_err(|e| EngineError::Profile(format!("{source}: {e}")))?;
        if f.profile.techniques.is_empty() {
            return Err(EngineError::Profile(format!(
                "{source}: techniques must list at least one class"
            )));
        }
        let blocked_source_domains = blocked_domains
            .map(|txt| {
                txt.lines()
                    .map(str::trim)
                    .filter(|l| !l.is_empty() && !l.starts_with('#'))
                    .map(str::to_ascii_lowercase)
                    .collect()
            })
            .unwrap_or_default();
        Ok(Self {
            name: f.profile.name,
            description: f.profile.description,
            techniques: f.profile.techniques,
            blocked_modules: f.profile.blocked_modules,
            default_depth: f.profile.default_depth,
            auto: f.profile.auto,
            categories: f.profile.categories,
            lints: f.lints,
            export: f.export,
            blocked_source_domains,
            source: source.to_string(),
        })
    }

    pub fn allows(&self, t: Technique) -> bool {
        self.techniques.contains(&t)
    }

    pub fn blocks_module(&self, name: &str) -> bool {
        self.blocked_modules.iter().any(|m| m == name)
    }

    /// One line for the activity rail: `passive only · no probing`.
    pub fn summary(&self) -> String {
        let mut parts = Vec::new();
        if self.techniques == [Technique::Passive] {
            parts.push("passive only".to_string());
        } else {
            parts.push(
                self.techniques
                    .iter()
                    .map(|t| t.as_str())
                    .collect::<Vec<_>>()
                    .join(" + "),
            );
        }
        if !self.allows(Technique::Probing) {
            parts.push("no probing".into());
        }
        if !self.blocked_modules.is_empty() {
            parts.push(format!("{} blocked", self.blocked_modules.len()));
        }
        parts.join(" · ")
    }

    /// Why a module with this technique is blocked, in the words the
    /// activity rail and audit log use.
    pub fn block_reason(&self, module: &str, t: Technique) -> Option<String> {
        if self.blocks_module(module) {
            return Some(format!("{} blocks module {module}", self.name));
        }
        if !self.allows(t) {
            return Some(match t {
                Technique::Probing => format!("{} forbids probing", self.name),
                Technique::Active => format!("{} forbids active techniques", self.name),
                Technique::Passive => format!("{} forbids passive techniques", self.name),
            });
        }
        None
    }
}

/// Built-in profiles plus `<config>/profiles/*.toml` (user profiles override
/// built-ins of the same name).
#[derive(Debug, Clone)]
pub struct Profiles {
    by_name: BTreeMap<String, RulesProfile>,
}

const NEWS: &str = include_str!("../data/profiles/news-and-aggregators.txt");

impl Profiles {
    pub fn builtin() -> EngineResult<Self> {
        let mut by_name = BTreeMap::new();
        for (src, blocked) in [
            (include_str!("../data/profiles/default.toml"), None),
            (include_str!("../data/profiles/tracelabs.toml"), Some(NEWS)),
        ] {
            let p = RulesProfile::parse(src, blocked, "built-in")?;
            by_name.insert(p.name.clone(), p);
        }
        Ok(Self { by_name })
    }

    /// Add user profiles from a directory; a missing directory is fine.
    pub fn load_dir(&mut self, dir: &Path) -> EngineResult<Vec<String>> {
        let mut warnings = Vec::new();
        let Ok(entries) = std::fs::read_dir(dir) else {
            return Ok(warnings);
        };
        let mut paths: Vec<_> = entries.filter_map(Result::ok).map(|e| e.path()).collect();
        paths.sort();
        for path in paths
            .into_iter()
            .filter(|p| p.extension().is_some_and(|e| e == "toml"))
        {
            let src = std::fs::read_to_string(&path)?;
            let profile_dir = path.parent().unwrap_or(dir);
            let blocked = toml::from_str::<ProfileFile>(&src)
                .ok()
                .and_then(|f| f.lints.blocked_source_domains_file)
                .map(|f| {
                    std::fs::read_to_string(profile_dir.join(&f)).or_else(|_| {
                        if f == "news-and-aggregators.txt" {
                            Ok(NEWS.to_string())
                        } else {
                            Err(())
                        }
                    })
                });
            let blocked = match blocked {
                Some(Ok(s)) => Some(s),
                Some(Err(())) => {
                    warnings.push(format!(
                        "{}: blocked_source_domains_file not found",
                        path.display()
                    ));
                    None
                }
                None => None,
            };
            match RulesProfile::parse(&src, blocked.as_deref(), &path.display().to_string()) {
                Ok(p) => {
                    self.by_name.insert(p.name.clone(), p);
                }
                Err(e) => warnings.push(e.to_string()),
            }
        }
        Ok(warnings)
    }

    pub fn get(&self, name: &str) -> Option<&RulesProfile> {
        self.by_name.get(name)
    }

    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.by_name.keys().map(String::as_str)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtins() {
        let p = Profiles::builtin().unwrap();
        let tl = p.get("tracelabs").unwrap();
        assert_eq!(tl.categories.len(), 9);
        assert_eq!(
            tl.categories[7].needs_approval.as_deref(),
            Some("staff coach")
        );
        assert!(
            tl.blocked_source_domains
                .contains(&"whitepages.com".to_string())
        );
        assert_eq!(tl.summary(), "passive only · no probing");
        assert_eq!(
            tl.block_reason("probe", Technique::Probing).unwrap(),
            "tracelabs forbids probing"
        );
        assert!(tl.block_reason("github", Technique::Passive).is_none());
        let d = p.get("default").unwrap();
        assert_eq!(d.categories[0].name, "General");
        assert!(d.categories[0].points.is_none());
    }

    #[test]
    fn user_profiles_override() {
        let dir = std::env::temp_dir().join(format!("spektr-profiles-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("tracelabs.toml"),
            "[profile]\nname = \"tracelabs\"\ntechniques = [\"passive\"]\nblocked_modules = [\"wayback\"]\n",
        )
        .unwrap();
        std::fs::write(dir.join("broken.toml"), "[profile]\nname = 1").unwrap();
        let mut p = Profiles::builtin().unwrap();
        let warnings = p.load_dir(&dir).unwrap();
        assert_eq!(warnings.len(), 1, "{warnings:?}");
        assert!(p.get("tracelabs").unwrap().blocks_module("wayback"));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
