//! Entity attributes (DATA_MODEL §2): a name maps to a list of values, one
//! per source, each citing the observation it came from.

use std::collections::BTreeMap;

use jiff::Timestamp;
use jiff::civil::Date;
use serde::{Deserialize, Serialize};

use crate::ids::ObservationId;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AttrValue {
    Text(String),
    Int(i64),
    Bool(bool),
    Time(Timestamp),
    Date(Date),
    Geo { lat: f64, lon: f64 },
    Url(String),
}

impl AttrValue {
    /// The instant this value marks on the timeline: a time, or a date at
    /// 00:00 UTC.
    pub fn instant(&self) -> Option<Timestamp> {
        match self {
            AttrValue::Time(t) => Some(*t),
            AttrValue::Date(d) => d
                .to_zoned(jiff::tz::TimeZone::UTC)
                .ok()
                .map(|z| z.timestamp()),
            _ => None,
        }
    }

    /// Plain-text rendering.
    pub fn display(&self) -> String {
        match self {
            AttrValue::Text(s) | AttrValue::Url(s) => s.clone(),
            AttrValue::Int(i) => i.to_string(),
            AttrValue::Bool(b) => b.to_string(),
            AttrValue::Time(t) => t.strftime("%Y-%m-%d %H:%M").to_string(),
            AttrValue::Date(d) => d.to_string(),
            AttrValue::Geo { lat, lon } => format!("{lat:.5}, {lon:.5}"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Attr {
    pub value: AttrValue,
    /// Provenance; analyst-entered attributes point at an analyst observation.
    pub observation: ObservationId,
    /// True: the value appears on the timeline.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub event: bool,
}

/// Name → one value per source. A `BTreeMap` keeps serialization canonical.
pub type Attrs = BTreeMap<String, Vec<Attr>>;

/// Attribute names: `[a-z0-9_]{1,64}`.
pub fn valid_name(name: &str) -> bool {
    (1..=64).contains(&name.len())
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

/// Add `attr` under `name` unless the same value from the same observation is
/// already there. Returns whether anything changed.
pub fn insert(attrs: &mut Attrs, name: &str, attr: Attr) -> bool {
    let list = attrs.entry(name.to_string()).or_default();
    if list
        .iter()
        .any(|a| a.value == attr.value && a.observation == attr.observation)
    {
        return false;
    }
    list.push(attr);
    true
}

/// Standard event attribute names (they feed the timeline tab).
pub const EVENT_NAMES: &[&str] = &[
    names::CREATED_AT,
    names::LAST_ACTIVE,
    names::POSTED_AT,
    names::TAKEN_AT,
    names::ARCHIVED_FIRST,
    names::ARCHIVED_LAST,
    names::REGISTERED_AT,
    names::EXPIRES_AT,
];

/// Well-known attribute names.
pub mod names {
    pub const CREATED_AT: &str = "created_at";
    pub const LAST_ACTIVE: &str = "last_active";
    pub const POSTED_AT: &str = "posted_at";
    pub const TAKEN_AT: &str = "taken_at";
    pub const ARCHIVED_FIRST: &str = "archived_first";
    pub const ARCHIVED_LAST: &str = "archived_last";
    pub const REGISTERED_AT: &str = "registered_at";
    pub const EXPIRES_AT: &str = "expires_at";

    pub const DISPLAY_NAME: &str = "display_name";
    pub const BIO: &str = "bio";
    pub const AVATAR_URL: &str = "avatar_url";
    pub const AVATAR_PHASH: &str = "avatar_phash";
    pub const PROFILE_URL: &str = "profile_url";
    pub const STATED_LOCATION: &str = "stated_location";
    pub const TITLE: &str = "title";
    pub const FAVICON_MMH3: &str = "favicon_mmh3";
    pub const ANALYTICS_IDS: &str = "analytics_ids";
    pub const LINE_TYPE: &str = "line_type";
    pub const COUNTRY: &str = "country";
    pub const DEVICE: &str = "device";
    pub const SOFTWARE: &str = "software";
    pub const SERVER: &str = "server";
    pub const KARMA: &str = "karma";
    pub const DID: &str = "did";
    pub const WEBSITE: &str = "website";
    pub const REGION: &str = "region";
    pub const VALID: &str = "valid";
    pub const MX_PRESENT: &str = "mx_present";
    pub const SYNTAX_VALID: &str = "syntax_valid";
    pub const PROVIDER_CLASS: &str = "provider_class";
    pub const NETWORK_NAME: &str = "network_name";
    pub const DATA_CLASSES: &str = "data_classes";
    pub const BREACH_DATE: &str = "breach_date";
    pub const ISSUER: &str = "issuer";
    pub const COMMON_NAME: &str = "common_name";
    pub const NOT_BEFORE: &str = "not_before";
    pub const NOT_AFTER: &str = "not_after";
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[test]
    fn json_shape_matches_the_exec_protocol() {
        let v: AttrValue = serde_json::from_str(r#"{"text":"Jordan M."}"#).unwrap();
        assert_eq!(v, AttrValue::Text("Jordan M.".into()));
        let v: AttrValue = serde_json::from_str(r#"{"time":"2026-10-16T09:12:00Z"}"#).unwrap();
        assert_eq!(v.instant().unwrap().to_string(), "2026-10-16T09:12:00Z");
        let v: AttrValue = serde_json::from_str(r#"{"geo":{"lat":45.52,"lon":-122.68}}"#).unwrap();
        assert_eq!(v.display(), "45.52000, -122.68000");
        let v: AttrValue = serde_json::from_str(r#"{"date":"2019-03-01"}"#).unwrap();
        assert_eq!(v.instant().unwrap().to_string(), "2019-03-01T00:00:00Z");
    }

    #[test]
    fn insert_dedupes_same_value_and_source() {
        let mut attrs = Attrs::new();
        let o = ObservationId(Uuid::nil());
        let a = Attr {
            value: AttrValue::Text("x".into()),
            observation: o,
            event: false,
        };
        assert!(insert(&mut attrs, "bio", a.clone()));
        assert!(!insert(&mut attrs, "bio", a));
        assert!(valid_name("last_active"));
        assert!(!valid_name("Last-Active"));
    }
}
