//! The case vocabulary: built-in types and predicates plus what installed
//! modules declare (extension types, extension predicates, platforms).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::platforms::PlatformRegistry;
use crate::predicates::{CorePredicate, Predicate, VocabError};
use crate::types::{EntityType, ExtName};

/// The fixed normalization rules an extension type may pick (DATA_MODEL §1.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ExtNormalize {
    Exact,
    Trim,
    Lowercase,
    Casefold,
    Url,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtPredicate {
    pub src: Vec<EntityType>,
    pub dst: Vec<EntityType>,
}

/// Everything needed to validate entities and edges in a case.
#[derive(Debug, Clone, Default)]
pub struct Vocabulary {
    pub platforms: PlatformRegistry,
    types: BTreeMap<ExtName, ExtNormalize>,
    predicates: BTreeMap<ExtName, ExtPredicate>,
}

impl Vocabulary {
    /// Built-in types, predicates and platforms only.
    pub fn builtin() -> Self {
        Self::default()
    }

    fn check_namespace(module: &str, name: &ExtName) -> Result<(), VocabError> {
        if name.module() != module {
            return Err(VocabError::Namespace {
                name: name.to_string(),
                module: module.to_string(),
            });
        }
        Ok(())
    }

    /// `[[module.types]]`.
    pub fn declare_type(
        &mut self,
        module: &str,
        name: &str,
        rule: ExtNormalize,
    ) -> Result<(), VocabError> {
        let n = ExtName::parse(name).ok_or_else(|| VocabError::UnknownType(name.to_string()))?;
        Self::check_namespace(module, &n)?;
        if self.types.contains_key(&n) {
            return Err(VocabError::Duplicate(name.to_string()));
        }
        self.types.insert(n, rule);
        Ok(())
    }

    /// `[[module.predicates]]`. Source and target types must already be known.
    pub fn declare_predicate(
        &mut self,
        module: &str,
        name: &str,
        src: &[&str],
        dst: &[&str],
    ) -> Result<(), VocabError> {
        let n =
            ExtName::parse(name).ok_or_else(|| VocabError::UnknownPredicate(name.to_string()))?;
        Self::check_namespace(module, &n)?;
        if self.predicates.contains_key(&n) {
            return Err(VocabError::Duplicate(name.to_string()));
        }
        let resolve = |list: &[&str]| -> Result<Vec<EntityType>, VocabError> {
            list.iter().map(|t| self.entity_type(t)).collect()
        };
        let pred = ExtPredicate {
            src: resolve(src)?,
            dst: resolve(dst)?,
        };
        self.predicates.insert(n, pred);
        Ok(())
    }

    /// A known entity type: core, or a declared extension.
    pub fn entity_type(&self, s: &str) -> Result<EntityType, VocabError> {
        let ty: EntityType = s
            .parse()
            .map_err(|_| VocabError::UnknownType(s.to_string()))?;
        match &ty {
            EntityType::Core(_) => Ok(ty),
            EntityType::Ext(n) if self.types.contains_key(n) => Ok(ty),
            EntityType::Ext(_) => Err(VocabError::UnknownType(s.to_string())),
        }
    }

    pub fn ext_rule(&self, name: &ExtName) -> Option<ExtNormalize> {
        self.types.get(name).copied()
    }

    /// A known predicate: core, or a declared extension.
    pub fn predicate(&self, s: &str) -> Result<Predicate, VocabError> {
        let p: Predicate = s.parse()?;
        match &p {
            Predicate::Core(_) => Ok(p),
            Predicate::Ext(n) if self.predicates.contains_key(n) => Ok(p),
            Predicate::Ext(_) => Err(VocabError::UnknownPredicate(s.to_string())),
        }
    }

    /// Reject any edge whose predicate, source type or target type isn't in
    /// the registry (DATA_MODEL §3).
    pub fn validate_edge(
        &self,
        src: &EntityType,
        pred: &Predicate,
        dst: &EntityType,
    ) -> Result<(), VocabError> {
        for ty in [src, dst] {
            if let EntityType::Ext(n) = ty
                && !self.types.contains_key(n)
            {
                return Err(VocabError::UnknownType(n.to_string()));
            }
        }
        let (src_ok, dst_ok) = match pred {
            Predicate::Core(c) => {
                let (s, d) = check_core(*c, src, dst);
                (s, d)
            }
            Predicate::Ext(n) => {
                let rule = self
                    .predicates
                    .get(n)
                    .ok_or_else(|| VocabError::UnknownPredicate(n.to_string()))?;
                (rule.src.contains(src), rule.dst.contains(dst))
            }
        };
        if !src_ok {
            return Err(VocabError::BadSource {
                pred: pred.to_string(),
                ty: src.to_string(),
            });
        }
        if !dst_ok {
            return Err(VocabError::BadTarget {
                pred: pred.to_string(),
                ty: dst.to_string(),
            });
        }
        Ok(())
    }
}

/// Symmetric core predicates accept either orientation of an allowed pair.
fn check_core(c: CorePredicate, src: &EntityType, dst: &EntityType) -> (bool, bool) {
    let fwd = (c.sources().allows(src), c.targets().allows(dst));
    if c.is_symmetric() && !(fwd.0 && fwd.1) {
        let rev = (c.sources().allows(dst), c.targets().allows(src));
        if rev.0 && rev.1 {
            return (true, true);
        }
    }
    fwd
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::CoreType;

    fn t(s: &str) -> EntityType {
        s.parse().unwrap()
    }

    #[test]
    fn core_edges() {
        let v = Vocabulary::builtin();
        let has_account = Predicate::Core(CorePredicate::HasAccount);
        assert!(
            v.validate_edge(&t("username"), &has_account, &t("account"))
                .is_ok()
        );
        assert!(
            v.validate_edge(&t("person"), &has_account, &t("account"))
                .is_ok()
        );
        assert!(matches!(
            v.validate_edge(&t("email"), &has_account, &t("account")),
            Err(VocabError::BadSource { .. })
        ));
        assert!(matches!(
            v.validate_edge(&t("username"), &has_account, &t("url")),
            Err(VocabError::BadTarget { .. })
        ));
        let shares = Predicate::Core(CorePredicate::SharesArtifact);
        assert!(
            v.validate_edge(&t("domain"), &shares, &t("subdomain"))
                .is_ok()
        );
    }

    #[test]
    fn unknown_things_are_rejected() {
        let v = Vocabulary::builtin();
        assert!(v.predicate("knows").is_err());
        assert!(v.predicate("x.lemmy.member_of_instance").is_err());
        assert!(v.entity_type("x.lemmy.instance").is_err());
        let ext = t("x.lemmy.instance");
        assert!(matches!(
            v.validate_edge(
                &ext,
                &Predicate::Core(CorePredicate::SharesArtifact),
                &t("domain")
            ),
            Err(VocabError::UnknownType(_))
        ));
    }

    #[test]
    fn extensions() {
        let mut v = Vocabulary::builtin();
        v.declare_type("lemmy", "x.lemmy.instance", ExtNormalize::Lowercase)
            .unwrap();
        assert!(matches!(
            v.declare_type("other", "x.lemmy.thing", ExtNormalize::Exact),
            Err(VocabError::Namespace { .. })
        ));
        v.declare_predicate(
            "lemmy",
            "x.lemmy.member_of_instance",
            &["account"],
            &["x.lemmy.instance"],
        )
        .unwrap();
        assert!(
            v.declare_predicate("lemmy", "x.lemmy.bad", &["account"], &["x.lemmy.nope"])
                .is_err()
        );
        let p = v.predicate("x.lemmy.member_of_instance").unwrap();
        assert!(
            v.validate_edge(&t("account"), &p, &t("x.lemmy.instance"))
                .is_ok()
        );
        assert!(
            v.validate_edge(&t("username"), &p, &t("x.lemmy.instance"))
                .is_err()
        );
        assert_eq!(
            v.entity_type("x.lemmy.instance").unwrap().core(),
            None::<CoreType>
        );
    }
}
