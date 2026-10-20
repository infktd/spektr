//! Built-in modules plus the shared network (`net`) service
//! (docs/MODULES.md §4, §6, §10), media helpers (EXIF, dHash, mmh3) and web
//! capture.
//!
//! All network-, image- and parsing-heavy code lives here so that editing
//! the engine or the TUI never recompiles it.

pub mod builtins;
pub mod bundled;
pub mod capture;
pub mod media;
pub mod net;

pub use builtins::{BuiltinOptions, builtins, builtins_with};
pub use capture::{Capture, find_browser};
pub use net::{Fixtures, Net, NetError, Transport, install_crypto_provider};

/// Name of this crate, for diagnostics.
pub const CRATE_NAME: &str = env!("CARGO_PKG_NAME");
