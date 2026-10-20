# spektr — Architecture

## 1. Shape

spektr is a single binary with no server. One process runs three things:

1. **The TUI** (or the headless CLI).
2. **The engine:** scheduler, module host, ingestion, correlation.
3. **The store:** one SQLite case file.

```
 ┌──────────────┐  Action / Query    ┌───────────────────────────┐  StoreCmd   ┌──────────────┐
 │  spektr-tui  │ ─────────────────▶ │       spektr-engine        │ ──────────▶ │ spektr-store │
 │  (or CLI)    │ ◀───────────────── │ scheduler · ingest · corr. │ ◀────────── │ SQLite (WAL) │
 └──────────────┘  EngineEvent (bc)  └─────┬──────────────┬──────┘             └──────────────┘
                                           │ ModuleMessage│
                    ┌──────────────────────┴───┐      ┌───┴──────────────────┐
                    │ spektr-modules            │      │ exec modules          │
                    │ built-ins · net · media · │      │ subprocess, JSON-lines│
                    │ capture                   │      └───────────────────────┘
                    └───────────────────────────┘
```

## 2. Workspace: 7 crates

```
spektr/
  Cargo.toml            # workspace, [workspace.dependencies], profiles
  rust-toolchain.toml
  crates/
    spektr-model/       # types, normalization, vocabulary — pure, no I/O
    spektr-protocol/    # Manifest + ModuleMessage + exec protocol — what third-party modules depend on
    spektr-store/       # SQLite schema, queries, audit chain
    spektr-engine/      # scheduler, module host, ingestion, correlation, EngineHandle
    spektr-modules/     # built-in modules + net (HTTP/DNS) + media (EXIF, hashing) + web capture
    spektr-tui/         # the terminal UI, incl. tutor lessons
    spektr/             # the binary: CLI, config, wiring
  xtask/                # dev tasks (schemas, fixture recording, demos, deps-check) — not published
  demos/                # VHS .tape files
  docs/
```

**Why this split.** Third-party crates compile once and stay cached, however you split your own code. The split decides only how much of *spektr's* code recompiles after an edit. A crate boundary exists only where it pays for itself:

| Crate | Boundary exists because | Heavy third-party deps it holds |
|---|---|---|
| `spektr-model` | Everything depends on it, so it stays small and pure | `url`, `idna`, `psl`, `phonenumber`, `jiff`, `uuid`, `toml` (embedded `hubs.toml`) |
| `spektr-protocol` | Third-party Rust modules depend on this alone | `serde_json`, `schemars` (behind the `schema` feature) |
| `spektr-store` | Keeps the bundled-SQLite C build and the persistence logic separate from the engine | `rusqlite` (bundled), `zstd`, `sha2` |
| `spektr-engine` | Core logic that changes often. It must not pull in HTTP, image or UI stacks | `tokio`, `tokio-util`, `futures`, `toml` (rules profiles, exec `module.toml`) |
| `spektr-modules` | All network-, image- and parsing-heavy code lives here: built-ins, `net` (shared `Http`/`Dns` with rate limiting and record/replay), `media` (EXIF, dHash, mmh3), `capture` (web capture, Chrome subprocess) | `reqwest` (rustls + ring), `hickory-proto`, `governor`, `scraper`, `nom-exif`, `image`, `image_hasher`, `which` |
| `spektr-tui` | The other fast-changing area. Must not pull in network or storage stacks | `ratatui`, `crossterm`, `ratatui-image`, `ratatui-textarea`, `nucleo-matcher`, `image`, `open` (the `o`/`O` keys) |
| `spektr` (bin) | Wiring: constructs the store, engine and modules, registers modules and capture into the engine, runs the TUI or headless commands | `clap`, `toml`, `dirs`, `tracing-subscriber`, `tracing-appender`, `anyhow`, `open` |

Inside `spektr-modules`, use **modules (`mod net; mod media; mod capture; mod builtins;`), not crates.** Promote one to its own crate only if `cargo build --timings` shows it materially slowing incremental builds.

**Dependency rules.** CI enforces these with `cargo xtask deps-check`, which inspects `cargo metadata`. Rules 1–3 apply to **direct** dependencies only: the TUI necessarily reaches the store transitively, through the engine. Rule 4 applies to the whole resolved graph. deps-check also fails if `serde_json`'s `preserve_order` feature is enabled anywhere through feature unification, because audit hashing relies on sorted keys (DATA_MODEL §9).

1. `spektr-engine` must **not** depend on `spektr-modules` or `spektr-tui`. Built-in modules and the capture service are *injected* by the binary.
2. `spektr-tui` must **not** depend on `spektr-store` or `spektr-modules`. All data comes through `EngineHandle`.
3. `spektr-model` and `spektr-protocol` stay light: no async runtime, no I/O, no C dependencies.
4. **No `async-trait`, no `sqlx`, no Chrome DevTools crates** (`chromiumoxide`, `headless_chrome`).

What each edit recompiles:

| You edit | Recompiles | Does **not** recompile |
|---|---|---|
| A view or the keymap | `spektr-tui`, bin | store, engine, modules |
| A built-in module | `spektr-modules`, bin | tui, engine, store |
| Engine logic | engine, modules, tui, bin | store, model, protocol |
| The model | all spektr crates (rare; keep the model small) | any third-party crate |

## 3. Process model and concurrency

- **Runtime:** a `tokio` multi-threaded runtime, started by the binary.
- **Store actor:**
  - `rusqlite::Connection` is not `Sync`, so the store runs on a **dedicated OS thread** owning the **writer connection**. It takes `StoreCmd` over `mpsc` and replies on `oneshot`.
  - An ingestion batch is one transaction.
  - Reads use a small pool of **read connections** (WAL allows concurrent readers), each on its own thread, so UI queries never wait behind ingestion.
- **Scheduler** (a tokio task):
  - eligibility checks; a priority queue (manual > auto, then shallower depth); a global semaphore (16)
  - per-module concurrency
- **Rate limiting:** per-host `governor` limiters live in `spektr-modules::net`. Exec modules are limited by the engine, per job and per module.
- **Module host:**
  - each native job is a tokio task
  - exec jobs are `tokio::process` children, using line-delimited reads on stdout and a stderr drain to `tracing`
  - each job gets a `CancellationToken`
- **Ingestion** (a tokio task):
  - validates and normalizes `ModuleMessage`s
  - batches up to 200 messages or 50 ms and sends them to the store
  - runs the correlators on the affected entities
  - emits `EngineEvent`s
- **Events:** `tokio::sync::broadcast` of `EngineEvent` (job state, entity added, edge added or updated, correlation, review item, rules block, budget). A lagging UI resyncs by re-querying.
- **TUI loop** (main task): `tokio::select!` over:
  - crossterm `EventStream`
  - the engine broadcast
  - the TUI's own async effect results
  - a 250 ms tick

  `update(&mut AppState, Msg) -> Vec<Effect>` is pure. Effects run asynchronously and report back as `Msg`. Rendering happens only when the state is dirty.
- **Images:** `ratatui-image` resizing and encoding run on a background worker.

## 4. Key interfaces (sketch)

```rust
// spektr-engine
pub struct EngineHandle { /* cheap Clone */ }
impl EngineHandle {
    pub async fn act(&self, a: EngineAction) -> Result<ActionOutcome>;  // seed, run, merge, confirm, capture, finding ops …
    pub async fn query(&self, q: Query) -> Result<QueryResult>;          // ego(focus, page), entities(filter), timeline, path, finding(id), …
    pub fn subscribe(&self) -> broadcast::Receiver<EngineEvent>;
}

pub trait Module: Send + Sync + 'static {
    fn manifest(&self) -> &Manifest;
    fn run<'a>(&'a self, input: JobInput, cx: ModuleCx) -> BoxFuture<'a, Result<(), ModuleError>>;
}
pub struct ModuleCx { pub emit: Emitter, pub config: serde_json::Value, pub secrets: Secrets, pub cancel: CancellationToken }

pub trait CaptureService: Send + Sync + 'static {
    fn capture<'a>(&'a self, req: CaptureRequest, emit: Emitter) -> BoxFuture<'a, Result<(), CaptureError>>;
}

// spektr-modules
pub struct Net { pub http: Http, pub dns: Dns }   // Transport: Live | Record(path) | Replay(fixtures)
pub fn builtins(net: Net) -> Vec<Box<dyn spektr_engine::Module>>;
pub fn capture_service(net: Net) -> impl spektr_engine::CaptureService;

// spektr (bin)
let net = Net::new(transport_for(&args));                 // Replay for tutor and tests
let engine = Engine::open(case_path, config)?;
for m in spektr_modules::builtins(net.clone()) { engine.register(m); }
engine.set_capture(spektr_modules::capture_service(net));
```

`Query` results are **view-ready**: ego groups already paginated, strength already computed. The TUI does no graph logic.

## 5. Configuration and paths

- **Config:** `$XDG_CONFIG_HOME/spektr/config.toml`, falling back to `~/.config/spektr/config.toml`. XDG paths apply on macOS too, as is normal for CLI tools.
- **One `paths` helper in the bin resolves every directory** from the `XDG_*` variables, falling back to `dirs::home_dir()` joined with `.config`, `.local/state` or `.local/share`. **Never use `dirs::config_dir()`,** `data_dir()` or `state_dir()`: on macOS they return `~/Library/…`. The resolved paths are passed into the engine, modules and TUI; no other crate computes paths.
- **State:** `$XDG_STATE_HOME/spektr/`, falling back to `~/.local/state/spektr/`. It holds `recent.toml`, `tutor.toml` and logs.
- **Data:** `$XDG_DATA_HOME/spektr/`. It holds the downloaded headless Chrome and the tutor case template.
- **Cases** live wherever the user puts them. `spektr new` writes to the current directory unless `--dir` is given.
- **Secrets:** `config.toml` `[secrets]` or environment variables `SPEKTR_SECRET_<NAME>`. Environment wins.
  - Secrets are never written to a case file or log.
  - A `tracing` layer redacts known secret values.
- **Logs:** `tracing` to a rolling file in the state dir, never to the terminal while the TUI is running.

## 6. Build-time strategy

**Measured** on 2026-10-20 on a 2-vCPU Linux sandbox with Rust 1.99.1. The stub workspace had the full dependency set in `DEPENDENCIES.md`. It used an earlier 10-crate layout with the same isolation of heavy dependencies; merging net, media, capture and tutor into their parent crates doesn't change any third-party compile work:

| Scenario | Time |
|---|---|
| Clean `cargo build` (dev), every third-party dependency (~470 compilation units) | **1 m 52 s** wall, 3 m 08 s CPU |
| Adding the last few small deps (psl, schemars, which, …) to a warm build | 18 s |
| Rebuild after touching the tui, modules, engine or model stub | under 1 s each: only the edited crate and the bin rebuilt, no third-party crate recompiled |
| reqwest with default TLS (aws-lc-rs), clean | 40 s |
| reqwest with rustls + ring, clean | 23 s |
| `chromiumoxide` alone, clean | 83 s |

Expect clean builds to be several times faster on a modern 8–12 core laptop. Clean builds happen rarely anyway; incremental rebuilds are what you live with.

**Rules:**

1. **TLS: rustls with the ring provider** (`reqwest` feature `rustls-no-provider`, plus `rustls` feature `ring`). Install the provider at startup.
2. **Screenshots: a Chrome subprocess,** not a Chrome DevTools crate.
3. **`rusqlite` with bundled SQLite,** not `sqlx`. No build-time database or query macros.
4. **Trim features:**
   - `image`: png, jpeg, webp and gif only
   - `ratatui-image`: `default-features = false` plus `crossterm` (its default `chafa-dyn` needs system libchafa)
   - `tokio`: only the features in use
5. **Profiles:**
   ```toml
   [profile.dev]
   debug = "line-tables-only"
   [profile.dev.package."*"]
   debug = false
   [profile.dev.package.image]          # keep image decoding usable in dev
   opt-level = 2
   [profile.dev.package.zune-jpeg]
   opt-level = 2
   [profile.dev.package.sha2]
   opt-level = 2
   [profile.dev.package.zstd-sys]
   opt-level = 2
   [profile.release]
   lto = "thin"
   [profile.dist]                       # release CI
   inherits = "release"
   lto = "fat"
   codegen-units = 1
   strip = true
   ```
6. **Linker and the primary dev machine.** The primary dev machine is an **Apple Silicon Mac** (M1 Pro, `aarch64-apple-darwin`).
   - **macOS:** Apple's current linker (ld-prime, which ships with the Xcode Command Line Tools) is fast, and Rust's default `split-debuginfo = "unpacked"` on macOS already avoids the slow dsymutil step. **Don't add a custom linker by default.** `mold` is Linux-only, and macOS support in `wild` and `ld64.lld` is not a clear win over ld-prime.
   - **The biggest macOS-specific win is Gatekeeper/XProtect scanning.** macOS scans each newly built executable on first run, including every build script, which can noticeably slow Rust builds. Fix it once:
     - run `sudo spctl developer-mode enable-terminal`
     - add your terminal app under *System Settings → Privacy & Security → Developer Tools*

     Document this in `CONTRIBUTING.md`.
   - **Linux (CI and contributors):** Rust already links with `lld` by default on `x86_64-unknown-linux-gnu`.
   - **Measure before changing anything.** Run `cargo build --timings` after a one-line edit in `spektr-tui`. If linking the bin takes more than about 40% of that rebuild, try an alternative linker through `.cargo/config.toml` **per target**, with a fallback so contributors without it still build. Record the before/after numbers in the COMPLETE doc.
   - **CI** tests all four shipping targets (§9). The primary platform, arm64 macOS, is always tested natively.
7. **Tests:** `cargo nextest`. Neither snapshot tests nor fixture tests need network.
8. **CI:** `Swatinem/rust-cache`. Upload `cargo build --timings` as an artifact on main. A rise of more than 25% in clean-build time between releases must be explained in the PR.

## 7. Error handling

- **Libraries:** `thiserror` enums per crate. The bin uses `anyhow` only at the top level.
- **Module errors** never crash the engine; a failed job is an event with an error code.
- **Store errors** during ingestion roll back the batch, emit a `hostile` activity row and leave the case intact.
- **Corruption:** opening a case runs `PRAGMA quick_check`. On failure, spektr refuses to open the case and suggests `spektr verify`.
- **Panics:** a panic hook restores the terminal, then writes a crash report to the state dir.

## 8. Testing strategy

| Layer | Tests |
|---|---|
| model | Unit + `proptest` for every normalizer (idempotence; key stability across case and Unicode variants) |
| store | Migrations, repositories, audit chain (tamper a row → `verify` fails), merge / unmerge round-trip, graph queries on generated graphs |
| engine | Eligibility matrix (rules × depth × scope × hub × cache), ingestion validation (bad predicate, missing evidence, undeclared output), correlators, exec protocol against a fake module script |
| modules | Fixture replay per module; unmatched request = failure. `--features live` for opt-in live tests |
| tui | `update()` unit tests; `insta` snapshots of every screen at 170×48 and 120×36; keymap conflict test; scripted tutor end-to-end |
| cli | `assert_cmd` for `new`, `run`, `export`, `verify`, `module check` |
| e2e | The real binary in a PTY (`portable-pty` + `vt100`), with keyboard, mouse, resize and color-mode assertions. Lives in `crates/spektr/tests/`. See `QA.md` §2 |
| agent QA | `cargo xtask qa`: scripted scenarios save frames as text, ANSI, HTML and PNG; the agent reviews them against the artboards. `--live` runs approved targets only. See `QA.md` §3–4 |
| demos | `cargo xtask demos` runs every `demos/*.tape` through VHS in CI on release tags; a failing tape fails the release |

**Test-control environment variables.** The bin reads these, so the E2E harness and QA can drive the real binary deterministically. Each is documented in `--help` under "Testing", and refused unless `SPEKTR_TEST_MODE=1` is also set:

| Variable | Effect |
|---|---|
| `SPEKTR_TRANSPORT=replay:<fixture-dir>` (or `record:<dir>`) | Replaces live network for every built-in module and for capture |
| `SPEKTR_CLOCK=<RFC 3339 timestamp>` | Fixed clock, for status line, audit timestamps and job times |
| `SPEKTR_SEED=<u64>` | Deterministic UUIDv7 randomness |

While `SPEKTR_TEST_MODE=1` (and always inside the tutor), **nothing is handed to the OS**: launchers, `O`, `o`, `$PAGER` and `$EDITOR` are not opened. Instead, each request is appended as a JSON line to `$XDG_STATE_HOME/spektr/opened.jsonl` (kind, target, entity) and shown in the activity rail as `↗ would open <target>`. E2E tests assert against that file.

## 9. Release

- **Targets:** x86_64 and arm64 on both macOS and Linux. Every target is built and tested in CI on every PR, not only at release:

| Target | CI runner | Tests |
|---|---|---|
| `aarch64-apple-darwin` | GitHub arm64 macOS runner (`macos-latest`) | native |
| `x86_64-apple-darwin` | an Intel macOS runner if GitHub still offers one at kickoff; otherwise cross-compile on the arm64 runner | native, or under Rosetta 2 when cross-compiled |
| `x86_64-unknown-linux-gnu` | `ubuntu-22.04` | native |
| `aarch64-unknown-linux-gnu` | GitHub arm64 Linux runner (`ubuntu-22.04-arm` or the current equivalent) | native |

  - **Build Linux releases on Ubuntu 22.04,** so the binaries run on older glibc versions.
  - **The bundled C code** (SQLite, zstd, ring) compiles with `cc` on every target, so no cross toolchain is needed when each target builds on its own native runner. Prefer native runners over `cross`.
  - **Check runner labels at kickoff** and record them in the Phase 0 COMPLETE doc, since GitHub's runner lineup changes over time.
- **Builds:** `dist` (cargo-dist) produces the release binaries for all four targets.
- **Publishes:** GitHub Releases and a Homebrew tap (`infktd/homebrew-tap`).
- **Tag versions as `vX.Y.Z`.** `CHANGELOG.md` follows Keep a Changelog.
- `cargo install spektr` works from crates.io, which requires publishing every library crate.

## 10. CLI reference (v1)

Every command that takes `<case>` accepts a path to a `.spektr` file. Commands that read the case support `--json`, which prints one JSON object per line on stdout. Exit codes:

| Code | Meaning |
|---|---|
| 0 | OK |
| 1 | Error |
| 2 | Usage error |
| 3 | `verify` found a problem |

| Command | Arguments | Notes |
|---|---|---|
| `spektr` | — | Launcher (TUI) |
| `spektr new <name>` | `--profile <p>` (default `default`) `--subject "<name>"` `--last-seen <YYYY-MM-DD>` `--region <CC>` (default `US`) `--depth <n>` (default from the profile) `--scope [deny] <rule>` (repeatable) `--dir <path>` | Creates `<name>.spektr` |
| `spektr open <case>` | — | Opens the TUI on a case |
| `spektr run <case>` | `--seed <type>:<value>` (repeatable) `--module <name>` (repeatable; default: all eligible auto modules) `--playbook <name>` `--depth <n>` `--timeout <dur>` | Runs to quiescence, then prints a summary. With `--playbook`, also prints step results and open checklist items |
| `spektr playbook …` | `list` · `info <name>` · `check <file>` · `new <name> --seed <type>` | PLAYBOOKS §5 |
| `spektr query <case>` | `--type <type>` `--match <text>` (fuzzy) `--neighbors <type>:<value>` `--predicate <p>` `--path <type>:<value> <type>:<value>` `--findings` `--json` | Default output is a table; `--json` gives one entity, edge, path or finding per line |
| `spektr export <case>` | `--format tracelabs\|md\|json` `--finding <id>` (repeatable; default all) `--with-blobs` (json only) `-o <file>` (default stdout) | |
| `spektr capture <case> <url>` | `--entity <type>:<value>` (default: the url entity) `--no-screenshot` | |
| `spektr attach <case> <path>` | `--entity <type>:<value>` `--note "<text>"` | An image file also creates an `image` entity |
| `spektr verify <case>` | `--json` | Exits with 3 on failure |
| `spektr doctor` | `--json` | Config, secrets, binaries, terminal, image protocol, tmux mouse |
| `spektr tutor` | `--reset` | |
| `spektr setup capture` | — | Downloads headless Chrome into the data dir |
| `spektr module …` | `new <name> --lang python\|rust\|shell` · `test <name> --input <type>:<value> [--record \| --fixture <file>]` · `check <name>` · `install <path\|git-url>` · `install --update <name>` · `list` · `info <name>` · `enable <name>` · `disable <name>` | MODULES §9 |

**Entity argument syntax:** `<type>:<value>`, split at the **first** colon. So `account:github:jmarlow-wav` and `url:https://…` work.

**Scope rule syntax** (CLI `--scope`, palette `scope add [deny] <rule>`):

| Rule | Example |
|---|---|
| `domain:<suffix>` | `domain:*.kestrel.example` or `domain:kestrel.example` |
| `cidr:<net>` | `cidr:203.0.113.0/24` |
| `platform:<id>` | `platform:github` |
| `regex:<re>` | Matched against entity keys |

Rules are allow rules unless prefixed with `deny`. Deny always beats allow. With no allow rules, everything not denied is in scope.
