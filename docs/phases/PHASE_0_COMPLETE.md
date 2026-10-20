# Phase 0 complete: scaffold and build baseline

Branch `phase-0-scaffold` · PR [#1](https://github.com/infktd/spektr/pull/1) · 2026-10-20

## What shipped

- **Workspace** exactly as in ARCHITECTURE §2: `spektr-model`, `spektr-protocol`, `spektr-store`, `spektr-engine`, `spektr-modules`, `spektr-tui` and `spektr` (bin) under `crates/`, plus `xtask/` and `demos/`.
  - Each crate declares the third-party dependencies that ARCHITECTURE §2 assigns to it, so the build graph and the timing baseline are realistic from the start. The code is stubs: each library exposes one constant, and the binary parses `--version`/`--help` with clap.
  - `[workspace.dependencies]` holds every version (re-verified below), and every member uses `{ workspace = true }`.
  - `Cargo.lock` is committed.
- **Toolchain:** `rust-toolchain.toml` pins **1.99.1**, which is still the latest stable at kickoff (see below), with clippy and rustfmt on the minimal profile.
- **Profiles** copied verbatim from ARCHITECTURE §6 (`dev`, the per-package `dev` overrides, `release`, `dist`).
- **`cargo xtask deps-check`** (`xtask/src/deps_check.rs`) checks `cargo metadata` with default features and again with `--all-features`, filtered to the four shipping targets:
  - **rule 0:** exactly the seven crates plus `xtask`
  - **rule 1:** `spektr-engine` doesn't depend on `spektr-modules` or `spektr-tui`
  - **rule 2:** `spektr-tui` doesn't depend on `spektr-store` or `spektr-modules`
  - **rule 3:** `spektr-model` and `spektr-protocol` have no async runtime, I/O crate or heavier spektr crate as a direct dependency, and nothing in their dependency closure compiles C
  - **rule 4:** no `sqlx*`, `async-trait`, `chromiumoxide*`, `headless_chrome` or `aws-lc-*` anywhere in the graph, and no default-features `reqwest`
  - **rule 5:** no `serde_json/preserve_order`
  - **rule 6:** every member dependency is `{ workspace = true }`

  It has 13 unit tests, one or more per rule, including negative cases.
- **cargo-deny** (`deny.toml`):
  - the license allowlist from DEPENDENCIES.md
  - advisories, with yanked crates denied
  - the same banned crates as deps-check rule 4
  - duplicate versions reported as warnings, not failures
  - crates.io as the only source
- **CI** (`.github/workflows/ci.yml`):
  - `lint` job, once, on `ubuntu-22.04`: fmt, clippy, deps-check, `cargo deny check`
  - `test` matrix on all four targets, each on a native runner: build with `--timings`, then `cargo nextest run`
  - uploads the `--timings` report from `aarch64-apple-darwin`
- **Docs:** `LICENSE` (Apache-2.0, full text from apache.org), `README.md` (one paragraph plus a status note), `CONTRIBUTING.md` (setup, the macOS Developer Tools steps from ARCHITECTURE §6, everyday commands, rules), `CHANGELOG.md` (Keep a Changelog), `demos/README.md`, and `docs/phases/BUILD_TIMINGS.md`.

## Exit criteria

| Criterion | Status | Evidence |
|---|---|---|
| Everything builds; gates pass on all four CI targets | ✓ | [CI run](https://github.com/infktd/spektr/actions/runs/39380801561), output below |
| Runner labels listed | ✓ | Table below |
| `BUILD_TIMINGS.md`: clean build, four one-line-edit rebuilds, binary link time, on Jay's M1 Pro | ✓ | [`BUILD_TIMINGS.md`](BUILD_TIMINGS.md), measured on the M1 Pro itself |
| `cargo xtask deps-check` passes in CI from Phase 0 on | ✓ | `lint` job; local output below |

### CI runners

The labels were checked against `actions/runner-images` at kickoff:

| Target | Runner label | Notes |
|---|---|---|
| `aarch64-apple-darwin` | `macos-latest` | macOS 26, arm64 |
| `x86_64-apple-darwin` | `macos-26-intel` | Intel macOS is still offered as a standard runner, so this target runs natively; no cross-compile or Rosetta needed |
| `x86_64-unknown-linux-gnu` | `ubuntu-22.04` | Also runs the `lint` job |
| `aarch64-unknown-linux-gnu` | `ubuntu-22.04-arm` | |

Every `test` job also checks that `rustc --print host-tuple` equals its matrix target, so a runner silently changing architecture fails the job.

Pinned actions: `actions/checkout@v7.0.1`, `Swatinem/rust-cache@v2.9.2`, `taiki-e/install-action@v2.87.29` (installing `cargo-nextest@0.9.148` and `cargo-deny@0.20.2`), `actions/upload-artifact@v7.0.2`.

## Kickoff version check

Rust's stable channel manifest (`static.rust-lang.org/dist/channel-rust-stable.toml`, dated 2026-10-15) shows `rust` at `1.99.1 (58cf8f9c3 2026-10-13)`, so the pinned 1.99.1 is current.

Each crate and tool in DEPENDENCIES.md was compared with the crates.io index, ignoring yanked and pre-release versions. Every one is already at its latest version, so nothing was bumped:

```
serde                spec 1.0.229    latest 1.0.229
serde_json           spec 1.0.154    latest 1.0.154
toml                 spec 1.1.8      latest 1.1.8+spec-1.1.0
schemars             spec 1.2.3      latest 1.2.3
thiserror            spec 2.0.21     latest 2.0.21
anyhow               spec 1.0.104    latest 1.0.104
url                  spec 2.5.8      latest 2.5.8
idna                 spec 1.1.0      latest 1.1.0
psl                  spec 2.1.243    latest 2.1.243
phonenumber          spec 0.3.10     latest 0.3.10+9.0.33
jiff                 spec 0.2.38     latest 0.2.38
uuid                 spec 1.28.0     latest 1.28.0
rusqlite             spec 0.40.2     latest 0.40.2
zstd                 spec 0.14.3     latest 0.14.3
sha2                 spec 0.11.1     latest 0.11.1
tokio                spec 1.54.0     latest 1.54.0
tokio-util           spec 0.7.20     latest 0.7.20
futures              spec 0.3.34     latest 0.3.34
governor             spec 0.10.4     latest 0.10.4
reqwest              spec 0.13.5     latest 0.13.5
rustls               spec 0.23.46    latest 0.23.46
hickory-resolver     spec 0.26.3     latest 0.26.3      (replaced, see Deviations)
scraper              spec 0.27.0     latest 0.27.0
which                spec 8.0.6      latest 8.0.6
nom-exif             spec 3.8.0      latest 3.8.0
image                spec 0.25.10    latest 0.25.10
image_hasher         spec 3.1.1      latest 3.1.1
ratatui              spec 0.30.2     latest 0.30.2
crossterm            spec 0.29.0     latest 0.29.0
ratatui-image        spec 11.1.0     latest 11.1.0
ratatui-textarea     spec 0.9.3      latest 0.9.3
nucleo-matcher       spec 0.3.1      latest 0.3.1
clap                 spec 4.6.7      latest 4.6.7
dirs                 spec 7.0.0      latest 7.0.0
open                 spec 5.4.4      latest 5.4.4
tracing              spec 0.1.44     latest 0.1.44
tracing-subscriber   spec 0.3.23     latest 0.3.23
tracing-appender     spec 0.2.5      latest 0.2.5
insta                spec 1.49.0     latest 1.49.0
proptest             spec 1.12.0     latest 1.12.0
assert_cmd           spec 2.2.2      latest 2.2.2
tempfile             spec 3.27.0     latest 3.27.0
portable-pty         spec 0.9.0      latest 0.9.0
vt100                spec 0.18.1     latest 0.18.1
cargo-nextest        spec 0.9.148    latest 0.9.148
cargo-deny           spec 0.20.2     latest 0.20.2
cargo-dist           spec 0.32.0     latest 0.32.0
cargo-insta          spec 1.49.0     latest 1.49.0
```

The `toml` and `phonenumber` index entries carry build metadata (`+spec-1.1.0`, `+9.0.33`), so they are the same release as the spec, not newer. The output above is from two script runs: the first run's version-sorting bug made `toml` look like `0.9.8`, and I checked the index entries by hand to confirm 1.1.8 is the newest. `hickory-proto` 0.26.3, the replacement, is also its latest release.

## Deviations from the spec, and why

1. **`hickory-resolver` → `hickory-proto`, decided by Jay.** `hickory-resolver` 0.26.3 depends on `hickory-net` 0.26.3, which has a **non-optional** dependency on `async-trait` (`#[async_trait]` in `src/runtime.rs` and `src/udp/udp_stream.rs`). That broke ARCHITECTURE §2 rule 4 ("no `async-trait`", whole graph): deps-check failed on the spec's own dependency set. I gave Jay three options. He chose to depend on `hickory-proto` 0.26.3 only (DNS wire format, default `std` feature, with no `async-trait`, runtime or TLS) and to write spektr's own small stub resolver in `spektr-modules::net` in Phase 2.
   - **Spec updated in the same PR:**
     - DEPENDENCIES.md: the dependency line, plus a row in "Notes on choices"
     - ARCHITECTURE §2: the heavy-dependency table
     - MODULES §6: the `dns` row, which now names `/etc/resolv.conf` or a config key `[dns] nameservers` as the nameserver source. That config key is a new, small addition; it's implemented with the resolver in Phase 2.
   - **Effect:** `Cargo.lock` went from 569 to 558 package entries (`grep -c '^name = ' Cargo.lock`). The crates dropped include `hickory-net`, `hickory-resolver`, `async-trait`, `moka`, `resolv-conf`, `tagptr` and `widestring`. `async-trait` is now also denied in `deny.toml`.
2. **deps-check is a little stricter than the minimum ARCHITECTURE §2 asks for.** These are self-triage calls:
   - **Rules 1–2** also count dev and build dependencies. A dev-dependency from the engine on the modules would still couple the crates.
   - **Rule 3's "no C dependencies"** is checked across the whole dependency closure of `spektr-model` and `spektr-protocol`, not just their direct dependencies, because C reached through a direct dependency is still a C dependency. "Compiles C" means a build dependency on `cc`, `cmake`, `pkg-config` or `bindgen`. Cargo's `links` key isn't used as the signal: `wasm-bindgen-shared` and `defmt` set it with no C involved, and an early version of the check flagged both.
   - **Rules 0 and 6** (exact workspace shape, `workspace = true` everywhere) and the `aws-lc-*` ban are additions. They enforce DISPATCH rule 3, the DEPENDENCIES.md policy, and the "default-features `reqwest`" ban.
   - **Both metadata runs are filtered to the four shipping targets,** so wasm- or Windows-only upstream dependencies don't count.
3. **`clippy::unwrap_used` and `clippy::expect_used` are on workspace-wide** (`[workspace.lints]`), and `clippy.toml` allows them in tests. This enforces DISPATCH rule 5 mechanically. A truly unreachable `expect` will need a local `#[allow]` with a comment.
4. **`default-members = ["crates/spektr"]`**, so plain `cargo build` / `cargo run` target the product binary. The binary depends on all six library crates, so the clean-build number covers everything except `xtask`. CI and the gates always pass `--workspace`.
5. **The CI timings artifact is uploaded on every run,** not only on `main` as ARCHITECTURE §6 says. It's a superset: the report is available on PRs too.
6. **`xtask` implements only `deps-check`.** `schemas`, `record`, `demos`, `qa` and `qa-shots` belong to Phases 2–5. An unknown task exits with code 2 and prints usage.
7. **The spec's link-time source isn't available as described.** `cargo build --timings` on 1.99.1 reports no link section for binary units. `BUILD_TIMINGS.md` gives the binary unit's total from `--timings` (0.12–0.13 s) **and** a direct measurement of the linker command (0.06 s).

## Known issues

- **macOS Developer Tools registration (CONTRIBUTING.md) on this machine is unverified.** `spctl developer-mode` has no status query. If it's off, the clean-build number may include some Gatekeeper scan time. Jay can check *System Settings → Privacy & Security → Developer Tools*.
- **Timings were taken during a normal desktop session,** not on an idle machine: load average 7.8 before the first build, with Spotlight indexing `target/`. Three runs each agree within 0.4 s.
- **cargo-deny warnings (not failures):**
  - 17 duplicate-version reports, all transitive: `hashbrown`, `rand`, `rand_core`, `getrandom` (3 versions each); `syn`, `thiserror`, `thiserror-impl`, `strum`, `strum_macros`, `nom`, `base64`, `rustix`, `regex-syntax`, `miniz_oxide`, `linux-raw-sys`, `core-foundation`, `rand_chacha` (2 each). None come from spektr's own requirements.
  - `nom-exif` declares only `license-file`. cargo-deny reads that file as MIT, which is allowed.
  - The `CC0-1.0` allowance currently matches no crate.
- **The stub binary prints a placeholder line,** and `--version`/`--help` are its only real behavior. That's expected for Phase 0.

## Command output

### Local gates (M1 Pro)

```
$ cargo fmt --check && echo "fmt: ok"
fmt: ok
$ cargo clippy --workspace --all-targets --locked -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 8.25s
$ cargo nextest run --workspace --locked
     Summary [   0.251s] 14 tests run: 14 passed, 0 skipped
$ cargo xtask deps-check
deps-check: ok (8 members, 486 resolved packages; default and all features)
$ cargo deny check
advisories ok, bans ok, licenses ok, sources ok
```

The 14 tests are deps-check's 13 rule tests plus `spektr::cli_smoke version_flag_prints_package_version`.

### deps-check catching the original spec conflict (before the hickory swap)

```
deps-check: rule 4: `async-trait` is in the dependency graph: banned: use futures::BoxFuture (DEPENDENCIES.md)
error: 1 dependency rule violation(s)
```

### deps-check against an injected violation in the real workspace

`spektr-store.workspace = true` and `serde = "1"` were temporarily added to `crates/spektr-tui/Cargo.toml`, then reverted. The revert left `git status` clean apart from these docs.

```
$ cargo xtask deps-check
deps-check: rule 2: `spektr-tui` must not depend on `spektr-store`
deps-check: rule 6: `spektr-tui` [dependencies] `serde` must use `{ workspace = true }` (declare the version in [workspace.dependencies])
error: 2 dependency rule violation(s)
exit=1
```

### CI

This is [run 39380801561](https://github.com/infktd/spektr/actions/runs/39380801561), on commit `254590a`, the last code commit. All five jobs passed:

```
lint (x86_64-unknown-linux-gnu): success (2026-10-20T06:30:56Z → 2026-10-20T06:32:41Z)
test (aarch64-apple-darwin): success (2026-10-20T06:31:03Z → 2026-10-20T06:33:58Z)
test (aarch64-unknown-linux-gnu): success (2026-10-20T06:30:59Z → 2026-10-20T06:32:28Z)
test (x86_64-apple-darwin): success (2026-10-20T06:30:56Z → 2026-10-20T06:36:32Z)
test (x86_64-unknown-linux-gnu): success (2026-10-20T06:30:56Z → 2026-10-20T06:33:03Z)
```

These lines were taken from the job logs, with ANSI color codes stripped:

```
lint (x86_64-unknown-linux-gnu) | rustc 1.99.1 (58cf8f9c3 2026-10-13)
lint (x86_64-unknown-linux-gnu) |     Finished `dev` profile [unoptimized + debuginfo] target(s) in 1m 13s
lint (x86_64-unknown-linux-gnu) | deps-check: ok (8 members, 486 resolved packages; default and all features)
lint (x86_64-unknown-linux-gnu) | advisories ok, bans ok, licenses ok, sources ok
test (aarch64-apple-darwin) | host: aarch64-apple-darwin
test (aarch64-apple-darwin) |     Finished `dev` profile [unoptimized + debuginfo] target(s) in 2m 03s
test (aarch64-apple-darwin) |      Summary [   0.115s] 14 tests run: 14 passed, 0 skipped
test (aarch64-unknown-linux-gnu) | host: aarch64-unknown-linux-gnu
test (aarch64-unknown-linux-gnu) |     Finished `dev` profile [unoptimized + debuginfo] target(s) in 1m 07s
test (aarch64-unknown-linux-gnu) |      Summary [   0.022s] 14 tests run: 14 passed, 0 skipped
test (x86_64-apple-darwin) | host: x86_64-apple-darwin
test (x86_64-apple-darwin) |     Finished `dev` profile [unoptimized + debuginfo] target(s) in 4m 21s
test (x86_64-apple-darwin) |      Summary [   0.165s] 14 tests run: 14 passed, 0 skipped
test (x86_64-unknown-linux-gnu) | host: x86_64-unknown-linux-gnu
test (x86_64-unknown-linux-gnu) |     Finished `dev` profile [unoptimized + debuginfo] target(s) in 1m 37s
test (x86_64-unknown-linux-gnu) |      Summary [   0.032s] 14 tests run: 14 passed, 0 skipped
```

The `fmt` and `clippy` steps print nothing on success; they show as passed steps in the `lint` job. The `cargo-timings-aarch64-apple-darwin` artifact (102,208 bytes) was uploaded. The `Finished` times above are cold-cache builds on GitHub-hosted runners and aren't comparable with the M1 Pro numbers.

### Timings

Summary only; the full output is in [`BUILD_TIMINGS.md`](BUILD_TIMINGS.md):

| Scenario (M1 Pro, Rust 1.99.1) | Result |
|---|---|
| Clean `cargo build` | 25.37 s / 25.52 s / 25.74 s |
| One-line edit in `spektr-tui` | 0.33 s ×3 (recompiles tui + bin) |
| One-line edit in `spektr-modules` | 0.34 / 0.33 / 0.34 s (modules + bin) |
| One-line edit in `spektr-engine` | 0.37 / 0.37 / 0.38 s (engine, modules, tui, bin) |
| One-line edit in `spektr-model` | 0.43 / 0.43 / 0.42 s (all spektr crates that depend on it) |
| Link time of the binary | 0.12–0.13 s binary unit total in `--timings`; 0.06 s linker alone |

## How Jay can test it

1. `git fetch && git checkout phase-0-scaffold`. Then `cargo build`: rustup installs 1.99.1 if needed, and the first build takes about 25 s.
2. Run the gates: `cargo fmt --check && cargo clippy --workspace --all-targets -- -D warnings && cargo nextest run --workspace && cargo xtask deps-check`. Expect `14 tests run: 14 passed` and `deps-check: ok`.
3. Break a rule and watch deps-check fail: add `spektr-store.workspace = true` under `[dependencies]` in `crates/spektr-tui/Cargo.toml`, run `cargo xtask deps-check` (it exits 1 with "rule 2"), then `git checkout crates/spektr-tui/Cargo.toml`.
4. Re-measure a rebuild: append a line to `crates/spektr-tui/src/lib.rs`, run `cargo build --timings`, and open `target/cargo-timings/cargo-timing.html`. Only `spektr-tui` and `spektr` should rebuild. Revert the edit afterwards.
5. Look at [PR #1](https://github.com/infktd/spektr/pull/1): five green checks (lint plus four targets), and the `cargo-timings-aarch64-apple-darwin` artifact on the run.
