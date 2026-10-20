//! Stored entity flags (DATA_MODEL §1.6). `new` is UI-only and not stored.

use std::ops::{BitOr, BitOrAssign};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct EntityFlags(pub u32);

impl EntityFlags {
    pub const NONE: Self = Self(0);
    pub const SEED: Self = Self(1);
    pub const SUBJECT: Self = Self(1 << 1);
    pub const HUB: Self = Self(1 << 2);
    pub const OUT_OF_SCOPE: Self = Self(1 << 3);

    pub fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    pub fn with(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    pub fn without(self, other: Self) -> Self {
        Self(self.0 & !other.0)
    }
}

impl BitOr for EntityFlags {
    type Output = Self;
    fn bitor(self, rhs: Self) -> Self {
        self.with(rhs)
    }
}

impl BitOrAssign for EntityFlags {
    fn bitor_assign(&mut self, rhs: Self) {
        *self = self.with(rhs);
    }
}
