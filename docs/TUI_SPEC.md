# spektr — TUI Specification

This is the contract for `spektr-tui`. The visual source of truth is the design canvas; `docs/design/README.md` lists which artboards apply to v1. Where this spec and an artboard disagree, **this spec wins**, and the disagreement should be noted in the phase's COMPLETE doc.

## 1. Principles

1. **Keyboard first, mouse equal.** Every action is reachable by keyboard. Mouse input maps onto the same actions, so the two can never drift apart.
2. **Color carries meaning, nothing else.** One neutral ground, one accent, three signal colors. Entity type is shown by glyph, never by color.
3. **The UI never blocks.** Network, disk, queries and image encoding all happen off the render path.
4. **One action registry.** Keys, mouse, menus, the palette and the help overlay are all generated from it.
5. **Calm density.** Show the focus plus one hop. Paginate large groups; never let them overflow.

## 2. Design tokens

All colors live in `theme.rs` as semantic tokens. No other file may name a color.

| Token | Truecolor | 256 | Use |
|---|---|---|---|
| `bg` | `#0D0E10` | 233 | Ground |
| `bg_raised` | `#111317` | 234 | Overlays: palette, menus, dialogs |
| `selection` | `#1B2028` | 235 | Cursor row background |
| `border` | `#2A2E35` | 236 | Pane borders, rules |
| `faint` | `#4A4D53` | 239 | Tree glyphs, connectors. **Decoration only, never text** |
| `fg_muted` | `#8E8A83` | 245 | Metadata, labels, hints; the lightest color allowed for text |
| `fg` | `#D8D5CE` | 252 | Content |
| `accent` | `#8EC5FF` | 117 | Focus border, cursor bar, live state, correlated edges, mode pill |
| `ok` | `#8FCB9B` | 114 | Done, verified, `confirmed` strength |
| `warn` | `#E3B467` | 179 | Hub, needs review, after-last-seen, COMMAND pill |
| `hostile` | `#F08A7E` | 210 | Failure, broken chain |

**Fallbacks:**

- **256-color:** auto-detected from `COLORTERM`, overridable in config.
- **`NO_COLOR`:** honored. Monochrome mode uses bold for focus and reverse video for the cursor row and mode pill, and keeps every glyph.

**Type rules:**

- One font weight, plus **bold** only for the single thing you're looking at: the focused entity name, or the finding title.
- No italics, no underline.
- The mode pill is the only filled block of color on screen.

**Borders:**

- Rounded borders (`BorderType::Rounded`), with the title inset in the top border and one cell of inner padding.
- Idle pane: `border`, with the title in `fg_muted`.
- Focused pane: `accent` border and title.
- Overlay: `bg_raised` fill with an `accent` or `border` edge. The workspace behind the palette is dimmed by drawing it with `fg_muted` and `faint` only.

## 3. Glyphs and markers

**Entity glyphs** are always `fg_muted` and single-width. The ASCII set applies when `glyphs = "ascii"`.

| ◎ person | ~ username | ◐ account | @ email | + phone | ▨ image | ⌖ location | ▣ organization |
|---|---|---|---|---|---|---|---|
| ↗ url | ◆ domain | ◇ subdomain | ● ip | ◈ asn | ⊟ certificate | ✕ breach | · extension type |

**Markers:**

| Marker | Color | Meaning |
|---|---|---|
| `▌` | accent | Cursor row (left bar, with a `selection` background) |
| `•` | accent | New since you last viewed |
| `⌂` | fg_muted | Seed (or "subject" for the case subject) |
| `⊛` | warn | Hub, not auto-expanded |
| `⊘` | fg_muted | Out of scope, or blocked by rules |
| `≈` | accent | Correlated, via `shares_artifact`. The label shows the artifact kind using the table below, e.g. `≈ same_avatar` |
| `▸` | accent | Running (activity rail), or the current focus (entity list) |
| `✓` | ok | Done or verified |
| `✕` | hostile | Failed |
| `!` | warn | Needs attention |
| `▲` | warn | Dated after the case's last-seen date |
| `▰▱▱` / `▰▰▱` / `▰▰▰` | fg_muted / fg / ok | Strength: single / corroborated / confirmed |

**Artifact display labels.** The stored `artifact` value (DATA_MODEL §3) is never shown raw; it's displayed as:

| `avatar_phash` | `analytics_id` | `favicon_mmh3` | `registrant_email` | `cert_san` | `ip` |
|---|---|---|---|---|---|
| `same_avatar` | `same_analytics_id` | `same_favicon` | `same_registrant` | `same_certificate` | `same_ip` |

**Mode pills** (status line, far left):

| Pill | Color |
|---|---|
| `NORMAL` | accent |
| `COMMAND` | warn |
| `FILTER` | fg |
| `INSERT` | fg (editing finding text) |
| `REVIEW` | accent |
| `FINDINGS` | accent |
| `CHECKLIST` | accent |
| `TUTOR` | ok |

## 4. Layout

Sizes are in terminal cells.

### 4.1 Workspace

```
row 0          top bar (1)
rows 1..H-10   main row: [Entities W_e] [Graph flex] [Inspector W_i]
rows H-9..H-2  activity rail (8, incl. borders)
row H-1        status line (1)
```

| Terminal width | `W_e` | `W_i` | Notes |
|---|---|---|---|
| ≥ 170 | 38 | 48 | Full layout, as drawn |
| 130–169 | 32 | 42 | |
| 100–129 | 30 | — | Inspector becomes a right-side overlay toggled with `i` |
| < 100 or height < 30 | — | — | Centered message: "spektr needs at least 100×30 (now W×H)" |

**Top bar:**

- Left: `spektr` (bold, accent) · case name · subject and last-seen (identity cases) *or* case path.
- Right:
  - `rules <profile>`
  - `findings N`
  - `review N` (warn), shown only when review items are pending
  - `checklist N open`, shown only when playbook checklist items are open
  - `● N running · M queued`
  - `chain ✓ <rows>` (or `✕` in hostile)

**Status line:**

- Left: the mode pill, then hints for the current mode (from the action registry, at most 7).
- Right: UTC clock (HH:MM:SS).

### 4.2 Entities pane

- Groups by type, in this order:
  - identity: person, username, account, email, phone, image, location, organization
  - web: url
  - infrastructure: domain, subdomain, ip, asn, certificate
  - breach
  - extension types last
- Each group header reads `▾ <glyph> <type>` with the count right-aligned.
- Groups with no members are hidden. `▸` means collapsed; groups default to expanded when they have 5 or fewer members.
- Rows show the entity value plus a right-aligned marker (`•`, `⌂`, `⊛ hub`, `subject`).
- Each expanded group shows up to 4 rows, then `··· N more`. `↵` on that line expands the group in place.
- Out-of-scope and not-found results are summarized in one muted line at the bottom.
- `/` filters with fuzzy matching (`nucleo-matcher`) across values. The filter line shows `N match`.

### 4.3 Graph pane: tabs `1 ego` · `2 timeline` · `3 path`

The tab label sits in the top border, right side; the active tab is drawn as `[1 ego]` in accent.

**Ego view** (artboard 08):

- **Breadcrumb line:** the pivot history, `◎ Jordan Marlow › ~ jmarlow.wav`. Earlier steps are `fg_muted`; the current one is `fg`.
- **Three columns:** inbound and symmetric groups on the left, the focus card in the center (20 cells wide), outbound groups on the right.
- **Group header:** `<predicate> ← N` (left side) or `→ N <predicate>` (right side), with a connector line (`faint`) running toward the focus card. Correlated groups render in accent as `≈ <artifact label> ↔ N`.
- **Group order:** by a priority table in `spektr-tui` (identity predicates first), then by count. At most 5 rows per group; `[` and `]` page through the group under the cursor, with a `··· N more ] p/q` footer.
- **Rows:** a tree glyph (`├`, `└` in faint), the entity glyph and value, and the strength bar right-aligned. The cursor row uses the `▌` accent bar on a `selection` background.
- **Focus card:** type (muted), value (bold), 2–3 count lines, then scope/seed status.
- **Footer line:** key hints on the left, the strength legend on the right.

**Timeline view:**

- One chronological list of every event attribute (DATA_MODEL §2) across the case:
  `YYYY-MM-DD HH:MM  <glyph> <entity>  <attr name>  <source module>`.
- When the case has a last-seen date, a full-width divider row reads `── last seen 2026-09-28 ──` (warn).
- Rows after it get `▲` (warn).
- `↵` jumps to that entity in the ego view; `e` filters by event kind.

**Path view:**

- Opened with `:path <entity>`, or `3` followed by a fuzzy picker.
- Shows the shortest path from the focus to the target, drawn vertically:
  `<entity>` / `│ <predicate> ▰▰▱` / `<entity>` …
- "No path within 6 hops" when there's none.

### 4.4 Inspector pane

The inspector follows the **cursor** (not the focus), in any pane. From top to bottom:

1. **Image preview**, when the entity is an image, an account with `avatar_url`, or a capture with a screenshot (§9).
2. **Header:** the value in bold; then type and normalized key in muted.
3. **Fields:** attributes as `label  value`, with the label column 14 wide. Conflicting values from different sources are listed one per line with their source. An event attribute dated after last-seen gets a second line: `▲ after last-seen date` in warn.
4. **Evidence N:** each observation and capture as `module  version  HH:MM  hash-prefix`. To use this list, focus the inspector pane (`tab` cycles panes), then move with `j`/`k`; `o` opens the selected item.

**Edge comparison.** When the cursor is on a correlated row (a member of a `≈` group in the ego view), the inspector switches to comparing the **edge**:
- **Header:** both entities, plus the artifact label.
- **For `same_avatar`:** both avatar images side by side, with `dHash distance N/64` beneath.
- **For other artifacts:** the shared value, with each side's source observation.
- **Then:** strength and evidence, as above.
- **Keys:** `v` confirms the edge, `x` rejects it.

This is where J2 step 5 and tutor lesson 6 take place.
5. **Strength:** the strength bar and label, who confirmed it and when, and a count of independent sources.
6. **Used in:** findings referencing this entity, as `F-n  category  status`.
7. **Checklist:** open checklist items for this entity, as `○ <item text, truncated>  <playbook>`. `↵` jumps to the item on the Checklist screen.
8. **Footer hints.**

### 4.5 Activity rail

- **Left:** a live event log, newest at the top, 6 visible rows (scrollable, retaining 500 rows). Each row reads `HH:MM:SS <status glyph> <module, padded to 15> <summary>`.
  - Status glyphs: `✓` ok, `▸` accent (running), `≈` accent (correlation), `⊛` warn (hub), `✕` hostile (failure, with the retry info), `⊘` muted (blocked by rules, with the reason).
- **Right column (30 cells, separated by a vertical rule):**
  - running, queued, done and failed counts
  - the active rules profile and a one-line summary of it
  - the running playbook, if any: `▸ playbook <name> · step N/M`
  - budget meters (`▰▰▱▱▱▱▱▱▱▱ 38/100`) for keyed or metered modules

## 5. Screens

| Screen | Open | Artboard | Notes |
|---|---|---|---|
| Launcher | `spektr` with no case | 01 | Recent cases (name, entities, edges, last opened, chain status); a doctor panel; spectral strip under the wordmark (the only multicolor element in the app) |
| Workspace | Open a case | 08 (02 for infrastructure data) | §4 |
| Command palette | `:` | 03 | Overlay ≈ 96 × 22 at 1/6 of the height; prompt; grouped matches (Modules, Commands, Recent); detail pane for the selected item; footer `3 of 14` |
| Context menu | right-click or `space` | 07 | §7 |
| Findings | `F` | 09 | List grouped by profile category (left, 70 cells); detail (right); checks panel; evidence with an image preview of the selected screenshot |
| Review | `R` | 05 (layout only) | Queue of `sweep_unsure` and `same_person` items; detail with a side-by-side field comparison (and avatars, when both people have them) |
| Checklist | `C` | 10 | Left (≈ 76 cells): playbook runs (nested runs indented under their parent) with step progress; the selected run's steps (`✓` done, `▸` running, `○` pending) with result counts; the available playbooks with their seed type and source (built-in or user). Right: checklist items grouped by step, filter tabs (`open`, `done`, `all`), and a detail block for the selected item showing its text and launchers |
| Help | `?` | — | Full-screen overlay listing every binding by context, generated from the registry; `/` searches it |
| Tutor | `spektr tutor` / `:tutor` | — | §10 |
| Dialogs | confirm, input | — | Centered `bg_raised`, accent border, at most 64 wide; `↵` confirms, `esc` cancels; destructive confirmations need `y` |

**Finding text editing:**

- `e` opens the title, claim and relevance in an inline editor (`ratatui-textarea`, mode `INSERT`).
- A new finding's title starts as the first 60 characters of its claim, or `<category> · <entity value>` while the claim is empty. Once edited by hand, the title stays as written.
- `E` opens the text in `$EDITOR` instead, restoring the terminal afterwards.

## 6. Action registry and keymap

- **All bindings live in one table** (`keymap.rs`). Each entry has: action id, default keys, contexts, description, an availability predicate (which entity types and states it applies to), and its palette command name.
- **Generated from the table:** the help overlay, status-line hints, menu hotkey labels, and palette entries.
- **Overrides:** users can change bindings in `config.toml` under `[keys]`.
- **Conflict check:** a unit test fails the build if any key is bound twice in the same context.
- **Context precedence**, from highest to lowest:
  1. text input
  2. overlay (palette, menu, dialog, help)
  3. screen (Checklist, Findings, Review)
  4. focused pane / Navigation
  5. Entity actions
  6. Global

  A key bound in a higher context overrides the same key in a lower one while that context is active. For example, on the Checklist screen `↵`, `tab`, `p` and `f` mean their Checklist actions. The conflict test checks for duplicates *within* each context, and checks that every override is listed in the keymap table's `overrides` field, so overrides are deliberate.
- **Text-input contexts capture the keyboard.** In `COMMAND`, `FILTER` and `INSERT` modes, and in any picker or dialog input, every printable key and `⌫` goes to the input. Only these keep their meaning there:
  - `esc`, `↵`
  - `tab` (completion)
  - `↑` `↓` (selection)
  - `ctrl-` chords

  Global single-key bindings (`q ? : / F R 1 2 3 …`) are suspended. The conflict test covers this rule too.
- **No chords that need the Kitty keyboard protocol.** Every binding must work in Terminal.app and inside tmux, so modified Enter (`⇧↵`, `ctrl-↵`) is never used.

### Global

| Key | Action |
|---|---|
| `q` | Quit (asks if jobs are running) · `ctrl-c` twice force-quits |
| `?` | Help overlay |
| `:` | Command palette |
| `/` | Filter the focused list |
| `tab` / `shift-tab` | Next / previous pane |
| `1` `2` `3` | Graph tabs: ego, timeline, path |
| `F` | Findings |
| `R` | Review queue (pending count shown in the top bar) |
| `C` | Checklist and playbook runs |
| `esc` | Close the overlay, clear the filter, or go back to the workspace |

### Navigation (lists and ego view)

| Key | Action |
|---|---|
| `j` `k` / `↓` `↑` | Move the cursor |
| `h` `l` / `←` `→` | Move between ego columns (inbound · focus card · outbound) |
| `gg` / `G` | Top / bottom |
| `ctrl-d` / `ctrl-u` | Half page |
| `[` / `]` | Previous / next page of the current group |
| `↵` | Pivot to the cursor entity (ego), expand `··· more`, or open the item |
| `⌫` | Back in the pivot history |

### Entity actions (on the cursor entity)

| Key | Action |
|---|---|
| `space` | Context menu |
| `r` | Run module ▸ (submenu of eligible modules) · `A` inside it runs all eligible |
| `c` | Capture (url, account, or image with a URL) |
| `f` | New finding from this entity |
| `a` | Attach the entity to the current finding (picker if none is open) |
| `n` | Add a note |
| `y` | Copy the value (OSC 52 clipboard) |
| `v` | Confirm the edge under the cursor (strength → confirmed) · `V` un-confirms |
| `x` | Reject the edge under the cursor (asks first) |
| `m` / `M` | Merge with… (picker) / unmerge |
| `s` / `S` | Add to scope / exclude from scope |
| `o` | Open evidence: raw response in `$PAGER`, a capture or image in the system viewer |
| `O` | Open the URL in the browser |
| `u` | Review the username-sweep's unsure hits for this entity |
| `p` | Run playbook ▸ (submenu of playbooks whose `seed` matches) |
| `i` | Toggle the inspector overlay (narrow terminals only) |

### Findings

| Key | Action |
|---|---|
| `n` | New finding |
| `e` / `E` | Edit inline / in `$EDITOR` |
| `a` | Attach evidence |
| `r` | Mark ready |
| `s` | Mark sent |
| `x` | Export ▸ (submission text to clipboard, Markdown, JSON) |
| `↵` | On a supporting-entity row: go to that entity in the workspace |
| `d` | Delete (asks first) |

### Checklist

| Key | Action |
|---|---|
| `d` | Mark the item done (prompts for an optional note) |
| `s` | Skip the item (prompts for a reason) |
| `n` | Add or edit the note |
| `o` | Open a launcher ▸ (opens directly when the item has only one). When every launcher is disabled (a local-only image), reveals the file in the system file manager instead |
| `f` | New finding from this item: links its entity, and the note becomes the draft relevance |
| `↵` | Go to the item's entity in the workspace, or open its screen (`review` / `findings`) |
| `p` | Run a playbook on the selected run's seed |
| `X` | Stop the selected run (asks first) |
| `tab` | Switch between the runs and checklist panes |

### Review

| Key | Action |
|---|---|
| `a` | Accept (sweep hit becomes an account; same-person opens the merge confirmation) |
| `x` | Reject |
| `s` | Later |
| `O` | Open in the browser |

### Palette commands (v1)

- **Investigation:** `seed <type>:<value>` · `run <module>` · `rerun <module>` · `expand` · `depth <n>`
- **Scope and rules:** `scope add|rm|list <rule>` · `rules <profile>`
- **Graph edits:** `merge` · `unmerge` · `confirm` · `reject` · `path <entity>`
- **Notes, findings and evidence:** `note` · `finding new` · `capture <url>` · `attach <path>`
- **Hand-off and output:** `open <launcher>` · `export <format>` · `verify`
- **Playbooks:** `playbook <name>` (on the focused entity) · `playbook stop` · `checklist` · `reload` (re-reads modules, playbooks, profiles and launchers from the config dir)
- **Settings and help:** `region <CC>` · `mouse on|off` · `images <mode>` · `glyphs unicode|ascii` · `tutor` · `tutor reset` · `help`
- **Argument syntax:** `<rule>` for scope, and `<type>:<value>` for entities, follows the CLI reference in ARCHITECTURE §10.

## 7. Context menus (artboard 07)

- **Opening:** right-click on any entity, edge or evidence row, or `space` on the cursor row. The menu is anchored at the click cell, or at the right end of the row.
- **Content:** built from the registry, filtered by the target's type and state. Sections are separated by rules:
  - **navigate:** Pivot here, Run module ▸, Run playbook ▸, Open in browser ▸ (launchers), Copy value
  - **scope:** Add to scope, Exclude from scope
  - **analysis:** Mark as finding, Add note, Merge with…, Confirm or Reject edge, Open raw response
- **Header:** the entity glyph and value in bold, then a muted line with the type and observation count.
- **Items:** label on the left; hotkey right-aligned in `fg_muted`.
  - **Disabled items stay visible,** in `fg_muted`, with the reason right-aligned (e.g. "covered by parent", "blocked by tracelabs").
  - **Run module ▸ items show their cost or last run:** "1 request", "ran 14:31 · rerun".
- **Submenus:** open to the right, or flip left when there isn't room.
- **Placement:** clamped inside the terminal, with a drop shadow emulated by a `bg` cell margin.
- **Keys:** `↑` `↓` select, `→` opens a submenu, `←` returns, `↵` runs, `esc` closes. Hotkeys work while the menu is open.
- The menu stays in `NORMAL` mode; only the status-line hints change.

## 8. Mouse

- Mouse capture is on by default. `mouse = false` in config, or `:mouse off`, turns it off.
- **The help overlay notes that while capture is on, shift-drag selects text** in most terminals.

| Gesture | Action |
|---|---|
| Left click | Focus the pane under the pointer and move the cursor to the row |
| Double click (≤ 400 ms) | `↵` on the row (pivot, open, expand) |
| Right click | Context menu |
| Wheel | Scroll the list under the pointer, 3 rows per notch |
| Click a tab label or the mode pill | Switch tab; the pill opens help |
| Click a palette or menu item | Run it |
| Click outside an overlay | Close it |

**Hit-testing:** each render returns a `Vec<(Rect, HitTarget)>` alongside the frame. A mouse event resolves against the last frame's regions, and is translated into the same `Action` the keyboard would produce.

**Hover:** motion tracking (`?1003`) is enabled only so the row under the pointer can get a subtle `selection` highlight. A redraw happens only when the hovered target changes.

**tmux:** the doctor warns if `TMUX` is set and `tmux show -gv mouse` isn't `on`.

## 9. Images

- **Library:** `ratatui-image`, with the protocol picked once at startup. The query to the terminal happens before raw mode is entered.
- **Config:** `images = "auto" | "kitty" | "sixel" | "iterm2" | "halfblocks" | "off"`. The doctor and the launcher show the detected protocol.
- **Placements:**
  - the inspector's top area, at most 40% of the inspector's height, aspect preserved
  - the inspector's edge comparison (§4.4): two avatars side by side, with `dHash distance 3/64` beneath
  - the Review detail for `same_person` items, when both people have avatars
  - the Findings evidence preview: the selected screenshot
- **Never in list rows.**
- **Performance:** decoding, resizing and protocol encoding run on a background worker, using the crate's thread-backed protocol. Until an image is ready, its area shows a muted `loading image…`. Encoded images are cached per `(sha256, area size)`, up to 64 entries.
- **Failure:** a corrupt or unsupported image shows `▨ <filename> · preview unavailable`, and the app keeps running.

## 10. Tutor

- `spektr tutor` creates a temporary copy of the bundled tutor case, with the `tracelabs` profile and every module in **replay** mode.
- **A coach strip** (2 rows, above the status line, `TUTOR` pill in ok) shows the current step and a goal. Each step completes on a **semantic event**, not a keystroke:
  - pivot to an entity
  - run a module
  - start a playbook / a playbook step completes
  - resolve a checklist item
  - open a launcher (recorded, not opened, in the tutor; see ARCHITECTURE §8)
  - confirm an edge
  - create a finding
- **Tutor fixtures** must cover every module the tutor's playbooks run on the tutor handle: username-sweep, github, bluesky, gitlab, mastodon, hackernews, avatar-hash, http-fingerprint and wayback. They also cover exif for the tutor image.
- **Lessons (about 15 minutes):**
  1. Move and focus
  2. Seed a username
  3. Run the `person-from-username` playbook and watch the activity rail
  4. Pivot and go back
  5. Read strength
  6. Review an avatar match and confirm it
  7. Timeline: spot the after-last-seen event
  8. Capture a page, and resolve a checklist item (open a launcher, mark it done)
  9. Write a finding
  10. Pass the checks and export
- **Progress** is saved to `$XDG_STATE_HOME/spektr/tutor.toml`. `:tutor` resumes; `:tutor reset` starts over.

## 11. Performance and correctness

- **Rendering:** only when state is dirty, with input coalesced to at most 60 fps. Target < 16 ms from input to render on a 2,000-entity case.
- **Lists are virtualized:** render only the visible rows.
- **View models** (ego groups, timeline, entity groups) are computed by store queries, off the render path, and delivered as `Msg`. The render functions are pure functions of `AppState`.
- **Testing:**
  - every screen is snapshot-tested (`ratatui` `TestBackend` + `insta`) at 170×48 and 120×36
  - the keymap conflict test
  - a scripted end-to-end tutor run
