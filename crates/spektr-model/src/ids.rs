//! Typed UUIDv7 identifiers and a generator that can be made deterministic
//! (`SPEKTR_SEED`, ARCHITECTURE §8).

use std::fmt;
use std::str::FromStr;
use std::sync::Mutex;

use jiff::Timestamp;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

macro_rules! id_type {
    ($(#[$doc:meta])* $name:ident) => {
        $(#[$doc])*
        #[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(pub Uuid);

        impl $name {
            /// The 16 raw bytes, as stored in SQLite.
            pub fn as_bytes(&self) -> &[u8; 16] {
                self.0.as_bytes()
            }

            pub fn from_bytes(bytes: [u8; 16]) -> Self {
                Self(Uuid::from_bytes(bytes))
            }

            /// From a 16-byte slice; `None` for any other length.
            pub fn from_slice(bytes: &[u8]) -> Option<Self> {
                Uuid::from_slice(bytes).ok().map(Self)
            }

            /// The first 8 hex digits, for compact display.
            pub fn short(&self) -> String {
                let s = self.0.simple().to_string();
                s[..8].to_string()
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                fmt::Display::fmt(&self.0, f)
            }
        }

        impl fmt::Debug for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{}({})", stringify!($name), self.0)
            }
        }

        impl FromStr for $name {
            type Err = uuid::Error;
            fn from_str(s: &str) -> Result<Self, Self::Err> {
                Uuid::parse_str(s).map(Self)
            }
        }

        impl From<Uuid> for $name {
            fn from(u: Uuid) -> Self {
                Self(u)
            }
        }
    };
}

id_type!(
    /// An entity row.
    EntityId
);
id_type!(
    /// An edge row.
    EdgeId
);
id_type!(
    /// An observation (raw evidence) row.
    ObservationId
);
id_type!(
    /// A module job.
    JobId
);
id_type!(
    /// A note.
    NoteId
);
id_type!(
    /// A review-queue item.
    ReviewItemId
);
id_type!(
    /// A merge record.
    MergeId
);
id_type!(
    /// A scope rule.
    ScopeRuleId
);
id_type!(
    /// A playbook run.
    PlaybookRunId
);
id_type!(
    /// A checklist item.
    ChecklistItemId
);

/// Generates UUIDv7s from a caller-supplied timestamp. Random by default;
/// [`IdGen::seeded`] makes the random bits deterministic for tests and QA.
#[derive(Debug)]
pub struct IdGen {
    seeded: Option<Mutex<SplitMix64>>,
}

impl IdGen {
    /// Random bits from the OS.
    pub fn random() -> Self {
        Self { seeded: None }
    }

    /// Deterministic bits from `seed`.
    pub fn seeded(seed: u64) -> Self {
        Self {
            seeded: Some(Mutex::new(SplitMix64(seed))),
        }
    }

    /// A new UUIDv7 whose timestamp is `now`.
    pub fn next(&self, now: Timestamp) -> Uuid {
        let millis = u64::try_from(now.as_millisecond()).unwrap_or(0);
        match &self.seeded {
            None => {
                let secs = millis / 1000;
                let nanos = u32::try_from((millis % 1000) * 1_000_000).unwrap_or(0);
                Uuid::new_v7(uuid::Timestamp::from_unix(uuid::NoContext, secs, nanos))
            }
            Some(rng) => {
                // A poisoned lock only means another thread panicked mid-draw;
                // the generator state is still a valid u64.
                let mut rng = rng.lock().unwrap_or_else(|e| e.into_inner());
                let mut bytes = [0u8; 10];
                bytes[..8].copy_from_slice(&rng.next_u64().to_le_bytes());
                bytes[8..].copy_from_slice(&rng.next_u64().to_le_bytes()[..2]);
                uuid::Builder::from_unix_timestamp_millis(millis, &bytes).into_uuid()
            }
        }
    }

    pub fn entity(&self, now: Timestamp) -> EntityId {
        EntityId(self.next(now))
    }
}

/// SplitMix64: a tiny, well-distributed deterministic generator.
#[derive(Debug)]
struct SplitMix64(u64);

impl SplitMix64 {
    fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seeded_ids_are_deterministic_v7() {
        let now: Timestamp = "2026-10-20T18:14:22Z".parse().unwrap();
        let a = IdGen::seeded(1);
        let b = IdGen::seeded(1);
        let xs: Vec<_> = (0..5).map(|_| a.next(now)).collect();
        let ys: Vec<_> = (0..5).map(|_| b.next(now)).collect();
        assert_eq!(xs, ys);
        assert!(xs.iter().all(|u| u.get_version_num() == 7));
        let set: std::collections::BTreeSet<_> = xs.iter().collect();
        assert_eq!(set.len(), 5);
    }

    #[test]
    fn random_ids_differ_and_roundtrip() {
        let g = IdGen::random();
        let now = Timestamp::now();
        let a = EntityId(g.next(now));
        let b = EntityId(g.next(now));
        assert_ne!(a, b);
        assert_eq!(EntityId::from_slice(a.as_bytes()), Some(a));
        assert_eq!(a.to_string().parse::<EntityId>().unwrap(), a);
    }
}
