//! Built-in modules plus the shared network (`net`) service
//! (docs/MODULES.md §4, §6, §10). Media (EXIF, dHash, mmh3) and web capture
//! arrive in Phase 4.
//!
//! All network-, image- and parsing-heavy code lives here so that editing
//! the engine or the TUI never recompiles it.

pub mod builtins;
pub mod net;

pub use builtins::{BuiltinOptions, builtins, builtins_with};
pub use net::{Fixtures, Net, NetError, Transport, install_crypto_provider};

/// Name of this crate, for diagnostics.
pub const CRATE_NAME: &str = env!("CARGO_PKG_NAME");
