//! The closed predicate vocabulary (DATA_MODEL §3) and shared-artifact kinds.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::types::{CoreType, EntityType, ExtName};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum CorePredicate {
    UsesHandle,
    HasAccount,
    VariantOf,
    HasEmail,
    HasPhone,
    RelatedTo,
    WorksAt,
    StudiedAt,
    LocatedIn,
    Posted,
    TakenAt,
    Depicts,
    HostedAt,
    LinksTo,
    AtDomain,
    OnHost,
    SubdomainOf,
    ResolvesTo,
    ReverseResolvesTo,
    MailHandledBy,
    RegisteredBy,
    IssuedFor,
    HostedInAsn,
    OperatedBy,
    AppearsInBreach,
    SharesArtifact,
}

/// Allowed endpoint types for a predicate.
#[derive(Debug, Clone, Copy)]
pub enum Ends {
    /// Any type, including extension types.
    Any,
    Of(&'static [CoreType]),
}

impl Ends {
    pub fn allows(self, ty: &EntityType) -> bool {
        match self {
            Ends::Any => true,
            Ends::Of(list) => ty.core().is_some_and(|c| list.contains(&c)),
        }
    }
}

use CoreType as T;

/// `(predicate, name, sources, targets, symmetric)`.
const TABLE: &[(CorePredicate, &str, Ends, Ends, bool)] = &[
    (
        CorePredicate::UsesHandle,
        "uses_handle",
        Ends::Of(&[T::Person]),
        Ends::Of(&[T::Username]),
        false,
    ),
    (
        CorePredicate::HasAccount,
        "has_account",
        Ends::Of(&[T::Person, T::Username]),
        Ends::Of(&[T::Account]),
        false,
    ),
    (
        CorePredicate::VariantOf,
        "variant_of",
        Ends::Of(&[T::Username]),
        Ends::Of(&[T::Username]),
        true,
    ),
    (
        CorePredicate::HasEmail,
        "has_email",
        Ends::Of(&[
            T::Person,
            T::Account,
            T::Organization,
            T::Url,
            T::Domain,
            T::Subdomain,
        ]),
        Ends::Of(&[T::Email]),
        false,
    ),
    (
        CorePredicate::HasPhone,
        "has_phone",
        Ends::Of(&[T::Person, T::Account, T::Organization]),
        Ends::Of(&[T::Phone]),
        false,
    ),
    (
        CorePredicate::RelatedTo,
        "related_to",
        Ends::Of(&[T::Person]),
        Ends::Of(&[T::Person]),
        true,
    ),
    (
        CorePredicate::WorksAt,
        "works_at",
        Ends::Of(&[T::Person]),
        Ends::Of(&[T::Organization]),
        false,
    ),
    (
        CorePredicate::StudiedAt,
        "studied_at",
        Ends::Of(&[T::Person]),
        Ends::Of(&[T::Organization]),
        false,
    ),
    (
        CorePredicate::LocatedIn,
        "located_in",
        Ends::Of(&[T::Person, T::Account, T::Organization]),
        Ends::Of(&[T::Location]),
        false,
    ),
    (
        CorePredicate::Posted,
        "posted",
        Ends::Of(&[T::Account]),
        Ends::Of(&[T::Image, T::Url]),
        false,
    ),
    (
        CorePredicate::TakenAt,
        "taken_at",
        Ends::Of(&[T::Image]),
        Ends::Of(&[T::Location]),
        false,
    ),
    (
        CorePredicate::Depicts,
        "depicts",
        Ends::Of(&[T::Image]),
        Ends::Of(&[T::Person]),
        false,
    ),
    (
        CorePredicate::HostedAt,
        "hosted_at",
        Ends::Of(&[T::Image]),
        Ends::Of(&[T::Url]),
        false,
    ),
    (
        CorePredicate::LinksTo,
        "links_to",
        Ends::Of(&[T::Account, T::Url, T::Domain, T::Subdomain]),
        Ends::Of(&[T::Url]),
        false,
    ),
    (
        CorePredicate::AtDomain,
        "at_domain",
        Ends::Of(&[T::Email]),
        Ends::Of(&[T::Domain, T::Subdomain]),
        false,
    ),
    (
        CorePredicate::OnHost,
        "on_host",
        Ends::Of(&[T::Url]),
        Ends::Of(&[T::Domain, T::Subdomain]),
        false,
    ),
    (
        CorePredicate::SubdomainOf,
        "subdomain_of",
        Ends::Of(&[T::Subdomain]),
        Ends::Of(&[T::Domain]),
        false,
    ),
    (
        CorePredicate::ResolvesTo,
        "resolves_to",
        Ends::Of(&[T::Domain, T::Subdomain]),
        Ends::Of(&[T::Ip]),
        false,
    ),
    (
        CorePredicate::ReverseResolvesTo,
        "reverse_resolves_to",
        Ends::Of(&[T::Ip]),
        Ends::Of(&[T::Domain, T::Subdomain]),
        false,
    ),
    (
        CorePredicate::MailHandledBy,
        "mail_handled_by",
        Ends::Of(&[T::Domain, T::Subdomain]),
        Ends::Of(&[T::Domain, T::Subdomain]),
        false,
    ),
    (
        CorePredicate::RegisteredBy,
        "registered_by",
        Ends::Of(&[T::Domain]),
        Ends::Of(&[T::Email, T::Organization, T::Person]),
        false,
    ),
    (
        CorePredicate::IssuedFor,
        "issued_for",
        Ends::Of(&[T::Certificate]),
        Ends::Of(&[T::Domain, T::Subdomain]),
        false,
    ),
    (
        CorePredicate::HostedInAsn,
        "hosted_in_asn",
        Ends::Of(&[T::Ip]),
        Ends::Of(&[T::Asn]),
        false,
    ),
    (
        CorePredicate::OperatedBy,
        "operated_by",
        Ends::Of(&[T::Asn]),
        Ends::Of(&[T::Organization]),
        false,
    ),
    (
        CorePredicate::AppearsInBreach,
        "appears_in_breach",
        Ends::Of(&[T::Email, T::Username]),
        Ends::Of(&[T::Breach]),
        false,
    ),
    (
        CorePredicate::SharesArtifact,
        "shares_artifact",
        Ends::Any,
        Ends::Any,
        true,
    ),
];

impl CorePredicate {
    pub const ALL: [CorePredicate; 26] = [
        CorePredicate::UsesHandle,
        CorePredicate::HasAccount,
        CorePredicate::VariantOf,
        CorePredicate::HasEmail,
        CorePredicate::HasPhone,
        CorePredicate::RelatedTo,
        CorePredicate::WorksAt,
        CorePredicate::StudiedAt,
        CorePredicate::LocatedIn,
        CorePredicate::Posted,
        CorePredicate::TakenAt,
        CorePredicate::Depicts,
        CorePredicate::HostedAt,
        CorePredicate::LinksTo,
        CorePredicate::AtDomain,
        CorePredicate::OnHost,
        CorePredicate::SubdomainOf,
        CorePredicate::ResolvesTo,
        CorePredicate::ReverseResolvesTo,
        CorePredicate::MailHandledBy,
        CorePredicate::RegisteredBy,
        CorePredicate::IssuedFor,
        CorePredicate::HostedInAsn,
        CorePredicate::OperatedBy,
        CorePredicate::AppearsInBreach,
        CorePredicate::SharesArtifact,
    ];

    fn row(self) -> &'static (CorePredicate, &'static str, Ends, Ends, bool) {
        // TABLE has one row per variant (checked by a test), so the fallback
        // to the last row is unreachable.
        TABLE
            .iter()
            .find(|r| r.0 == self)
            .unwrap_or(&TABLE[TABLE.len() - 1])
    }

    pub fn as_str(self) -> &'static str {
        self.row().1
    }

    pub fn sources(self) -> Ends {
        self.row().2
    }

    pub fn targets(self) -> Ends {
        self.row().3
    }

    /// Symmetric predicates are stored with `src < dst` (by id).
    pub fn is_symmetric(self) -> bool {
        self.row().4
    }

    /// Only an analyst may assert it (`depicts`).
    pub fn analyst_only(self) -> bool {
        self == CorePredicate::Depicts
    }

    pub fn parse(s: &str) -> Option<Self> {
        TABLE.iter().find(|r| r.1 == s).map(|r| r.0)
    }
}

impl fmt::Display for CorePredicate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A core predicate or a module-declared `x.<module>.<name>` predicate.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Predicate {
    Core(CorePredicate),
    Ext(ExtName),
}

impl Predicate {
    pub fn core(&self) -> Option<CorePredicate> {
        match self {
            Predicate::Core(c) => Some(*c),
            Predicate::Ext(_) => None,
        }
    }

    pub fn as_str(&self) -> &str {
        match self {
            Predicate::Core(c) => c.as_str(),
            Predicate::Ext(e) => e.as_str(),
        }
    }

    pub fn is_symmetric(&self) -> bool {
        self.core().is_some_and(CorePredicate::is_symmetric)
    }
}

impl From<CorePredicate> for Predicate {
    fn from(c: CorePredicate) -> Self {
        Predicate::Core(c)
    }
}

impl fmt::Display for Predicate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum VocabError {
    #[error("unknown predicate `{0}`")]
    UnknownPredicate(String),
    #[error("unknown entity type `{0}`")]
    UnknownType(String),
    #[error("`{pred}` does not allow source type `{ty}`")]
    BadSource { pred: String, ty: String },
    #[error("`{pred}` does not allow target type `{ty}`")]
    BadTarget { pred: String, ty: String },
    #[error("`{name}` is outside module `{module}`'s namespace (x.{module}.…)")]
    Namespace { name: String, module: String },
    #[error("`{0}` is already declared")]
    Duplicate(String),
    #[error("`shares_artifact` needs an artifact kind and value")]
    MissingArtifact,
    #[error("unknown artifact kind `{0}`")]
    UnknownArtifact(String),
}

impl FromStr for Predicate {
    type Err = VocabError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if let Some(c) = CorePredicate::parse(s) {
            return Ok(Predicate::Core(c));
        }
        ExtName::parse(s)
            .map(Predicate::Ext)
            .ok_or_else(|| VocabError::UnknownPredicate(s.to_string()))
    }
}

impl Serialize for Predicate {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for Predicate {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        s.parse().map_err(serde::de::Error::custom)
    }
}

/// What a `shares_artifact` edge shares (DATA_MODEL §3, §5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactKind {
    AvatarPhash,
    AnalyticsId,
    FaviconMmh3,
    RegistrantEmail,
    CertSan,
    Ip,
}

impl ArtifactKind {
    pub const ALL: [ArtifactKind; 6] = [
        ArtifactKind::AvatarPhash,
        ArtifactKind::AnalyticsId,
        ArtifactKind::FaviconMmh3,
        ArtifactKind::RegistrantEmail,
        ArtifactKind::CertSan,
        ArtifactKind::Ip,
    ];

    /// The stored value.
    pub fn as_str(self) -> &'static str {
        match self {
            ArtifactKind::AvatarPhash => "avatar_phash",
            ArtifactKind::AnalyticsId => "analytics_id",
            ArtifactKind::FaviconMmh3 => "favicon_mmh3",
            ArtifactKind::RegistrantEmail => "registrant_email",
            ArtifactKind::CertSan => "cert_san",
            ArtifactKind::Ip => "ip",
        }
    }

    /// The display label (TUI_SPEC §3); the stored value is never shown raw.
    pub fn label(self) -> &'static str {
        match self {
            ArtifactKind::AvatarPhash => "same_avatar",
            ArtifactKind::AnalyticsId => "same_analytics_id",
            ArtifactKind::FaviconMmh3 => "same_favicon",
            ArtifactKind::RegistrantEmail => "same_registrant",
            ArtifactKind::CertSan => "same_certificate",
            ArtifactKind::Ip => "same_ip",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        ArtifactKind::ALL.into_iter().find(|k| k.as_str() == s)
    }

    /// The edge discriminator for a shared value: `artifact:value`.
    pub fn discriminator(self, value: &str) -> String {
        format!("{}:{value}", self.as_str())
    }

    /// Split a discriminator back into `(kind, value)`.
    pub fn parse_discriminator(d: &str) -> Option<(Self, &str)> {
        let (k, v) = d.split_once(':')?;
        Some((Self::parse(k)?, v))
    }
}

impl fmt::Display for ArtifactKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_covers_every_predicate_once() {
        assert_eq!(TABLE.len(), CorePredicate::ALL.len());
        for p in CorePredicate::ALL {
            assert_eq!(TABLE.iter().filter(|r| r.0 == p).count(), 1, "{p:?}");
            assert_eq!(CorePredicate::parse(p.as_str()), Some(p));
        }
    }

    #[test]
    fn symmetric_set() {
        let sym: Vec<_> = CorePredicate::ALL
            .into_iter()
            .filter(|p| p.is_symmetric())
            .map(CorePredicate::as_str)
            .collect();
        assert_eq!(sym, ["variant_of", "related_to", "shares_artifact"]);
    }

    #[test]
    fn artifacts() {
        for k in ArtifactKind::ALL {
            let d = k.discriminator("abc:def");
            assert_eq!(ArtifactKind::parse_discriminator(&d), Some((k, "abc:def")));
        }
        assert_eq!(ArtifactKind::AvatarPhash.label(), "same_avatar");
    }
}
