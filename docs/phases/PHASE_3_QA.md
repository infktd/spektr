# Phase 3 QA: TUI core

Branch `phase-3-tui` · 2026-10-20 · `cargo xtask qa`, then `cargo xtask qa-shots`. Every frame below was opened and read against its artboard before its checklist was answered.

The Phase 2 scenarios still run: `p2-errors`, `p2-new-and-run`, `p2-query` and `p2-verify-doctor`. Phase 3 adds seven scenarios with 34 frames. The frames copied here are in `docs/phases/qa/phase-3/`. The remaining ones are in `qa/out/` after a run.

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
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 29.26s
46 frames, 10 artboards
```

All data is the Phase 2 fixture case (fictional `jmarlow.wav` handle, replay transport), or a generated case for the 2,000-entity scenario.

## p3-workspace: the workspace on the Phase 2 fixture case (170×48)

| Frame | Artboard 08 |
|---|---|
| ![](qa/phase-3/p3-workspace-workspace.png) | ![](qa/phase-3/artboard-Identity.png) |

The frame is the real fixture case, so its data differs from the artboard's mock. Phase 2's sweep found 7 accounts and no avatar correlation, and the subject isn't linked to the handle. The layout, type and color treatment follow the artboard. The snapshot fixture (`crates/spektr-tui/tests/support`) reproduces the artboard's data: uses_handle, variant_of, the `≈ same_avatar` group, the confirmed github account and the after-last-seen field. Its 170×48 snapshot (`snapshots__workspace_170x48.snap`) has the artboard's structure: the same groups on the same sides, the same card, inspector sections and rail. The panes are adjacent rather than separated by the artboard's 8 px gaps.

- ✓ **Top bar:**
  - left: `spektr` (accent, bold) `│` `demo-missing-01  subject ◎ Jordan Marlow · last seen 2026-09-28`
  - right: `rules tracelabs   findings 0   review 5` (warn), `● 0 running · 0 queued   chain ✓ 112` (ok)
- ✓ **Entities pane:**
  - groups in spec order with `▾`/`▸` and counts
  - `▸` (accent) marks the focus `jmarlow.wav`; `subject`, `⌂` and `⊛ hub` (warn) are right-aligned
  - account, url and domain are collapsed: they have more than 5 members (TUI_SPEC §4.2)
- ✓ **Ego view:**
  - breadcrumb `◎ Jordan Marlow  ›  ~ jmarlow.wav`, earlier step muted
  - card centered, 20 wide, accent border: `~ username` muted, `jmarlow.wav` bold, `accts 7 / obs 13`, `⌂ seed`
  - outbound `→ 7 has_account` on the right, with the connector rule in faint
- ✓ **Group headers:** predicate muted, `→ 7` in fg, faint connector toward the card.
- ✓ **Strength bars:** three levels. Here every row is `▰▱▱` (muted, one source); green only for confirmed, which shows in the snapshot and the large case.
- ✓ **Cursor row:** `▌` accent bar on the `selection` background, in the focused graph pane.
- ✓ **Inspector:** follows the cursor (`aboutme · jmarlow.wav`). Header bold, type muted, `Evidence 1` with a faint rule, `Strength ▰▱▱ single`, degree line.
- ✓ **Activity rail:** the case's job history (`✓ username-sweep  done ← ~ jmarlow.wav`). The right column, behind a vertical rule, shows `running/queued/done/failed`, `rules tracelabs` and the profile summary wrapped onto two lines.
- ✓ **Status line:** `NORMAL` pill (accent), then hints generated from the registry, `: command   / filter   tab pane   F findings   space menu   ? help`, the same order as the artboard. `18:14:22 UTC` on the right.
- ✓ **Nothing overflows:** long values truncate with `…`, and no stray characters sit at pane borders. At 120×36 the top bar drops its lowest-priority items first.

![](qa/phase-3/p3-workspace-inspector-focused.png)

- ✓ `tab` from the graph focuses the inspector: accent border and title, and the `▌` bar on the evidence row. The graph loses its accent border.

## p3-modes: filter, pivot, back, focus, group expansion

| filter | pivot |
|---|---|
| ![](qa/phase-3/p3-modes-filter.png) | ![](qa/phase-3/p3-modes-pivot.png) |

- ✓ **Filter:**
  - `FILTER` pill (fg), `/ git` with the caret, `3 match`
  - matching groups expand and non-matching ones hide; `gitlab.com ⊛ hub` keeps its marker
  - status hints read `↑↓ select   ↵ accept   esc clear`
- ✓ **Pivot:**
  - `↵` on the bluesky row: the breadcrumb grows to `◎ Jordan Marlow › ~ jmarlow.wav › ◐ bluesky · jmarlow.wav.bsky.social`, earlier steps muted
  - the card re-centers: `◐ account`, `accts 1 / links 1 / obs 1`
  - `has_account ← 1` on the left, `→ 1 links_to` on the right
- ✓ **Back:** `⌫` returns to `jmarlow.wav`. Frame `back` is in `qa/out`.

![](qa/phase-3/p3-modes-group-expanded.png)

- ✓ **Group expansion:** `↵` on the collapsed `account` header shows 4 rows, then `··· 3 more`. The inspector keeps the last entity while the cursor rests on a header.

## p3-overlays: palette, menu, submenu, help, dialogs

| palette (`: run`) | artboard 03 |
|---|---|
| ![](qa/phase-3/p3-overlays-palette-run.png) | ![](qa/phase-3/artboard-Palette.png) |

- ✓ **Palette geometry:** 96×22 at 1/6 of the height, `bg_raised`, accent border, title `Command`. The workspace behind it is dimmed to muted and faint only.
- ✓ **Palette content:**
  - prompt `: run` with the caret, `target ◐ aboutme · jmarlow.wav` on the right
  - matches grouped: Commands (`run`, `rerun`, `rules`, `reject`, …), with disabled commands in muted and `not available yet` beside them
  - detail pane: `run`, then its description
  - footer `tab complete   ↑↓ select   ↵ run   esc close` and `14 of 33`
  - no Modules group here: no built-in module takes an `account`. The same frame on a username lists `run github` / `run username-sweep` with their signatures.
- ✓ **Mode:** the `COMMAND` pill is warn.

| menu (`space`) | submenu (`→`) | artboard 07 |
|---|---|---|
| ![](qa/phase-3/p3-overlays-menu.png) | ![](qa/phase-3/p3-overlays-submenu.png) | ![](qa/phase-3/artboard-ContextMenu.png) |

- ✓ **Menu:**
  - header `◐ aboutme · jmarlow.wav` bold, then muted `account · 1 observation`, then a rule
  - sections navigate / scope / analysis, separated by rules; hotkeys right-aligned in muted
  - disabled items are muted with their reason (`not available yet`, `no URL`)
  - anchored at the right end of the cursor row and clamped inside the terminal, with the `bg` shadow margin
- ✓ **Submenu:** opens beside the menu, flipping left here because there's no room on the right. It reads `Modules for ◐ account`, then `no module takes account` (disabled), a rule, and `Run all eligible  A`.
- ✓ **Status hints in the menu:** `↑↓ select   → open submenu   ← back   ↵ run   esc close`, with the pill still `NORMAL` (TUI_SPEC §7).

![](qa/phase-3/p3-overlays-menu-rightclick.png)

- ✓ **Right-click:** the menu opens at the click cell on the gitlab row. Its observation count arrives with the detail query.

![](qa/phase-3/p3-overlays-help.png)

- ✓ **Help:**
  - full screen, generated from the registry, grouped by context
  - the shift-drag note shows (mouse capture is on)
  - `/ merge` filters it to `m Merge with…`, `M Unmerge` and the Review `a`. That frame (`help-search`) is in `qa/out`.

| note dialog | reject dialog |
|---|---|
| ![](qa/phase-3/p3-overlays-note-dialog.png) | ![](qa/phase-3/p3-overlays-reject-dialog.png) |

- ✓ **Note dialog:** centered, 64 wide, `bg_raised`, accent border, the target in the title, the caret in the field, the hint muted; footer `↵ confirm   esc cancel`.
- ✓ **Reject dialog:** destructive. The footer reads `y yes   esc cancel`, and `↵` alone shows "press y to confirm".

## p3-launcher: recent cases, doctor, new-case dialog (132×40)

| launcher (broken chain selected) | artboard 01 |
|---|---|
| ![](qa/phase-3/p3-launcher-launcher-broken.png) | ![](qa/phase-3/artboard-Launcher.png) |

- ✓ **Header:** wordmark `s p e k t r` bold; the six-color spectral strip, the only multicolor element; `terminal osint workbench · 0.1.0`.
- ✓ **Recent cases:** an accent box with the columns case / entities / edges / last opened / chain. The cursor row has the `▌` bar on `selection`.
- ✓ **Chain column:** the tampered case reads `✕ break at #1` in hostile; the good case shows `✓` in ok.
- ✓ **Box footer:** path · size · seeds for the selected case. The path is a temp dir in QA; in normal use it starts with `~/`.
- ✓ **Doctor:** two columns with `✓` and `!` glyphs; `terminal truecolor · 132×40` comes from the TUI itself.
- ✓ **Bottom line:** `↵ open   n new case   o open path   v verify chain   q quit`, with `spektr --help` on the right.

| new-case dialog | created and opened |
|---|---|
| ![](qa/phase-3/p3-launcher-new-case-filled.png) | ![](qa/phase-3/p3-launcher-created.png) |

- ✓ **New-case dialog:**
  - fields name / rules (`‹ tracelabs ›`, changed with `→`) / subject / last seen
  - the focused label is in accent, with the caret; the pill is `COMMAND`
  - `↵` creates the case and opens it straight into the workspace (subject `◎ Jordan` is the focus; `chain ✓ 4`)
  - at this width the ego footer keeps `↵ pivot  ⌫ back` and drops the legend

## p3-layout: breakpoints, the size floor, live resize

| 200×60 | 130×40 |
|---|---|
| ![](qa/phase-3/p3-layout-w200x60.png) | ![](qa/phase-3/p3-layout-w130x40.png) |
| **120×36 + `i`** | **100×30** |
| ![](qa/phase-3/p3-layout-w120x36-inspector.png) | ![](qa/phase-3/p3-layout-w100x30.png) |
| **99×30** | **live resize 170×48 → 120×36** |
| ![](qa/phase-3/p3-layout-w99x30.png) | ![](qa/phase-3/p3-layout-live-resize.png) |

- ✓ **200×60:** W_e 38, W_i 48; the graph takes the rest. `e2e_layout` asserts the pane corners at columns 0, 38 and 152.
- ✓ **130×40:** W_e 32, W_i 42 (corners at 0, 32, 88). With only outbound groups, that side gets three quarters of the width, so values stay legible.
- ✓ **120×36:** no inspector column. `i` docks the inspector as a right-side overlay with a one-cell margin; the rail's border stays intact.
- ✓ **100×30:** usable; the top bar drops `rules` first, and nothing overflows.
- ✓ **99×30:** centered `spektr needs at least 100×30 (now 99×30)`.
- ✓ **Live resize:** from 170×48 to 120×36 the layout re-flows with no artifacts. `e2e_layout` also resizes to 99×30 and back.

## p3-colors: NO_COLOR and 256-color, with `glyphs = "ascii"`

| NO_COLOR | 256-color |
|---|---|
| ![](qa/phase-3/p3-colors-no-color.png) | ![](qa/phase-3/p3-colors-color-256.png) |

- ✓ **NO_COLOR:**
  - no colors: `e2e_color_modes` asserts every cell is the default fg/bg
  - focus is bold; the cursor row and the `NORMAL` pill are reverse video
  - every glyph is kept (this scenario also sets the ASCII set)
- ✓ **256-color** (`COLORTERM` unset): the same layout in indexed colors from the token table; `e2e_color_modes` asserts the pill is index 117 and the border 236.
- ✓ **`glyphs = "ascii"`:**
  - entity glyphs `P ~ a U D d` and markers `> v ^ #--`
  - the rounded borders stay box-drawing characters, since the ASCII set covers glyphs and markers only
  - columns stay aligned

## p3-large: the 2,000-entity case

| `]` paged | entities scrolled |
|---|---|
| ![](qa/phase-3/p3-large-big-page2.png) | ![](qa/phase-3/p3-large-big-scrolled.png) |

- ✓ **Virtualized list:** `Entities 2012` draws only the visible rows. Expanding the 560-account group, then `ctrl-d` twice, scrolls with the cursor kept on screen.
- ✓ **Paging:** large groups show at most 5 rows with `··· 135 more ] 1/28`; `]` pages the group under the cursor (`410 more ] 2/84`). Confirmed rows show `▰▰▰` in ok.
- ✓ **Hubs:** none in the generated case. Hub flagging is covered by the fixture case (`gitlab.com ⊛ hub`, etc.).

## Real-terminal screenshots (Ghostty)

**Not taken. Manual steps for Jay below.** `cargo xtask qa --real-terminal` (new) builds the fixture case, opens Ghostty 1.3.1 at 170×48 in JetBrains Mono 13, captures the window with `screencapture -l` and closes that instance. I verified the capture path works on this machine: Screen Recording is permitted.

Ghostty asks **"Allow Ghostty to execute /bin/sh?"** for a command passed with `-e`. That's a security prompt on your machine, so I didn't answer it. I closed the instance I had started (PID 37316, matched by its arguments).

To take the screenshot:

1. `cargo xtask qa --real-terminal`
2. Click **Allow** when Ghostty asks.
3. The PNG lands in `qa/out/real-terminal/ghostty-workspace.png`. Compare it with `docs/phases/qa/phase-3/artboard-Identity.png`.

## Bugs found and fixed

| # | Found in | Expected | Actual | Fix |
|---|---|---|---|---|
| 1 | p3-workspace frame | the status line on the last row | a blank line after the frame's opening `<div>` pushed it off the PNG | `68704af` |
| 2 | p3-workspace frame | truecolor | the PTY had no `COLORTERM`, so the TUI drew in 256-color | `68704af` (harness defaults to truecolor; `p3-colors` covers 256) |
| 3 | p3-colors frame | aligned rows | glyphs from a fallback font shifted the rest of a row in the HTML | `68704af` (each span is exactly its cell count wide) |
| 4 | p3-overlays, p3-modes | keys act in the workspace | keys sent during the "opening…" screen were dropped, because it showed a NORMAL status line | `1432996` (no status line until the workspace exists; scenarios wait for loaded data) |
| 5 | p3-overlays submenu | the menu's border intact | the submenu's shadow overwrote it | `1432996` |
| 6 | p3-overlays menu | keyboard menus anchored at the end of the cursor row | anchored mid-pane | `1432996` (anchor from the last frame's hit rect) |
| 7 | p3-modes | the cursor stays on its entity when the filter clears | it stayed at the same index, on an unrelated row | `1432996` |
| 8 | p3-launcher | `tab` moves to the next field | `tab` cycled a choice field | `1432996` (`←`/`→` change choices) |
| 9 | p3-layout 130×40 | legible values | one-sided ego columns were 15 cells wide; the top bar clipped `chain` | `1432996` |
| 10 | p3-overlays help | full screen | inset, showing the workspace edge | `1432996` |
| 11 | p3-workspace | collapsed groups stack (artboard 08) | a blank line after every group | `1432996` |
| 12 | p3-launcher | `↵` in the new-case dialog opens the new case | it hung on "opening…": the creating engine kept the file locked and the case was opened a second time | `5bcf0b8` (`Host::create` returns the running engine) |
| 13 | p3-overlays palette | `: run` on screen | `: r`: a tick with an unchanged clock cleared a pending render | `8401991` |
| 14 | e2e_mouse | `mouse = false` ignores clicks | mouse sequences still arriving on stdin still acted | `8401991` |
| 15 | p3-layout 120×36 | the rail's border intact | the inspector overlay's shadow overwrote it | `8401991` |
| 16 | p3-launcher created | `↵ pivot  ⌫ back` at narrow widths | every hint dropped to fit the legend | `a83ab75` |
| 17 | p3-launcher created | no internal ids on screen | the person's `uuid:…` key showed in the inspector | `a83ab75` |
| 18 | p3-overlays right-click | `account · N observations` | the count was missing when the menu opened before the detail query returned | `a83ab75` |
| 19 | e2e_text_modes | `n` with nothing under the cursor says so | silent | `8401991` |
