//! Test-control environment variables (ARCHITECTURE §8). Each is refused
//! unless `SPEKTR_TEST_MODE=1` is also set.

use std::path::PathBuf;

use anyhow::{Result, bail};
use jiff::Timestamp;

pub const HELP: &str = "\
Testing:
  These variables make runs deterministic for the E2E harness and QA. Each is
  refused unless SPEKTR_TEST_MODE=1 is also set. In test mode nothing is handed
  to the OS: browser, viewer, $PAGER and $EDITOR requests are appended to
  $XDG_STATE_HOME/spektr/opened.jsonl instead.

  SPEKTR_TRANSPORT=replay:<fixture-dir> | record:<dir>
                                  replace live network for every built-in module
  SPEKTR_CLOCK=<RFC 3339>         fixed clock for audit timestamps and job times
  SPEKTR_SEED=<u64>               deterministic UUIDv7 randomness
  SPEKTR_TEST_KEYS=<keys>         keys the TUI types after it starts, e.g.
                                  \"wait:2000 l j j wait:1500\" (real-terminal QA)";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransportSpec {
    Live,
    Replay(PathBuf),
    Record(PathBuf),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TestEnv {
    pub test_mode: bool,
    pub transport: TransportSpec,
    pub clock: Option<Timestamp>,
    pub seed: Option<u64>,
    /// Keys the TUI types after it starts (real-terminal QA).
    pub keys: Option<String>,
}

impl TestEnv {
    pub fn from_env() -> Result<Self> {
        Self::parse(|k| std::env::var(k).ok().filter(|v| !v.is_empty()))
    }

    pub fn parse(get: impl Fn(&str) -> Option<String>) -> Result<Self> {
        let test_mode = get("SPEKTR_TEST_MODE").as_deref() == Some("1");
        let guarded = [
            "SPEKTR_TRANSPORT",
            "SPEKTR_CLOCK",
            "SPEKTR_SEED",
            "SPEKTR_TEST_KEYS",
        ];
        if !test_mode && let Some(v) = guarded.iter().find(|v| get(v).is_some()) {
            bail!("{v} is a testing variable and needs SPEKTR_TEST_MODE=1 (see `spektr --help`)");
        }
        let transport = match get("SPEKTR_TRANSPORT") {
            None => TransportSpec::Live,
            Some(t) => match t.split_once(':') {
                Some(("replay", dir)) if !dir.is_empty() => TransportSpec::Replay(dir.into()),
                Some(("record", dir)) if !dir.is_empty() => TransportSpec::Record(dir.into()),
                _ => bail!("SPEKTR_TRANSPORT must be replay:<dir> or record:<dir>, got `{t}`"),
            },
        };
        let clock = match get("SPEKTR_CLOCK") {
            None => None,
            Some(c) => Some(
                c.parse::<Timestamp>()
                    .map_err(|e| anyhow::anyhow!("SPEKTR_CLOCK `{c}`: {e}"))?,
            ),
        };
        let seed = match get("SPEKTR_SEED") {
            None => None,
            Some(s) => Some(
                s.parse::<u64>()
                    .map_err(|e| anyhow::anyhow!("SPEKTR_SEED `{s}`: {e}"))?,
            ),
        };
        Ok(Self {
            test_mode,
            transport,
            clock,
            seed,
            keys: get("SPEKTR_TEST_KEYS"),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn env(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let m: HashMap<String, String> = pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        move |k| m.get(k).cloned()
    }

    #[test]
    fn guarded_without_test_mode() {
        let e = TestEnv::parse(env(&[("SPEKTR_CLOCK", "2026-10-20T18:14:22Z")])).unwrap_err();
        assert!(e.to_string().contains("SPEKTR_TEST_MODE=1"), "{e}");
        let t = TestEnv::parse(env(&[
            ("SPEKTR_TEST_MODE", "1"),
            ("SPEKTR_TRANSPORT", "replay:/tmp/fx"),
            ("SPEKTR_CLOCK", "2026-10-20T18:14:22Z"),
            ("SPEKTR_SEED", "1"),
        ]))
        .unwrap();
        assert_eq!(t.transport, TransportSpec::Replay("/tmp/fx".into()));
        assert_eq!(t.seed, Some(1));
        assert!(
            TestEnv::parse(env(&[
                ("SPEKTR_TEST_MODE", "1"),
                ("SPEKTR_TRANSPORT", "carrier-pigeon")
            ]))
            .is_err()
        );
        assert_eq!(
            TestEnv::parse(env(&[])).unwrap().transport,
            TransportSpec::Live
        );
    }
}
