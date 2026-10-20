# spektr — Agent-run QA

Unit and snapshot tests aren't enough. The agent building spektr must also **run the real binary the way a user would, look at the result, and fix what it finds**, every phase. This document defines how.

There are three layers:

1. **E2E tests:** automated, in CI.
2. **QA scenarios:** run and *looked at* by the agent, each phase.
3. **Live smoke runs:** real network, approved targets only.

## 1. The terminal harness

The harness is a test-support module inside the bin crate (`crates/spektr/tests/support/term.rs`). It is **not** a new crate. It runs the real `spektr` binary in a pseudo-terminal and reads the screen back as a cell grid.

- **`portable-pty`** spawns `spektr` in a PTY of a given size, with a given `TERM`, `COLORTERM` and `NO_COLOR`.
- **`vt100`** parses the output into a screen model: text plus per-cell foreground, background and bold. This lets tests assert *colors*, not just text. For example, the cursor row must have the `selection` background, and the focused pane border must be `accent`.
- **API sketch:**

  ```rust
  let mut t = Term::spawn(&["open", case], Size { cols: 170, rows: 48 })?;
  t.wait_for_text("Entities", Duration::from_secs(5))?;
  t.keys("j j ↵")?;                 // key DSL → bytes
  t.mouse_click(42, 10)?;           // SGR mouse sequences (1006)
  t.right_click(42, 10)?;
  t.resize(120, 36)?;
  let s = t.screen();               // Screen { cells, cursor }
  assert!(s.row_text(3).contains("◆ kestrel.example"));
  assert_eq!(s.fg_at(1, 0), theme::ACCENT);  // border color
  t.save_frame("qa/out/j2-step3")?; // writes .txt (plain), .ansi (colored), .html (rendered)
  ```

- **Frame rendering:** `save_frame` writes an HTML file that draws the cell grid in JetBrains Mono at 13 px / 20 px, using the theme colors, the same metrics as the design artboards. `cargo xtask qa-shots` then converts every `.html` frame to PNG with headless Chrome (the same Chrome capture already uses). It also renders the artboards in `docs/design/` to PNG at the same scale, so the two can be compared directly.
- **Determinism:** the harness sets:
  - `SPEKTR_TEST_MODE=1`
  - `SPEKTR_TRANSPORT=replay:<fixture-dir>`
  - `SPEKTR_CLOCK=2026-10-20T18:14:22Z`
  - `SPEKTR_SEED=1`
  - a temporary `XDG_*` home

  These variables are defined in ARCHITECTURE §8.

  Runs are reproducible as a result.

## 2. E2E tests (CI, every PR)

These live in `crates/spektr/tests/e2e_*.rs` and run with `cargo nextest`. No network.

| Test | Covers |
|---|---|
| `e2e_launcher` | Launch with no case; recent list; doctor panel; `n` creates a case; quit |
| `e2e_j2_identity` | J2 steps 1–8 against the tutor fixtures: seed, live streaming into the ego view, pivot, inspector, edge comparison, confirm with `v`, capture, finding, export |
| `e2e_j3_rules` | A probing exec module is blocked under `tracelabs`; the `⊘` row appears; the audit log has the reason |
| `e2e_keyboard` | Every action in the registry is triggered by its key in its context and changes state (the test is generated from the registry) |
| `e2e_mouse` | Click focus, double-click pivot, wheel scroll, right-click menu, submenu, click outside to close |
| `e2e_text_modes` | In palette, filter and insert modes, `q`, `F`, `1` etc. type characters instead of acting |
| `e2e_layout` | Sizes 200×60, 170×48, 130×40, 120×36, 100×30 and 99×30 (message shown); live resize mid-session |
| `e2e_color_modes` | Truecolor, 256-color (`COLORTERM` unset) and `NO_COLOR` all render, and assert cell attributes |
| `e2e_j7_playbooks` | J7: `tracelabs-kickoff` with nested runs, checklist items created per avatar, a launcher open audited, done with a note, finding from an item, `:reload` picks up a user playbook, `playbook check` rejects a bad one |
| `e2e_headless` | J6 via `assert_cmd`; `query --json` output parses; exit codes |
| `e2e_verify` | J5: tamper with an audit row, then with a blob; `verify` exits with 3 and names the location |
| `e2e_tutor` | Full tutor run, driven by the lesson script's expected actions |

Failure output always includes the last saved frame (`.txt` and `.ansi`) as an artifact.

## 3. QA scenarios (the agent looks at the results)

At the end of every phase, the agent runs `cargo xtask qa`, which executes `qa/scenarios/*.toml` through the harness and saves frames. Then the agent **reads the PNGs itself** and reviews them against the design and the spec. Every scenario file lists its steps and a **checklist** for the agent to answer in its report.

```toml
# qa/scenarios/ego-view.toml
name = "Ego view, identity case"
case = "tutor"
size = [170, 48]
steps = ["seed username:jmarlow.wav", "wait-idle", "frame ego-initial", "keys l j", "frame ego-cursor-right"]
compare = "docs/design/Identity.dc.html"
checklist = [
  "Three columns: inbound and symmetric left, focus card center, outbound right",
  "Group headers show predicate and count, with a connector line toward the focus card",
  "Strength bars use the three levels; only confirmed is green",
  "Cursor row has the accent ▌ bar and the selection background",
  "No text overflows a pane; no stray characters at pane borders",
  "Colors come only from the token table (TUI_SPEC §2)",
]
```

The QA report, `docs/phases/PHASE_N_QA.md`, has one section per scenario:

- each frame's PNG, placed **next to the matching artboard PNG**
- an answer to every checklist item: pass, or fail with what's wrong
- **bugs found:** each with a frame, the expected and actual behavior, and the fix commit

**Bugs the agent finds in QA are fixed before the phase is declared complete.** Anything deliberately left open goes under "Known issues", with a reason.

Scenarios to cover by the end of Phase 5, with a frame for each screen state:

- every screen in TUI_SPEC §5
- every overlay: palette, context menu and submenu, help, dialogs
- the Checklist screen with a running nested playbook, compared with artboard 10
- every mode pill
- empty states: a new case with no entities, an empty findings list, an empty review queue
- error states: a module failure, a rate limit, a missing secret, a broken audit chain in the launcher
- a 2,000-entity case, scrolled
- each size breakpoint (TUI_SPEC §4.1)
- `NO_COLOR` and 256-color
- `glyphs = ascii`

## 4. Live smoke runs (real network, opt-in)

Fixtures prove parsing; live runs prove the integrations still work. At the end of Phases 2, 4 and 5, the agent runs:

```
cargo xtask qa --live
```

It runs **only** the targets listed in `qa/live-targets.toml`. That file is **written by Jay** and git-ignored:

```toml
# qa/live-targets.toml: targets Jay approves for live testing. Nothing else is ever queried.
domains   = ["example.com"]          # IANA-reserved, always safe
usernames = []                        # e.g. Jay's own handle, if he chooses to add it
emails    = []                        # only addresses Jay owns
```

Rules:

- **Never query a target that isn't in this file.** Not in tests, not in QA, not "just to check". If the file is missing or empty, live runs cover `example.com` only, and the report says so.
- **Live runs use the `tracelabs` profile:** passive only.
- **The report includes:** per module, ok or failed with the reason, latency, and whether the response shape still matches the fixtures. A shape change is a bug; record a new fixture with `cargo xtask record`.
- **Live results are never committed.** No case files and no fixtures from real people. Fixtures stay fictional (rewritten from live recordings when needed).

## 5. Things the agent can't fully verify headlessly

- **Image protocols** (Kitty, Sixel, iTerm2) need a real terminal emulator; the vt100 parser doesn't render graphics. Halfblock output *is* checked headlessly. For the real protocols, on Jay's Mac:
  1. Run `cargo xtask qa --real-terminal`. It opens Ghostty or iTerm2 (whichever is installed) on a scripted session.
  2. Capture the window with `screencapture -l <window-id>`. This needs Screen Recording permission for the terminal app; ask Jay once.
  3. If that isn't possible, list the exact manual steps for Jay in the QA report instead of claiming the check passed.
- **tmux:** tested headlessly by running the harness inside `tmux new-session -d`, with `capture-pane -e -p`.

## 6. Exit criteria added to every phase

- `cargo xtask qa` passes, and `PHASE_N_QA.md` is written with frames, artboard comparisons, checklist answers and fixed bugs.
- Phases 2, 4 and 5: the `cargo xtask qa --live` results are in the report.
- **No checklist item is marked pass without the agent having looked at the frame.** The audit agent spot-checks this.
