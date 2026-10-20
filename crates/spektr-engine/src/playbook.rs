//! Playbooks (docs/PLAYBOOKS.md): built-ins embedded from
//! `data/playbooks/`, user playbooks from `<config>/playbooks/*.toml` (a
//! user playbook with a built-in's name overrides it), and the checks that
//! need the installed modules, launchers and other playbooks (§2.9).

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use spektr_model::{CorePredicate, CoreType};
use spektr_protocol::playbook::{From, On, StepKind};
use spektr_protocol::{Manifest, PlaybookFile};

use crate::launcher::Launchers;

const BUILTIN: &[(&str, &str)] = &[
    (
        "person-from-username",
        include_str!("../data/playbooks/person-from-username.toml"),
    ),
    (
        "person-from-email",
        include_str!("../data/playbooks/person-from-email.toml"),
    ),
    (
        "person-from-phone",
        include_str!("../data/playbooks/person-from-phone.toml"),
    ),
    (
        "image-triage",
        include_str!("../data/playbooks/image-triage.toml"),
    ),
    (
        "domain-recon",
        include_str!("../data/playbooks/domain-recon.toml"),
    ),
    (
        "tracelabs-kickoff",
        include_str!("../data/playbooks/tracelabs-kickoff.toml"),
    ),
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    Builtin,
    User(PathBuf),
}

#[derive(Debug, Clone)]
pub struct Playbook {
    pub file: PlaybookFile,
    pub text: String,
    /// SHA-256 of the file text (first 16 hex digits), recorded on runs.
    pub hash: String,
    pub source: Source,
}

impl Playbook {
    pub fn name(&self) -> &str {
        &self.file.playbook.name
    }

    pub fn parse(text: &str, source: Source) -> Result<Self, String> {
        let file = PlaybookFile::from_toml(text).map_err(|e| e.to_string())?;
        Ok(Self {
            file,
            text: text.to_string(),
            hash: spektr_store::sha256(text.as_bytes()).hex()[..16].to_string(),
            source,
        })
    }
}

/// Whether a seed list (`username`, `account:github`, …) accepts an entity.
pub fn seed_matches(seeds: &[String], ty: &str, platform: Option<&str>) -> bool {
    seeds.iter().any(|s| match s.split_once(':') {
        Some((t, p)) => t == ty && platform == Some(p),
        None => s == ty,
    })
}

/// What a check needs to know about the installation.
pub struct CheckCtx<'a> {
    pub modules: &'a BTreeMap<String, Manifest>,
    /// Installed but disabled: a warning, not an error.
    pub disabled: &'a BTreeSet<String>,
    pub launchers: &'a Launchers,
    pub playbooks: &'a Playbooks,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Problems {
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
}

/// Entity types a module may produce: its declared targets, and the source
/// types of its declared predicates.
fn produced(m: &Manifest) -> BTreeSet<String> {
    let mut out: BTreeSet<String> = m.produces.iter().map(|(_, t)| t.clone()).collect();
    for (p, _) in &m.produces {
        if let Some(c) = CorePredicate::parse(p) {
            for t in CoreType::ALL {
                if c.sources().allows(&t.into()) {
                    out.insert(t.as_str().to_string());
                }
            }
        }
    }
    out
}

#[derive(Debug, Clone, Default)]
pub struct Playbooks {
    by_name: BTreeMap<String, Playbook>,
}

impl Playbooks {
    pub fn builtin() -> Result<Self, String> {
        let mut by_name = BTreeMap::new();
        for (name, text) in BUILTIN {
            let p = Playbook::parse(text, Source::Builtin)
                .map_err(|e| format!("built-in playbook {name}: {e}"))?;
            by_name.insert(name.to_string(), p);
        }
        Ok(Self { by_name })
    }

    /// Load `<dir>/*.toml`; a bad file is a warning and is skipped.
    pub fn load_dir(&mut self, dir: &Path) -> Vec<String> {
        let mut warnings = Vec::new();
        let Ok(entries) = std::fs::read_dir(dir) else {
            return warnings;
        };
        let mut files: Vec<PathBuf> = entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|x| x == "toml"))
            .collect();
        files.sort();
        for f in files {
            match std::fs::read_to_string(&f)
                .map_err(|e| e.to_string())
                .and_then(|t| Playbook::parse(&t, Source::User(f.clone())))
            {
                Ok(p) => {
                    self.by_name.insert(p.name().to_string(), p);
                }
                Err(e) => warnings.push(format!("{}: {e}", f.display())),
            }
        }
        warnings
    }

    pub fn get(&self, name: &str) -> Option<&Playbook> {
        self.by_name.get(name)
    }

    pub fn iter(&self) -> impl Iterator<Item = &Playbook> {
        self.by_name.values()
    }

    /// Playbooks whose `seed` accepts the entity, by name.
    pub fn for_entity(&self, ty: &str, platform: Option<&str>) -> Vec<&Playbook> {
        self.by_name
            .values()
            .filter(|p| seed_matches(&p.file.playbook.seed, ty, platform))
            .collect()
    }

    /// Entity types a playbook's steps can produce (for `from = "run"`).
    fn producible(&self, pb: &PlaybookFile, ctx: &CheckCtx<'_>, depth: u8) -> BTreeSet<String> {
        let mut out = BTreeSet::new();
        for s in &pb.steps {
            match s.kind() {
                Some(StepKind::Run(mods)) => {
                    for m in mods {
                        if let Some(man) = ctx.modules.get(m) {
                            out.extend(produced(man));
                        }
                    }
                }
                Some(StepKind::Playbook(child)) if depth == 0 => {
                    if let Some(c) = self.get(child) {
                        out.extend(self.producible(&c.file, ctx, 1));
                    }
                }
                _ => {}
            }
        }
        out
    }

    /// PLAYBOOKS §2.9: unknown modules (installed-but-disabled is a
    /// warning), unknown launchers, nesting deeper than one level, and
    /// `from = "run"` selectors for types nothing earlier can produce.
    pub fn check(&self, pb: &PlaybookFile, ctx: &CheckCtx<'_>) -> Problems {
        let mut p = Problems {
            errors: pb.lint(),
            warnings: Vec::new(),
        };
        let seeds: BTreeSet<String> = pb
            .playbook
            .seed
            .iter()
            .map(|s| s.split(':').next().unwrap_or(s).to_string())
            .collect();
        let mut can: BTreeSet<String> = seeds.clone();
        for (i, s) in pb.steps.iter().enumerate() {
            let n = i + 1;
            // The selector, before this step's own output counts.
            if let Some(On::Select(sel)) = &s.on
                && sel.from == From::Run
            {
                let base = sel.ty.split(':').next().unwrap_or(&sel.ty).to_string();
                if !can.contains(&base) {
                    p.errors.push(format!(
                        "step {n}: `from = \"run\"` selects `{}`, which no earlier step can produce and isn't the seed type",
                        sel.ty
                    ));
                }
            }
            match s.kind() {
                Some(StepKind::Run(mods)) => {
                    for m in mods {
                        match ctx.modules.get(m) {
                            Some(man) => {
                                if ctx.disabled.contains(m) {
                                    p.warnings.push(format!(
                                        "step {n}: module `{m}` is installed but disabled"
                                    ));
                                }
                                can.extend(produced(man));
                            }
                            None => p.errors.push(format!("step {n}: unknown module `{m}`")),
                        }
                    }
                }
                Some(StepKind::Playbook(child)) => match self.get(child) {
                    None => p
                        .errors
                        .push(format!("step {n}: unknown playbook `{child}`")),
                    Some(c) => {
                        if c.file.steps.iter().any(|cs| cs.playbook.is_some()) {
                            p.errors.push(format!("step {n}: playbook `{child}` itself runs playbooks; only one level of nesting is allowed"));
                        }
                        if c.name() == pb.playbook.name {
                            p.errors
                                .push(format!("step {n}: a playbook can't run itself"));
                        }
                        can.extend(self.producible(&c.file, ctx, 1));
                    }
                },
                Some(StepKind::Check { open, .. }) => {
                    for l in open {
                        if ctx.launchers.get(l).is_none() {
                            p.errors.push(format!("step {n}: unknown launcher `{l}`"));
                        }
                    }
                }
                None => {}
            }
        }
        p
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seeds() {
        let s = vec!["username".to_string(), "account:github".to_string()];
        assert!(seed_matches(&s, "username", None));
        assert!(seed_matches(&s, "account", Some("github")));
        assert!(!seed_matches(&s, "account", Some("gitlab")));
    }

    #[test]
    fn builtins_parse() {
        let p = Playbooks::builtin().unwrap();
        assert_eq!(p.iter().count(), 6);
        assert_eq!(p.for_entity("person", None).len(), 1);
    }
}
