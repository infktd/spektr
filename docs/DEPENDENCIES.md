# spektr — Toolchain and Dependencies

Every version below was checked on crates.io and static.rust-lang.org on **2026-10-20**. The full set was resolved together and clean-built in a stub workspace with Rust 1.99.1 (see `ARCHITECTURE.md` §6).

## Policy

- **Latest at kickoff.** When scaffolding, re-check each crate on crates.io:
  - **newer patch or minor:** take it
  - **new major:** stop, check compatibility (especially across the ratatui stack), and record the decision in the phase's COMPLETE doc
- **`Cargo.lock` is committed.** Workspace manifests use caret requirements at the verified version; the lockfile is what pins builds.
- **No automatic upgrades.**
  - Dependency bumps are deliberate commits of their own, titled `deps: bump <crate> x → y`, with a smoke test noted in the message.
  - No Dependabot auto-merge.
  - Don't run `cargo update` as part of unrelated work.
- **Every dependency goes through `[workspace.dependencies]`.** Member crates use `{ workspace = true }`, so each version lives in exactly one place.
- **Feature discipline:** turn off default features wherever a default pulls in something unused (see the notes below).
- **Licenses:** `cargo deny check` runs in CI, with an allowlist of Apache-2.0, MIT, BSD-2/3, ISC, Zlib, Unicode-3.0, MPL-2.0 and CC0-1.0.

## Toolchain

```toml
# rust-toolchain.toml
[toolchain]
channel = "1.99.1"
components = ["clippy", "rustfmt"]
profile = "minimal"
```

```toml
# Cargo.toml (workspace)
[workspace.package]
edition = "2024"
rust-version = "1.99"
license = "Apache-2.0"
```

Use resolver `"3"`, which is the default for edition 2024. Bump `channel` deliberately, like any other dependency.

## Workspace dependencies

```toml
[workspace.dependencies]
# internal
spektr-model    = { path = "crates/spektr-model",    version = "0.1.0" }
spektr-protocol = { path = "crates/spektr-protocol", version = "0.1.0" }
spektr-store    = { path = "crates/spektr-store",    version = "0.1.0" }
spektr-engine   = { path = "crates/spektr-engine",   version = "0.1.0" }
spektr-modules  = { path = "crates/spektr-modules",  version = "0.1.0" }
spektr-tui      = { path = "crates/spektr-tui",      version = "0.1.0" }

# core / serialization
serde        = { version = "1.0.229", features = ["derive"] }
serde_json   = "1.0.154"          # do NOT enable preserve_order (audit canonical JSON relies on sorted maps)
toml         = "1.1.8"
schemars     = "1.2.3"            # spektr-protocol, behind its `schema` feature
thiserror    = "2.0.21"
anyhow       = "1.0.104"          # bin + xtask only

# model
url          = "2.5.8"
idna         = "1.1.0"
psl          = "2.1.243"          # Public Suffix List; data updates ship as patch releases
phonenumber  = "0.3.10"
jiff         = { version = "0.2.38", features = ["serde"] }
uuid         = { version = "1.28.0", features = ["v7", "serde"] }

# store
rusqlite     = { version = "0.40.2", features = ["bundled"] }
zstd         = "0.14.3"
sha2         = "0.11.1"

# async / engine
tokio        = { version = "1.54.0", features = ["rt-multi-thread", "macros", "sync", "time", "process", "io-util", "fs", "signal"] }
tokio-util   = "0.7.20"           # CancellationToken, codecs
futures      = "0.3.34"           # BoxFuture, StreamExt
governor     = "0.10.4"           # per-host rate limiting

# spektr-modules (net · media · capture · built-ins)
reqwest      = { version = "0.13.5", default-features = false, features = ["rustls-no-provider", "json", "gzip", "brotli", "charset", "http2", "system-proxy"] }
rustls       = { version = "0.23.46", default-features = false, features = ["ring", "std", "tls12", "logging"] }
hickory-proto = "0.26.3"            # DNS wire format only; the stub resolver lives in spektr-modules::net
scraper      = "0.27.0"           # HTML title, links, analytics ids
which        = "8.0.6"            # capture: find chrome/chromium
nom-exif     = "3.8.0"
image        = { version = "0.25.10", default-features = false, features = ["png", "jpeg", "webp", "gif"] }
image_hasher = "3.1.1"            # dHash for avatar_phash

# tui
ratatui      = "0.30.2"
crossterm    = { version = "0.29.0", features = ["event-stream", "osc52"] }
ratatui-image = { version = "11.1.0", default-features = false, features = ["crossterm"] }
ratatui-textarea = "0.9.3"        # default feature `crossterm`; built on ratatui-core / ratatui-widgets, compatible with ratatui 0.30
nucleo-matcher = "0.3.1"          # fuzzy matching (filter, palette, pickers)

# bin
clap         = { version = "4.6.7", features = ["derive"] }
dirs         = "7.0.0"            # home_dir() only; XDG resolution is spektr's own (ARCHITECTURE §5)
open         = "5.4.4"            # system browser / viewer
tracing      = "0.1.44"
tracing-subscriber = { version = "0.3.23", features = ["env-filter"] }
tracing-appender   = "0.2.5"

# dev / test
insta        = { version = "1.49.0", features = ["json"] }
proptest     = "1.12.0"
assert_cmd   = "2.2.2"
tempfile     = "3.27.0"
portable-pty = "0.9.0"            # e2e: run the real binary in a PTY
vt100        = "0.18.1"           # e2e: parse the screen into cells with colors
```

### Notes on choices

| Choice | Why | Instead of |
|---|---|---|
| `rustls` with the **ring** provider | 23 s vs 40 s clean (measured); no aws-lc-sys C/cmake build | reqwest's default aws-lc-rs |
| Chrome **subprocess** for screenshots | `chromiumoxide` alone is 83 s clean (measured) | `chromiumoxide`, `headless_chrome` |
| `rusqlite` (bundled) | Synchronous API fits the store actor; no macros; no database needed at build time | `sqlx` |
| `ratatui-image` without default features | Its default `chafa-dyn` needs system libchafa at runtime | defaults |
| `nom-exif` | Actively maintained (Sept 2026), supports HEIC/HEIF and video metadata | `kamadak-exif` (last release Nov 2024) |
| `ratatui-textarea` | Maintained and targets the ratatui 0.30 split crates | `tui-textarea` (last release Oct 2024) |
| `BoxFuture` from `futures` | Object-safe async trait methods with no proc macro | `async-trait` |
| `hickory-proto` plus spektr's own stub resolver in `spektr-modules::net` | `hickory-resolver` 0.26 depends on `hickory-net`, which requires `async-trait` (banned graph-wide). spektr needs only A/AAAA/MX/PTR/TXT lookups on FQDNs with no search domains or caching, so a small UDP + TCP-fallback resolver is enough. Decided by Jay in Phase 0 | `hickory-resolver` |
| favicon mmh3 implemented in `spektr-modules::media` | It's about 40 lines; the `murmur3` crate's last release was in 2022 | `murmur3` |
| `crossterm` `osc52` | Clipboard copy that works over SSH, with no X11 or Wayland dependencies | `arboard` |

## Tooling (installed in CI, not crate dependencies)

| Tool | Version (2026-10-20) | Use |
|---|---|---|
| `cargo-nextest` | 0.9.148 | Test runner |
| `cargo-deny` | 0.20.2 | Licenses, advisories, duplicate-version report |
| `dist` (cargo-dist) | 0.32.0 | Release binaries, Homebrew tap |
| `vhs` | latest release at kickoff; pin it in CI | `demos/*.tape` → GIF |
| `cargo-insta` | matches `insta` | Snapshot review |
