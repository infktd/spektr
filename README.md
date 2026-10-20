# spektr

spektr is a terminal workbench for open-source investigations. You seed what you know (a username, an email, a photo, a domain) and spektr expands it into a typed graph of entities, automatically and within the rules you set: scope, depth, hubs, budgets, and passive-only rules profiles such as Trace Labs'. Every edge stays tied to the exact evidence it came from, every action lands in a hash-chained audit log anyone can verify, and the whole investigation is one portable `.spektr` file. You turn what you learn into findings and export them.

> **Status:** early development. The engine, the headless CLI and the TUI core work (Phases 0–3): `spektr` opens the launcher and `spektr open <case>` the workspace; `new`, `run`, `query`, `verify`, `export --format json` and `doctor` run headless. Modules so far: `dns`, `github` and `username-sweep`. Timeline, path, review, playbooks and findings are in progress. The v1 specification is in [`docs/`](docs/PRD.md).

Licensed under [Apache-2.0](LICENSE).
