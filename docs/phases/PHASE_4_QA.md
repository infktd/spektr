# Phase 4 QA: the full investigation loop

Branch `phase-4-loop` · 2026-10-20 · `cargo xtask qa`, then `cargo xtask qa-shots`. I opened every frame below and read it against its artboard before answering its checklist.

The Phase 2 and Phase 3 scenarios still run and still pass. Phase 4 adds five scenarios with 31 frames: `p4-checklist`, `p4-review`, `p4-tabs`, `p4-inspector` and `p4-menu-capture`. The frames copied here are in `docs/phases/qa/phase-4/`. The rest are in `qa/out/` after a run.

One frame is deliberately not copied: `p4-menu-capture/launcher-doctor-images`. The launcher's doctor panel shows the machine's own resolver address and temp paths.

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
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 59.00s
77 frames, 10 artboards
```

All data comes from the fixture replay: the fictional `jmarlow.wav` handle, `.example` domains and documentation IP ranges. The harness renders images as halfblocks, because test mode defaults `images` to `halfblocks`. The Kitty and iTerm2 protocols need a real terminal (§ Real terminal, below).

## p4-checklist: tracelabs-kickoff with nested runs (artboard 10)

| Frame | Artboard 10 |
|---|---|
| ![](qa/phase-4/p4-checklist-checklist-done.png) | ![](qa/phase-4/artboard-Checklist.png) |

The run is the real fixture case. `tracelabs-kickoff` was started from the subject through the context menu (below). It ran `person-from-username` and `person-from-email` as nested runs.

- ✓ **Top bar:** `tl-kickoff  playbooks` (muted). On the right:
  - `rules tracelabs   findings 0   review 5` (warn)
  - `checklist 11 open   ● 0 running · 0 queued   chain ✓ 860`
- ✓ **Runs pane, `Playbooks 3 runs`:** `✓ tracelabs-kickoff  ◎ Jordan Marlow  done 7/7`, with the two nested runs indented by 2 under it. `done N/N` is muted. The running state (`▸`, `step N/M` in accent) is in the snapshot test (`snapshots__checklist_170x48.snap`): replay finishes the run before the first frame is taken.
- ✓ **`Steps · tracelabs-kickoff`:** a faint rule; each step's `✓ n  playbook person-from-username   on 1 ~ username   1 run`, then the four `check …  on seed  1 item` steps. Step 3, `playbook person-from-phone  on + phone  0 runs`, is muted (no phone was known).
- ✓ **Available:** the six built-ins, each with its seed glyph and type, `built-in` and `· tracelabs` for the kickoff.
- ✓ **Rules note** at the bottom of the left pane: "Rules still apply inside a playbook: the tracelabs profile blocks anything that isn't passive."
- ✓ **Right pane:**
  - `Checklist 11 open` in its title, with `[open]  done  all` in the border
  - items grouped under a bold step title, followed by a muted `playbook · step N`
- ✓ **Item rows** (next frame):
  - `○ ▨ avatar · github · jmarlow-wav   lens · yandex · tineye`, matching the artboard's wording
  - the review item shows `○ ~ jmarlow.wav  opens Review`, its run's seed (the step has no `on`)
- ✓ **Selected block:** the item text, then `o open ▸   Google Lens · Yandex Images · TinEye` and `Opening a launcher is recorded in the audit log.`
- ✓ **Footer:** `d done   s skip   n note   f finding from item   ↵ go to entity`.
- ✓ **Status line:** `CHECKLIST` pill (accent), then `tab pane   p run playbook   X stop run   esc back to graph`.

| run playbook ▸ (on ◎ Jordan Marlow) | items pane, an avatar item |
|---|---|
| ![](qa/phase-4/p4-checklist-menu-run-playbook.png) | ![](qa/phase-4/p4-checklist-checklist-items.png) |

| `o`: launcher menu | `d`: done dialog | `]`: the done tab |
|---|---|---|
| ![](qa/phase-4/p4-checklist-checklist-launcher-menu.png) | ![](qa/phase-4/p4-checklist-checklist-done-dialog.png) | ![](qa/phase-4/p4-checklist-checklist-done-tab.png) |

- ✓ `o` on an avatar item opens a menu of its three launchers, with the item's entity as the header.
- ✓ `d` opens `Mark done` with the item text and an optional `note` field (COMMAND pill).
- ✓ After `matched ↵`, the `[done]` tab shows `✓ ▨ avatar · … done 18:14 · note`, and the Selected block shows `note  matched`.

## p4-review: the unsure sweep hits (artboard 05 layout)

| Frame | Artboard 05 |
|---|---|
| ![](qa/phase-4/p4-review-review-queue.png) | ![](qa/phase-4/artboard-Correlations.png) |

- ✓ **Top bar:** `demo  review`, with `review 5` in warn.
- ✓ **Queue pane:** `Review` in accent, with `[pending 5]  accepted 0  rejected 0  later 0` in the border. Its column headers are `kind / site / link`.
- ✓ **Rows:** `sweep  Facebook  ~ jmarlow.wav  →  ◐ facebook · jmarlow.wav  login wall`. The reason is muted and truncated with `…`. The cursor row has the `▌` and the selection background.
- ✓ **Detail pane:**
  - `account ◐ facebook · jmarlow.wav`, with `account` in warn and the rest bold
  - the muted line `unsure sweep hit · Facebook · login wall`
  - the fields, then the accept note
  - footer `a accept   x reject   s later   O open`
- ✓ **Status line:** `REVIEW` pill, then the screen's hints and `esc back to graph`.

| `a` accepted one | empty filter | back to the graph |
|---|---|---|
| ![](qa/phase-4/p4-review-review-accepted-one.png) | ![](qa/phase-4/p4-review-review-empty-rejected.png) | ![](qa/phase-4/p4-review-back-to-graph.png) |

- ✓ After `a`: `[pending 4]  accepted 1`, the toast `accepted · okru · jmarlow.wav added`, and `review 4` in the top bar.
- ✓ Back in the graph, `◐ okru  jmarlow.wav •` is the strongest has_account row, `▰▰▱` (the sweep's response plus the analyst's decision). The cursor stayed on `aboutme`, the row it was on.
- ✓ An empty filter reads `nothing rejected`; the detail pane reads `nothing selected`.

The `same_person` detail (a side-by-side field table with `=` `≠` `·`, and both avatars when both people have one) is covered by the snapshot test. The fixtures have no second person, so no same-person item is produced live.

## p4-tabs: timeline and path

| timeline | `e`: one kind |
|---|---|
| ![](qa/phase-4/p4-tabs-timeline.png) | ![](qa/phase-4/p4-tabs-timeline-kind.png) |

- ✓ **Tab labels** in the graph border: `1 ego  [2 timeline]  3 path`, with the active tab in accent.
- ✓ **Header:** `10 of 10 events   e kind: all`. Each row is `YYYY-MM-DD HH:MM  ◐ github · jmarlow-wav  created at  github`.
- ✓ **The divider:** a full-width warn rule reading `── last seen 2026-09-28 ──`. The two `last active` rows after it carry `▲` (warn).
- ✓ **`e`:** filters to one kind, with the header reading `4 of 10 events   e kind: created at`. The inspector follows the timeline cursor: the github account, with its avatar preview.

| path picker (`3`) | path |
|---|---|
| ![](qa/phase-4/p4-tabs-path-picker.png) | ![](qa/phase-4/p4-tabs-path.png) |

- ✓ `3` with no target opens `Path to…` with a find field and entity picks.
- ✓ The path view shows `path ~ jmarlow.wav → @ hello@jmarlow.example`. The entities are stacked, with `│ → has_account ▰▱▱`, `│ → links_to`, `│ → has_email` between them, and the cursor on the first row. The inspector follows the path cursor.

## p4-inspector: image preview, edge comparison, confirm (J2 step 5)

| ego with the correlation | Artboard 08 |
|---|---|
| ![](qa/phase-4/p4-inspector-ego-with-correlation.png) | ![](qa/phase-4/artboard-Identity.png) |

- ✓ The username's ego shows `≈ same_avatar ↔ 3` in accent, listing bluesky, github and gitlab, with the footer `avatar hash match · 3 sources`. The card shows `corr 3`. This is the artboard's arrangement.

| cursor on a ≈ row | after `v` |
|---|---|
| ![](qa/phase-4/p4-inspector-edge-compare.png) | ![](qa/phase-4/p4-inspector-edge-confirmed.png) |

- ✓ **Edge comparison:** `≈ same_avatar`, then `◐ github · jmarlow-wav` and `↔ ◐ bluesky · jmarlow.wav.bsky.social`. Under them are the two avatars side by side (halfblocks here) and `dHash distance 0/64`.
- ✓ **Strength and evidence:** `▰▱▱ single · 1 source`, then `Evidence 1` (`correlator:avatar_phash`), with the footer `v confirm   x reject`.
- ✓ **After `v`:** `▰▰▰ confirmed · 1 source`, and `confirmed by the analyst 2026-10-20 18:14` in ok.

| image entity | an account with an avatar |
|---|---|
| ![](qa/phase-4/p4-inspector-image-preview.png) | ![](qa/phase-4/p4-inspector-account-avatar.png) |

- ✓ **An image entity:** the preview sits at the top of the inspector, then `▨ jmarlow.png`, `image · sha256:…`, `source url`, `Evidence 2` (`exif`, `avatar-hash`), and a `Checklist` section with `○ Reverse image search  person-from-usern…`.
- ✓ **An account:** its avatar is previewed at the top, above the header and fields.

## p4-menu-capture: the Phase 4 menu entries, capture

| `space` on the username, `p` | `O` on github: Open in browser ▸ |
|---|---|
| ![](qa/phase-4/p4-menu-capture-submenu-playbooks.png) | ![](qa/phase-4/p4-menu-capture-submenu-launchers.png) |

- ✓ The menu's navigate section reads `Pivot here`, `Run module  r ▸`, `Run playbook  p ▸`, `Open in browser  O ▸`, `Copy value`. `Open raw response  o` is enabled.
- ✓ `Run playbook ▸` shows `Playbooks for ~ username`, with `person-from-username  6 steps`.
- ✓ `Open in browser ▸` on an account offers `Profile page  O`. No built-in launcher takes accounts (MODULES §8). The submenu flips to the left when there's no room on the right (TUI_SPEC §7).

| `c` on a url | captured |
|---|---|
| ![](qa/phase-4/p4-menu-capture-capture-dialog.png) | ![](qa/phase-4/p4-menu-capture-captured.png) |

- ✓ **Capture dialog:** prefilled with the URL, and `Saved as evidence for ↗ https://jmarlow.example.`.
- ✓ **After `↵`:** the toast says `captured`, and the url's evidence gains a `capture  0.1.0` row. Replay takes no screenshot; a live capture does (below).

## Bugs found and fixed

| Frame | Expected | Actual | Fix |
|---|---|---|---|
| `p4-tabs/timeline` | one row per event | every event twice, once per observation | `8810434`: rows deduplicated by (entity, attribute, time) |
| `p4-tabs/timeline` | ▲ only on activity after last seen | `expires at 2027…/2030…` marked ▲, in the timeline and the inspector | `8810434`: an expiry date is never "after last seen" (a deviation, see PHASE_4_COMPLETE) |
| `p4-tabs/path` | the inspector on the path cursor's entity | it kept the ego's entity until the cursor moved | `8810434` |
| `p4-inspector/edge-confirmed` | the cursor stays on the confirmed row | the confirmed row re-sorted to the top and the cursor's index pointed at another row | `8810434`: the ego cursor follows its row through a refresh |
| `p4-menu-capture/submenu-launchers` | `O` in the menu opens Open in browser ▸ | ran the plain action: "no URL to open" | `8810434`: a submenu item's hotkey opens its submenu |
| `p4-menu-capture/submenu-launchers` | an account's profile page | "no URL or launcher" | `8810434`: Open in browser ▸ and `O` use a url's own URL, an account's profile or an image's source |
| `p4-checklist/checklist-items` | `avatar · github · jmarlow-wav` (artboard 10) | `1001.png` | `8810434`: items name an image by the account that posted it |
| `p4-review/review-accepted-one` | the account as displayed | `okru:jmarlow.wav` | `8810434` |
| `p4-tabs/timeline-kind` | `e kind: created at` | `created_at` | `b5abe37` |
| live run | http-fingerprint ok on example.com | `bad URL data:,` (example.com declares `<link rel="icon" href="data:,">`) | `0ddccf2`: a non-http(s) icon means no favicon |

## Live smoke run (`cargo xtask qa --live`)

`qa/live-targets.toml` doesn't exist, so the run covers example.com only.

```
# Live smoke run

Targets: domain:example.com (from the default: qa/live-targets.toml is missing or empty, so example.com only).

| Module | Target | Result | Latency | Detail | Response shape |
|---|---|---|---|---|---|
| dns | domain:example.com | ok | 86 ms | 4 addresses · 1 MX | matches fixtures (resolves_to ip) |
| rdap | domain:example.com | ok | 365 ms | 2336799_DOMAIN_COM-VRSN · 0 contacts | edges: {} (fixtures: registered_by) |
| crtsh | domain:example.com | failed | 7185 ms | upstream: HTTP 502 from crt.sh | n/a |
| http-fingerprint | domain:example.com | ok | 78 ms | Example Domain | edges: {} |
| capture | https://example.com/ | ok | 844 ms | captured https://example.com/ · HTTP 200 | png    9ff49038017e0025c461963b010d56d760efc6b0293f26bc4987de0a79be312a |
```

- **crtsh failed upstream.** crt.sh answered 502 to a direct request as well: `curl 'https://crt.sh/?q=example.com&output=json'` returned `502 0.537261s`. The module reports the upstream error, as designed.
- **rdap shape is fine.** It returns no registrant entity for example.com (IANA-reserved, no contacts), so the absence of `registered_by` isn't a shape change.
- **capture took a real screenshot,** through the system Chrome that the doctor found.
- **wayback isn't in the live plan.** It is manual-only (`auto = false`), and `spektr run` only plans automatic modules.

## Real terminal (Kitty / iTerm2 image protocols)

Not captured by me. Last time, `cargo xtask qa --real-terminal` stopped at Ghostty's "Allow /bin/sh" prompt, and answering it is Jay's call. The command now does three things:

- sets `images = "auto"`
- uses `SPEKTR_TEST_KEYS` to move to an avatar and then to the ≈ row
- saves `qa/out/real-terminal/ghostty-workspace.png`, `ghostty-avatar-preview.png` and `ghostty-edge-compare.png`

Manual steps for Jay:

1. **Ghostty (Kitty protocol):** run `cargo xtask qa --real-terminal` and click **Allow** if Ghostty asks. If `screencapture` fails, give the terminal Screen Recording permission. Check that `ghostty-avatar-preview.png` shows a sharp (not blocky) avatar at the top of the inspector, and that `ghostty-edge-compare.png` shows two avatars side by side above `dHash distance 0/64`.
2. **iTerm2:** from the repository root, open the case built by step 1:
   ```
   R=$PWD/qa/out/real-terminal; cd $R/work && HOME=$R/home XDG_CONFIG_HOME=$R/home/.config XDG_STATE_HOME=$R/home/.local/state XDG_DATA_HOME=$R/home/.local/share SPEKTR_TEST_MODE=1 SPEKTR_TRANSPORT=replay:$R/../../../crates/spektr-modules/fixtures $R/../../../target/debug/spektr open demo-missing-01.spektr
   ```
   Then press `j j` (github's avatar) and `h h j j` (the comparison). `spektr doctor` in iTerm2 should report `images  iterm2`.
3. **Fallback:** in Terminal.app, the doctor reports `images  halfblocks` (warn), and the previews are halfblocks.

## tmux

`e2e_tmux` runs spektr in a detached `tmux new-session` and drives it with `send-keys`. It checks:

- single keys, `gg`, the context menu, the Checklist screen, the tabs and the palette
- that `capture-pane -e -p` keeps the styles
- that `spektr doctor` inside tmux warns `mouse off · run tmux set -g mouse on`

tmux isn't installed on this Mac, so the test skips here. CI installs tmux and sets `SPEKTR_REQUIRE_TMUX=1`, so it runs there on all four targets (see PHASE_4_COMPLETE for the CI run).
