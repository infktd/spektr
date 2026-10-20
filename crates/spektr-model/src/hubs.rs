//! The shipped known-hub list (`data/hubs.toml`, DATA_MODEL §7). The
//! degree-threshold half of hub detection lives in the store.

use std::collections::BTreeSet;
use std::net::IpAddr;

use serde::Deserialize;

use crate::types::{CoreType, EntityType};

const BUILTIN: &str = include_str!("../data/hubs.toml");

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct HubFile {
    #[serde(default)]
    domains: Vec<String>,
    #[serde(default)]
    host_suffixes: Vec<String>,
    #[serde(default)]
    ip_ranges: Vec<String>,
    #[serde(default)]
    asns: Vec<String>,
    #[serde(default)]
    ca_intermediates: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct KnownHubs {
    domains: BTreeSet<String>,
    host_suffixes: Vec<String>,
    ip_ranges: Vec<Cidr>,
    asns: BTreeSet<String>,
    ca_intermediates: BTreeSet<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum HubsError {
    #[error("hubs.toml: {0}")]
    Parse(String),
    #[error("hubs.toml: invalid CIDR `{0}`")]
    Cidr(String),
}

impl KnownHubs {
    /// The embedded list. It is checked by a unit test, so a parse failure
    /// here means a broken build, not bad user input.
    pub fn builtin() -> Result<Self, HubsError> {
        Self::parse(BUILTIN)
    }

    pub fn parse(src: &str) -> Result<Self, HubsError> {
        let f: HubFile = toml::from_str(src).map_err(|e| HubsError::Parse(e.to_string()))?;
        let ip_ranges = f
            .ip_ranges
            .iter()
            .map(|s| Cidr::parse(s).ok_or_else(|| HubsError::Cidr(s.clone())))
            .collect::<Result<_, _>>()?;
        Ok(Self {
            domains: f
                .domains
                .into_iter()
                .map(|d| d.to_ascii_lowercase())
                .collect(),
            host_suffixes: f
                .host_suffixes
                .into_iter()
                .map(|d| d.to_ascii_lowercase())
                .collect(),
            ip_ranges,
            asns: f.asns.into_iter().map(|a| a.to_ascii_uppercase()).collect(),
            ca_intermediates: f
                .ca_intermediates
                .into_iter()
                .map(|c| c.to_lowercase())
                .collect(),
        })
    }

    /// Whether a normalized entity is on the list.
    pub fn is_hub(&self, ty: &EntityType, key: &str, value: &str) -> bool {
        match ty.core() {
            Some(CoreType::Domain) => self.domains.contains(key),
            // A host under a listed platform domain (`web.archive.org`) is a
            // hub too, as is any host under a listed suffix.
            Some(CoreType::Subdomain) => {
                self.host_suffixes
                    .iter()
                    .chain(self.domains.iter())
                    .any(|s| {
                        key.len() > s.len()
                            && key.ends_with(s.as_str())
                            && key.as_bytes()[key.len() - s.len() - 1] == b'.'
                    })
            }
            Some(CoreType::Ip) => key
                .parse::<IpAddr>()
                .is_ok_and(|ip| self.ip_ranges.iter().any(|c| c.contains(ip))),
            Some(CoreType::Asn) => self.asns.contains(key),
            Some(CoreType::Certificate) => {
                self.ca_intermediates.contains(&value.trim().to_lowercase())
            }
            _ => false,
        }
    }
}

/// An IPv4 or IPv6 network.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cidr {
    net: IpAddr,
    prefix: u8,
}

impl Cidr {
    pub fn parse(s: &str) -> Option<Self> {
        let (addr, prefix) = s.trim().split_once('/')?;
        let net: IpAddr = addr.parse().ok()?;
        let prefix: u8 = prefix.parse().ok()?;
        let max = if net.is_ipv4() { 32 } else { 128 };
        (prefix <= max).then_some(Self { net, prefix })
    }

    pub fn contains(&self, ip: IpAddr) -> bool {
        match (self.net, ip) {
            (IpAddr::V4(n), IpAddr::V4(i)) => {
                let mask = u32::MAX
                    .checked_shl(32 - u32::from(self.prefix))
                    .unwrap_or(0);
                u32::from(n) & mask == u32::from(i) & mask
            }
            (IpAddr::V6(n), IpAddr::V6(i)) => {
                let mask = u128::MAX
                    .checked_shl(128 - u32::from(self.prefix))
                    .unwrap_or(0);
                u128::from(n) & mask == u128::from(i) & mask
            }
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(s: &str) -> EntityType {
        s.parse().unwrap()
    }

    #[test]
    fn builtin_list_parses_and_matches() {
        let h = KnownHubs::builtin().unwrap();
        assert!(h.is_hub(&t("domain"), "gmail.com", "gmail.com"));
        assert!(h.is_hub(&t("domain"), "github.com", "github.com"));
        assert!(!h.is_hub(&t("domain"), "kestrel.example", "kestrel.example"));
        assert!(h.is_hub(&t("subdomain"), "alt1.aspmx.l.google.com", ""));
        assert!(!h.is_hub(&t("subdomain"), "notgoogle.com", ""));
        assert!(!h.is_hub(&t("subdomain"), "vpn.kestrel.example", ""));
        assert!(h.is_hub(&t("ip"), "104.16.1.1", ""));
        assert!(h.is_hub(&t("ip"), "2606:4700::1111", ""));
        assert!(!h.is_hub(&t("ip"), "203.0.113.24", ""));
        assert!(h.is_hub(&t("asn"), "AS13335", ""));
        assert!(!h.is_hub(&t("asn"), "AS64500", ""));
        assert!(h.is_hub(&t("certificate"), "crtsh:1", "r11"));
        assert!(!h.is_hub(&t("certificate"), "crtsh:2", "*.kestrel.example · R11"));
        assert!(!h.is_hub(&t("username"), "gmail.com", ""));
    }

    #[test]
    fn cidr_edges() {
        let c = Cidr::parse("203.0.113.0/24").unwrap();
        assert!(c.contains("203.0.113.255".parse().unwrap()));
        assert!(!c.contains("203.0.114.0".parse().unwrap()));
        assert!(
            Cidr::parse("0.0.0.0/0")
                .unwrap()
                .contains("1.2.3.4".parse().unwrap())
        );
        assert!(Cidr::parse("1.2.3.4/33").is_none());
        assert!(!c.contains("::1".parse().unwrap()));
    }
}
