//! Entity types: the 15 core types (DATA_MODEL §1.1) and module-declared
//! extension types `x.<module>.<name>` (§1.2).

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum CoreType {
    Person,
    Username,
    Account,
    Email,
    Phone,
    Image,
    Location,
    Organization,
    Url,
    Domain,
    Subdomain,
    Ip,
    Asn,
    Certificate,
    Breach,
}

impl CoreType {
    /// In the entities-pane group order (TUI_SPEC §4.2).
    pub const ALL: [CoreType; 15] = [
        CoreType::Person,
        CoreType::Username,
        CoreType::Account,
        CoreType::Email,
        CoreType::Phone,
        CoreType::Image,
        CoreType::Location,
        CoreType::Organization,
        CoreType::Url,
        CoreType::Domain,
        CoreType::Subdomain,
        CoreType::Ip,
        CoreType::Asn,
        CoreType::Certificate,
        CoreType::Breach,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            CoreType::Person => "person",
            CoreType::Username => "username",
            CoreType::Account => "account",
            CoreType::Email => "email",
            CoreType::Phone => "phone",
            CoreType::Image => "image",
            CoreType::Location => "location",
            CoreType::Organization => "organization",
            CoreType::Url => "url",
            CoreType::Domain => "domain",
            CoreType::Subdomain => "subdomain",
            CoreType::Ip => "ip",
            CoreType::Asn => "asn",
            CoreType::Certificate => "certificate",
            CoreType::Breach => "breach",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        CoreType::ALL.into_iter().find(|t| t.as_str() == s)
    }

    /// Unicode glyph (TUI_SPEC §3).
    pub fn glyph(self) -> char {
        match self {
            CoreType::Person => '◎',
            CoreType::Username => '~',
            CoreType::Account => '◐',
            CoreType::Email => '@',
            CoreType::Phone => '+',
            CoreType::Image => '▨',
            CoreType::Location => '⌖',
            CoreType::Organization => '▣',
            CoreType::Url => '↗',
            CoreType::Domain => '◆',
            CoreType::Subdomain => '◇',
            CoreType::Ip => '●',
            CoreType::Asn => '◈',
            CoreType::Certificate => '⊟',
            CoreType::Breach => '✕',
        }
    }

    /// ASCII glyph (`glyphs = "ascii"`).
    pub fn ascii(self) -> char {
        match self {
            CoreType::Person => 'P',
            CoreType::Username => '~',
            CoreType::Account => 'a',
            CoreType::Email => '@',
            CoreType::Phone => '+',
            CoreType::Image => 'i',
            CoreType::Location => 'L',
            CoreType::Organization => 'O',
            CoreType::Url => 'U',
            CoreType::Domain => 'D',
            CoreType::Subdomain => 'd',
            CoreType::Ip => 'I',
            CoreType::Asn => 'A',
            CoreType::Certificate => 'C',
            CoreType::Breach => 'B',
        }
    }

    /// Group order index (identity, web, infrastructure, breach).
    pub fn order(self) -> usize {
        CoreType::ALL.iter().position(|t| *t == self).unwrap_or(0)
    }
}

impl fmt::Display for CoreType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A namespaced extension name, `x.<module>.<name>`: module `[a-z0-9-]{2,32}`,
/// name `[a-z0-9_]{1,32}`. Used for extension types and predicates.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ExtName(String);

impl ExtName {
    pub fn parse(s: &str) -> Option<Self> {
        let rest = s.strip_prefix("x.")?;
        let (module, name) = rest.split_once('.')?;
        let module_ok = (2..=32).contains(&module.len())
            && module
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
        let name_ok = (1..=32).contains(&name.len())
            && name
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_');
        (module_ok && name_ok).then(|| Self(s.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The declaring module.
    pub fn module(&self) -> &str {
        self.0[2..].split('.').next().unwrap_or("")
    }
}

impl fmt::Display for ExtName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum EntityType {
    Core(CoreType),
    Ext(ExtName),
}

impl EntityType {
    pub fn core(&self) -> Option<CoreType> {
        match self {
            EntityType::Core(c) => Some(*c),
            EntityType::Ext(_) => None,
        }
    }

    pub fn is(&self, c: CoreType) -> bool {
        self.core() == Some(c)
    }

    pub fn as_str(&self) -> &str {
        match self {
            EntityType::Core(c) => c.as_str(),
            EntityType::Ext(e) => e.as_str(),
        }
    }

    pub fn glyph(&self) -> char {
        self.core().map_or('·', CoreType::glyph)
    }

    pub fn ascii(&self) -> char {
        self.core().map_or('x', CoreType::ascii)
    }

    /// Sort key for the entities pane: core types in order, extensions last.
    pub fn order(&self) -> (usize, &str) {
        match self {
            EntityType::Core(c) => (c.order(), ""),
            EntityType::Ext(e) => (CoreType::ALL.len(), e.as_str()),
        }
    }
}

impl From<CoreType> for EntityType {
    fn from(c: CoreType) -> Self {
        EntityType::Core(c)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("unknown entity type `{0}`")]
pub struct UnknownType(pub String);

impl FromStr for EntityType {
    type Err = UnknownType;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if let Some(c) = CoreType::parse(s) {
            return Ok(EntityType::Core(c));
        }
        ExtName::parse(s)
            .map(EntityType::Ext)
            .ok_or_else(|| UnknownType(s.to_string()))
    }
}

impl fmt::Display for EntityType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl Serialize for EntityType {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for EntityType {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        s.parse().map_err(serde::de::Error::custom)
    }
}

/// `<type>:<value>`, split at the **first** colon (ARCHITECTURE §10), so
/// `account:github:jmarlow-wav` and `url:https://…` work.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntityRef {
    pub ty: EntityType,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum EntityRefError {
    #[error("expected <type>:<value>, got `{0}`")]
    Syntax(String),
    #[error(transparent)]
    Type(#[from] UnknownType),
}

impl FromStr for EntityRef {
    type Err = EntityRefError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (ty, value) = s
            .split_once(':')
            .ok_or_else(|| EntityRefError::Syntax(s.to_string()))?;
        if value.trim().is_empty() {
            return Err(EntityRefError::Syntax(s.to_string()));
        }
        Ok(EntityRef {
            ty: ty.trim().parse()?,
            value: value.to_string(),
        })
    }
}

impl fmt::Display for EntityRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.ty, self.value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn core_types_roundtrip_and_glyphs_are_single_chars() {
        for t in CoreType::ALL {
            assert_eq!(CoreType::parse(t.as_str()), Some(t));
            assert_eq!(
                t.as_str().parse::<EntityType>().unwrap(),
                EntityType::Core(t)
            );
        }
        let glyphs: std::collections::BTreeSet<_> =
            CoreType::ALL.iter().map(|t| t.glyph()).collect();
        assert_eq!(glyphs.len(), 15);
    }

    #[test]
    fn extension_names() {
        assert!(ExtName::parse("x.shodan.service").is_some());
        assert_eq!(
            ExtName::parse("x.lemmy.instance").unwrap().module(),
            "lemmy"
        );
        assert!(ExtName::parse("x.s.service").is_none());
        assert!(ExtName::parse("x.Shodan.service").is_none());
        assert!(ExtName::parse("shodan.service").is_none());
        assert!(ExtName::parse("x.shodan").is_none());
        assert_eq!(
            "x.shodan.service".parse::<EntityType>().unwrap().glyph(),
            '·'
        );
    }

    #[test]
    fn entity_ref_splits_at_first_colon() {
        let r: EntityRef = "account:github:jmarlow-wav".parse().unwrap();
        assert_eq!(r.ty, EntityType::Core(CoreType::Account));
        assert_eq!(r.value, "github:jmarlow-wav");
        let r: EntityRef = "url:https://jmarlow.example/".parse().unwrap();
        assert_eq!(r.value, "https://jmarlow.example/");
        assert!("nonsense".parse::<EntityRef>().is_err());
        assert!("bogus:x".parse::<EntityRef>().is_err());
        assert!("email:".parse::<EntityRef>().is_err());
    }
}
