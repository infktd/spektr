# Phase 3 complete: TUI core

Branch `phase-3-tui` · 2026-10-20 · QA report: [`PHASE_3_QA.md`](PHASE_3_QA.md)

## What shipped

### `spektr-tui` (TUI_SPEC §2–8, §11)
- **`theme.rs`:**
  - the 11 tokens with truecolor and 256-color values; the spectral strip
  - detection order: `NO_COLOR`, then config `colors`, then `COLORTERM`
  - monochrome: bold focus, reverse-video cursor row and pill
  - no other file names a color
- **`keymap.rs`, the action registry:**
  - every binding: action, stable name, default keys, context, description, short label, status-line rank, palette name and declared overrides
  - 13 contexts with the spec's precedence; Findings, Checklist and Review bindings are already in the table
  - text-input contexts capture printable keys and `⌫`; only `esc ↵ tab ↑ ↓` and `ctrl-` chords pass through
  - `[keys]` overrides from config.toml; unknown or clashing overrides are skipped with a warning
  - generated from the table: status-line hints, menu hotkeys, footers and the help overlay
  - **the conflict test:** a key bound twice in one context, an undeclared override, or a printable key in a text context all fail the build
- **Launcher (artboard 01):**
  - wordmark, spectral strip, version
  - recent cases with entities, edges, last opened and a verified chain state (`✓` / `✕ break at #n`)
  - path · size · seeds for the selected case
  - the doctor panel, plus the TUI's own terminal line
  - new-case and open-path dialogs
- **Workspace:**
  - **top bar:** case, subject and last seen, or the path; rules, findings, review, running/queued, chain. Low-priority items drop first when it's narrow.
  - **entities pane:** spec group order, ≤ 4 rows then `··· N more`, auto-expanded groups of ≤ 5, focus `▸`, markers, an out-of-scope summary line, nucleo fuzzy filter with `N match`
  - **ego view:** breadcrumb, inbound and symmetric groups left, card center, outbound right; connector rules; correlated groups in accent with their artifact label and note; paging with `[`/`]`; account rows in aligned columns; strength bars
  - **inspector:** follows the cursor; header, fields with sources, `▲ after last-seen date`, Evidence N, Strength, Used in, Notes, footer hints
  - **activity rail:** live engine events (job states, correlation, hub, rules blocks, failures with retry) seeded with the case's job history; the right column
  - **status line:** mode pill, registry hints, toasts, UTC clock
  - **layout:** the breakpoints at 170 / 130 / 100, the narrow-terminal inspector overlay (`i`), and the size floor message
- **Overlays:**
  - **command palette:** 96×22; Modules / Commands / Recent; fuzzy matching; signature and cost or last-run columns; a detail pane; `N of M`
  - **context menus (artboard 07):** built from the registry; disabled items show their reason; `Run module ▸` lists cost or last run; `A` runs all eligible; the submenu flips left when there's no room; clamped, with a shadow
  - **help:** full screen, every binding by context, `/` search, the shift-drag note
  - **dialogs:** note, merge picker, reject (needs `y`), quit while jobs run, new case, open path
- **Mouse:**
  - each frame returns hit regions
  - click focuses and moves the cursor; a double click within 400 ms activates; the wheel moves 3 rows; right-click opens the menu
  - clicking a tab or the pill works; clicking outside an overlay closes it
  - hovered rows highlight, redrawing only when the hovered target changes
  - `mouse = false` or `:mouse off` turns capture off, and any mouse input that still arrives is ignored
- **Architecture:**
  - `update(&mut AppState, Msg) -> Vec<Effect>` is pure; rendering is a pure function of the state
  - the runtime selects over input, engine events, effect results, a 250 ms tick and a 100 ms refresh debounce
  - it renders only when the state is dirty, at most every 16 ms
- **`Host` trait:** what the TUI needs from the binary (recent cases, doctor, verify, profiles, open, create). The TUI never sees paths, config or the store.

### `spektr` (binary)
- **Commands:** `spektr` with no arguments opens the launcher; `spektr open <case>` opens a case.
- **`recent.toml`:** in the state dir, updated by `open`, `new` and the launcher.
- **`TuiHost`:** recent-case info read-only through the store (counts, seeds, `Db::verify`), the doctor checks (now a function shared with `spektr doctor`), and creating or opening cases with every module registered.
- **Settings:** `mouse`, `glyphs`, `colors` and `[keys]` from config.toml.
- **Non-TTY:** the TUI refuses a non-terminal stdin/stdout and points to `run` / `query` / `export`.

### Tests and QA tooling
- **Snapshots:** 24 insta snapshots (`crates/spektr-tui/tests/snapshots/`) of every screen and overlay at 170×48 and 120×36, plus the inspector overlay and the size floor. They use a fixture that reproduces artboard 08's data.
- **E2E tests** (QA.md §2, real binary in a PTY):
  - `e2e_launcher`
  - `e2e_keyboard`, generated from the registry
  - `e2e_mouse`
  - `e2e_text_modes`
  - `e2e_layout`
  - `e2e_color_modes`
- **Latency:** `perf_tui`, the input-to-render measurement.
- **QA runner:**
  - interactive `do = [...]` actions: wait, keys, settle, frame, click, dclick, rclick, hover, wheel, resize, sleep
  - per-step env and size; config, generated (synth) cases and a tamper hook per scenario
  - seven Phase 3 scenarios
- **`cargo xtask qa --real-terminal`:** the Ghostty screenshot run (QA.md §5).

## Exit criteria

| Criterion | Status | Evidence |
|---|---|---|
| Every screen has insta snapshots at 170×48 and 120×36 | ✓ | **Snapshotted:** launcher, the new-case dialog, workspace, filter, empty case, palette, menu, submenu, help, the reject and note dialogs, at both sizes. Also the inspector overlay and the size floor. **Not yet built:** Findings (Phase 5), Review and Checklist (Phase 4). |
| The workspace on the Phase 2 fixture case visually matches `design/Identity.dc.html` | ✓ for the harness frames, ⚠ for the real-terminal screenshots | The frame and the artboard are side by side in PHASE_3_QA, with checklist answers. **Ghostty screenshots: not taken.** Ghostty asks "Allow Ghostty to execute /bin/sh?", and that's Jay's call. `cargo xtask qa --real-terminal` is ready and the capture path is verified (steps under "How Jay can test it") |
| Input-to-render < 16 ms on the 2,000-entity case (measured) | ✓ | **p95 4.37 ms, max 4.96 ms** over 320 samples, release build, M1 Pro. Each sample is the key → `update` → engine queries (real engine, real SQLite) → `Terminal::draw` at 170×48. The output is below |
| E2E tests (QA.md §2): `e2e_launcher`, `e2e_keyboard`, `e2e_mouse`, `e2e_text_modes`, `e2e_layout`, `e2e_color_modes` | ✓ | All pass; `e2e_keyboard` exercises 76 bindings; output below |
| QA scenarios for every screen and overlay built in this phase | ✓ | Seven scenarios, 34 frames, all reviewed; 19 bugs found and fixed (PHASE_3_QA) |

## Deviations from the spec, and why

1. **Dialog keys were added; the default bindings are unchanged.** TUI_SPEC §5 gives dialogs only `↵`, `esc` and `y`, but the new-case dialog has four fields and a choice. I added, in the Dialog context only:
   - `tab` and `↓`: next field
   - `↑`: previous field
   - `←` / `→`: change a choice
   - `↑` / `↓` also move through the merge picker's list

   Nothing in the spec's tables changed.

   **Jay:** DISPATCH asks me to check with you before changing the keymap's defaults. These are additions to a context the spec left open; say if you want them different.
2. **The context menu uses the spec's keys only.** That's `↑ ↓ → ← ↵ esc` plus entity hotkeys; `j`/`k` don't move inside the menu. With the menu open, the active contexts are Menu, Entity and Global, not Navigation, so the list underneath doesn't move.
3. **Help's own keys:**
   - help is a full-screen overlay where the Navigation keys scroll
   - `/` searches
   - `esc` or `?` closes it
   - `q` still quits, asking first if jobs are running
4. **Not-yet-built features stay visible but disabled.** Menu items and palette commands for capture, findings, playbooks, launchers, raw evidence, timeline, path and tutor show `not available yet`. The keys show a toast; Findings and Checklist arrive in Phases 4–5.

   `Open in browser ▸` opens `url` entities directly until the launchers arrive (Phase 4).
5. **Timeline and path tabs exist but show a placeholder.** The tab bar works; the views are Phase 4 (DISPATCH).
6. **Entity groups auto-expand at ≤ 5 members, as TUI_SPEC §4.2 says.** Artboard 08 shows a 9-account group expanded; the spec wins (TUI_SPEC §0).
7. **Panes are adjacent.** The artboards space them with 8 px gaps, which have no exact terminal equivalent. W_e and W_i are exactly the spec's widths.
8. **A lone ego side gets three quarters of the width.** When one side has no groups, the card moves off center. Otherwise one-sided focuses at 130 columns truncate every value to 5 cells.
9. **The activity rail is seeded from the case's job history.** The jobs table doesn't store the module's progress note, so past rows read `done ← <entity>`; live rows have the full summary. The right column has no budget meters or running playbook yet: no keyed modules exist, and playbooks are Phase 4.
10. **The inspector's strength section has no "who confirmed it and when" or source count.** `EntityDetail` doesn't carry them; it shows the bar, the label and the degree. The engine query gains those fields with the edge-comparison inspector in Phase 4.
11. **No "N sites not found · hidden" line in the entities pane.** No query returns the sweep summaries; only the out-of-scope summary line is shown.
12. **The `•` new marker is per session.** It marks entities added since the TUI opened and not yet visited. It isn't persisted.
13. **The top bar's chain state is verified in the background on open.** It shows `· N` until the verify finishes, then `✓ N` or `✕`. The launcher verifies every recent case when it opens.
14. **The engine re-exports `CostClass`, `LogLevel`, `Runtime` and `Technique`** from the protocol crate, for the TUI. No crate boundary changed.
15. **`Host::create` returns the running engine.** The case is open, not opened twice: reopening deadlocked on the file the creating engine still held (QA bug 12).

## Known issues

- **The real-terminal (Ghostty) screenshots need one click from Jay.** See the exit criteria and the steps below.
- **The launcher re-hashes every recent case on start.** That runs off the render path and the UI stays responsive, but a very large case delays its `✓`.
- **A case can't be switched from inside the TUI.** Quit, then `spektr open` the other case, or use the launcher.
- **The harness's HTML frames count cells naïvely for wide (CJK) characters.** That affects the PNGs only; the TUI measures with `unicode-width`.
- **`e2e_keyboard` skips the 22 Findings, Checklist and Review bindings** until those screens exist; the test asserts that these are the only skips.

## Command output

```
$ cargo fmt --check && echo ok           → ok
$ cargo clippy --workspace --all-targets --all-features -q -- -D warnings   → (clean)
$ cargo nextest run --workspace
     Summary [   6.950s] 160 tests run: 160 passed, 2 skipped
$ cargo xtask deps-check
deps-check: ok (8 members, 490 resolved packages; default and all features)
$ cargo xtask schemas --check
schemas: up to date
```

(The skipped tests are the `qa` scenario runner, run by `cargo xtask qa`, and `perf_tui`, which is release-only.)

The Phase 3 E2E tests:

```
PASS [   1.705s] (1/7) spektr::e2e_launcher launcher_new_case_and_recent_list
PASS [   2.353s] (2/7) spektr::e2e_mouse mouse_off_disables_capture
PASS [   2.580s] (3/7) spektr::e2e_color_modes truecolor_256_and_no_color
PASS [   2.634s] (4/7) spektr::e2e_text_modes global_keys_type_in_text_inputs
PASS [   3.025s] (5/7) spektr::e2e_mouse mouse_maps_onto_the_same_actions
PASS [   3.635s] (6/7) spektr::e2e_layout breakpoints_and_live_resize
PASS [   7.251s] (7/7) spektr::e2e_keyboard every_registry_action_changes_the_screen
Summary [   7.253s] 7 tests run: 7 passed, 0 skipped

76 bindings exercised, 22 skipped (screens not built yet: ["finding-new", …, "review-open"])
```

The TUI crate's tests (keymap conflict test, snapshots):

```
Summary [   0.111s] 20 tests run: 20 passed, 0 skipped
```

Input-to-render latency (`cargo test --release -p spektr --test perf_tui -- --ignored --nocapture`, M1 Pro):

```
case: 2012 entities, focus has 2 ego groups
j (cursor)                         p50   0.28 ms  p95   0.56 ms  max   0.86 ms
k (cursor)                         p50   0.29 ms  p95   0.49 ms  max   0.55 ms
] (next page: ego query)           p50   4.22 ms  p95   4.81 ms  max   4.96 ms
[ (prev page: ego query)           p50   4.13 ms  p95   4.52 ms  max   4.82 ms
tab (pane)                         p50   0.30 ms  p95   0.31 ms  max   0.33 ms
shift-tab (pane)                   p50   0.29 ms  p95   0.30 ms  max   0.31 ms
↵ (pivot: ego + detail queries)    p50   0.76 ms  p95   0.78 ms  max   1.00 ms
⌫ (back: ego query)                p50   0.76 ms  p95   0.79 ms  max   4.09 ms
all                                p50   0.60 ms  p95   4.37 ms  max   4.96 ms  (320 samples)
```

The slowest path is paging the seed's 420-member `has_account` group, which re-runs the ego query: about 4.2 ms.

CI: see PR #4; the run is recorded below once it finishes.

## How Jay can test it

1. `git checkout main && git pull && cargo build`. Run it on the fixture case:
   ```sh
   export SPEKTR_TEST_MODE=1 SPEKTR_TRANSPORT=replay:$PWD/crates/spektr-modules/fixtures
   target/debug/spektr new demo --profile tracelabs --subject "Jordan Marlow" --last-seen 2026-09-28
   target/debug/spektr run demo.spektr --seed username:jmarlow.wav
   target/debug/spektr open demo.spektr
   ```
2. In the workspace:
   - `j`/`k`, `tab`, `↵` to pivot, `⌫` to go back
   - `:` for the palette, `space` for the menu, `→` for its submenu
   - `/` to filter, `?` for help
   - click, double-click and right-click rows
   - resize the window below 100×30
3. Run `target/debug/spektr` with no arguments for the launcher: the recent case, the doctor, and `n` to create a case.
4. For the real-terminal screenshot: `cargo xtask qa --real-terminal`, then click **Allow** in Ghostty. The PNG lands in `qa/out/real-terminal/ghostty-workspace.png`.
5. For the latency measurement: `cargo test --release -p spektr --test perf_tui -- --ignored --nocapture`.
6. For the QA frames: `cargo xtask qa && cargo xtask qa-shots`, then open `qa/out/p3-*/*.png`.
