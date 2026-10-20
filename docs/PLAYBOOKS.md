# spektr — Playbooks

A **playbook** is a saved investigation workflow: an ordered list of steps that run modules, call other playbooks, or add manual checklist items. Playbooks are TOML. There are no variables, loops or conditionals beyond the `on` selector, so anyone can read one.

`<config>` means spektr's config directory (ARCHITECTURE §5).

## 1. Format

```toml
[playbook]
name = "person-from-username"            # [a-z0-9-]{2,40}, unique
version = "1.0.0"
description = "Find a person's accounts and profile details from one handle"
seed = ["username"]                      # entity types (or account:<platform>) it can start from
requires_profile = ""                    # optional: refuse to run unless the case uses this profile

[[step]]
title = "Sweep the handle"
run = ["username-sweep"]                 # modules to run on each selected entity
on = "seed"

[[step]]
title = "Profile details"
run = ["github", "bluesky", "gitlab", "mastodon", "hackernews"]
on = "seed"

[[step]]
title = "Avatar hashes"
run = ["avatar-hash"]
on = { type = "account" }                # entities found during this run (default `from = "run"`)

[[step]]
title = "Reverse image search"
check = "Reverse image search each avatar. Look for the same photo on other profiles, shops, forums or news."
open = ["lens", "yandex-images", "tineye"]
on = { type = "image" }
```

**Step kinds.** Exactly one of these per step:

| Key | Effect |
|---|---|
| `run = [module, …]` | Queues each listed module on each selected entity |
| `playbook = "<name>"` | Starts that playbook with each selected entity as its seed. One level of nesting only: a nested playbook may not itself contain `playbook` steps |
| `check = "<text>"` | Creates one checklist item per selected entity, or a single item if `on` is omitted. `open = [launcher ids]` attaches launchers (MODULES §8). `screen = "review"` (or `"findings"`) makes the item open that screen |

**`on` selector:**

- `"seed"` selects the run's seed entity.
- A table selects entities with these keys:

  | Key | Values | Default |
  |---|---|---|
  | `type` | a type, or `account:<platform>` | required |
  | `from` | `"run"` (produced or confirmed by this run's jobs, see §2.3), `"seed-links"` (directly linked to the seed by any edge), or `"case"` (anywhere in the case) | `"run"` |
  | `min_strength` | `single`, `corroborated`, `confirmed` | `single` |
  | `max` | cap on entities selected | 50, and never above 200; the cap is shown in the step's result |

- Selection order is strongest first, then newest.
- Hubs and out-of-scope entities are never selected.

## 2. Semantics

1. **Steps run in order.**
   - **`run` steps** complete when every job they queued has finished: done, failed, blocked or cached.
   - **`playbook` steps** start one child run per selected entity, **one after another** (not in parallel). They complete when every child run has reached a terminal state. A child that fails or is stopped is recorded on the step, and the parent continues.
   - **`check` steps** complete immediately; their items stay open independently.
2. **Rules always apply.** Playbook jobs go through the same eligibility checks as any job (MODULES §5), with these differences:
   - the trigger is `playbook`, which works like `manual`: `auto = false` modules run, and the case depth limit doesn't apply
   - the rules profile, scope, hubs, secrets, cache and budgets all still apply
   - a blocked or ineligible job is recorded with its reason, and the step continues
3. **What a run "found" (`from = "run"`).** A playbook usually starts on entities the case already knows, so the run counts results that already exist:
   - **Already cached:** the job is recorded with `state = cached` and `cached_observation` pointing at the existing observation.
   - **Already queued or running** (for example, an auto job on the same module, version and input): the playbook job **joins** it instead of running twice, recorded as `joined_job`.
   - **Run membership:** an entity belongs to the run if it is reachable from the seed through edges whose evidence includes an observation that one of the run's jobs produced, joined or hit in the cache, or that a child run's jobs did. So a playbook started on an already-expanded handle still selects its accounts and avatars.
4. **Automatic expansion continues alongside a playbook.** Entities a playbook creates trigger `auto` modules normally, within the case depth.
5. **Dedup across steps:** a module never runs twice on the same entity within one playbook run, including its child runs.
6. **One top-level run at a time per case.** Starting a second asks whether to stop the current one or cancel; child runs don't count toward this. `:playbook stop` and `X` stop that run and all its children: remaining steps are cancelled, in-flight jobs started by the run are cancelled (joined auto jobs keep going), and results already ingested stay.
7. **Run states:**
   - `running`
   - `done`: every step completed
   - `stopped`: by the analyst
   - `failed`: the runner hit an internal error, e.g. a store error, or a playbook that changed on disk and no longer validates on resume. Failed module jobs never fail a run.

   Runs in progress when spektr exits resume on the next open.
8. **Auditing:** starting, finishing and stopping a run, completing a step, and resolving a checklist item (done or skipped) each write an audit row. Opening a launcher from a checklist item does too.
9. **Validation:** `spektr playbook check <file>` and load-time validation reject the playbook if it:
   - names a module that doesn't exist (a warning, not an error, for modules that are installed but disabled)
   - refers to an unknown launcher
   - nests playbooks deeper than one level
   - has a `from = "run"` selector for a type that no earlier step can produce and that isn't the seed type. `seed-links` and `case` selectors are exempt
   - fails TOML or schema validation (`schemas/playbook.v1.json`)

## 3. Storage

Three tables, part of the DATA_MODEL §8 schema. They're created in Phase 1, with the rest of the schema:

```sql
CREATE TABLE playbook_run (id BLOB PRIMARY KEY, playbook TEXT NOT NULL, playbook_version TEXT NOT NULL,
  playbook_hash TEXT NOT NULL, seed_entity BLOB NOT NULL REFERENCES entity(id), parent_run BLOB REFERENCES playbook_run(id),
  state TEXT NOT NULL,          -- running | done | stopped | failed
  current_step INTEGER NOT NULL, started_at TEXT NOT NULL, finished_at TEXT);

CREATE TABLE playbook_step (run_id BLOB NOT NULL REFERENCES playbook_run(id), step INTEGER NOT NULL,
  kind TEXT NOT NULL,           -- run | playbook | check
  state TEXT NOT NULL,          -- pending | running | done | stopped
  selected INTEGER NOT NULL DEFAULT 0, capped INTEGER NOT NULL DEFAULT 0,   -- entities selected; how many the `max` cap dropped
  result TEXT NOT NULL DEFAULT '{}',   -- JSON: job counts by state, entities produced, child run ids, items created
  started_at TEXT, finished_at TEXT, PRIMARY KEY (run_id, step));

CREATE TABLE checklist_item (id BLOB PRIMARY KEY, run_id BLOB NOT NULL REFERENCES playbook_run(id), step INTEGER NOT NULL,
  entity_id BLOB REFERENCES entity(id), text TEXT NOT NULL, launchers TEXT NOT NULL DEFAULT '[]', screen TEXT,
  state TEXT NOT NULL DEFAULT 'open',   -- open | done | skipped
  note TEXT, resolved_at TEXT);
```

- `job.trigger` includes `playbook`, and `job` carries `run_id`, `cached_observation` and `joined_job` (DATA_MODEL §8).
- **Run membership** (§2.3) is computed by a query over those columns and `edge_evidence`. It isn't a stored list.

## 4. Where playbooks come from

- **Built-in:** embedded from `crates/spektr-engine/data/playbooks/*.toml`.
- **User:** `<config>/playbooks/*.toml`. A user playbook with a built-in's name **overrides** that built-in, and the TUI marks it `user`.
- **Sharing:** playbooks are plain files; copy one in, or keep a directory in git. (Install-from-git, like modules, is v1.1.)

## 5. CLI and TUI

| Where | How |
|---|---|
| CLI | `spektr run <case> --playbook <name> --seed <type>:<value>`: runs to completion, then prints the step results and any open checklist items |
| CLI | `spektr playbook list` · `info <name>` (prints the steps) · `check <file>` · `new <name> --seed <type>` (scaffold in `<config>/playbooks/`) |
| TUI palette | `:playbook <name>` on the focused entity · `:playbook stop` · `:checklist` |
| TUI context menu | **Run playbook ▸**, listing the playbooks whose `seed` matches the entity |
| TUI screen | **Checklist** (`C`, artboard 10): playbook runs and their steps (left); checklist items grouped by step (right) |
| TUI rail | `▸ playbook person-from-username · step 4/6` while running |
| TUI inspector | A "Checklist" section listing the open items for the cursor entity |

## 6. Built-in playbooks (v1)

These six files ship in `crates/spektr-engine/data/playbooks/`. The TOML below is their exact content.

### person-from-username.toml

```toml
[playbook]
name = "person-from-username"
version = "1.0.0"
description = "Find a person's accounts and profile details from one handle"
seed = ["username"]

[[step]]
title = "Sweep the handle"
run = ["username-sweep"]
on = "seed"

[[step]]
title = "Profile details"
run = ["github", "bluesky", "gitlab", "mastodon", "hackernews"]
on = "seed"

[[step]]
title = "Avatar hashes"
run = ["avatar-hash"]
on = { type = "account" }

[[step]]
title = "Linked sites"
run = ["http-fingerprint", "wayback"]
on = { type = "url", max = 25 }

[[step]]
title = "Reverse image search"
check = "Reverse image search each avatar. Look for the same photo on other profiles, shops, forums or news."
open = ["lens", "yandex-images", "tineye"]
on = { type = "image" }

[[step]]
title = "Review unsure sweep hits"
check = "Review the sites where the sweep couldn't tell whether the account exists."
screen = "review"
```

### person-from-email.toml

```toml
[playbook]
name = "person-from-email"
version = "1.0.0"
description = "Profiles, linked accounts and breach exposure for one email address"
seed = ["email"]

[[step]]
title = "Email basics"
run = ["email-check", "gravatar", "hibp"]
on = "seed"

[[step]]
title = "Avatar hashes"
run = ["avatar-hash"]
on = { type = "account" }

[[step]]
title = "Linked sites"
run = ["http-fingerprint"]
on = { type = "url", max = 25 }

[[step]]
title = "Search the exact address"
check = "Search the exact address in quotes. Note forum posts, documents and profiles that mention it."
open = ["google-exact", "bing-exact", "duckduckgo-exact"]
on = "seed"

[[step]]
title = "Handle from the local part"
check = "If the part before the @ looks like a handle, seed it as a username and run person-from-username."
on = "seed"
```

`hibp` is skipped with "missing secret" unless a key is configured. That is expected, and isn't an error.

### person-from-phone.toml

```toml
[playbook]
name = "person-from-phone"
version = "1.0.0"
description = "Parse a phone number and search for where it is published"
seed = ["phone"]

[[step]]
title = "Parse the number"
run = ["phone-parse"]
on = "seed"

[[step]]
title = "Search common formats"
check = "Search the number in its common formats: E.164, national, and with separators."
open = ["google-exact", "bing-exact"]
on = "seed"

[[step]]
title = "Listings"
check = "Check business listings, classifieds and community boards for the number. Do not call, text or add it to a messaging app."
on = "seed"
```

### image-triage.toml

```toml
[playbook]
name = "image-triage"
version = "1.0.0"
description = "Metadata, location and reverse image search for one image"
seed = ["image"]

[[step]]
title = "Metadata"
run = ["exif"]
on = "seed"

[[step]]
title = "Reverse image search"
check = "Reverse image search the image and note earlier or larger copies."
open = ["lens", "yandex-images", "tineye", "bing-visual"]
on = "seed"

[[step]]
title = "Geolocate if no GPS"
check = "If EXIF has no GPS, geolocate from visible landmarks, signs, terrain and shadows."
on = "seed"
```

### domain-recon.toml

```toml
[playbook]
name = "domain-recon"
version = "1.0.0"
description = "Registration, DNS, certificates and web fingerprint for a domain"
seed = ["domain"]

[[step]]
title = "Registration, DNS and certificates"
run = ["rdap", "dns", "crtsh"]
on = "seed"

[[step]]
title = "Subdomain DNS"
run = ["dns"]
on = { type = "subdomain", max = 50 }

[[step]]
title = "Web fingerprint and history"
run = ["http-fingerprint", "wayback"]
on = { type = "subdomain", max = 25 }

[[step]]
title = "Web fingerprint of the domain"
run = ["http-fingerprint", "wayback"]
on = "seed"

[[step]]
title = "Reverse DNS and ASN"
run = ["rdns"]
on = { type = "ip", max = 50 }
```

### tracelabs-kickoff.toml

```toml
[playbook]
name = "tracelabs-kickoff"
version = "1.0.0"
description = "Start a Trace Labs case: run the person playbooks on everything known about the subject, then work the scoring categories"
seed = ["person"]
requires_profile = "tracelabs"

[[step]]
title = "Known handles"
playbook = "person-from-username"
on = { type = "username", from = "seed-links" }

[[step]]
title = "Known emails"
playbook = "person-from-email"
on = { type = "email", from = "seed-links" }

[[step]]
title = "Known phones"
playbook = "person-from-phone"
on = { type = "phone", from = "seed-links" }

[[step]]
title = "Employment"
check = "Employment: search for the subject's name with likely employers, schools and job titles. Record workplaces, supervisors and dates."
open = ["google-exact"]
on = "seed"

[[step]]
title = "Family and friends"
check = "Family and friends: look for obituaries, public family trees, tagged photos and meaningful interactions. Explain each connection."
open = ["google-exact"]
on = "seed"

[[step]]
title = "Day last seen"
check = "Day last seen: look for posts and activity around the last-seen date. News articles are leads only; Trace Labs doesn't accept them as evidence."
open = ["google-exact"]
on = "seed"

[[step]]
title = "Advancing the timeline"
check = "Look for any activity dated after the last-seen date (timeline tab, ▲ markers)."
on = "seed"
```

## 7. Built-in launchers these playbooks use

These must exist in the built-in `launchers.toml` (MODULES §8):

- `google-exact`, `bing-exact`, `duckduckgo-exact`: an exact-phrase search on the entity's value
- `lens`, `yandex-images`, `tineye`, `bing-visual`: reverse image search, using the image's source URL (the `source_url` of the observation that holds the image's blob). Disabled, with a reason, for local-only images that have no URL. **When every launcher on an item is disabled, `o` instead reveals the image file in the system file manager**, and the item tells the user to upload it manually.
