# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Cargo workspace: `spektr-model`, `spektr-protocol`, `spektr-store`, `spektr-engine`, `spektr-modules`, `spektr-tui`, the `spektr` binary, and `xtask`.
- `cargo xtask deps-check`, enforcing crate boundaries and banned dependencies.
- `spektr-model`: the 15 entity types and their normalizers, platform handle rules, the predicate vocabulary with extension types and predicates, attributes, strength and the known-hub list.
- `spektr-store`: the case-file schema (including playbook tables), store actor and read pool, audited repositories, merge/unmerge, graph queries (ego, degree, k-hop, shortest path, timeline), zstd blobs and the verifiable audit chain.
- `spektr-protocol`: the module manifest and exec protocol v1, with generated JSON Schemas.
- `spektr-engine`: scheduler, eligibility, rules profiles, scope, ingestion, the `ip` correlator, the review queue and the exec runtime (per-job and serve).
- `spektr-modules`: HTTP and a DNS stub resolver with record/replay; built-in `dns`, `github` and `username-sweep` (307 sites).
- `spektr new`, `run`, `query`, `verify`, `export --format json`, `doctor`.
- CI on `aarch64-apple-darwin`, `x86_64-apple-darwin`, `x86_64-unknown-linux-gnu` and `aarch64-unknown-linux-gnu`.
