# Design references

These are the HTML sources of the spektr design canvas artboards. Each one is a static mockup of a terminal screen, drawn at about 13 px JetBrains Mono with a 20 px line height, so **one text row ≈ one terminal row** and **roughly 7.8 px ≈ one column**.

Open them in a browser to see the screens. They aren't code to port; they show layout, spacing, color use and copy. `TUI_SPEC.md` is authoritative wherever the two differ.

| File | Screen | Status for v1 |
|---|---|---|
| `Launcher.dc.html` | 01 · Launcher | **Authoritative** |
| `Main.dc.html` | 02 · Workspace, infrastructure case | **Layout authoritative.** Superseded: 5-segment numeric confidence (now 3-level strength), the `overview` tab (now `timeline`, overview moved to v2), the `whois` module (now `rdap`) |
| `Palette.dc.html` | 03 · Command palette | **Authoritative.** `whois-contacts` is now part of `rdap` |
| `Overview.dc.html` | 04 · Braille graph overview | **v2.** Not built in v1 |
| `Correlations.dc.html` | 05 · Correlation review | **Layout reused for the Review screen** (`R`). Superseded: inferred rules (handle similarity, registration window) are v2; v1 Review holds only `sweep_unsure` and `same_person` items; numeric confidence is gone |
| `Tokens.dc.html` | 06 · Design language | **Authoritative for colors, markers, borders and modes.** The glyph table is incomplete: use `TUI_SPEC.md` §3 (adds ◐ account, ▨ image, ⌖ location, and the INSERT, FINDINGS, CHECKLIST and TUTOR pills) |
| `ContextMenu.dc.html` | 07 · Context menu | **Authoritative**, except that "Run all eligible" is bound to `A`, not `⇧↵` (TUI_SPEC §6) |
| `Identity.dc.html` | 08 · Workspace, identity case | **Authoritative: the primary v1 reference.** `≈ same_avatar` is the display label for the stored artifact `avatar_phash` |
| `Findings.dc.html` | 09 · Findings | **Authoritative.** The dashed screenshot box becomes a real image preview (`TUI_SPEC.md` §9) |
| `Checklist.dc.html` | 10 · Playbooks & checklist | **Authoritative.** Shows a user playbook's path as `~/.config/spektr/playbooks`; the real path is the resolved config dir |

All data in the mockups is fictional: `.example` domains, documentation IP ranges (203.0.113.0/24, 198.51.100.0/24), and documentation ASNs.
