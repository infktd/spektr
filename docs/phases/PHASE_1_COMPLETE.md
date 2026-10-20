# Phase 1 complete: model and store

Branch `phase-1-model-store` · 2026-10-20

## What shipped

### `spektr-model` (DATA_MODEL §1–5, §7)

- **Types:** the 15 core entity types with glyphs, ASCII glyphs and the entities-pane order (`types.rs`). Extension names follow `x.<module>.<name>`. `EntityRef` parses `<type>:<value>`, split at the first colon.
- **Normalizers** (`normalize.rs`), one per type, with automatic hostname typing (`domain` ↔ `subdomain`). Edges implied at insert come back already normalized:
  - `subdomain_of`
  - `at_domain`
  - `on_host`
  - an account's `links_to` its profile URL

  Person and organization keys take a hint: `uuid:<v7>` or `dedup:<module>:<id>`. They never derive from the name. Image keys are `sha256:<hex>`. Certificate keys are `sha256:<fingerprint>` or `crtsh:<id>`. Bad input is a hard `NormalizeError`.
- **Platforms** (`platforms.rs`): handle rules for 126 built-in platforms, covering case sensitivity, allowed characters, length and the profile-URL template. Mastodon and Pixelfed use the `user@instance` form. Module-declared platforms are supported, and so is the fallback rule for unknown platforms.
- **Vocabulary** (`predicates.rs`, `extension.rs`):
  - the 26 core predicates, each with its allowed source and target types; symmetric predicates accept either orientation
  - extension types (`exact`, `trim`, `lowercase`, `casefold`, `url`) and extension predicates, namespace-checked against the declaring module
  - `validate_edge` rejects any unknown predicate or endpoint type
  - artifact kinds with their display labels (`avatar_phash` → `same_avatar`, …)
- **Attributes** (`attrs.rs`): `Attr { value, observation, event }`, with the same JSON shape as the exec protocol (`{"text": …}`, `{"time": …}`, …), plus the event and well-known names.
- **Strength** (`strength.rs`): single, corroborated or confirmed, computed from the number of distinct source keys. A source key comes from the registrable domain of the source URL.
- **Known hubs** (`data/hubs.toml`, embedded): email providers, platform and CDN domains, MX/CDN host suffixes, CDN and cloud CIDR ranges, the major cloud ASNs, and public CA intermediates.
- **Other building blocks:** `IdGen` (UUIDv7, optionally seeded for `SPEKTR_SEED`), `Clock` (system or fixed, for `SPEKTR_CLOCK`), the canonical timestamp format, `Actor` and `EntityFlags`.

### `spektr-store` (DATA_MODEL §6, §8–10; PLAYBOOKS §3)

- **Schema:** the whole of DATA_MODEL §8, **including `playbook_run`, `playbook_step` and `checklist_item`**, as embedded migration 1. Columns are exactly as specified; indexes were added for the query paths.
- **Open-time checks:**
  - pragmas `WAL`, `foreign_keys=ON`, `synchronous=NORMAL`, `busy_timeout=5000`
  - `application_id` `0x53504B52` ("SPKR") and `user_version` 1
  - `quick_check` on open
  - clear errors for a foreign file, a newer schema ("upgrade spektr to open it"), an existing path or a corrupt file
- **`Db` (sync core):** runs closures in a `Tx` (an IMMEDIATE transaction) or a `Reader` (a deferred-transaction snapshot).
- **`Store` (async handle):** a dedicated writer thread plus a pool of read-only connections, each on its own thread (default 4). It has async `write`, `read`, `verify` and `flush`, and `*_blocking` variants for callers without a runtime.
- **Repositories, every one audited:** meta, blobs, observations, entities (upsert, attributes, flags, the degree-based hub refresh), edges (upsert, evidence, confirm, unconfirm, reject, restore), merge and unmerge, jobs, findings (including entity and evidence links), notes, scope rules, module settings, review items, playbook runs and steps, checklist items, and `record` for audit-only analyst actions.
- **Graph queries:**
  - `ego`: paginated groups by predicate × direction (plus artifact kind for `shares_artifact`), with per-row strength and source counts. Merged entities resolve to their roots, self-loops created by a merge are dropped, and duplicate edges fold into one row.
  - also `degree`, `entity_strength`, `edge_strength`, `khop`, `shortest_path` (max hops) and `timeline`
- **Blobs:** zstd level 3, keyed by the SHA-256 of the uncompressed bytes, deduplicated, with a configurable size limit.
- **Audit chain:**
  - one row per mutation, written in the same transaction; no-ops write none
  - canonical JSON payloads
  - `hash = SHA-256(prev ‖ seq BE ‖ ts ‖ 0 ‖ actor ‖ 0 ‖ action ‖ 0 ‖ payload)`
  - `verify` recomputes the chain, re-hashes every blob after decompressing it, and checks that every observation's blob exists. It reports the first failure with its location: audit row, chain tip, blob, or observation.
- **`synth`:** a deterministic generator of fictional cases. The 2,000-entity benchmark uses it, and Phase 3 will reuse it for its 2,000-entity performance case.

## Exit criteria

| Criterion | Status | Evidence |
|---|---|---|
| Each normalizer has property tests: idempotent, stable across case and Unicode variants | ✓ | `crates/spektr-model/tests/normalize_props.rs`: 14 property tests, 256 cases each. They cover every type (identity keys included) against case flips, full-width forms, decomposed accents, padding (including U+3000), `@` prefixes, trailing dots, IDN vs punycode, default ports, IPv4-mapped/expanded IPv6, ASN prefixes, phone formats, and geo/place spellings |
| Tamper test: changing any audit row or blob makes `verify` fail and name the location | ✓ | `crates/spektr-store/tests/audit.rs`. Every column (ts, actor, action, payload, prev_hash, hash) of every row is tampered one at a time; each is caught at exactly that row. Deleted rows, a truncated tail (caught at the chain tip), corrupted blob bytes, substituted blob content and a missing blob are all caught too |
| Merge/unmerge round trip leaves the graph exactly as before | ✓ | `crates/spektr-store/tests/merge.rs`. The comparison covers every entity row, edge and evidence row, plus every root's ego view and the root list. It includes chained merges with re-pointing, unmerged newest-first |
| Ego query on a generated 2,000-entity case < 20 ms on the M1 Pro | ✓ | `cargo bench -p spektr-store --bench ego`: p95 **3.43 ms** for the largest ego (560 neighbours), 71 µs for a typical account (output below) |

## Deviations from the spec, and why

1. **A chain-tip record in `case_meta` (`audit_tip`).** A hash chain alone can't detect that its last rows were deleted. Every write transaction now records `{seq, hash}` for the newest row, and `verify` compares against it. The tip is bookkeeping, so it isn't itself audited (it couldn't be without recursion). Its key is reserved, so `set_meta` refuses it.
2. **Review-item states** are `pending`, `accepted`, `rejected` and `later`. The schema only names the `pending` default; the other three come from the Review keys in TUI_SPEC §6.
3. **Derived entities sit one level deeper than the entity that implied them.** For example, a seed email at depth 0 puts its domain at depth 1. MODULES §5 defines depth for entities that jobs create, but says nothing about implied ones. Depth + 1 is the conservative reading, consistent with "avoid rabbit holes".
4. **Entity strength** (DATA_MODEL §4, "strongest among edges connecting it to the seed or subject") means the strongest *direct* active edge between the entity and a seed or the subject. An entity with no such edge, or a seed itself, has no entity strength. Path-based strength would be ambiguous and expensive to compute.
5. **Normalizer readings** where the spec leaves room:
   - `url` accepts only http/https; other schemes are a hard error (`mailto:` becomes `has_email`, not a URL)
   - usernames may not contain whitespace
   - phone numbers need only parse, not be "valid", because the reserved fictional 555-01XX numbers fail libphonenumber's validity check; `phone-parse` reports validity as an attribute
   - `location` treats `lat, lon` (optionally `geo:`-prefixed) as coordinates and anything else as a place
   - hostnames that are IPs, a single label, or a bare public suffix are rejected
6. **Edges keep the endpoints they were written against.** DATA_MODEL §6 says edges are never rewritten, so reads resolve endpoints through `merged_into` and fold duplicates. Re-ingesting a merged entity's key returns that entity's own row, which keeps unmerge exact.
7. **Merges resolve both sides to their roots, and must be undone newest-first** wherever later merges involve the same entities. An out-of-order unmerge is refused with an explanation.
8. **A rejected edge stays rejected when a module reports it again.** New evidence is still attached, but the analyst's decision stands.
9. **The hub list adds major cloud ASNs** (Cloudflare, Google, AWS, Microsoft, Fastly, Akamai, Meta, GitHub) alongside the four categories in DATA_MODEL §7. CDN IP ranges are already a hub category, and these ASNs are their natural pair.
10. **New dependencies** in their own `deps:` commit: `icu_normalizer` 2.3.0 (NFKC; it was already in the graph via `idna`) and `icu_casemap` 2.3.0 (full case folding for `place:` keys and the `casefold` rule). Both come from ICU4X and are Unicode-3.0 licensed. deps-check and cargo-deny pass.
11. **`tokio` in `spektr-store`.** The store uses only `tokio::sync::oneshot`, which needs no runtime, so the actor's replies can be awaited. ARCHITECTURE §2 doesn't list tokio for the store, but it adds no build cost because the engine and the binary already compile it.

## Known issues

- The 2,000-entity ego benchmark runs in the `bench` (release) profile for the exit criterion. In the dev profile, p95 is 11.3 ms with one 53 ms outlier, from a warm-up hiccup (output below). Phase 3's input-to-render target will be measured in whatever profile the TUI runs in.
- `Reader::entity_observations` issues one query per incident edge. The ego view uses a batched path, and `entity_observation_count` reuses it. The inspector list will move to the batched path if Phase 3 shows it is slow on large egos.

## Bug found by the property tests

`normalize_asn` sliced `s[..2]` without checking for a character boundary, so any input whose first character was multibyte panicked (at `normalize.rs:497`). Found by `idempotent_on_arbitrary_input`, fixed with `str::get` before the model commit. I didn't record proptest's minimal failing input; the panic location and message were enough to fix it.

## Command output

```
$ cargo fmt --check && echo fmt-ok
fmt-ok
$ cargo clippy --workspace --all-targets -- -D warnings
(no warnings)
$ cargo nextest run --workspace
Starting 85 tests across 16 binaries
Summary [   0.695s] 85 tests run: 85 passed, 0 skipped
$ cargo xtask deps-check
deps-check: ok (8 members, 490 resolved packages; default and all features)
```

The exit-criterion tests, by name, from that run:

```
spektr-model::normalize_props account_variants
spektr-model::normalize_props asn_variants
spektr-model::normalize_props breach_and_certificate_variants
spektr-model::normalize_props email_variants
spektr-model::normalize_props hostname_variants
spektr-model::normalize_props idempotent_identity_keys
spektr-model::normalize_props idempotent_on_arbitrary_input
spektr-model::normalize_props idempotent_on_plausible_input
spektr-model::normalize_props ip_variants
spektr-model::normalize_props location_variants
spektr-model::normalize_props phone_variants
spektr-model::normalize_props url_variants
spektr-model::normalize_props username_accents
spektr-model::normalize_props username_variants
spektr-store::audit chain_is_valid_and_formula_matches_spec
spektr-store::audit deleting_rows_is_detected
spektr-store::audit every_mutation_writes_exactly_one_row_and_no_ops_write_none
spektr-store::audit tampering_a_blob_is_detected_at_that_blob
spektr-store::audit tampering_any_audit_row_is_detected_at_that_row
spektr-store::graph cache_lookup_and_entity_observations
spektr-store::graph derived_edges_are_created_at_insert_with_evidence
spektr-store::graph ego_groups_page_and_correlations_group_by_artifact
spektr-store::graph paths_khop_and_hubs
spektr-store::graph strength_levels_and_edge_states
spektr-store::graph symmetric_edges_are_stored_once
spektr-store::graph synthetic_case_has_two_thousand_entities_and_a_large_ego
spektr-store::graph timeline_lists_event_attributes_in_order
spektr-store::merge chained_merges_repoint_and_undo_newest_first
spektr-store::merge merge_folds_edges_and_unmerge_restores_exactly
spektr-store::merge merge_rules
```

### CI

[Run 39385087007](https://github.com/infktd/spektr/actions/runs/39385087007) on `ed1f5f4`:

```
success ed1f5f4
lint (x86_64-unknown-linux-gnu): success
test (aarch64-unknown-linux-gnu): success
test (aarch64-apple-darwin): success
test (x86_64-unknown-linux-gnu): success
test (x86_64-apple-darwin): success
```

### Ego benchmark (M1 Pro, `bench` profile)

```
$ cargo bench -p spektr-store --bench ego
    Finished `bench` profile [optimized] target(s) in 32.32s
generated 2012 entities, 2887 edges in 114.55ms (profile: release)
seed ego: 560 neighbours in 2 groups
seed (largest ego)     runs 200  min   3.356ms  p50   3.413ms  p95   3.433ms  max   3.577ms  ok (< 20 ms)
typical account        runs 200  min  69.708µs  p50  70.292µs  p95  71.208µs  max  73.042µs  ok (< 20 ms)
seed, last page        runs 200  min   3.350ms  p50   3.415ms  p95   3.429ms  max   3.586ms  ok (< 20 ms)
```

The same benchmark in the dev profile, for reference:

```
$ cargo bench -p spektr-store --bench ego --profile dev
generated 2012 entities, 2887 edges in 359.99ms (profile: dev)
seed ego: 560 neighbours in 2 groups
seed (largest ego)     runs 200  min  10.453ms  p50  10.674ms  p95  11.269ms  max  53.471ms  ok (< 20 ms)
typical account        runs 200  min 208.417µs  p50 217.292µs  p95 232.167µs  max 323.667µs  ok (< 20 ms)
seed, last page        runs 200  min  10.456ms  p50  10.678ms  p95  11.011ms  max  11.802ms  ok (< 20 ms)
```

Each query is timed end to end through `Db::read`, which includes beginning and finishing the read transaction. The seed is the worst case: its 560 neighbours fold into two groups.

## How Jay can test it

1. `git checkout main && git pull`, then run `cargo nextest run --workspace`. Expect `85 tests run: 85 passed`.
2. Run the benchmark: `cargo bench -p spektr-store --bench ego`. All three lines should end in `ok (< 20 ms)`.
3. Run the tamper test on its own and read it: `cargo nextest run -p spektr-store --test audit`. Then see `crates/spektr-store/tests/audit.rs` for what gets tampered.
4. Try the normalizers in a scratch test, or read `crates/spektr-model/src/normalize.rs` (its `spec_examples` unit test shows the DATA_MODEL examples). For instance, `account:github:JMarlow-WAV` → `github:jmarlow-wav`, and `HTTPS://JMarlow.Example:443/#top` → `https://jmarlow.example/`.

There's no CLI to drive yet; `spektr new`, `run` and the rest arrive in Phase 2.
