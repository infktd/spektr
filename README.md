# spektr

spektr is a terminal workbench for open-source investigations. You seed what you know (a username, an email, a photo, a domain) and spektr expands it into a typed graph of entities, automatically and within the rules you set: scope, depth, hubs, budgets, and passive-only rules profiles such as Trace Labs'. Every edge stays tied to the exact evidence it came from, every action lands in a hash-chained audit log anyone can verify, and the whole investigation is one portable `.spektr` file. You turn what you learn into findings and export them.

Licensed under [Apache-2.0](LICENSE).

## Install

From the first release on:

```
brew install infktd/tap/spektr          # macOS and Linux
cargo install spektr                    # from crates.io
```

Release tarballs for macOS and Linux (arm64 and x86_64) and a shell installer are on the [releases page](https://github.com/infktd/spektr/releases).

From source (Rust 1.99.1, pinned in `rust-toolchain.toml`):

```
git clone https://github.com/infktd/spektr && cd spektr
cargo install --path crates/spektr
```

## First run: the tutor

```
spektr tutor
```

Ten short lessons (about 15 minutes) on a fictional case, entirely offline: seed a username, run a playbook, pivot, read strength, confirm an avatar match, find activity after the last-seen date, capture a page, work the checklist, write a finding and export it. A strip at the bottom says what to do next; each step completes when you've actually done it. `spektr tutor --reset` starts over.

`spektr` on its own opens the launcher: recent cases and a doctor panel that checks your setup. `spektr doctor` prints the same checks.

## A case in five commands

```
spektr new tl-1101 --profile tracelabs --subject "Jordan Marlow" --last-seen 2026-09-28
spektr run tl-1101.spektr --seed username:<a handle you may look up>
spektr open tl-1101.spektr                         # the TUI
spektr export tl-1101.spektr --format tracelabs    # paste-ready submissions
spektr verify tl-1101.spektr                       # re-hash the audit chain and every blob
```

Everything the TUI does also runs headless: `run --playbook`, `query --json`, `capture`, `attach`, `export --format md|json`. `spektr --help` lists the commands; `ARCHITECTURE.md` §10 has the full reference.

## Working in the TUI

| Key | |
|---|---|
| `↵` / `⌫` | Pivot to the cursor entity / go back |
| `tab` | Next pane (entities, graph, inspector) |
| `1` `2` `3` | Graph tabs: ego, timeline, path |
| `:` | Command palette (`seed`, `run`, `playbook`, `capture`, `path`, `scope add`, …) |
| `space` | Context menu for the cursor entity |
| `p` | Run a playbook on the entity |
| `c` | Capture the entity's page (HTML, headers, screenshot) |
| `f` | New finding from the entity |
| `v` / `x` | Confirm / reject the edge under the cursor |
| `F` `R` `C` | Findings, review queue, checklist |
| `?` | Every key, by context |

The mouse works too: click to focus, double-click to pivot, right-click for the menu.

## Concepts

- **Rules profiles** decide what may run. `tracelabs` allows passive techniques only, defines the Trace Labs categories and points, and lints submissions; `default` is passive with one category. A module that would probe or scan is blocked, and the block is recorded. Your own profiles go in `~/.config/spektr/profiles/`.
- **Scope** limits automatic expansion (`spektr new --scope 'deny domain:cdn.example'`, or `:scope add` in the TUI); hubs (big shared hosts) are never expanded automatically.
- **Strength** is how well an edge is supported: `▰▱▱` one source, `▰▰▱` corroborated by independent sources, `▰▰▰` confirmed by you.
- **Evidence:** every entity, edge and attribute cites the observation (the raw response, a capture, an attachment) it came from. `o` opens it.
- **Playbooks** chain modules and manual checks: `person-from-username`, `person-from-email`, `person-from-phone`, `image-triage`, `domain-recon` and `tracelabs-kickoff` are built in. Manual steps become checklist items with pivot launchers (reverse image search, exact-match search) that open in your browser. `spektr playbook new` scaffolds your own.
- **Findings** hold a claim, its relevance, supporting entities and evidence, and pass the profile's checks before they're marked ready.

## Modules

18 built-in modules, all passive: `username-sweep` (307 sites), `github`, `gitlab`, `bluesky`, `mastodon`, `hackernews`, `gravatar`, `email-check`, `hibp` (needs `HIBP_KEY`), `phone-parse`, `exif`, `avatar-hash`, `http-fingerprint`, `wayback`, `dns`, `rdns`, `rdap` and `crtsh`. `spektr module list` shows them with their state.

Write your own in any language: a module is a program that speaks a small JSON-lines protocol.

```
spektr module new lemmy --lang python     # also rust or shell
spektr module test ./lemmy --input username:example --fixture lemmy/fixtures/username_example.jsonl
spektr module check ./lemmy
spektr module install ./lemmy             # shows its hosts and secrets, asks first
```

Installed modules are pinned to their files: change one and it stays off until `spektr module install --update` re-approves it. Exec modules run with your permissions, so install only code you trust. The protocol is in [`docs/MODULES.md`](docs/MODULES.md) §3, with JSON Schemas in [`schemas/`](schemas/).

## Configuration

`~/.config/spektr/config.toml` (or `$XDG_CONFIG_HOME/spektr/`):

```toml
mouse = true                 # mouse capture
images = "auto"              # auto | kitty | sixel | iterm2 | halfblocks | off
glyphs = "unicode"           # or "ascii"

[secrets]
GITHUB_TOKEN = "…"           # optional: higher API limits
HIBP_KEY = "…"               # enables hibp

[keys]
quit = ["Q"]                 # rebind an action by name (? shows the names)
```

Secrets can also come from `SPEKTR_SECRET_<NAME>` environment variables, which win. They're sent only to modules that declare them, and never written to a case file or a log.

Screenshots need Chrome or Chromium; `spektr setup capture` downloads a headless one if you don't have it.

## Using it responsibly

spektr is for authorized investigations: missing-persons work such as Trace Labs Search Parties, research into your own footprint, or engagements where you have permission. Built-in modules read only what any visitor could; searches that services forbid automating open in your browser instead. Look up only people and accounts you're allowed to.

## Documentation

- [`docs/PRD.md`](docs/PRD.md): what spektr is for, the user journeys
- [`docs/TUI_SPEC.md`](docs/TUI_SPEC.md): every screen and key
- [`docs/MODULES.md`](docs/MODULES.md), [`docs/PLAYBOOKS.md`](docs/PLAYBOOKS.md): modules, the protocol, profiles, launchers, playbooks
- [`docs/DATA_MODEL.md`](docs/DATA_MODEL.md), [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md): the case file, the crates, the CLI reference
- [`demos/`](demos/): VHS tapes of the tutor, a pivot, a capture and a finding export

## Development

```
cargo build
cargo nextest run --workspace                         # no network
cargo clippy --workspace --all-targets -- -D warnings
cargo xtask deps-check                                # crate boundaries, banned dependencies
cargo xtask qa                                        # QA scenarios through a PTY harness
cargo xtask demos                                     # render demos/*.tape (needs VHS)
```

Tests never touch the network: modules replay recorded fixtures (fictional data only) from `crates/spektr-modules/fixtures/`.
