# Phase 4 complete: the full investigation loop

Branch `phase-4-loop` · 2026-10-20 · QA report: [`PHASE_4_QA.md`](PHASE_4_QA.md)

## What shipped

### Modules (`spektr-modules`, MODULES §6)
- **The 15 remaining built-ins**, all passive and all with fictional replay fixtures:
  - bluesky, gitlab, mastodon, hackernews
  - gravatar, email-check, hibp
  - phone-parse, exif, avatar-hash
  - http-fingerprint, wayback, rdns, rdap, crtsh
- **Media helpers:**
  - dHash (image_hasher, gradient 8×8) for `avatar_phash`
  - MurmurHash3 for `favicon_mmh3`
  - EXIF via nom-exif
- **Capture service:** one GET through `net`, so it records and replays like a module. Screenshots come from a headless Chrome or Chromium subprocess at 1280×2400, found in the data dir, then `PATH`, then the standard install paths. Replay never starts a browser.
- **Fixtures:**
  - the tutor's infrastructure (`mail.example`)
  - the `kestrel.example` set
  - generated binary assets (avatars, a JPEG with EXIF), whose dHash distances and EXIF are checked by a test
  - a fixture may give a readable `body`
  - two fixtures that answer one request differently are an error

### Engine
- **The remaining correlators:**
  - `avatar_phash`: dHash within 6 of 64
  - `analytics_id`
  - `favicon_mmh3`: on 20 hosts or fewer
  - `registrant_email`
  - `cert_san`: across registrable domains
  - `same_person` candidates into the review queue
- **Playbooks (PLAYBOOKS):**
  - the format, its file-only checks and `schemas/playbook.v1.json` (in spektr-protocol)
  - checks against the installed modules, launchers and playbooks
  - the six built-ins, with PLAYBOOKS §6's exact content
  - `<config>/playbooks/` overrides by name
- **The runner:**
  - ordered `run` / `playbook` / `check` steps
  - selectors: `seed`; `from` run, seed-links or case; `min_strength`; `max`, capped at 200
  - run membership over produced, cached and joined observations
  - one module per entity per run; one top-level run at a time
  - stop, and resume on open
  - the `playbook_run` and `checklist_item` tables
- **Launchers:** the nine built-ins (MODULES §8) plus `<config>/launchers.toml`. `{value}` is URL-encoded; `{url}` is a url's key, an image's `source_url` or an account's `profile_url`. Opening a launcher writes a `launcher.open` audit row.
- **Evidence:**
  - capture: an HTML blob, plus a screenshot blob with its hash in the summary
  - attachments: an image also becomes an `image` entity (`posted` from an account, `depicts` to a person), and exif runs on it
  - review decisions: accepting an unsure sweep hit adds the account, with the sweep's response and the analyst's decision as evidence
- **Actions:** StartPlaybook, StopPlaybook, ResolveItem, ItemNote, OpenLauncher, Reload, DecideReview, Capture, Attach.
- **Queries:** Playbooks, Runs, Checklist, Launchers, Raw, Blob, Edge. The inspector's detail now carries open items, launchers and the preview image.
- **The ego view** includes correlations among the focus's neighbours as `≈` groups (artboard 08).

### CLI
- **`spektr playbook`:** `list | info <name> | check <file> | new <name> --seed <type>`. `check` exits 1 on errors.
- **`spektr run --playbook <name>`:** starts the playbook on the first seed, then prints its steps, its nested runs and the open checklist items. `--json` adds `playbook_run` and `checklist_item` lines.
- **`spektr capture <case> <url> [--entity …] [--no-screenshot]`** and **`spektr attach <case> <path> [--entity …] [--note …]`**.
- **`spektr setup capture`:** downloads Chrome for Testing's headless shell into the data dir, using the system `curl` and `unzip`. It refuses to run in test mode.
- **`spektr doctor`:** reports the image protocol, queried from the terminal.

### TUI
- **Checklist screen (`C`, artboard 10):**
  - runs, with nested runs indented, plus the selected run's steps and result counts
  - the available playbooks with their seed type and source
  - items grouped by step, with `open / done / all` tabs (`[` `]` or a click)
  - the selected item's detail and launchers
  - keys: `d`, `s` and `n` (dialogs), `o` (a launcher, a menu of them, or reveal a local-only image), `↵` (the entity, or Review), `p` (a playbook on the run's seed), `X` (stop, asks first), `tab` (pane)
- **Review screen (`R`, artboard 05's layout):**
  - the `sweep_unsure` and `same_person` queue, with filter tabs
  - `a` accepts; for same-person it opens the merge confirmation. `x` rejects, `s` defers, `O` opens.
  - the detail, with a side-by-side field comparison and both avatars
  - `u` opens it on one username's hits
- **Timeline tab (`2`):** every event attribute, the warn `── last seen … ──` divider, `▲` on later activity. `e` filters by kind, and `↵` pivots.
- **Path tab (`3` or `:path`):** a fuzzy picker, then the shortest path within 6 hops, drawn vertically with predicates and strength. Otherwise "No path within 6 hops".
- **Inspector:**
  - an image preview at the top (an image, an account's avatar, a person's depicting image)
  - the edge comparison on a `≈` row: both avatars side by side with `dHash distance N/64`, or the shared value with each side's sources; then strength, confirmation and evidence. `v` confirms, `x` rejects.
  - a Checklist section listing open items
- **Images (TUI_SPEC §9):**
  - `ratatui-image`, with the protocol picked once at startup (before raw mode)
  - decode, resize and encode run off the render path; a 64-entry cache keyed by (sha256, size)
  - `loading image…` while encoding; `▨ name · preview unavailable` on failure
  - `images = auto | kitty | sixel | iterm2 | halfblocks | off`, and `:images` to switch
- **Context menu:**
  - `Run playbook ▸`
  - `Open in browser ▸`: the entity's URL, an account's profile page, an image's source, then the launchers
  - `Open raw response`
- **Palette:** `path`, `capture`, `attach`, `open`, `playbook`, `playbook stop`, `checklist`, `reload`, `images`.
- **Keys:** `c` captures; `o` opens evidence (the pager for a raw response, the viewer for a screenshot or an image).
- **Top bar and rail:** `checklist N open` in the top bar; the rail shows `▸ playbook <name> · step N/M`.
- **External opens** (browser, viewer, file manager, `$PAGER`) go through the host. Test mode appends them to `<state>/opened.jsonl` and the rail shows `↗ would open …` (ARCHITECTURE §8).

### Tests and QA
- **New end-to-end tests:**
  - `e2e_j2_identity`, `e2e_j3_rules`, `e2e_j7_playbooks`
  - `e2e_tmux` (CI)
  - `e2e_test_keys`
- **`e2e_keyboard`** now covers the Checklist and Review bindings: 89 of 98 bindings; Findings is Phase 5.
- **Engine-level playbook tests:** every built-in playbook runs end to end against fixtures (`crates/spektr-modules/tests/playbooks.rs`).
- **Snapshots:** the Checklist, Review, timeline, path and edge-comparison screens at 170×48 and 120×36.
- **QA:** five Phase 4 scenarios, 31 frames reviewed, 10 bugs found and fixed (PHASE_4_QA).
- **`cargo xtask qa --live`** now covers the Phase 4 domain modules and capture.
- **`cargo xtask qa --real-terminal`** sets `images = "auto"` and uses `SPEKTR_TEST_KEYS` to screenshot the avatar preview and the edge comparison.

## Exit criteria

| Criterion | Status | Evidence |
|---|---|---|
| J2 steps 1–6 work against fixtures, end to end | ✓ | `e2e_j2_identity`: <br>• the seed <br>• accounts streaming into the ego view <br>• `≈ same_avatar ↔ 3` <br>• the cursor on a `≈` row shows the avatars and `dHash distance` <br>• `v` → `confirmed by the analyst` <br>• github's `▲ after last-seen date` in the inspector, and its `last active` row after the timeline's divider <br>It also covers step 7's capture. The audit has one `edge.confirm` row and one capture observation. |
| `e2e_j2_identity` and `e2e_j3_rules` pass | ✓ | Output below. In `e2e_j3_rules`, a user-installed `probing` exec module (whose command would touch a marker file) is blocked: automatically, and when run from the palette. The rail shows `⊘ rules  email-account-probe blocked · tracelabs forbids probing`, the audit rows carry the reason, and the marker file never appears. |
| `e2e_j7_playbooks` passes for J7 steps 1–4, 6 and 7 | ✓ | **1** Run playbook ▸ tracelabs-kickoff on ◎ Jordan Marlow. <br>**2** The nested person-from-username and person-from-email runs are indented under the parent; the audit has 3 `playbook.start` rows. <br>**3** At least 3 avatar items, plus the category items. <br>**4** `o` → Google Lens: `opened.jsonl` records the lens URL, and the audit has one `launcher.open` row. <br>**6** A user playbook appears in Run playbook ▸ after `:reload`. <br>**7** `playbook check` exits 1 on `unknown module`. <br>Step 5's "done with a note" also passes (`checklist.resolve` carries the note); the "finding from an item" half waits for Phase 5. |
| Every built-in playbook passes `spektr playbook check` and runs end to end against fixtures | ✓ | Six `✓` lines below; `playbooks.rs` runs all six against fixtures with no unmatched requests. |
| Images render on Kitty-protocol and iTerm2 terminals and fall back to halfblocks elsewhere | ⚠ halfblocks ✓; Kitty/iTerm2: steps for Jay | Halfblocks are verified headlessly: frames in PHASE_4_QA, and `images::tests`. The real protocols need a terminal emulator. `cargo xtask qa --real-terminal` is ready but stops at Ghostty's "Allow /bin/sh" prompt, which is Jay's call. The manual steps are in PHASE_4_QA § Real terminal, and step 5 below. |
| The tmux behavior is tested and documented | ✓ (CI) | `e2e_tmux` drives spektr in `tmux new-session -d` with `send-keys`, reads `capture-pane -e -p`, and checks the doctor's `tmux set -g mouse on` warning. tmux isn't installed on this Mac, so it skips here. CI installs it and sets `SPEKTR_REQUIRE_TMUX=1`; see the CI run below. What a user needs to know is under "tmux" below. |

### tmux, for users
- **Every binding works inside tmux:** no chord needs the Kitty keyboard protocol (TUI_SPEC §6).
- **Mouse:** `set -g mouse on` is needed for clicks and the wheel. The doctor warns when it's off.
- **Truecolor:** add `set -as terminal-features ',*:RGB'`. Without it, tmux downgrades colors, and spektr's 256-color fallback applies.
- **Images:** `ratatui-image` detects tmux and wraps its sequences. Kitty and iTerm2 previews also need `set -g allow-passthrough on`. Without it, use `images = "halfblocks"`.

## Deviations from the spec, and why

1. **The ego view shows the neighbours' correlations.** Artboard 08 shows `≈ same_avatar ↔ 3` on the username's ego, but the `shares_artifact` edges join the accounts to each other, not to the username. `Reader::ego` now adds a `≈` group per artifact kind among the focus's neighbours, one row per neighbour acting on its account–account edges. The focus's degree doesn't count them, and it's skipped above 300 neighbours (hubs). No schema change.
2. **A seed in a case with a subject is linked to the subject** with an analyst observation (`uses_handle`, `has_email`, `has_phone`, `has_account`). J7 starts from "◎ Jordan Marlow (with a known handle and email)", and `from = "seed-links"` needs those links.
3. **An entity that gains a new attribute name gets its automatic modules planned again**, quietly: no cached/blocked rows. That way avatar-hash runs once `avatar_url` arrives.
4. **The `produces` check is relaxed:** an entity type may be emitted when it's a valid source of a declared predicate (gravatar's account behind `has_email`).
5. **github tries the one GitHub-legal variant of a handle** GitHub can't have (dots → hyphens: `jmarlow.wav` → `jmarlow-wav`). It records `matched_variant`.
6. **The playbook format lives in spektr-protocol** with a JSON Schema, like the manifest, so `playbook check` and third-party tooling share one definition. The engine adds the checks that need the installed modules.
7. **`CaptureService` returns what it fetched (`Captured`), and the engine stores it.** The service no longer writes to the store, keeping the "modules never write to the store" rule.
8. **Hosts under a known platform domain are hubs** (e.g. `web.archive.org` under `archive.org`), so playbook selection skips them.
9. **An expiry date is never marked `▲ after last seen`.** TUI_SPEC §4.3 marks every row after the divider, but `expires_at` is set at registration and isn't activity. Flagging it would invite a wrong "Advancing the timeline" finding. It stays on the timeline, unmarked.
10. **No new default key bindings.**
    - The Checklist's filter tabs cycle with `[` / `]` (Navigation's page keys) or a click.
    - The Review filter cycles with `tab`/`shift-tab` (Global's pane keys), `[` / `]`, or a click.
    - The keymap gained only hint labels for the Checklist and Review bindings.
11. **The Review screen's columns are `kind / site / link / reason`.** Artboard 05 has `tier / rule / link / conf / obs`, but v1 has no inferred tiers and no numeric confidence (design README).
12. **The Checklist's step results are job counts** (`5 jobs`, `2/4 jobs`, `4 items`, `1 run`), not the artboard's `7 found` / `3 accounts`: the runner doesn't track entities per step.
13. **The launcher's doctor panel shows the startup detection, and only `spektr doctor` queries the terminal.** A query from the TUI's background doctor stole the TUI's input (found by `e2e_launcher`).
14. **`:images` switches protocol without querying again:** it uses the font size from startup.
15. **The Checklist's `f` (finding from an item) shows "not available yet"** until Findings exist (Phase 5, DISPATCH).
16. **`SPEKTR_TEST_KEYS` is a new test-mode variable** (ARCHITECTURE §8 updated). It types keys after the case opens, so the real-terminal QA can reach the image previews.
17. **`spektr setup capture` shells out to `curl` and `unzip`.** That avoids adding an HTTP client and a zip crate to the binary for a one-off download.

## Known issues

- **The Kitty and iTerm2 screenshots are Jay's step:** see the exit criteria and step 5 below.
- **The Checklist's running state can't be caught live in replay.** Runs finish before the first frame. The snapshot test renders it (`▸ … step 4/6`, `2/4 jobs`).
- **No `same_person` item appears in the fixture cases:** they have only one person. The comparison view is covered by the snapshot fixture.
- **crt.sh returned HTTP 502 during the live run.** A direct request returned 502 too, so it's upstream. The module reports the upstream error.
- **wayback is manual-only,** so `qa --live` can't plan it: `spektr run` runs only automatic modules.
- **Toasts don't expire under `SPEKTR_CLOCK`,** because the clock is frozen. Real use is unaffected.
- **The harness's PNG font draws `⌖` (location) a cell wide** ("locatio" looks clipped). The TUI measures with `unicode-width`, and terminals draw it in one cell.

## Command output

```
$ cargo fmt --check && echo "fmt ok"
fmt ok
$ cargo clippy --workspace --all-targets --all-features -q -- -D warnings
(clean)
$ cargo nextest run --workspace
     Summary [   9.353s] 194 tests run: 194 passed, 2 skipped
$ cargo xtask deps-check
deps-check: ok (8 members, 490 resolved packages; default and all features)
$ cargo xtask schemas --check
schemas: up to date
```

(The skipped tests are the `qa` scenario runner, run by `cargo xtask qa`, and `perf_tui`, which is release-only.)

The Phase 4 end-to-end tests:

```
        PASS [   0.033s] (1/6) spektr::e2e_tmux spektr_inside_tmux          (skipped inside: tmux isn't installed here; CI requires it)
        PASS [   2.229s] (2/6) spektr::e2e_j3_rules probing_module_never_runs_under_tracelabs
        PASS [   3.132s] (3/6) spektr::e2e_j2_identity j2_identity_case
        PASS [   3.693s] (4/6) spektr::e2e_j7_playbooks j7_playbooks
        PASS [   4.512s] (5/6) spektr::e2e_test_keys the_key_script_is_typed_after_the_case_opens
        PASS [   9.188s] (6/6) spektr::e2e_keyboard every_registry_action_changes_the_screen
     Summary [   9.189s] 6 tests run: 6 passed, 0 skipped
89 bindings exercised, 9 skipped (Findings is Phase 5: ["finding-new", "finding-edit", "finding-edit-external", "finding-attach", "finding-ready", "finding-sent", "finding-export", "finding-go", "finding-delete"])
```

Every built-in playbook, against fixtures and with `playbook check`:

```
        PASS [   0.051s] (1/6) spektr-modules::playbooks every_builtin_playbook_checks_clean
        PASS [   0.210s] (2/6) spektr-modules::playbooks domain_recon_finds_the_infrastructure_and_correlates_it
        PASS [   0.244s] (3/6) spektr-modules::playbooks stop_cancels_the_rest
        PASS [   0.382s] (4/6) spektr-modules::playbooks person_from_username_runs_every_step
        PASS [   0.394s] (5/6) spektr-modules::playbooks tracelabs_kickoff_nests_the_person_playbooks
        PASS [   0.454s] (6/6) spektr-modules::playbooks image_triage_and_person_from_phone_run
     Summary [   0.455s] 6 tests run: 6 passed, 0 skipped

$ spektr playbook check domain-recon.toml
✓ domain-recon 1.0.0 · 5 steps
$ spektr playbook check image-triage.toml
✓ image-triage 1.0.0 · 3 steps
$ spektr playbook check person-from-email.toml
✓ person-from-email 1.0.0 · 5 steps
$ spektr playbook check person-from-phone.toml
✓ person-from-phone 1.0.0 · 3 steps
$ spektr playbook check person-from-username.toml
✓ person-from-username 1.0.0 · 6 steps
$ spektr playbook check tracelabs-kickoff.toml
✓ tracelabs-kickoff 1.0.0 · 7 steps
$ spektr playbook list
domain-recon           domain         built-in
image-triage           image          built-in
person-from-email      email          built-in
person-from-phone      phone          built-in
person-from-username   username       built-in
tracelabs-kickoff      person         built-in · tracelabs
```

`spektr run --playbook` on the fixtures:

```
spektr run · tl · rules tracelabs · depth 2
  jobs    done 62 · cached 17 · blocked 0 · failed 0
  graph   58 entities · 75 edges · 94 observations
  review  5 pending
  playbook person-from-username on ~ jmarlow.wav · done
    ✓ 1 Sweep the handle             on seed                 done 1
    ✓ 2 Profile details              on seed                 cached 4 · done 1
    ✓ 3 Avatar hashes                on account from run     cached 4 · done 4
    ✓ 4 Linked sites                 on url from run, max 25 cached 9 · done 9
    ✓ 5 Reverse image search         on image from run       4 items
    ✓ 6 Review unsure sweep hits     on —                    1 items
  checklist  5 open
  chain   ✓ 739 rows
```

Input-to-render latency on the 2,000-entity case (`cargo test --release -p spektr --test perf_tui -- --ignored --nocapture`, M1 Pro). The ego query now includes the neighbour clusters:

```
↵ (pivot: ego + detail queries)    p50   0.85 ms  p95   0.95 ms  max   2.08 ms
⌫ (back: ego query)                p50   0.84 ms  p95   0.87 ms  max   3.94 ms
all                                p50   0.81 ms  p95   3.94 ms  max   4.62 ms  (320 samples)
```

The live smoke run (example.com only) is in PHASE_4_QA.

## CI

Run [39419080316](https://github.com/infktd/spektr/actions/runs/39419080316) on PR #5: **success**.

| Job | Result | nextest |
|---|---|---|
| lint (x86_64-unknown-linux-gnu): fmt, clippy, deps-check, schemas, cargo-deny | ✓ | — |
| test (aarch64-apple-darwin) | ✓ | 194 tests run: 194 passed, 2 skipped (26.7 s) |
| test (x86_64-apple-darwin) | ✓ | 194 tests run: 194 passed, 2 skipped (25.8 s) |
| test (x86_64-unknown-linux-gnu) | ✓ | 194 tests run: 194 passed, 2 skipped (14.1 s) |
| test (aarch64-unknown-linux-gnu) | ✓ | 194 tests run: 194 passed, 2 skipped (16.4 s) |

`e2e_tmux` ran for real on every target (`SPEKTR_REQUIRE_TMUX=1`): 4.0 s on arm64 macOS, 4.7 s on Intel macOS, 4.7 s on x86_64 Linux, 3.2 s on arm64 Linux.

Two earlier runs on this PR failed, both on test timing:

- **Run 39417608824, `e2e_tmux` on all targets:** `send-keys Escape C` reads as Alt-C, so Escape is now sent on its own.
- **Run 39417608824, `e2e_j3_rules` on Intel macOS:** the blocked row scrolled out of the rail before a poll saw it. The test now scrolls back to it.
- **Run 39418080974, `e2e_tmux` on arm64 macOS:** that runner's tmux defaults the mouse on, so the doctor rightly reported `mouse on`. The test now sets `mouse off` itself.

## How Jay can test it

1. **J7 in the TUI:**
   ```
   spektr new tl --profile tracelabs --subject "Jordan Marlow" --last-seen 2026-09-28
   spektr run tl.spektr --seed username:<a handle you own> --seed email:<an address you own>
   spektr open tl.spektr
   ```
   Then `/` Jordan `↵`, `p`, `↵` to start tracelabs-kickoff, and `C`: the nested runs are indented under it. `tab` to the items, `o` on an avatar item opens Google Lens in your browser, and `d` marks it done with a note.
2. **J2's edge comparison:** in the same case, press `h h j j` from the username's ego to put the cursor on a `≈ same_avatar` row. The inspector shows both avatars and the dHash distance; press `v` to confirm. Then `2` for the timeline: activity after last seen carries `▲`.
3. **Playbooks from the CLI:**
   - `spektr playbook list` and `spektr playbook info person-from-email`
   - `spektr playbook new forum-sweep --seed username`, then `spektr playbook check ~/.config/spektr/playbooks/forum-sweep.toml`
   - change `run = ["dns"]` to `run = ["nope"]`, and the check fails with exit code 1
   - in the TUI, `:reload` makes the new playbook appear under Run playbook ▸
4. **Capture:** `spektr setup capture` (or have Chrome installed), then `spektr capture tl.spektr https://example.com/`. It prints the HTML and PNG hashes. In the TUI, `o` on the capture row opens the screenshot.
5. **Images in a real terminal:** `cargo xtask qa --real-terminal`; click **Allow** if Ghostty asks, and grant Screen Recording if `screencapture` fails. Check `qa/out/real-terminal/ghostty-avatar-preview.png` and `ghostty-edge-compare.png`: the avatars should be sharp (Kitty protocol), not blocks. In iTerm2, `spektr doctor` should say `images  iterm2`; in Terminal.app, `halfblocks`.
6. **tmux:** inside tmux with `set -g mouse off`, `spektr doctor` warns `mouse off · run tmux set -g mouse on`.
