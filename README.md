# spektr

spektr is a terminal workbench for open-source investigations. You seed what you know (a username, an email, a photo, a domain) and spektr expands it into a typed graph of entities, automatically and within the rules you set: scope, depth, hubs, budgets, and passive-only rules profiles such as Trace Labs'. Every edge stays tied to the exact evidence it came from, every action lands in a hash-chained audit log anyone can verify, and the whole investigation is one portable `.spektr` file. You turn what you learn into findings and export them.

> **Status:** early development. The engine and the headless CLI work (Phases 0–2): `spektr new`, `run`, `query`, `verify`, `export --format json` and `doctor`, with the `dns`, `github` and `username-sweep` modules. The TUI is in progress. The v1 specification is in [`docs/`](docs/PRD.md).

Licensed under [Apache-2.0](LICENSE).
