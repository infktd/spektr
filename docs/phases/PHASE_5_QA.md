# Phase 5 QA: findings, the tutor, error and empty states

Branch `phase-5-release` · 2026-10-20 · `cargo xtask qa`, then `cargo xtask qa-shots`. I opened every frame below and read it against its artboard before answering its checklist. Frames I reviewed and then fixed were run again and reviewed again; the copies here are from the run after the fixes.

The Phase 2–4 scenarios still run and still pass. Phase 5 adds four scenarios with 23 frames: `p5-findings`, `p5-tutor`, `p5-errors` and `p5-empty`. The copies are in `docs/phases/qa/phase-5/`. The rest are in `qa/out/` after a run.

Two frames are deliberately not copied: `p5-errors/launcher-doctor-secrets` and `p5-tutor/launcher-tutor-hint`. The launcher's doctor panel shows this machine's temp paths and the fixtures path. I reviewed both from `qa/out/`; their text is quoted below.

```
qa: p2-errors: 5 frames
qa: p2-new-and-run: 2 frames
qa: p2-query: 3 frames
qa: p2-verify-doctor: 2 frames
qa: p3-colors: 2 frames
qa: p3-large: 3 frames
qa: p3-launcher: 5 frames
qa: p3-layout: 7 frames
qa: p3-modes: 5 frames
qa: p3-overlays: 9 frames
qa: p3-workspace: 3 frames
qa: p4-checklist: 9 frames
qa: p4-inspector: 5 frames
qa: p4-menu-capture: 7 frames
qa: p4-review: 5 frames
qa: p4-tabs: 5 frames
qa: p5-empty: 3 frames
qa: p5-errors: 5 frames
qa: p5-findings: 10 frames
qa: p5-tutor: 5 frames
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 88.71s
100 frames, 10 artboards
```

All data is fictional and comes from the fixture replay: the `jmarlow.wav` handle, `.example` domains and documentation IP ranges. The rate-limit state uses a new fictional fixture, `github/rl-demo.jsonl`: a 429 with `retry-after: 3`.

## QA.md §3 coverage, by the end of Phase 5

| Required | Where |
|---|---|
| Every screen in TUI_SPEC §5 | Launcher: `p3-launcher`. Workspace: `p3-workspace`. Ego, timeline and path: `p3-modes`, `p4-tabs`. Review: `p4-review`. Checklist: `p4-checklist`. Findings: `p5-findings` |
| Every overlay | Palette, menu, submenu, help and dialogs: `p3-overlays`, `p4-menu-capture`. The editor and the export menu: `p5-findings` |
| Checklist with a running nested playbook, against artboard 10 | `p4-checklist` |
| Every mode pill | NORMAL, FILTER, COMMAND, INSERT: `p3-modes`, `p3-overlays`, `p5-findings`. REVIEW, CHECKLIST: Phase 4. FINDINGS: `p5-findings`. TUTOR: `p5-tutor` |
| Empty states: new case, empty findings, empty review | `p5-empty` (case, review, checklist), `p5-findings/findings-empty`, `p4-review/review-empty-rejected` |
| Errors: a module failure, a rate limit, a missing secret, a broken chain in the launcher | `p5-errors`. The broken chain is `p3-launcher/launcher-broken` |
| A 2,000-entity case, scrolled | `p3-large` |
| Each size breakpoint | `p3-layout` (200×60, 130×40, 120×36, 100×30, 99×30, live resize) |
| `NO_COLOR` and 256-color | `p3-colors` |
| `glyphs = ascii` | `p3-colors` (the whole scenario runs with `glyphs = "ascii"`) |

## p5-findings: the Findings screen (artboard 09)

| Empty | Artboard 09 |
|---|---|
| ![](qa/phase-5/p5-findings-findings-empty.png) | ![](qa/phase-5/artboard-Findings.png) |

- ✓ **Empty:** the right pane says `No findings yet. n creates one; f on an entity in the graph files it with the entity.` The left pane lists the tracelabs categories with points (`5,000` Location … `10` Family) and `found 0`. The border reads `[all]  draft  ready  sent`. The footer reads `sent 0   ready 0   draft 0   · open value 0 pts` and `Categories come from the tracelabs rules profile.`
- ✓ **Status line:** `FINDINGS` pill (accent), then `n new finding   ↵ go to entity   d delete   esc back to graph`. The detail footer reads `e edit   a attach evidence   r mark ready   s mark sent   x export ▸`. The artboard's hint wording differs slightly (`submitted` vs `sent`); `sent` is the case-file status name.

| `f` on ◐ github · jmarlow-wav | After ↵: the detail, failing |
|---|---|
| ![](qa/phase-5/p5-findings-finding-new-dialog.png) | ![](qa/phase-5/p5-findings-findings-detail-failing.png) |

- ✓ **New finding dialog:** `From ◐ github · jmarlow-wav. ← → picks the category; e edits the rest later.`, then `category ‹ Advancing the Timeline ›` and the claim field (accent label when focused). COMMAND pill.
- ✓ **List:** `▾ Advancing the Timeline  1,000  1` first (categories with findings first), then `▌ F-1  Subject's GitHub account was active…  draft  ▰▱▱`, then the other categories by points.
- ✓ **Detail:** `Advancing the Timeline · 1,000 pts · draft · edited 18:14` bold; Claim; Relevance `—`; Supporting entities `◐ github · jmarlow-wav ▰▱▱`; Checks `✕ write the relevance` (hostile), `✓ 1 source URL`, `✓ no news or aggregator sources`, `✓ not used in another category`, `! needs staff coach approval` (warn); Evidence 1, the GitHub capture.
- ✓ **Toast:** `F-1 created · e edits it`.
- ✗→✓ **Bug 1:** the detail pane always had the accent border, even when the list had focus. Fixed: it follows focus, as in the artboard.
- ✗→✓ **Bug 2:** the detail printed the title on its own line above Claim, repeating the claim (the automatic title is the claim's first 60 characters). Fixed: only a title written by hand is shown, or the automatic `<category> · <entity>` while the claim is empty.
- ✗→✓ **Bug 3:** the dialog's claim field showed the *start* of a long value (`…2026-10-1…`), so the end you were typing was hidden. Fixed: a focused field shows its end (`…ject's GitHub account was active on 2026-10-16.`), via `text::tail`.

| `e`: the editor | After ↵: the checks pass |
|---|---|
| ![](qa/phase-5/p5-findings-finding-editor.png) | ![](qa/phase-5/p5-findings-findings-detail-passing.png) |

- ✓ **Editor:** `Edit F-1` overlay with the drop shadow; `title`, `claim`, `relevance` fields, the focused field's label in accent with a block caret; footer `↵ confirm   ↓ field   esc cancel`; INSERT pill. The checklist line said "the focused field has the accent border"; the design marks it with the accent label, which is what it does.
- ✓ **After saving:** `F-1 saved ✓`; every check is `✓` except the `!` approval warning, which doesn't block `ready`: only `✕` checks do.
- ✓ The capture is text only (`text/html`): replay never starts a browser, so there's no screenshot in the fixture case. The image block is covered by the snapshot test and by Phase 4's `p4-inspector/image-preview`.

| `r`: ready | `x`: the export menu |
|---|---|
| ![](qa/phase-5/p5-findings-findings-ready.png) | ![](qa/phase-5/p5-findings-findings-export-menu.png) |

- ✓ `r` → `ready` in the ok-blue status color in both panes; `ready 1   draft 0`; toast `F-1 ready ✓`.
- ✓ `x` opens a menu anchored at the footer's `x export ▸`: `↗ export F-1`, then `Submission text  clipboard`, `Markdown report  file`, `JSON  file`.
- ✓ ↵ on Submission text → toast `copied 1 submission` (frame `findings-exported`).

| `:export md` | Written |
|---|---|
| ![](qa/phase-5/p5-findings-palette-export-md.png) | ![](qa/phase-5/p5-findings-palette-export-written.png) |

- ✗→✓ **Bug 4:** `:export json|md|tracelabs` was listed in the palette but disabled. Fixed: it runs the same export as `x`, for every finding. `md` and `json` ask where (`file  tl-ctf-1101-findings.md`), then toast `wrote tl-ctf-1101-findings.md`.

## p5-tutor: the coach strip

| Lesson 1 | Lesson 1, step 2 | Lesson 3 |
|---|---|---|
| ![](qa/phase-5/p5-tutor-tutor-lesson-1.png) | ![](qa/phase-5/p5-tutor-tutor-step-advanced.png) | ![](qa/phase-5/p5-tutor-tutor-lesson-3.png) |

| After `p ↵`: the playbook ran (artboard 01 layout) |
|---|
| ![](qa/phase-5/p5-tutor-tutor-playbook-running.png) |

- ✓ **TUTOR pill** in the status line; the coach strip is the two rows directly above it.
- ✓ **Strip row 1:** `Lesson 4/10 · Pivot and go back` bold with `step 1/2` muted. **Row 2:** the goal, `esc clears the filter; tab to the graph; l l moves to the accounts on the right; ↵ pivots to one.`
- ✓ The panes are two rows shorter; nothing overlaps the strip.
- ✓ Steps advance on what happened, not on a key: `tab` → step 2/2; the seed → lesson 3; `p ↵` on `jmarlow.wav` runs `person-from-username` on bundled fixtures, and when it finishes lesson 4 starts on its own. Replay finishes the playbook before the frame is taken, so the frame shows lesson 4 with `playbook person-from-username: started` in the toast and the run's jobs in the rail.
- ✓ **Launcher hint** (frame not copied): `New to spektr?  spektr tutor  walks through a case in about 15 minutes, offline.` under the recent list.

## p5-errors: module failures, a rate limit, a missing secret

| A rate limit, retrying | After the retries: failures |
|---|---|
| ![](qa/phase-5/p5-errors-rate-limited-retrying.png) | ![](qa/phase-5/p5-errors-module-failures.png) |

- ✗→✓ **Bug 5:** a job queued again for a retry showed as a muted `⊘ github  queued ← ~ rl-demo` row, dropping the error and the countdown that `spektr run` prints. The retries themselves did happen. Fixed: the row is `✕ github  rate_limited: HTTP 429 from api.github.com  retry in 3s · 2 attempts left` (error color, countdown muted). After the last retry it's `✕ … ← ~ rl-demo`.
- ✓ **Failures:** `✕ username-sweep` and `✕ mastodon` rows in the error color, each with its message and entity; `failed 6` in the rail's counts (error color). A replay miss reads `no fixture for GET …`; on the network it's the HTTP error.
- ✓ No crash or garbled rows; the case stays usable (next frames).

| Missing secret: the palette | ↵ on it |
|---|---|
| ![](qa/phase-5/p5-errors-missing-secret-palette.png) | ![](qa/phase-5/p5-errors-missing-secret.png) |

- ✓ `run hibp` on `@ jordan.marlow@mail.example` is listed, muted, with `needs HIBP_KEY` on the right and in the detail.
- ✓ ↵ on it doesn't run; the palette's error line says `needs HIBP_KEY` in the error color. Automatic planning skips hibp quietly (MODULES §5: a missing secret is "ineligible" when automatic, "blocked" when asked for).
- ✓ **Launcher doctor** (frame not copied): `! hibp key   HIBP_KEY not set · module off`.

## p5-empty: a new case with no entities

| New case | Review | Checklist |
|---|---|---|
| ![](qa/phase-5/p5-empty-empty-case.png) | ![](qa/phase-5/p5-empty-empty-review.png) | ![](qa/phase-5/p5-empty-empty-checklist.png) |

- ✓ **Entities:** `no entities yet` and `: seed <type>:<value>`. **Graph:** `no focus yet` / `: seed <type>:<value> to start`, centered. **Inspector:** `nothing under the cursor`. **Activity:** `no activity yet`.
- ✓ **Top bar:** `blank  blank.spektr`, `rules tracelabs   findings 0   ● 0 running · 0 queued   chain ✓ 1`; no review or checklist badge.
- ✓ **Review:** `[pending 0]  accepted 0  rejected 0  later 0`, `nothing pending`, and the explanation of what waits there.
- ✓ **Checklist:** `no runs yet · p on an entity runs a playbook`; `no checklist items · playbooks add them for manual steps`.

## Bugs found and fixed

| # | Frame | Expected | Was | Fix |
|---|---|---|---|---|
| 1 | `findings-detail-failing` | Accent border only on the focused pane | Detail pane always accent | `d015a2f` |
| 2 | `findings-detail-failing` | No repeated title | The automatic title repeated the claim | `d015a2f` |
| 3 | `finding-new-dialog` | The typed end of a long value visible | The start shown, the end cut | `d015a2f` (`text::tail`, tested) |
| 4 | `palette-export-md` | `:export` runs | Listed but disabled | `d015a2f` |
| 5 | `rate-limited-retrying` | `✕ … retry in 3s · 2 attempts left` | `⊘ queued`, no error | `d015a2f` |

Found while writing the tutor's e2e test, fixed in `a417184`:

- `/` extended the previous filter instead of starting a new search, so the second `/ jmarlow-wav` in a session matched nothing.
- After a pivot, the ego cursor tried to keep its row from the previous focus, landing on an unrelated entity.

## The stranger's-eye pass

I don't have a clean machine. Instead I followed README §Install → "From source" into an empty `--root`, with an empty `HOME` and no `SPEKTR_*` variables, then ran `spektr tutor` in a real PTY (Python `pty`, 170×48, answering the terminal's startup queries as a terminal does) under `sandbox-exec` with `(deny network*)`. I sent each step's own keys from the lesson script. Output:

```
$ cargo install -q --path crates/spektr --root <scratch>/root --locked
$ env -i HOME=<scratch>/home PATH=<scratch>/root/bin:/usr/bin:/bin TERM=xterm-256color spektr --version
spektr 0.1.0
$ … spektr doctor
Doctor
✓ config       ~/.config/spektr/config.toml · not created; using defaults
✓ state        ~/.local/state/spektr
✓ resolver     <this machine's resolver> · 1.1.1.1
✓ github key   GITHUB_TOKEN not set (optional)
! hibp key     HIBP_KEY not set · module off
✓ chrome       /Applications/Google Chrome.app/Contents/MacOS/Google Chrome
✓ python3      /usr/bin/python3
✓ terminal     256-color · TERM=xterm-256color
✓ images       auto · not a terminal, not queried
$ sandbox-exec -p '(version 1)(allow default)(deny network*)' spektr tutor   # driven through a PTY
steps sent: 20
Tutor complete: True
$ cat <scratch>/home/.local/state/spektr/tutor.toml
# spektr tutor progress
lesson = 10
step = 0
```

What this shows, and what it doesn't:

- ✓ The README's from-source install works as written, and the binary runs with no environment of its own.
- ✓ The whole tutor runs with the network denied at the OS level, outside test mode, and saves its progress.
- ✗ `brew install` and `cargo install spektr` can't be tried until the first release is published (Jay's step).
- ✗ A first-time reader's judgement of the README's clarity. Reading it cold, I fixed one wrong example (`[keys]` named a non-existent action, `pivot`; it's now the tested `quit = ["Q"]`) and checked every command it names against `--help`.
- One problem turned up: on a terminal that never answers the DA1 query, the first keystroke is lost (Known issues, PHASE_5_COMPLETE).

## Real terminal (Kitty / iTerm2)

Unchanged from Phase 4: the image protocols need a real terminal emulator, and `cargo xtask qa --real-terminal` stops at Ghostty's "Allow /bin/sh" prompt, which is Jay's call. The steps are in PHASE_4_QA § Real terminal.
