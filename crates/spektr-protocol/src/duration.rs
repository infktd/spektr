//! `"60s"`, `"24h"`, `"1/s"` — the small string formats manifests use.

use std::time::Duration;

/// `<n><unit>` with unit `ms`, `s`, `m`, `h` or `d`.
pub fn parse_duration(s: &str) -> Option<Duration> {
    let s = s.trim();
    let split = s.find(|c: char| !c.is_ascii_digit())?;
    let (n, unit) = s.split_at(split);
    let n: u64 = n.parse().ok()?;
    let secs = match unit {
        "ms" => return Some(Duration::from_millis(n)),
        "s" => n,
        "m" => n.checked_mul(60)?,
        "h" => n.checked_mul(3600)?,
        "d" => n.checked_mul(86_400)?,
        _ => return None,
    };
    Some(Duration::from_secs(secs))
}

/// `"N/s"` or `"N/min"` → (count, per period).
pub fn parse_rate(s: &str) -> Option<(u32, Duration)> {
    let (n, per) = s.trim().split_once('/')?;
    let n: u32 = n.trim().parse().ok().filter(|n| *n > 0)?;
    let per = match per.trim() {
        "s" | "sec" => Duration::from_secs(1),
        "min" | "m" => Duration::from_secs(60),
        "h" => Duration::from_secs(3600),
        _ => return None,
    };
    Some((n, per))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durations_and_rates() {
        assert_eq!(parse_duration("60s"), Some(Duration::from_secs(60)));
        assert_eq!(parse_duration("24h"), Some(Duration::from_secs(86_400)));
        assert_eq!(parse_duration("250ms"), Some(Duration::from_millis(250)));
        assert_eq!(parse_duration("1d"), Some(Duration::from_secs(86_400)));
        assert_eq!(parse_duration("60"), None);
        assert_eq!(parse_duration("s"), None);
        assert_eq!(parse_rate("1/s"), Some((1, Duration::from_secs(1))));
        assert_eq!(parse_rate("30/min"), Some((30, Duration::from_secs(60))));
        assert_eq!(parse_rate("0/s"), None);
        assert_eq!(parse_rate("fast"), None);
    }
}
