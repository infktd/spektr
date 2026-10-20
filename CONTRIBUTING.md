# Contributing to spektr

Thanks for your interest. spektr is specified up front: read [`docs/PRD.md`](docs/PRD.md) first, then the doc for the area you're touching (`docs/ARCHITECTURE.md`, `docs/DATA_MODEL.md`, `docs/MODULES.md`, `docs/PLAYBOOKS.md`, `docs/TUI_SPEC.md`, `docs/QA.md`).

## Setup

1. Install Rust with [rustup](https://rustup.rs). The pinned toolchain in `rust-toolchain.toml` (Rust 1.99.1, with clippy and rustfmt) is installed automatically the first time you run `cargo` in the repo, or explicitly with `rustup toolchain install`.
2. Install the test runner and the license checker:

   ```sh
   cargo install cargo-nextest --locked   # or: curl -LsSf https://get.nexte.st/latest/mac | tar zxf - -C ~/.cargo/bin
   cargo install cargo-deny --locked
   ```

No system libraries are needed: SQLite, zstd and ring are compiled from bundled C sources with the platform C compiler (Xcode Command Line Tools on macOS, `build-essential` on Debian/Ubuntu).

### macOS: register your terminal as a developer tool

macOS's Gatekeeper/XProtect scans every newly built executable on its first run, and a Rust build runs many of them (every build script, every test binary). That noticeably slows builds and test runs. Fix it once:

1. Run:

   ```sh
   sudo spctl developer-mode enable-terminal
   ```

2. Open *System Settings → Privacy & Security → Developer Tools* and turn on your terminal app (Terminal, iTerm2, Ghostty, WezTerm, …). Add it with **+** if it isn't listed. If you build from an editor's integrated terminal, add the editor too.
3. Restart the terminal app.

Don't add a custom linker on macOS: Apple's default linker (ld-prime, shipped with the Xcode Command Line Tools) is fast. See ARCHITECTURE §6.

## Everyday commands

```sh
cargo build                                          # dev build
cargo nextest run --workspace                        # tests (no network)
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --check
cargo xtask deps-check                               # crate-boundary + banned-dependency rules
cargo deny check                                     # licenses, advisories, banned crates
```

Every commit must pass `cargo fmt --check`, clippy with `-D warnings`, `cargo nextest run --workspace` and `cargo xtask deps-check`. CI runs these on all four targets (macOS and Linux, arm64 and x86_64).

## Rules

- **Crate boundaries are fixed** (ARCHITECTURE §2): seven crates plus `xtask`. Use Rust modules, not new crates, for internal structure. `spektr-engine` never depends on `spektr-modules` or `spektr-tui`; `spektr-tui` never depends on `spektr-store` or `spektr-modules`; `spektr-model` and `spektr-protocol` stay free of async runtimes, I/O and C code.
- **Every dependency is declared once**, in the root `[workspace.dependencies]`, and members use `{ workspace = true }`. `Cargo.lock` is committed.
- **Dependency bumps are their own commits**, titled `deps: bump <crate> x → y`, with a smoke test noted in the message. Don't run `cargo update` as part of other work.
- **Banned dependencies:** `sqlx`, `async-trait`, `chromiumoxide`, `headless_chrome`, and a default-features build of `reqwest` (it pulls aws-lc).
- **No `unwrap()` or `expect()`** outside tests (clippy enforces this), except for truly unreachable states, with a comment saying why.
- **No network in tests.** Use the record/replay fixtures in `crates/spektr-modules/fixtures/`.
- **Fictional data only** in fixtures, snapshots, the tutor and demos: `.example` domains, documentation IP ranges (192.0.2.0/24, 198.51.100.0/24, 203.0.113.0/24), documentation ASNs, invented people.
- **Colors live only in `spektr-tui/src/theme.rs`; key bindings only in `spektr-tui/src/keymap.rs`.**
- **Commits** are small and conventional: `feat(engine): …`, `fix(tui): …`, `deps: bump …`.

## License

By contributing, you agree that your contributions are licensed under the [Apache License 2.0](LICENSE).
