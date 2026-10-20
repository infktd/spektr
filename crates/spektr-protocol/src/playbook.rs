//! The playbook file format (docs/PLAYBOOKS.md §1): TOML a person reads.
//! Parsing and the checks that need nothing but the file live here, beside
//! the manifest format, so `schemas/playbook.v1.json` is generated the same
//! way. Checks against installed modules, launchers and other playbooks
//! are the engine's.

use serde::{Deserialize, Serialize};

/// The `max` default and ceiling for a selector (PLAYBOOKS §1).
pub const DEFAULT_MAX: u32 = 50;
pub const MAX_CAP: u32 = 200;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct PlaybookFile {
    pub playbook: PlaybookMeta,
    #[serde(default, rename = "step")]
    pub steps: Vec<StepDef>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct PlaybookMeta {
    /// `[a-z0-9-]{2,40}`, unique.
    pub name: String,
    /// semver.
    pub version: String,
    pub description: String,
    /// Entity types (or `account:<platform>`) it can start from.
    pub seed: Vec<String>,
    /// Refuse to run unless the case uses this rules profile.
    #[serde(default)]
    pub requires_profile: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct StepDef {
    pub title: String,
    /// Modules to queue on each selected entity.
    #[serde(default)]
    pub run: Option<Vec<String>>,
    /// A playbook to start with each selected entity as its seed.
    #[serde(default)]
    pub playbook: Option<String>,
    /// A checklist item's text.
    #[serde(default)]
    pub check: Option<String>,
    /// Launcher ids attached to the checklist items (`check` steps only).
    #[serde(default)]
    pub open: Vec<String>,
    /// `review` or `findings`: the item opens that screen (`check` only).
    #[serde(default)]
    pub screen: Option<String>,
    /// Which entities the step applies to. Omitted on a `check` step: one
    /// item for the run.
    #[serde(default)]
    pub on: Option<On>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(untagged)]
pub enum On {
    /// `"seed"`: the run's seed entity.
    Seed(String),
    Select(Selector),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum From {
    /// Produced or confirmed by this run's jobs (PLAYBOOKS §2.3).
    #[default]
    Run,
    /// Directly linked to the seed by any edge.
    SeedLinks,
    /// Anywhere in the case.
    Case,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Selector {
    /// A type, or `account:<platform>`.
    #[serde(rename = "type")]
    pub ty: String,
    #[serde(default)]
    pub from: From,
    /// `single`, `corroborated` or `confirmed`.
    #[serde(default)]
    pub min_strength: Option<String>,
    /// Cap on entities selected (default 50, never above 200).
    #[serde(default)]
    pub max: Option<u32>,
}

/// What a step does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StepKind<'a> {
    Run(&'a [String]),
    Playbook(&'a str),
    Check {
        text: &'a str,
        open: &'a [String],
        screen: Option<&'a str>,
    },
}

impl StepDef {
    /// The step's kind; `None` unless exactly one of run/playbook/check.
    pub fn kind(&self) -> Option<StepKind<'_>> {
        match (&self.run, &self.playbook, &self.check) {
            (Some(r), None, None) => Some(StepKind::Run(r)),
            (None, Some(p), None) => Some(StepKind::Playbook(p)),
            (None, None, Some(c)) => Some(StepKind::Check {
                text: c,
                open: &self.open,
                screen: self.screen.as_deref(),
            }),
            _ => None,
        }
    }
}

impl Selector {
    /// `max`, defaulted and capped.
    pub fn cap(&self) -> u32 {
        self.max.unwrap_or(DEFAULT_MAX).min(MAX_CAP)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum PlaybookError {
    #[error("{0}")]
    Toml(#[from] toml::de::Error),
    #[error("{}", .0.join("; "))]
    Invalid(Vec<String>),
}

fn valid_name(s: &str) -> bool {
    (2..=40).contains(&s.len())
        && s.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

impl PlaybookFile {
    /// Parse and check everything that needs only the file.
    pub fn from_toml(text: &str) -> Result<Self, PlaybookError> {
        let p: PlaybookFile = toml::from_str(text)?;
        let problems = p.lint();
        if problems.is_empty() {
            Ok(p)
        } else {
            Err(PlaybookError::Invalid(problems))
        }
    }

    pub fn lint(&self) -> Vec<String> {
        let mut out = Vec::new();
        let m = &self.playbook;
        if !valid_name(&m.name) {
            out.push(format!("name `{}` must match [a-z0-9-]{{2,40}}", m.name));
        }
        if !crate::manifest::valid_semver(&m.version) {
            out.push(format!("version `{}` is not semver", m.version));
        }
        if m.seed.is_empty() {
            out.push("seed lists no entity types".into());
        }
        if self.steps.is_empty() {
            out.push("no steps".into());
        }
        for (i, s) in self.steps.iter().enumerate() {
            let n = i + 1;
            let Some(kind) = s.kind() else {
                out.push(format!(
                    "step {n} (`{}`): needs exactly one of run, playbook or check",
                    s.title
                ));
                continue;
            };
            if !matches!(kind, StepKind::Check { .. }) && (!s.open.is_empty() || s.screen.is_some())
            {
                out.push(format!(
                    "step {n}: `open` and `screen` belong to check steps"
                ));
            }
            if let StepKind::Run(mods) = kind
                && mods.is_empty()
            {
                out.push(format!("step {n}: `run` lists no modules"));
            }
            if let Some(sc) = &s.screen
                && sc != "review"
                && sc != "findings"
            {
                out.push(format!(
                    "step {n}: screen `{sc}` must be review or findings"
                ));
            }
            match &s.on {
                Some(On::Seed(v)) if v != "seed" => out.push(format!(
                    "step {n}: `on = \"{v}\"`: the only string selector is \"seed\""
                )),
                Some(On::Select(sel)) => {
                    if let Some(ms) = &sel.min_strength
                        && !["single", "corroborated", "confirmed"].contains(&ms.as_str())
                    {
                        out.push(format!("step {n}: min_strength `{ms}` must be single, corroborated or confirmed"));
                    }
                    if sel.max.is_some_and(|m| m == 0 || m > MAX_CAP) {
                        out.push(format!("step {n}: max must be 1–{MAX_CAP}"));
                    }
                }
                None if !matches!(kind, StepKind::Check { .. }) => {
                    out.push(format!("step {n}: run and playbook steps need `on`"));
                }
                _ => {}
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_lints() {
        let ok = r#"
[playbook]
name = "person-from-username"
version = "1.0.0"
description = "x"
seed = ["username"]

[[step]]
title = "Sweep"
run = ["username-sweep"]
on = "seed"

[[step]]
title = "Avatars"
run = ["avatar-hash"]
on = { type = "account" }

[[step]]
title = "Look"
check = "Reverse image search."
open = ["lens"]
on = { type = "image", max = 25 }
"#;
        let p = PlaybookFile::from_toml(ok).unwrap();
        assert_eq!(p.steps.len(), 3);
        assert!(matches!(
            p.steps[1].on,
            Some(On::Select(Selector {
                from: From::Run,
                ..
            }))
        ));
        let bad = ok
            .replace(
                "check = \"Reverse image search.\"",
                "check = \"x\"\nrun = [\"a\"]",
            )
            .replace("max = 25", "max = 500");
        let e = PlaybookFile::from_toml(&bad).unwrap_err().to_string();
        assert!(e.contains("exactly one of run, playbook or check"), "{e}");
        assert!(PlaybookFile::from_toml("[playbook]\nname=\"x\"").is_err());
    }
}
