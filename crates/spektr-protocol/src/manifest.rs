//! The module manifest (MODULES §2). Exec modules keep it in `module.toml`;
//! native modules build the same struct in Rust.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::duration::{parse_duration, parse_rate};

/// `module.toml`: one `[module]` table.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct ManifestFile {
    pub module: Manifest,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "lowercase")]
pub enum Runtime {
    Native,
    Exec,
}

/// Technique classes (MODULES §7).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "lowercase")]
pub enum Technique {
    /// Reads public data the way any visitor would.
    Passive,
    /// Interacts with a service to learn something it doesn't publish.
    Probing,
    /// Sends traffic a target could notice as reconnaissance.
    Active,
}

impl Technique {
    pub fn as_str(self) -> &'static str {
        match self {
            Technique::Passive => "passive",
            Technique::Probing => "probing",
            Technique::Active => "active",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "lowercase")]
pub enum CostClass {
    Free,
    /// No network (phone-parse, exif).
    Local,
    Keyed,
    Metered,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Cost {
    pub class: CostClass,
    /// keyed/metered: optional jobs-per-day limit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub daily_budget: Option<u32>,
    /// metered: units each run uses.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub per_run: Option<u32>,
}

impl Default for Cost {
    fn default() -> Self {
        Self {
            class: CostClass::Free,
            daily_budget: None,
            per_run: None,
        }
    }
}

/// `[[module.platforms]]`: an account platform this module introduces.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct PlatformDecl {
    pub id: String,
    #[serde(default)]
    pub case_sensitive: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile_url: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "lowercase")]
pub enum ConfigType {
    String,
    Int,
    Bool,
    List,
}

/// `[module.config]` entries: user configuration with a type and default.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct ConfigField {
    #[serde(rename = "type")]
    pub ty: ConfigType,
    #[serde(default)]
    pub default: serde_json::Value,
}

/// `[[module.types]]`: an extension entity type.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct TypeDecl {
    pub name: String,
    /// `exact`, `trim`, `lowercase`, `casefold` or `url`.
    pub normalize: String,
}

/// `[[module.predicates]]`: an extension predicate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct PredicateDecl {
    pub name: String,
    pub src: Vec<String>,
    pub dst: Vec<String>,
}

fn default_concurrency() -> u32 {
    4
}
fn default_timeout() -> String {
    "60s".into()
}
fn default_true() -> bool {
    true
}
fn default_ttl() -> String {
    "24h".into()
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    /// `[a-z0-9-]{2,32}`, unique; must not clash with a built-in.
    pub name: String,
    /// Semver.
    pub version: String,
    pub description: String,
    pub runtime: Runtime,
    /// exec only; resolved relative to the module dir.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub command: Vec<String>,
    /// exec only: one long-lived process, many jobs.
    #[serde(default)]
    pub serve: bool,
    /// Most concurrent jobs for this module.
    #[serde(default = "default_concurrency")]
    pub concurrency: u32,
    #[serde(default = "default_timeout")]
    pub timeout: String,
    pub technique: Technique,
    /// Eligible for automatic expansion.
    #[serde(default = "default_true")]
    pub auto: bool,
    /// Entity types, or `account:<platform>`.
    pub consumes: Vec<String>,
    /// `[predicate, entity type]` pairs the module may emit.
    #[serde(default)]
    pub produces: Vec<(String, String)>,
    #[serde(default = "default_ttl")]
    pub cache_ttl: String,
    /// Per destination host: `N/s` or `N/min`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rate_limit: Option<String>,
    #[serde(default)]
    pub cost: Cost,
    /// Required; the job is ineligible without them.
    #[serde(default)]
    pub secrets: Vec<String>,
    /// Used when present.
    #[serde(default)]
    pub optional_secrets: Vec<String>,
    /// Declared, informational; shown on install and in the doctor.
    #[serde(default)]
    pub hosts: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_spektr: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub platforms: Vec<PlatformDecl>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub config: BTreeMap<String, ConfigField>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub types: Vec<TypeDecl>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub predicates: Vec<PredicateDecl>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ManifestError {
    #[error("module.toml: {0}")]
    Parse(String),
    #[error("invalid manifest: {}", .0.join("; "))]
    Invalid(Vec<String>),
}

fn valid_module_name(s: &str) -> bool {
    (2..=32).contains(&s.len())
        && s.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

pub(crate) fn valid_semver(s: &str) -> bool {
    let core = s.split(['-', '+']).next().unwrap_or("");
    let parts: Vec<&str> = core.split('.').collect();
    parts.len() == 3
        && parts
            .iter()
            .all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
}

impl Manifest {
    /// Parse and lint a `module.toml`.
    pub fn from_toml(src: &str) -> Result<Self, ManifestError> {
        let file: ManifestFile =
            toml::from_str(src).map_err(|e| ManifestError::Parse(e.to_string()))?;
        file.module.check()?;
        Ok(file.module)
    }

    /// Syntax checks that need no vocabulary. The engine additionally checks
    /// types and predicates against the vocabulary at load time.
    pub fn check(&self) -> Result<(), ManifestError> {
        let mut problems = Vec::new();
        if !valid_module_name(&self.name) {
            problems.push(format!("name `{}` must match [a-z0-9-]{{2,32}}", self.name));
        }
        if !valid_semver(&self.version) {
            problems.push(format!("version `{}` is not semver", self.version));
        }
        match self.runtime {
            Runtime::Exec if self.command.is_empty() => {
                problems.push("exec modules need a command".into())
            }
            Runtime::Native if !self.command.is_empty() || self.serve => {
                problems.push("command and serve are for exec modules only".into());
            }
            _ => {}
        }
        if self.concurrency == 0 {
            problems.push("concurrency must be at least 1".into());
        }
        if parse_duration(&self.timeout).is_none() {
            problems.push(format!(
                "timeout `{}` is not a duration (e.g. 60s)",
                self.timeout
            ));
        }
        if parse_duration(&self.cache_ttl).is_none() {
            problems.push(format!(
                "cache_ttl `{}` is not a duration (e.g. 24h)",
                self.cache_ttl
            ));
        }
        if let Some(r) = &self.rate_limit
            && parse_rate(r).is_none()
        {
            problems.push(format!("rate_limit `{r}` must be N/s or N/min"));
        }
        if self.consumes.is_empty() {
            problems.push("consumes must list at least one type".into());
        }
        for c in &self.consumes {
            if c.trim().is_empty() || c.contains(char::is_whitespace) {
                problems.push(format!("consumes entry `{c}` is malformed"));
            }
        }
        let prefix = format!("x.{}.", self.name);
        for t in &self.types {
            if !t.name.starts_with(&prefix) {
                problems.push(format!("type `{}` must be named {prefix}<name>", t.name));
            }
            if !["exact", "trim", "lowercase", "casefold", "url"].contains(&t.normalize.as_str()) {
                problems.push(format!(
                    "type `{}`: normalize must be exact, trim, lowercase, casefold or url",
                    t.name
                ));
            }
        }
        for p in &self.predicates {
            if !p.name.starts_with(&prefix) {
                problems.push(format!(
                    "predicate `{}` must be named {prefix}<name>",
                    p.name
                ));
            }
            if p.src.is_empty() || p.dst.is_empty() {
                problems.push(format!("predicate `{}` needs src and dst types", p.name));
            }
        }
        if !matches!(self.cost.class, CostClass::Keyed | CostClass::Metered)
            && self.cost.daily_budget.is_some()
        {
            problems.push("daily_budget only applies to keyed or metered modules".into());
        }
        for s in self.secrets.iter().chain(&self.optional_secrets) {
            if s.is_empty()
                || !s
                    .bytes()
                    .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_')
            {
                problems.push(format!("secret name `{s}` must be UPPER_SNAKE_CASE"));
            }
        }
        if problems.is_empty() {
            Ok(())
        } else {
            Err(ManifestError::Invalid(problems))
        }
    }

    /// Whether this module accepts an entity of `ty` (and, for accounts, on
    /// `platform`).
    pub fn consumes_entity(&self, ty: &str, platform: Option<&str>) -> bool {
        self.consumes.iter().any(|c| match c.split_once(':') {
            Some(("account", p)) => ty == "account" && platform == Some(p),
            _ => c == ty,
        })
    }

    pub fn produces_predicate(&self, predicate: &str) -> bool {
        self.produces.iter().any(|(p, _)| p == predicate)
    }

    pub fn produces_type(&self, ty: &str) -> bool {
        self.produces.iter().any(|(_, t)| t == ty)
    }

    pub fn timeout_duration(&self) -> std::time::Duration {
        parse_duration(&self.timeout).unwrap_or(std::time::Duration::from_secs(60))
    }

    pub fn cache_ttl_duration(&self) -> std::time::Duration {
        parse_duration(&self.cache_ttl).unwrap_or(std::time::Duration::from_secs(86_400))
    }

    /// The user-config defaults as a JSON object.
    pub fn default_config(&self) -> serde_json::Value {
        let map: serde_json::Map<String, serde_json::Value> = self
            .config
            .iter()
            .map(|(k, f)| (k.clone(), f.default.clone()))
            .collect();
        serde_json::Value::Object(map)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LEMMY: &str = r#"
[module]
name        = "lemmy"
version     = "0.1.0"
description = "Public Lemmy profile lookup"
runtime     = "exec"
command     = ["python3", "lemmy.py"]
serve       = false
concurrency = 4
timeout     = "60s"
technique   = "passive"
auto        = true
consumes    = ["username", "account:lemmy"]
produces    = [["has_account", "account"], ["links_to", "url"]]
cache_ttl   = "24h"
rate_limit  = "1/s"
cost        = { class = "free" }
secrets          = []
optional_secrets = []
hosts       = ["lemmy.world", "lemmy.example"]
min_spektr  = "0.1.0"

[[module.platforms]]
id = "lemmy"
case_sensitive = false
profile_url = "https://{instance}/@{user}"

[module.config]
instances = { type = "list", default = ["lemmy.world"] }

[[module.types]]
name = "x.lemmy.instance"
normalize = "lowercase"

[[module.predicates]]
name = "x.lemmy.member_of_instance"
src = ["account"]
dst = ["x.lemmy.instance"]
"#;

    #[test]
    fn the_spec_example_parses() {
        let m = Manifest::from_toml(LEMMY).unwrap();
        assert_eq!(m.name, "lemmy");
        assert_eq!(m.runtime, Runtime::Exec);
        assert!(m.consumes_entity("account", Some("lemmy")));
        assert!(!m.consumes_entity("account", Some("github")));
        assert!(m.consumes_entity("username", None));
        assert!(m.produces_predicate("has_account"));
        assert_eq!(
            m.default_config(),
            serde_json::json!({"instances": ["lemmy.world"]})
        );
        assert_eq!(
            m.platforms[0].profile_url.as_deref(),
            Some("https://{instance}/@{user}")
        );
    }

    #[test]
    fn lint_catches_mistakes() {
        let bad = LEMMY
            .replace("name        = \"lemmy\"", "name        = \"Lemmy!\"")
            .replace("\"60s\"", "\"sixty\"")
            .replace(
                "x.lemmy.instance\"\nnormalize",
                "x.other.instance\"\nnormalize",
            );
        let err = Manifest::from_toml(&bad).unwrap_err().to_string();
        assert!(err.contains("name `Lemmy!`"), "{err}");
        assert!(err.contains("timeout `sixty`"), "{err}");
        assert!(err.contains("x.other.instance"), "{err}");
        assert!(Manifest::from_toml("[module]\nname = 1").is_err());
        assert!(
            Manifest::from_toml(
                &LEMMY.replace("technique   = \"passive\"", "technique   = \"sneaky\"")
            )
            .is_err()
        );
        assert!(
            Manifest::from_toml(&format!("{LEMMY}\n[extra]\nx = 1")).is_err(),
            "unknown tables are refused"
        );
    }
}
