# spektr — Product Requirements

Status: v1 spec · Owner: Jay (infktd) · Last updated: 2026-10-20

## 1. What spektr is

spektr is a terminal workbench for open-source investigations. You seed what you know: a username, an email, a photo, a domain. spektr expands it into a typed graph of entities, automatically and within rules you set. Every edge stays tied to the exact evidence it came from. You turn what you learn into **findings**, and export them.

The whole investigation is one portable `.spektr` file with a hash-chained audit log anyone can verify.

**One line:** the graph is how you work; findings are what you deliver; evidence is why anyone should believe them.

### Who it's for

- **Primary:** Trace Labs Search Party CTF participants. Their work is passive, people-focused OSINT on missing persons, and every submission needs evidence and an explanation of relevance.
- **Secondary:** solo investigators and security researchers who want a keyboard-driven, scriptable, evidence-grade tool instead of a heavy GUI.
- **Also:** an open-source portfolio project. Visual and interaction quality are first-class requirements.

### Positioning

| Tool | What it is | spektr's difference |
|---|---|---|
| SpiderFoot | Scan-everything automation | spektr expands within scope, depth, budget and rules; you stay in control |
| Maltego | GUI graph with licensed transforms | Keyboard-first terminal UI, free modules, one plain file per case |
| Hunchly | Web evidence capture | spektr captures *and* links evidence to entities and findings |

## 2. How professionals work, and how spektr maps to it

This is drawn from the Berkeley Protocol, the Trace Labs participant and scoring guides, and practitioner write-ups:

1. **The loop is short and run by the analyst:** seed → find sources → query → note → pick the next pivot → stop when answered.
2. **Strength comes from independent pivots.** One shared username is weak; username + matching avatar + linked site is strong.
3. **Documentation is evidence.** Record the search strategy, capture artifacts unaltered with metadata, preserve them, and show your reasoning.
4. **The output is findings.** For Trace Labs, findings are filed under scored categories, each with relevance, a URL or screenshot, and strict passive-only rules.

| Practice | spektr feature |
|---|---|
| Record the search strategy | The audit log records every module run, query and analyst action |
| Collect artifacts unaltered | Raw responses stored compressed; SHA-256 computed by the engine |
| Preserve web evidence | Built-in web capture: HTML, headers, screenshot, hash, timestamp |
| Independent corroboration | Three-level edge strength: single source, corroborated, confirmed |
| Show the reasoning | Findings hold a claim, a relevance note, supporting entities and evidence |
| Passive-only rules | Rules profiles (e.g. `tracelabs`) block forbidden techniques before they run |
| Avoid rabbit holes | Shallow automatic expansion by default (depth 2: seed → accounts → their details); hubs are never auto-expanded |

## 3. Core concepts

- **Case:** one investigation, stored as one `.spektr` file (SQLite). It holds:
  - the subject (optional)
  - the last-seen date (optional)
  - a rules profile
  - scope rules
  - all entities, edges, observations, findings and the audit log
- **Entity:** a typed, normalized thing, such as `username:jmarlow.wav` or `email:jordan.marlow@mail.example`. Its normalized value is its identity, and there is only one entity per normalized value.
- **Edge:** a typed link between two entities, using a predicate from a closed vocabulary (e.g. `has_account`, `resolves_to`). Every edge carries its origin and evidence.
- **Observation:** the raw evidence from one module run (or capture) with provenance: module, version, timestamp, source URL, raw bytes and SHA-256.
- **Strength:** per edge, one of three levels.
  - **single** (`▰▱▱`): backed by one source
  - **corroborated** (`▰▰▱`): two or more *independent* sources
  - **confirmed** (`▰▰▰`): the analyst confirmed it

  There are no numeric confidence scores.
- **Module:** a transform from one entity to new entities, edges and observations. Built-in modules are written in Rust; user modules are any executable that speaks the exec protocol.
- **Rules profile:** a named policy. It sets which technique classes are allowed, which modules are blocked, the finding categories (and points), submission lint rules, and default depth.
- **Finding:** an analyst's claim, with a category, a relevance note, supporting entities, evidence, and a status (draft, ready or sent).
- **Capture:** preserved web evidence (HTML, headers, optional screenshot) or an attached local file, stored as an observation.

## 4. v1 scope

### 4.1 Must have (v1)

**Case & integrity**
- Create, open and list cases; one SQLite file per case (`.spektr`).
- A hash-chained append-only audit log, and `spektr verify` to check it.
- Case metadata: name, subject, last-seen date, rules profile, scope rules, default depth.

**Model**
- 15 core entity types, a closed predicate vocabulary, and normalization. See `DATA_MODEL.md`.
- Module-declared extension types and predicates, namespaced `x.<module>.<name>` and validated by the engine.
- Merge and unmerge, both reversible and audited. Nothing merges automatically across non-identical normalized values.

**Engine**
- A typed transform graph: each new entity in scope can trigger `auto` modules that accept its type.
- Guardrails:
  - depth limit (default 2; see MODULES §5)
  - scope allow/deny rules
  - hub suppression (degree threshold plus a known-hub list)
  - per-host rate limits
  - daily budgets for keyed APIs
  - dedup and cache by `(module, version, normalized input)` within a TTL
- Rules profiles enforced *before* a job is queued. A blocked job is recorded in the activity rail and the audit log with the reason.
- Correlation:
  - **identity merges:** happen automatically at insert, through normalization
  - **shared-artifact edges:** written automatically, for avatar perceptual hash, analytics ID, favicon hash, registrant email, certificate SAN overlap, and shared non-hub IP
- A review queue for:
  - username-sweep "unsure" hits
  - same-person candidates (two person entities linked to the same email, phone or account at strength corroborated or higher)

**Modules (built-in, 18):** see `MODULES.md` §6. Identity-first, all passive. HIBP is optional and needs a key.

**Playbooks (workflows):** see `PLAYBOOKS.md`.
- Ordered TOML steps that run modules, call another playbook, or add manual checklist items with launcher buttons.
- **Six built-ins:** `person-from-username`, `person-from-email`, `person-from-phone`, `image-triage`, `domain-recon`, `tracelabs-kickoff`.
- **User playbooks** go in the config dir. A user playbook with a built-in's name overrides it.
- **Ways to run:** the palette, the context menu, or `spektr run --playbook`.
- The **Checklist** screen tracks the manual steps, and every resolution is audited.
- Rules profiles always apply inside a playbook.

**Case metadata also holds `default_region`,** set with `spektr new --region US` or `:region US` (default `US`). Phone normalization needs it.

**Exec module runtime:** user modules in any language over JSON-lines on stdin/stdout. Supporting commands: `spektr module new | test | check | install | list | info | enable | disable`.

**Evidence**
- Web capture via `c` in the TUI or `spektr capture`: HTML, headers and timestamp, plus a screenshot when Chrome/Chromium is available (`spektr setup capture` fetches headless Chrome).
- Attach local files (screenshots taken by hand, downloaded images) as evidence linked to an entity.
- Pivot launchers: `:open <launcher>` builds search URLs for the focused entity (search-engine queries, reverse image search, platform search) and opens them in the system browser. Launchers are user-configurable.

**Findings**
- Create a finding from any entity; attach evidence; categories come from the rules profile.
- Submission checks (linting) from the rules profile:
  - claim and relevance present
  - at least one URL or screenshot
  - no blocked source types
  - not filed in another category
  - flags that need approval
- Export:
  - paste-ready submission text (Trace Labs format)
  - a Markdown report
  - JSON

**TUI:** see `TUI_SPEC.md`.
- Screens: launcher, workspace (ego, timeline and path tabs), command palette, context menus, findings, review, checklist, help overlay.
- Mouse support.
- Inline image previews (Kitty, Sixel or iTerm2, with halfblock fallback).

**Learning & docs**
- `spektr tutor`: an interactive, offline tutorial running on a bundled fictional case built from recorded fixtures.
- `?` help overlay generated from the action registry.
- A VHS `.tape` demo suite in `demos/`, regenerated in CI on release, run against the tutor case.

**CLI (headless):** `new`, `open`, `run` (including `--playbook`), `query`, `export`, `capture`, `attach`, `verify`, `doctor`, `tutor`, `module …`, `playbook …`, `setup capture`. Full reference: `ARCHITECTURE.md` §10.

**Distribution**
- Single binary for macOS (arm64, x86_64) and Linux (x86_64, aarch64).
- Install via GitHub Releases, `cargo install`, and a Homebrew tap.

### 4.2 Deferred (v1.x / v2)

| Item | Target |
|---|---|
| Recipe modules (declarative TOML for HTTP APIs and CLI tools) | v1.1 |
| Installing playbooks from a git URL | v1.1 |
| Braille graph overview tab | v2 |
| Inferred-link suggestion engine (handle similarity, timing patterns) | v2 |
| Browser extension that sends pages to spektr (Hunchly-style) | v2 |
| WASM module runtime (sandboxed shared modules) | v2 |
| Chain-of-custody manifest export (signed bundle) | v2 |
| Curated community module index | v2 |
| Windows support | v2 |

### 4.3 Non-goals

- No web UI, no multi-user, no server, no cloud sync, no accounts.
- **No active or probing techniques by default.** No port scanning, no logging in, no posting or contacting anyone, and no checking whether an account exists by submitting an email to a site's forms. Modules declare a technique class, and non-passive classes are off unless the case's rules profile allows them.
- No bundled breach data, people-search databases or paid-data integrations.
- No scraping of services whose terms forbid it (e.g. reverse image search engines). Pivot launchers open those in the user's browser instead.

## 5. Key user journeys (acceptance scenarios)

**J1: First run.**
1. `brew install spektr && spektr`.
2. The launcher shows the doctor panel and a hint to run `spektr tutor`.
3. The tutor completes offline in about 15 minutes, covering seed, run, pivot, strength, review, capture, finding and export.

**J2: Trace Labs case.**
1. `spektr new tl-ctf-1101 --profile tracelabs --subject "Jordan Marlow" --last-seen 2026-09-28`.
2. Seed `username:jmarlow.wav`.
3. Automatic expansion runs username-sweep. Accounts stream into the ego view live.
4. Avatar hashes correlate three accounts (`shares_artifact`, artifact `avatar_phash`, displayed as `≈ same_avatar`).
5. With the cursor on the correlated edge, the inspector shows the avatars side by side with their hash distance. The analyst compares them, then confirms the edge with `v`.
6. The GitHub account's last-active date shows as after last-seen in the inspector and the timeline tab.
7. Press `c` to capture the profile, then `f` to create a finding in "Advancing the Timeline".
8. Checks pass; export submission text to the clipboard.

**J3: Blocked technique.** With the `tracelabs` profile active, a user-installed module of class `probing` never runs. The activity rail shows `⊘ rules … blocked`, with the reason recorded in the audit log.

**J4: Build a module.**
1. `spektr module new lemmy --lang python` scaffolds a working exec module.
2. `spektr module test lemmy --input username:jmarlow` shows validated output.
3. `spektr module check` passes.
4. `spektr module install ./lemmy` lists its declared hosts and secrets and asks for confirmation.
5. The module appears in the palette and the context menus.

**J5: Integrity.** `spektr verify case.spektr` re-hashes the audit chain and all evidence blobs. It reports `✓` or the first broken row. Tampering with any audit row or evidence blob is detected. (Checking the graph tables against the audit log is v2.)

**J6: Headless.** `spektr run case.spektr --seed email:… --depth 2 && spektr export case.spektr --format md` works in a script with no TTY. In Phase 2 this is checked with a username seed and `--format json`; the full version is checked in Phase 5.

**J7: Playbooks.**
1. On `◎ Jordan Marlow` (subject, with a known handle and email), run `:playbook tracelabs-kickoff`.
2. The rail shows step progress. Nested `person-from-username` and `person-from-email` runs appear under the parent on the Checklist screen.
3. The Checklist fills with reverse-image items, one per avatar, plus the category items.
4. `o` opens Google Lens for an avatar, and an audit row is written.
5. `d` marks the item done with a note; `f` turns an item into a finding.
6. A user playbook dropped into `<config>/playbooks/` appears in the context menu after `:reload` (or a relaunch).
7. `spektr playbook check` rejects a playbook that names an unknown module.

## 6. Quality bar

- **Visual:** matches the design canvas (`docs/design/`). Aesthetic drift is a bug. Truecolor with a 256-color fallback; usable at 120×36; minimum 100×30, with a clear message below that.
- **Responsiveness:** the UI never blocks on network, disk or image encoding. Input-to-render latency under 16 ms on a 2,000-entity case.
- **Correctness:** normalization is property-tested. The audit chain is verified in tests. Module output is validated against the vocabulary before ingestion.
- **Offline determinism:** every built-in module has recorded fixtures. `cargo test` never touches the network.
- **Build time:** see `ARCHITECTURE.md` §6. Editing TUI or module code must not recompile the network, storage or image stacks.
- **Privacy:**
  - case files are local only
  - secrets come only from config or environment, and are never written to a case file or log
  - demo and tutor data is fictional, using reserved `.example` domains and documentation IP ranges

## 7. Success metrics (personal / OSS)

- A Trace Labs CTF completed using spektr as the case file, with at least 80% of submitted flags exported from spektr findings.
- A new user goes from install to a first exported finding in under 20 minutes (tutor included).
- A third-party exec module written by someone else, using only `MODULES.md`.

## 8. Document map

| Doc | Contents |
|---|---|
| `PRD.md` | This file: what and why |
| `ARCHITECTURE.md` | Crates, process model, data flow, build-time strategy |
| `DATA_MODEL.md` | Entity types, predicates, normalization, strength, schema, audit chain |
| `TUI_SPEC.md` | Screens, layout, tokens, glyphs, keymap, mouse, menus, images |
| `MODULES.md` | Module contract, exec protocol, manifest, built-in modules, rules profiles, launchers |
| `PLAYBOOKS.md` | Workflow format and semantics, the checklist, the six built-in playbooks |
| `DEPENDENCIES.md` | Toolchain and pinned crate versions (verified 2026-10-20) |
| `QA.md` | How the building agent runs and checks the real app: PTY end-to-end tests, visual QA against artboards, live smoke runs |
| `design/` | Design canvas artboards (HTML sources) and a README explaining which apply |
