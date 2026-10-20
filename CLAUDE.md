# spektr — notes for coding agents

The task brief is `DISPATCH.md`. The specs are in `docs/` (start with `docs/PRD.md`). Workflows (playbooks) are specified in `docs/PLAYBOOKS.md`.

## Commands

```
cargo build                          # dev build
cargo nextest run --workspace        # tests (no network)
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --check
cargo xtask deps-check               # crate-boundary + banned-dependency rules
cargo xtask schemas                  # regenerate exec-protocol + manifest JSON Schemas
cargo xtask record <module> <input>  # record fixtures (live network, explicit opt-in)
cargo xtask demos                    # render demos/*.tape with VHS
cargo build --timings                # build-time report
cargo xtask qa                       # run QA scenarios, save frames (txt/ansi/html/png) to qa/out/
cargo xtask qa-shots                 # render frames + design artboards to PNG for side-by-side review
cargo xtask qa --live                # live smoke run: ONLY targets in qa/live-targets.toml
```

## Testing means using it

At the end of each phase, run the app through the PTY harness (`docs/QA.md`). Look at every frame PNG, compare it with its artboard, answer each checklist item, and fix what's wrong. Don't mark a checklist item as passing without having looked at the frame.

## Rules that bite

- **Seven crates, fixed boundaries** (`docs/ARCHITECTURE.md` §2). The engine never depends on modules or the TUI; the TUI never depends on the store or modules.
- **Colors only in `spektr-tui/src/theme.rs`; key bindings only in `spektr-tui/src/keymap.rs`.**
- **Modules never write to the store.** They emit `ModuleMessage`s, and every entity, edge and attribute must cite an observation.
- **No network in tests.** Use record/replay fixtures in `crates/spektr-modules/fixtures/`.
- **Fictional data only** in fixtures, snapshots, the tutor and demos.
- **Dependency bumps are their own commits.** `Cargo.lock` is committed.
- **Banned dependencies:** `sqlx`, `async-trait`, `chromiumoxide`, `headless_chrome`, default-features `reqwest`.
- **Verify before claiming.** Paste real output into phase COMPLETE docs.
