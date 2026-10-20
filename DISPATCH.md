# Dispatch: build spektr v1

You are building **spektr**, a terminal OSINT workbench in Rust, from scratch, in this repository. Everything you need is in `docs/`:

| Read | Before |
|---|---|
| `docs/PRD.md` | Anything. It says what and why |
| `docs/ARCHITECTURE.md` | Phase 0 |
| `docs/DEPENDENCIES.md` | Phase 0 |
| `docs/DATA_MODEL.md` | Phase 1 |
| `docs/MODULES.md` | Phase 2 |
| `docs/PLAYBOOKS.md` | Phase 4 |
| `docs/TUI_SPEC.md` + `docs/design/` | Phase 3 |
| `docs/QA.md` | Phase 2. It defines how you test the running app yourself |

Read every doc once, end to end, before writing code. Re-read the relevant doc at the start of each phase.

## Who's who

- **Jay** is the product owner and decision-maker. He tests each phase himself before the next starts.
- **You** build autonomously within this spec and triage problems yourself (rules below). Check in only on genuine blockers.
- **An adversarial audit agent** reviews every phase against this spec and the code: fabricated results, skipped exit criteria, spec drift. Write your COMPLETE docs knowing they will be checked.

**Primary dev machine:** Apple Silicon (M1 Pro, `aarch64-apple-darwin`). CI also runs on Linux.

## Non-negotiable rules

1. **Verify before you claim.** Every claim in a COMPLETE doc must be backed by output you actually observed: test counts, timings, screenshots. Paste real output; never paraphrase it. If you didn't run it, say so.
2. **Latest versions at kickoff.** Re-check every crate in `DEPENDENCIES.md` on crates.io in Phase 0. Take newer patch or minor releases. A new major release means checking compatibility and recording the decision. After Phase 0, **never upgrade a dependency as part of other work.** A bump is its own commit.
3. **Crate boundaries and dependency rules are fixed** (ARCHITECTURE §2). Seven crates plus `xtask`. Use Rust modules, not new crates, for internal structure. `cargo xtask deps-check` must pass in CI from Phase 0 on.
4. **No network in `cargo test`.** Everything external goes through record/replay fixtures. Live tests sit behind `--features live` and are never run in CI.
5. **Correctness over shortcuts.** No `unwrap()` or `expect()` outside tests and truly unreachable states (comment why). No silent fallbacks: an error is either handled or surfaced.
6. **Visual quality is a requirement.** The TUI must match `TUI_SPEC.md` and the authoritative artboards. Aesthetic drift is a bug, and colors come only from `theme.rs`.
7. **One action registry.** Keys, mouse, menus, palette and help come from `keymap.rs`. Never handle a key inside a view.
8. **Fictional data only** in fixtures, the tutor, snapshots and demos: `.example` domains, documentation IP ranges, invented people.
9. **Clean gates on every commit:**
   - `cargo fmt --check`
   - `cargo clippy --workspace --all-targets -- -D warnings`
   - `cargo nextest run --workspace`
   - `cargo xtask deps-check`
10. **You test the running app yourself, not just the code.** From Phase 2 on, every phase ends with the QA pass in `docs/QA.md`:
    - E2E tests through the PTY harness
    - `cargo xtask qa`, after which you **look at every frame PNG**, compare it with its artboard, and answer every checklist item
    - live smoke runs against Jay's approved targets only
    - fixing the bugs you find

    "Tests pass" is not "done". Done means you used it and it works and looks right.
11. **Live network only against `qa/live-targets.toml`.** Never query a real person, account, email or domain that Jay hasn't listed there.
12. **Commits are small and conventional:** `feat(engine): …`, `fix(tui): …`, `deps: bump …`. Use one branch per phase, `phase-N-<name>`. Never force-push main.

## Self-triage

**Resolve yourself, and note it in the COMPLETE doc:**
- naming inside a crate
- module layout within a crate
- test organization
- choice of small utility crates, if they're maintained, permissively licensed and pinned
- spec ambiguities where one reading is clearly more consistent with the rest of the docs

**Stop and ask Jay:**
- anything that would change a crate boundary, the case-file schema after Phase 1, the exec protocol after Phase 2, or the keymap's default bindings
- a spec requirement that is impossible or clearly wrong, with evidence
- a new major version of ratatui, crossterm, tokio, reqwest or rusqlite at kickoff that breaks compatibility within the stack
- a technique that might not be `passive` (MODULES §7)

**Never:**
- add `sqlx`, `async-trait`, `chromiumoxide`, `headless_chrome`, or a default-features build of `reqwest` (which pulls aws-lc)
- write secrets to case files, logs, fixtures or snapshots
- implement probing or active techniques in built-in modules

## Phases

Finish one phase at a time. At the end of each phase:

1. From Phase 2 on, run the QA pass (`docs/QA.md` §6), fix what it finds, and write `docs/phases/PHASE_N_QA.md`.
2. Write `docs/phases/PHASE_N_COMPLETE.md`. Cover:
   - what shipped
   - **deviations from the spec, and why**
   - known issues
   - real command output: test summary, `--timings` numbers where asked
   - how Jay can test it, in three to six steps
3. Open a PR from the phase branch.
4. **Stop.** Don't start the next phase until Jay says so.

### Phase 0: Scaffold and build baseline

- Create the workspace exactly as in ARCHITECTURE §2: 7 crates plus `xtask`, `demos/`, `rust-toolchain.toml` (1.99.1, or a newer stable if one exists at kickoff; record it), the profiles from ARCHITECTURE §6, and `[workspace.dependencies]` from DEPENDENCIES.md, re-verified.
- Add Apache-2.0 `LICENSE`, `README.md` (one paragraph plus a status note), `CONTRIBUTING.md` (including the macOS Developer Tools setup from ARCHITECTURE §6), and `CHANGELOG.md`.
- Set up CI (GitHub Actions) across **all four targets**: x86_64 and arm64, macOS and Linux (runner table in ARCHITECTURE §9). Run fmt, clippy, deps-check and `cargo deny check` once, on Linux x86_64. Run build + nextest on every target. Upload the `--timings` artifact from arm64 macOS.
- Implement `cargo xtask deps-check`, enforcing ARCHITECTURE §2's dependency rules plus the "never" list above.
- **Exit:**
  - Everything builds, and the gates pass on all four CI targets. List the runner labels you used.
  - `docs/phases/BUILD_TIMINGS.md` holds these numbers, measured on Jay's M1 Pro:
    - a clean `cargo build`
    - a rebuild after a one-line edit in `spektr-tui`, then the same for `spektr-modules`, `spektr-engine` and `spektr-model`
    - the link time of the binary, taken from `--timings`

    If you can't run on that machine, record that the numbers came from yours, and give your machine's specs.

### Phase 1: Model and store

- `spektr-model`, everything in DATA_MODEL §1–5 and §7:
  - all 15 types with their normalizers
  - platform rules (enough for the site list in Phase 2)
  - the predicate registry with source/target validation
  - extension-type rules, attributes, strength computation, and the known-hub list
- `spektr-store`, everything in DATA_MODEL §6, §8–10:
  - schema (all of DATA_MODEL §8, **including the playbook tables from PLAYBOOKS §3**, so later phases don't change the schema), migrations, the store actor and the read pool
  - repositories, merge / unmerge, graph queries (ego groups with pagination, k-hop, shortest path, degree, timeline events)
  - blobs with zstd and SHA-256
  - the audit chain and `verify`
- **Exit:**
  - Each normalizer has property tests: idempotent, stable across case and Unicode variants.
  - A tamper test: changing any audit row or blob makes `verify` fail and name the location.
  - A merge / unmerge round-trip test leaves the graph exactly as before.
  - The ego query on a generated 2,000-entity case returns in < 20 ms on the M1 Pro (benchmark output in the COMPLETE doc).

### Phase 2: Engine, protocol, first modules, headless CLI

- `spektr-protocol`: the manifest and message types (MODULES §2–3), plus the JSON Schemas via `cargo xtask schemas`.
- `spektr-engine`, MODULES §4–5:
  - `EngineHandle`, `EngineEvent`
  - the scheduler and eligibility matrix
  - rules profiles (`default`, `tracelabs`), scope, depth, hubs, cache, budgets
  - the ingestion pipeline with validation
  - the correlator framework, plus two rules: identity merge (at insert) and `ip`. The other correlators (`avatar_phash`, `analytics_id`, `favicon_mmh3`, `registrant_email`, `cert_san`, same-person candidates) come in Phase 4, with the modules that produce their inputs
  - the review queue
  - the exec runtime, both per-job and serve modes
- `spektr-modules`:
  - `net` (Live / Record / Replay transports; per-host rate limiting)
  - three built-ins: `dns`, `github`, `username-sweep` (with the site list)
- Binary commands: `new`, `run`, `query`, `verify`, `export --format json`, `doctor` (text output).
- QA groundwork (`docs/QA.md`):
  - the PTY terminal harness, with frame saving
  - `cargo xtask qa` and `qa-shots`
  - the `e2e_headless` and `e2e_verify` tests
  - a `qa/live-targets.toml.example`
  - the first live smoke run (example.com, plus any targets Jay has added)
- **Exit:**
  - The Phase 2 variant of J6 works headless against fixtures: `spektr run` with a **username** seed at depth 2, then `spektr export --format json`.
  - The eligibility matrix test covers every rule × trigger combination.
  - An exec-protocol test uses a fake module script (Python, standard library only, in `tests/fixtures`).
  - J3 (blocked technique) passes as a test.
  - The fixture-replay test fails on an unmatched request (prove it with a negative test).

### Phase 3: TUI core

- In `spektr-tui`, following TUI_SPEC §2–8 and §11:
  - theme tokens with fallbacks (256-color, `NO_COLOR`)
  - the action registry and keymap, with the conflict test
  - the launcher
  - the workspace: top bar; entities pane; ego view; inspector (without images yet); activity rail; status line; responsive layout and the size floor
  - the command palette, context menus and submenus, and the help overlay
  - mouse support (hit-testing, double-click, wheel, right-click, hover), with `:mouse off`
- Binary: `spektr` (launcher) and `spektr open`.
- E2E tests (`docs/QA.md` §2): `e2e_launcher`, `e2e_keyboard`, `e2e_mouse`, `e2e_text_modes`, `e2e_layout`, `e2e_color_modes`. Plus QA scenarios for every screen and overlay built in this phase.
- **Exit:**
  - Every screen has `insta` snapshots at 170×48 and 120×36.
  - The workspace on the Phase 2 fixture case visually matches `design/Identity.dc.html`. Put real terminal screenshots, taken on the M1 Pro in Ghostty, WezTerm or iTerm2, next to the artboard in the COMPLETE doc.
  - Input-to-render < 16 ms on the 2,000-entity case (measured).

### Phase 4: Full investigation loop

- The remaining 15 built-ins (MODULES §6), each with fixtures.
- Playbooks (`docs/PLAYBOOKS.md`): the runner in the engine, the `playbook_run` and `checklist_item` tables, the six built-in playbook files, the built-in launchers, `spektr playbook …` and `run --playbook`, the Checklist screen (artboard 10), the context-menu and palette entries, and `:reload`.
- The remaining correlators (listed in Phase 2) and the inspector's edge comparison (TUI_SPEC §4.4).
- Capture: HTML/headers, the screenshot via a Chrome subprocess, and `spektr setup capture`.
- Attachments and pivot launchers.
- Timeline and path tabs.
- The Review screen.
- Images (TUI_SPEC §9): inspector preview, avatar comparison, and protocol detection shown in the doctor.
- **Exit:**
  - J2 steps 1–6 work against fixtures, end to end.
  - `e2e_j2_identity` and `e2e_j3_rules` pass. `e2e_j7_playbooks` passes for J7 steps 1–4, 6 and 7; step 5's "finding from an item" waits for Phase 5, when findings exist. Every built-in playbook passes `spektr playbook check`, and runs end to end against fixtures.
  - Images render on Kitty-protocol and iTerm2 terminals and fall back to halfblocks elsewhere. Verify with `cargo xtask qa --real-terminal` (QA.md §5) and include the screenshots, or give Jay the manual steps if screen capture isn't permitted.
  - The tmux behavior is tested and documented.

### Phase 5: Findings, tutor, module tooling, release

- Findings: the screen, inline and `$EDITOR` editing, checks from the rules profile, export (`tracelabs`, `md`, `json`).
- The tutor (TUI_SPEC §10): the full lesson set, offline.
- `spektr module new | test | check | install | list | info | enable | disable`, plus Python, Rust and shell templates.
- A VHS demo suite (`demos/*.tape`: tutor, pivot, capture, finding export) and `cargo xtask demos`.
- Release: `dist` config for the four targets, a Homebrew tap formula, and crates.io publishing.
- **Exit:**
  - J1, J2 (complete), J4, J5, the full J6 (email seed, `--format md`) and the full J7 from PRD §5 all pass.
  - All demo tapes render in CI.
  - `e2e_tutor` passes, and the QA scenario list in `docs/QA.md` §3 is fully covered, with every frame reviewed.
  - A dry-run release builds artifacts for all four targets.
  - A stranger's-eye review: follow only `README.md` and the tutor on a clean machine, or document why you couldn't.
