# spektr

spektr is a terminal workbench for open-source investigations. You seed what you know (a username, an email, a photo, a domain) and spektr expands it into a typed graph of entities, automatically and within the rules you set: scope, depth, hubs, budgets, and passive-only rules profiles such as Trace Labs'. Every edge stays tied to the exact evidence it came from, every action lands in a hash-chained audit log anyone can verify, and the whole investigation is one portable `.spektr` file. You turn what you learn into findings and export them.

> **Status:** early development. The data model and the case-file store are built (Phases 0–1); the engine, CLI and TUI are in progress, so there is nothing to run yet. The v1 specification is in [`docs/`](docs/PRD.md).

Licensed under [Apache-2.0](LICENSE).
