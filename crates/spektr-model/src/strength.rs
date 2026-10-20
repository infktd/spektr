//! Edge strength (DATA_MODEL §4): computed from evidence, never asserted by
//! modules.

use std::fmt;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Strength {
    /// One distinct source key.
    Single,
    /// Two or more distinct source keys.
    Corroborated,
    /// The analyst confirmed the edge, regardless of count.
    Confirmed,
}

impl Strength {
    /// `None` only for an edge with no evidence, which ingestion never writes.
    pub fn compute(distinct_source_keys: usize, confirmed: bool) -> Option<Self> {
        if confirmed {
            Some(Strength::Confirmed)
        } else {
            match distinct_source_keys {
                0 => None,
                1 => Some(Strength::Single),
                _ => Some(Strength::Corroborated),
            }
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Strength::Single => "single",
            Strength::Corroborated => "corroborated",
            Strength::Confirmed => "confirmed",
        }
    }

    /// `▰▱▱` / `▰▰▱` / `▰▰▰`.
    pub fn bar(self) -> &'static str {
        match self {
            Strength::Single => "▰▱▱",
            Strength::Corroborated => "▰▰▱",
            Strength::Confirmed => "▰▰▰",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "single" => Some(Strength::Single),
            "corroborated" => Some(Strength::Corroborated),
            "confirmed" => Some(Strength::Confirmed),
            _ => None,
        }
    }
}

impl fmt::Display for Strength {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Source key for an observation with a source URL: the registrable domain
/// of its host (`api.github.com` → `github.com`), or the IP itself.
pub fn source_key_for_url(url: &str) -> Option<String> {
    let u = url::Url::parse(url).ok()?;
    match u.host()? {
        url::Host::Domain(d) => {
            let d = d.trim_end_matches('.');
            Some(psl::domain_str(d).unwrap_or(d).to_string())
        }
        url::Host::Ipv4(ip) => Some(ip.to_string()),
        url::Host::Ipv6(ip) => Some(ip.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn levels() {
        assert_eq!(Strength::compute(0, false), None);
        assert_eq!(Strength::compute(1, false), Some(Strength::Single));
        assert_eq!(Strength::compute(2, false), Some(Strength::Corroborated));
        assert_eq!(Strength::compute(0, true), Some(Strength::Confirmed));
        assert!(Strength::Confirmed > Strength::Corroborated);
        assert!(Strength::Corroborated > Strength::Single);
    }

    #[test]
    fn source_keys() {
        assert_eq!(
            source_key_for_url("https://api.github.com/users/x").as_deref(),
            Some("github.com")
        );
        assert_eq!(
            source_key_for_url("https://jmarlow.github.io/").as_deref(),
            Some("jmarlow.github.io")
        );
        assert_eq!(
            source_key_for_url("http://203.0.113.24/").as_deref(),
            Some("203.0.113.24")
        );
        assert_eq!(source_key_for_url("not a url"), None);
    }
}
