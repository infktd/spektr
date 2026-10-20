# spektr — Data Model

This document defines the entity types, predicates, normalization, strength, attributes, storage schema and the audit chain. It is the contract for `spektr-model` and `spektr-store`.

## 1. Entities

An entity is `(type, key, value, attributes)`:

- **`key`** is the normalized identity. It is unique per type.
- **`value`** is the display form. It is usually the input as first seen, after trimming whitespace.

Two inputs that normalize to the same key are the same entity. This is the identity merge, and it happens automatically at insert.

### 1.1 Core entity types (closed set, v1)

| Type | Glyph | ASCII | Key (normalized identity) | Example value |
|---|---|---|---|---|
| `person` | ◎ | P | `uuid:<v7>`, or `dedup:<module>:<id>` when the source provides a stable id (§1.3); never merged by name | Jordan Marlow |
| `username` | ~ | ~ | NFKC, strip leading `@`, lowercase | jmarlow.wav |
| `account` | ◐ | a | `platform:handle`, with the platform's own handle rules (§1.4) | github:jmarlow-wav |
| `email` | @ | @ | trim, lowercase, IDNA domain → punycode | jordan.marlow@mail.example |
| `phone` | + | + | E.164 (libphonenumber), using the case's `default_region` | +15555550142 |
| `image` | ▨ | i | `sha256:<hex>` of the image bytes; the bytes are always stored as a blob (§1.5) | IMG_2214.jpg |
| `location` | ⌖ | L | `geo:<lat>,<lon>` to 5 decimals, or `place:<casefolded text>` | 45.52, -122.68 |
| `organization` | ▣ | O | `uuid:<v7>`, or `dedup:<module>:<id>` (§1.3); never merged by name | Kestrel Labs LLC |
| `url` | ↗ | U | WHATWG parse; lowercase scheme and host; IDNA host; drop default port and fragment | https://jmarlow.example/ |
| `domain` | ◆ | D | registrable domain (eTLD+1, Public Suffix List), lowercase, punycode, no trailing dot | kestrel.example |
| `subdomain` | ◇ | d | full hostname with more labels than its registrable domain; same rules as `domain` | vpn.kestrel.example |
| `ip` | ● | I | canonical `IpAddr`; IPv4-mapped IPv6 becomes IPv4; IPv6 compressed lowercase | 203.0.113.24 |
| `asn` | ◈ | A | `AS<number>` | AS64500 |
| `certificate` | ⊟ | C | `sha256:<DER fingerprint>` when DER is known, else `crtsh:<id>` | *.kestrel.example · R11 |
| `breach` | ✕ | B | `<source>:<slug>` | hibp:examplebreach2024 |

Rules:

- **Hostname typing is automatic.** Normalizing a hostname decides whether it's a `domain` or a `subdomain`. A subdomain gets a `subdomain_of` edge to its registrable domain at insert (`origin = model:normalize`).
- **Some derived edges are created at insert:**
  - email → `at_domain` → domain
  - url → `on_host` → domain or subdomain
  - account → `links_to` → its profile URL, when known
- **`person` and `organization` never merge automatically.** Two people with the same name are not the same person. Merging them is always an analyst action, possibly prompted by the review queue.
- **Unparseable input is a hard error,** returned to the caller. That covers a phone without a region, a malformed URL, or an invalid IP. Modules that emit bad values get their message rejected and logged; the rest of the job continues.
- **Platform handle rules** live in a table in `spektr-model` (`platforms.rs`). Each platform has an id, display name, case sensitivity, allowed characters, and a profile URL template. v1 ships rules for every platform in the username-sweep site list.

### 1.2 Extension types

Modules may declare types named `x.<module>.<name>`, e.g. `x.shodan.service`. A declaration must pick a normalization rule from a fixed set: `exact`, `trim`, `lowercase`, `casefold`, `url`. That keeps identity deterministic. Extension types render with the glyph `·` and the ASCII `x`.

### 1.3 Person and organization dedup

A module emitting a person or organization may include `dedup_id`: a stable identifier from its source, such as an RDAP entity handle or a GitHub user id. The key becomes `dedup:<module>:<dedup_id>`, so repeated runs update one entity instead of creating duplicates. Without `dedup_id`, the key is `uuid:<v7>` and every emission is a new entity. Analyst-created people and organizations always get `uuid:` keys.

### 1.4 Account handle rules

- **Known platforms:** a platform listed in `spektr-model/src/platforms.rs` uses its own rules: case sensitivity, allowed characters, profile URL template.
- **Module-declared platforms:** a user module may declare a platform in its manifest (`[[module.platforms]]`: `id`, `case_sensitive`, `profile_url`), e.g. an exec `lemmy` module with `user@instance` handles. Built-in platforms, including `mastodon`, are defined only in `platforms.rs`.
- **Fallback for unknown platforms:** the platform id must match `[a-z0-9.-]{1,32}`; the handle is trimmed, NFKC-normalized and lowercased. The `account` key is then `<platform>:<handle>`.

### 1.5 Image bytes

An `image` entity is always backed by a blob holding its bytes. Every source works the same way:

- **Module-found images:** the module emits an observation whose raw data is the image, then an `entity` message of type `image` that references it (`"blob": "<observation id>"`).
- **Attachments and captures:** these create the blob themselves.

The store computes the key from that blob's SHA-256. Modules that consume images (`exif`) receive the bytes in their job input (MODULES §3.2, §4).

### 1.6 Flags

Each entity carries these flags:

- `seed`
- `subject` (the case's subject)
- `hub`
- `out_of_scope`
- `new` (UI-only, derived from last-viewed time; not stored)

## 2. Attributes

`attrs` is a map from name to a list of `Attr`:

```rust
struct Attr {
    value: AttrValue,           // Text | Int | Bool | Time (jiff Timestamp) | Date | Geo{lat,lon} | Url
    observation: ObservationId, // provenance; analyst-entered attrs point at an analyst observation
    event: bool,                // true → appears on the timeline
}
```

- **Different sources can disagree.** A list per name keeps every source's value, and the inspector shows the conflicts.
- **Event attributes feed the timeline tab.** The standard names are:
  - `created_at`
  - `last_active`
  - `posted_at`
  - `taken_at`
  - `archived_first`
  - `archived_last`
  - `registered_at`
  - `expires_at`
- **Well-known attribute names** include:
  - `display_name`
  - `bio`
  - `avatar_url`
  - `avatar_phash`
  - `profile_url`
  - `stated_location`
  - `title` (web page)
  - `favicon_mmh3`
  - `analytics_ids`
  - `line_type` (phone)
  - `country`
  - `device` (EXIF)

  The full list is in `spektr-model::attrs`.

## 3. Predicates (closed vocabulary, v1)

Each predicate declares its allowed source and target types. `↔` marks symmetric predicates, which are stored with `src < dst` (by id) so each pair appears once.

| Predicate | Source → Target | Notes |
|---|---|---|
| `uses_handle` | person → username | |
| `has_account` | person, username → account | |
| `variant_of` ↔ | username ↔ username | Handle variants (e.g. jm4rlow) |
| `has_email` | person, account, organization, url, domain, subdomain → email | From url/domain/subdomain: an address published on the site (e.g. a mailto link) |
| `has_phone` | person, account, organization → phone | |
| `related_to` ↔ | person ↔ person | attr `relation`: family, friend, partner, colleague, unknown |
| `works_at` | person → organization | attrs `role`, `from`, `to` |
| `studied_at` | person → organization | |
| `located_in` | person, account, organization → location | attr `kind`: stated, home, work, seen |
| `posted` | account → image, url | |
| `taken_at` | image → location | From EXIF GPS |
| `depicts` | image → person | Analyst only |
| `hosted_at` | image → url | |
| `links_to` | account, url, domain, subdomain → url | |
| `at_domain` | email → domain, subdomain | Created at insert |
| `on_host` | url → domain, subdomain | Created at insert |
| `subdomain_of` | subdomain → domain | Created at insert |
| `resolves_to` | domain, subdomain → ip | |
| `reverse_resolves_to` | ip → domain, subdomain | |
| `mail_handled_by` | domain, subdomain → domain, subdomain | |
| `registered_by` | domain → email, organization, person | |
| `issued_for` | certificate → domain, subdomain | |
| `hosted_in_asn` | ip → asn | |
| `operated_by` | asn → organization | |
| `appears_in_breach` | email, username → breach | |
| `shares_artifact` ↔ | any ↔ any | attrs `artifact` (`avatar_phash`, `analytics_id`, `favicon_mmh3`, `registrant_email`, `cert_san`, `ip`) and `value`. The TUI labels these by artifact (TUI_SPEC §3), e.g. `avatar_phash` displays as `same_avatar` |

Extension predicates are `x.<module>.<name>` and must declare their source and target types in the module manifest. Any edge whose predicate, source type or target type isn't in the registry is rejected.

**Edge identity:** `(src, predicate, dst, discriminator)`. The discriminator is empty except for `shares_artifact`, where it is `artifact:value`. The same fact reported again adds evidence to the existing edge instead of creating a new one.

## 4. Strength

Strength is **computed, not asserted by modules.** Modules never send confidence numbers.

- Each evidence row on an edge has a **source key**:
  - for an observation with a source URL: the registrable domain of that URL (`api.github.com` → `github.com`)
  - for local modules (exif, phone-parse): the module name
  - for correlator edges: `correlator:<rule>`
  - for edges created at insert (`subdomain_of`, `at_domain`, `on_host`): the source key of the observation that created the entity
- **Every edge has evidence; there are no evidence-less edges.** Non-module sources write *synthetic observations*:

| Source | `kind` | `module` | `module_version` | `manifest_hash` | `blob_sha256` | `summary` |
|---|---|---|---|---|---|---|
| Insert-time edges | — | — | — | — | — | Reuse the observation that created the entity |
| Correlator | `correlator` | `correlator:<rule>` | spektr version | `builtin` | NULL | Artifact kind, value, and the entities compared (e.g. dHash distance) |
| Analyst action (confirm, manual entity, note-backed attr) | `analyst` | `analyst` | spektr version | `builtin` | NULL, or the attached file | Action and free text |
| Web capture | `capture` | `capture` | spektr version | `builtin` | The HTML; screenshot and headers are linked in `summary` | URL, final URL, status, screenshot blob hash |
| Attachment | `attachment` | `attachment` | spektr version | `builtin` | The file | Original filename, size, MIME type |

- Strength:

| Strength | Rule | Bar |
|---|---|---|
| `single` | 1 distinct source key | `▰▱▱` |
| `corroborated` | ≥ 2 distinct source keys | `▰▰▱` |
| `confirmed` | analyst confirmed the edge (`v`), regardless of count | `▰▰▰`, the only bar drawn in the ok color |

- Edge states:
  - **`active`**
  - **`rejected`:** the analyst rejected it. It's hidden by default, kept for audit, and excluded from strength and correlation.
- **Entity strength** in lists and the inspector is the strongest strength among edges connecting it to the seed or subject. It's computed on read and cached in memory.

## 5. Correlation (v1)

Correlation runs inside the engine after every ingestion batch.

| Rule | Trigger | Output |
|---|---|---|
| Identity merge | Same normalized key | No edge; it's the same entity (insert-time upsert) |
| `avatar_phash` | Two accounts whose `avatar_phash` values are within Hamming distance ≤ 6 (64-bit dHash) | `shares_artifact` |
| `analytics_id` | Two urls, domains or subdomains sharing a GA, GTM or AdSense id | `shares_artifact` |
| `favicon_mmh3` | Two hosts with the same favicon hash, **only if** that hash is seen on ≤ 20 hosts in the case | `shares_artifact` |
| `registrant_email` | Two domains with `registered_by` → the same email | `shares_artifact` |
| `cert_san` | Two domains or subdomains in the SANs of the same certificate | `shares_artifact` |
| `ip` | Two domains or subdomains resolving to the same **non-hub** IP | `shares_artifact` |
| Same-person candidate | Two `person` entities linked to the same email, phone or account with strength ≥ corroborated | Review queue item; never an edge or a merge |

Hub entities (§7) are excluded from every artifact rule.

## 6. Merge

- **Merging** writes a `merge` row `(survivor, merged)` and sets `entity.merged_into` on the merged entity.
- **`merged_into` always points at the root survivor, never at another merged entity**, so a single `COALESCE(merged_into, id)` resolves every read.
  - Merging into an entity that is itself merged first resolves that entity to its root.
  - When a survivor S, which already has merged entities, is merged into T, every entity pointing at S is re-pointed to T in the same transaction. Each re-point is recorded in the merge row's `repointed` list.
- **Edges are not rewritten.** Unmerge is exact: it clears `merged_into` on the merged entity, restores the pointers listed in `repointed`, and marks the merge row `undone_at`.
- Both operations write audit rows.
- Merging is allowed only between entities of the same type.

## 7. Hubs

An entity is a hub if either:

- its degree in the case exceeds `hub_threshold` (default 50; configurable per case), or
- it matches the shipped **known-hub list** (`spektr-model/data/hubs.toml`), which covers:
  - major email providers' domains and MX hosts (gmail.com, outlook.com, …)
  - major CDN and cloud IP ranges
  - public CA intermediates
  - platform domains themselves (github.com, bsky.app, …)

Hubs are never expanded automatically. The analyst can force expansion with `:expand`.

## 8. Storage schema (SQLite, one file per case)

Pragmas on open:

- `journal_mode=WAL`
- `foreign_keys=ON`
- `synchronous=NORMAL`
- `busy_timeout=5000`

`application_id` is set to a fixed spektr value, and `user_version` is the schema version.

IDs are UUIDv7 stored as 16-byte BLOBs, except `finding.id` and `audit.seq`, which are integers.

```sql
CREATE TABLE case_meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);  -- JSON values
-- keys: name, created_at, subject_entity, last_seen (date), rules_profile, default_depth,
--       default_region, hub_threshold, schema_version, spektr_version

CREATE TABLE entity (
  id BLOB PRIMARY KEY, type TEXT NOT NULL, key TEXT NOT NULL, value TEXT NOT NULL,
  attrs TEXT NOT NULL DEFAULT '{}', flags INTEGER NOT NULL DEFAULT 0, depth INTEGER NOT NULL,
  merged_into BLOB REFERENCES entity(id), created_at TEXT NOT NULL, created_by TEXT NOT NULL,
  UNIQUE (type, key));

CREATE TABLE edge (
  id BLOB PRIMARY KEY, src BLOB NOT NULL REFERENCES entity(id), dst BLOB NOT NULL REFERENCES entity(id),
  predicate TEXT NOT NULL, discriminator TEXT NOT NULL DEFAULT '', attrs TEXT NOT NULL DEFAULT '{}',
  state TEXT NOT NULL DEFAULT 'active', confirmed_at TEXT, origin TEXT NOT NULL, created_at TEXT NOT NULL,
  UNIQUE (src, predicate, dst, discriminator));
CREATE INDEX edge_src ON edge(src); CREATE INDEX edge_dst ON edge(dst);

CREATE TABLE observation (
  id BLOB PRIMARY KEY, job_id BLOB REFERENCES job(id), module TEXT NOT NULL, module_version TEXT NOT NULL,
  manifest_hash TEXT NOT NULL, input_entity BLOB REFERENCES entity(id), source_url TEXT,
  content_type TEXT, blob_sha256 BLOB REFERENCES blob(sha256), summary TEXT NOT NULL DEFAULT '{}',
  kind TEXT NOT NULL DEFAULT 'module',   -- module | capture | attachment | analyst | correlator  (see §4)
  created_at TEXT NOT NULL);
CREATE INDEX obs_cache ON observation(module, module_version, input_entity, created_at);

CREATE TABLE edge_evidence (edge_id BLOB NOT NULL REFERENCES edge(id),
  observation_id BLOB NOT NULL REFERENCES observation(id), source_key TEXT NOT NULL,
  PRIMARY KEY (edge_id, observation_id));

CREATE TABLE blob (sha256 BLOB PRIMARY KEY, size INTEGER NOT NULL, compression TEXT NOT NULL, data BLOB NOT NULL);
-- sha256 is over the UNCOMPRESSED bytes, computed by spektr-store on insert, never by a module.

CREATE TABLE job (id BLOB PRIMARY KEY, module TEXT NOT NULL, input_entity BLOB NOT NULL REFERENCES entity(id),
  trigger TEXT NOT NULL,                 -- auto | manual | playbook | tutor
  run_id BLOB REFERENCES playbook_run(id),   -- set when trigger = playbook (PLAYBOOKS §3)
  cached_observation BLOB REFERENCES observation(id),   -- set when state = cached
  joined_job BLOB REFERENCES job(id),         -- set when a playbook job joined an already queued/running job
  depth INTEGER NOT NULL, state TEXT NOT NULL,  -- queued|running|done|failed|blocked|cached
  reason TEXT, error TEXT, queued_at TEXT NOT NULL, started_at TEXT, finished_at TEXT);

CREATE TABLE finding (id INTEGER PRIMARY KEY AUTOINCREMENT, title TEXT NOT NULL, category TEXT NOT NULL,
  claim TEXT NOT NULL DEFAULT '', relevance TEXT NOT NULL DEFAULT '', status TEXT NOT NULL DEFAULT 'draft',
  created_at TEXT NOT NULL, updated_at TEXT NOT NULL);
CREATE TABLE finding_entity (finding_id INTEGER REFERENCES finding(id), entity_id BLOB REFERENCES entity(id),
  PRIMARY KEY (finding_id, entity_id));
CREATE TABLE finding_evidence (finding_id INTEGER REFERENCES finding(id), observation_id BLOB REFERENCES observation(id),
  PRIMARY KEY (finding_id, observation_id));

CREATE TABLE note (id BLOB PRIMARY KEY, entity_id BLOB REFERENCES entity(id), finding_id INTEGER REFERENCES finding(id),
  body TEXT NOT NULL, created_at TEXT NOT NULL);
CREATE TABLE scope_rule (id BLOB PRIMARY KEY, kind TEXT NOT NULL,   -- allow|deny
  matcher TEXT NOT NULL,                                              -- domain_suffix|cidr|platform|regex
  pattern TEXT NOT NULL, created_at TEXT NOT NULL);
CREATE TABLE module_setting (module TEXT PRIMARY KEY, enabled INTEGER NOT NULL, config TEXT NOT NULL DEFAULT '{}');
CREATE TABLE review_item (id BLOB PRIMARY KEY, kind TEXT NOT NULL,  -- sweep_unsure|same_person
  payload TEXT NOT NULL, state TEXT NOT NULL DEFAULT 'pending', created_at TEXT NOT NULL, decided_at TEXT);
CREATE TABLE merge (id BLOB PRIMARY KEY, survivor BLOB NOT NULL REFERENCES entity(id), merged BLOB NOT NULL REFERENCES entity(id),
  repointed TEXT NOT NULL DEFAULT '[]',  -- JSON list of [entity_id, previous merged_into] re-pointed by this merge
  created_at TEXT NOT NULL, undone_at TEXT);

-- playbook_run, playbook_step and checklist_item: defined in PLAYBOOKS.md §3, and part of this same schema.

CREATE TABLE audit (seq INTEGER PRIMARY KEY, ts TEXT NOT NULL, actor TEXT NOT NULL, action TEXT NOT NULL,
  payload TEXT NOT NULL, prev_hash BLOB NOT NULL, hash BLOB NOT NULL);
```

Migrations are embedded and run on open. A case created by a newer schema refuses to open with a clear message. It is never downgraded.

## 9. Audit chain

- **Every mutation writes exactly one audit row in the same transaction.** That covers entities, edges, evidence, merges, findings, notes, scope, settings, review decisions, jobs queued or blocked, playbook runs and steps, and checklist resolutions.
- **Actors:**
  - `analyst`
  - `engine`
  - `module:<name>@<version>`
  - `correlator:<rule>`
  - `playbook:<name>@<version>`
  - `tutor`
- **`payload` is canonical JSON:** keys sorted, compact, UTF-8. Use `serde_json::Value`, whose default `Map` is ordered by key. Do not enable `preserve_order`.
- **`hash = SHA-256(prev_hash ‖ seq as u64 BE ‖ ts ‖ 0x00 ‖ actor ‖ 0x00 ‖ action ‖ 0x00 ‖ payload)`.** Row 1 uses 32 zero bytes as `prev_hash`.
- **`spektr verify`:**
  - recomputes the chain
  - re-hashes every blob (decompressing first) and compares it with its key
  - checks every observation's blob exists
  - reports `✓ N rows` or the first failing row, with the reason
- The launcher shows chain status per case. Verifying every case on every launch would be slow, so it shows the cached result from the last verify, with its date.

## 10. Blobs and size

- Raw responses and captures are zstd-compressed (level 3) in `blob`.
- Captures over 25 MB are refused, with a message.
- The launcher shows each case's file size.
- `spektr export --format json` can include blobs as base64 (`--with-blobs`) or leave them out.
