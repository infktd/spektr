//! The clock (fixable with `SPEKTR_CLOCK`, ARCHITECTURE §8) and the canonical
//! timestamp format used in the case file and the audit chain.

use jiff::{Timestamp, Unit};

/// Where "now" comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Clock {
    System,
    /// Every call returns this instant (tests, QA, the tutor's demos).
    Fixed(Timestamp),
}

impl Clock {
    pub fn now(&self) -> Timestamp {
        match self {
            Clock::System => Timestamp::now(),
            Clock::Fixed(ts) => *ts,
        }
    }
}

/// Canonical text form: UTC, millisecond precision, `Z` suffix,
/// e.g. `2026-10-20T18:14:22.000Z`. Sorts lexicographically.
pub fn format_ts(ts: Timestamp) -> String {
    let ts = ts.round(Unit::Millisecond).unwrap_or(ts);
    ts.strftime("%Y-%m-%dT%H:%M:%S%.3fZ").to_string()
}

/// Parse any RFC 3339 timestamp.
pub fn parse_ts(s: &str) -> Result<Timestamp, jiff::Error> {
    s.parse()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_format_roundtrips() {
        let ts: Timestamp = "2026-10-20T18:14:22Z".parse().unwrap();
        let s = format_ts(ts);
        assert_eq!(s, "2026-10-20T18:14:22.000Z");
        assert_eq!(parse_ts(&s).unwrap(), ts);
    }
}
