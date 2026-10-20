//! Scope rules (ARCHITECTURE §10): `domain:<suffix>`, `cidr:<net>`,
//! `platform:<id>`, `regex:<re>`, optionally prefixed with `deny`.
//!
//! Deny always beats allow. Allow rules constrain only the entities they can
//! apply to (hostnames for `domain:`, IPs for `cidr:`, accounts for
//! `platform:`, every key for `regex:`); with no applicable allow rule, an
//! entity that isn't denied is in scope.

use std::net::IpAddr;

use regex::Regex;
use spektr_model::hubs::Cidr;
use spektr_model::{CoreType, EntityType};
use spektr_store::{ScopeKind, ScopeMatcher, ScopeRuleRow};

use crate::error::{EngineError, EngineResult};

#[derive(Debug, Clone)]
enum Compiled {
    Domain(String),
    Cidr(Cidr),
    Platform(String),
    Regex(Regex),
}

#[derive(Debug, Clone)]
struct Rule {
    kind: ScopeKind,
    m: Compiled,
}

/// Parse CLI/palette syntax into `(kind, matcher, pattern)`.
pub fn parse_rule(s: &str) -> EngineResult<(ScopeKind, ScopeMatcher, String)> {
    let s = s.trim();
    let (kind, rest) = match s.strip_prefix("deny") {
        Some(r) if r.starts_with(char::is_whitespace) => (ScopeKind::Deny, r.trim()),
        _ => (ScopeKind::Allow, s),
    };
    let (m, pattern) = rest.split_once(':').ok_or_else(|| {
        EngineError::Invalid(format!(
            "scope rule `{s}`: expected domain:, cidr:, platform: or regex:"
        ))
    })?;
    let pattern = pattern.trim();
    let (matcher, pattern) = match m.trim() {
        "domain" => {
            let p = pattern
                .strip_prefix("*.")
                .unwrap_or(pattern)
                .trim_end_matches('.')
                .to_ascii_lowercase();
            if p.is_empty() || p.contains(char::is_whitespace) {
                return Err(EngineError::Invalid(format!(
                    "scope rule `{s}`: bad domain"
                )));
            }
            (ScopeMatcher::DomainSuffix, p)
        }
        "cidr" => {
            Cidr::parse(pattern)
                .ok_or_else(|| EngineError::Invalid(format!("scope rule `{s}`: bad CIDR")))?;
            (ScopeMatcher::Cidr, pattern.to_string())
        }
        "platform" => (ScopeMatcher::Platform, pattern.to_ascii_lowercase()),
        "regex" => {
            Regex::new(pattern)
                .map_err(|e| EngineError::Invalid(format!("scope rule `{s}`: {e}")))?;
            (ScopeMatcher::Regex, pattern.to_string())
        }
        other => {
            return Err(EngineError::Invalid(format!(
                "scope rule `{s}`: unknown matcher `{other}`"
            )));
        }
    };
    Ok((kind, matcher, pattern))
}

/// Display form of a stored rule, as the user would type it.
pub fn format_rule(row: &ScopeRuleRow) -> String {
    let m = match row.matcher {
        ScopeMatcher::DomainSuffix => "domain",
        ScopeMatcher::Cidr => "cidr",
        ScopeMatcher::Platform => "platform",
        ScopeMatcher::Regex => "regex",
    };
    let deny = if row.kind == ScopeKind::Deny {
        "deny "
    } else {
        ""
    };
    format!("{deny}{m}:{}", row.pattern)
}

/// Compiled scope rules for a case.
#[derive(Debug, Clone, Default)]
pub struct Scope {
    rules: Vec<Rule>,
}

impl Scope {
    pub fn from_rows(rows: &[ScopeRuleRow]) -> Self {
        let rules = rows
            .iter()
            .filter_map(|r| {
                let m = match r.matcher {
                    ScopeMatcher::DomainSuffix => Compiled::Domain(r.pattern.clone()),
                    ScopeMatcher::Cidr => Compiled::Cidr(Cidr::parse(&r.pattern)?),
                    ScopeMatcher::Platform => Compiled::Platform(r.pattern.clone()),
                    ScopeMatcher::Regex => Compiled::Regex(Regex::new(&r.pattern).ok()?),
                };
                Some(Rule { kind: r.kind, m })
            })
            .collect();
        Self { rules }
    }

    pub fn is_empty(&self) -> bool {
        self.rules.is_empty()
    }

    pub fn in_scope(&self, ty: &EntityType, key: &str) -> bool {
        let host = host_of(ty, key);
        let ip = ip_of(ty, key);
        let platform = (ty.core() == Some(CoreType::Account))
            .then(|| key.split_once(':').map_or(key, |p| p.0));
        // Some(matched) if the rule applies to this entity at all.
        let eval = |m: &Compiled| -> Option<bool> {
            match m {
                Compiled::Domain(suffix) => host
                    .as_deref()
                    .map(|h| h == suffix || h.ends_with(&format!(".{suffix}"))),
                Compiled::Cidr(c) => ip.map(|ip| c.contains(ip)),
                Compiled::Platform(p) => platform.map(|pl| pl == p),
                Compiled::Regex(re) => Some(re.is_match(key)),
            }
        };
        if self
            .rules
            .iter()
            .filter(|r| r.kind == ScopeKind::Deny)
            .any(|r| eval(&r.m) == Some(true))
        {
            return false;
        }
        let applicable: Vec<bool> = self
            .rules
            .iter()
            .filter(|r| r.kind == ScopeKind::Allow)
            .filter_map(|r| eval(&r.m))
            .collect();
        applicable.is_empty() || applicable.into_iter().any(|m| m)
    }
}

fn host_of(ty: &EntityType, key: &str) -> Option<String> {
    match ty.core()? {
        CoreType::Domain | CoreType::Subdomain => Some(key.to_string()),
        CoreType::Email => key.rsplit_once('@').map(|(_, d)| d.to_string()),
        CoreType::Url => url::Url::parse(key)
            .ok()
            .and_then(|u| u.host_str().map(|h| h.trim_end_matches('.').to_string())),
        _ => None,
    }
}

fn ip_of(ty: &EntityType, key: &str) -> Option<IpAddr> {
    match ty.core()? {
        CoreType::Ip => key.parse().ok(),
        CoreType::Url => url::Url::parse(key).ok().and_then(|u| match u.host()? {
            url::Host::Ipv4(v4) => Some(IpAddr::V4(v4)),
            url::Host::Ipv6(v6) => Some(IpAddr::V6(v6)),
            url::Host::Domain(_) => None,
        }),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scope(rules: &[&str]) -> Scope {
        let rows: Vec<ScopeRuleRow> = rules
            .iter()
            .map(|r| {
                let (kind, matcher, pattern) = parse_rule(r).unwrap();
                ScopeRuleRow {
                    id: spektr_model::ScopeRuleId(uuid::Uuid::nil()),
                    kind,
                    matcher,
                    pattern,
                    created_at: jiff::Timestamp::UNIX_EPOCH,
                }
            })
            .collect();
        Scope::from_rows(&rows)
    }

    fn t(s: &str) -> EntityType {
        s.parse().unwrap()
    }

    #[test]
    fn allow_and_deny() {
        let s = scope(&[
            "domain:*.kestrel.example",
            "deny domain:cdn.kestrel.example",
            "cidr:203.0.113.0/24",
        ]);
        assert!(s.in_scope(&t("domain"), "kestrel.example"));
        assert!(s.in_scope(&t("subdomain"), "vpn.kestrel.example"));
        assert!(!s.in_scope(&t("subdomain"), "cdn.kestrel.example"));
        assert!(!s.in_scope(&t("subdomain"), "a.cdn.kestrel.example"));
        assert!(!s.in_scope(&t("domain"), "other.example"));
        assert!(s.in_scope(&t("email"), "ops@kestrel.example"));
        assert!(s.in_scope(&t("url"), "https://dev.kestrel.example/x"));
        assert!(s.in_scope(&t("ip"), "203.0.113.24"));
        assert!(!s.in_scope(&t("ip"), "198.51.100.7"));
        // no allow rule applies to usernames
        assert!(s.in_scope(&t("username"), "jmarlow.wav"));
        assert!(!s.in_scope(&t("domain"), "notkestrel.example"));
    }

    #[test]
    fn empty_scope_allows_everything_and_deny_only() {
        assert!(Scope::default().in_scope(&t("domain"), "anything.example"));
        let s = scope(&["deny platform:reddit", "deny regex:^spam"]);
        assert!(!s.in_scope(&t("account"), "reddit:jmarlow"));
        assert!(s.in_scope(&t("account"), "github:jmarlow"));
        assert!(!s.in_scope(&t("username"), "spammer"));
    }

    #[test]
    fn parse_errors() {
        assert!(parse_rule("kestrel.example").is_err());
        assert!(parse_rule("cidr:300.0.0.0/8").is_err());
        assert!(parse_rule("regex:(").is_err());
        assert!(parse_rule("planet:mars").is_err());
        let (k, m, p) = parse_rule("deny domain:*.Kestrel.Example.").unwrap();
        assert_eq!(
            (k, m, p.as_str()),
            (
                ScopeKind::Deny,
                ScopeMatcher::DomainSuffix,
                "kestrel.example"
            )
        );
    }
}
