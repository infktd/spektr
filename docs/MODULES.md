# spektr — Modules

`<config>` below means spektr's config directory, resolved as in ARCHITECTURE §5: `$XDG_CONFIG_HOME/spektr`, else `~/.config/spektr`, on macOS too.

This document covers the module contract, the manifest, the exec protocol, the native trait, the engine pipeline, the 18 built-in modules, rules profiles, launchers, and the developer workflow. Playbooks, which chain modules into workflows, are in `PLAYBOOKS.md`.

## 1. What a module is

A module is a **transform**. It takes one typed entity as input and emits observations (raw evidence), entities, edges and attributes. A module **never writes to the case file**: it emits messages, and the engine does everything else.

| Engine owns | Module owns |
|---|---|
| Deciding *whether* a job runs (type, rules profile, scope, depth, hubs, budget, cache) | Fetching data |
| Rate limits per host, concurrency, timeouts, cancellation | Parsing it |
| Normalization and vocabulary validation | Mapping it to entities, edges and attributes |
| SHA-256 of raw evidence, blob storage, provenance | Reporting errors, including when to retry |
| Strength computation, correlation, the audit log | |

Native (Rust) modules and exec (subprocess) modules produce the **same message type** (`spektr_protocol::ModuleMessage`). The engine has exactly one ingestion path.

## 2. Manifest

Every module, native or exec, has a manifest. Exec modules keep it in `module.toml` next to the executable. Native modules embed it in Rust, using the same struct (`spektr_protocol::Manifest`).

```toml
[module]
name        = "lemmy"               # [a-z0-9-]{2,32}, unique; must not clash with a built-in
version     = "0.1.0"               # semver
description = "Public Lemmy profile lookup"
runtime     = "exec"                # native | exec
command     = ["python3", "lemmy.py"]      # exec only; resolved relative to the module dir
serve       = false                 # exec only; true = one long-lived process, many jobs
concurrency = 4                     # max concurrent jobs for this module
timeout     = "60s"
technique   = "passive"             # passive | probing | active  (see §7)
auto        = true                  # eligible for automatic expansion
consumes    = ["username", "account:lemmy"]      # a type, or "account:<platform>" for one platform's accounts
produces    = [["has_account", "account"], ["links_to", "url"]]
cache_ttl   = "24h"
rate_limit  = "1/s"                 # per destination host; "N/s" or "N/min"
cost        = { class = "free" }    # free | local | keyed | metered
                                    #   local: no network (phone-parse, exif)
                                    #   keyed/metered: optional daily_budget = N; metered adds per_run = N
secrets          = []               # required; job is ineligible without them. e.g. ["SOME_API_KEY"]
optional_secrets = []               # used when present (e.g. a token that raises rate limits)
hosts       = ["lemmy.world", "lemmy.example"]   # declared, informational; shown on install + doctor
min_spektr  = "0.1.0"

[[module.platforms]]                # optional: account platforms this module introduces (DATA_MODEL §1.4)
id = "lemmy"
case_sensitive = false
profile_url = "https://{instance}/@{user}"   # handle form "user@instance"

[module.config]                     # optional user config, with types and defaults
instances = { type = "list", default = ["lemmy.world"] }

[[module.types]]                    # optional extension entity types
name = "x.lemmy.instance"
normalize = "lowercase"             # exact | trim | lowercase | casefold | url

[[module.predicates]]               # optional extension predicates
name = "x.lemmy.member_of_instance"
src = ["account"]
dst = ["x.lemmy.instance"]
```

**The manifest drives the UI.** It supplies:

- **Palette signatures:** `consumes → produces`.
- **Context-menu submenus:** the modules eligible for the cursor's entity type.
- **Cost hints:** "free", "1 request", "38/100 today".
- **Doctor checks:** missing secrets, missing binaries.
- **The install confirmation:** declared hosts and secrets.

`spektr module check` rejects invalid manifests. So does the engine at load time, with a doctor warning.

## 3. Exec protocol (v1)

### 3.1 Transport

- Messages are UTF-8 JSON, **one object per line**, over the module's stdin and stdout.
- stderr is captured into the debug log, prefixed with the module name. It is never parsed.
- The maximum line length is 32 MiB; a longer line fails the job.
- **Per-job mode** (`serve = false`):
  1. The host spawns the process.
  2. It writes `hello` and then `run`.
  3. It reads lines until `done` or `error`, then closes stdin.
  4. If the process doesn't exit within 2 s, it is killed.
- **Serve mode** (`serve = true`):
  1. The host spawns the process once and writes `hello`.
  2. The module replies `ready`.
  3. The host sends up to `concurrency` `run` messages at a time. Every module message carries its `job` id.
  4. `shutdown` asks the process to exit.

### 3.2 Host → module

```json
{"type":"hello","protocol":1,"spektr":"0.1.0"}
{"type":"run","job":"j_01JB…","input":{"type":"username","key":"jmarlow.wav","value":"jmarlow.wav","attrs":{}},
 "config":{"instances":["lemmy.world"]},"secrets":{},"deadline":"2026-10-20T18:06:12Z"}
{"type":"cancel","job":"j_01JB…"}
{"type":"shutdown"}
```

- **Secrets** appear only in `run`, and only those the manifest declares (required plus any optional ones that are set). The host redacts secret values from every log line, including captured stderr.
- **Image inputs:** when the input entity is an `image`, `input` also carries `"blob_b64"` (the image bytes, at most 25 MB).

### 3.3 Module → host

```json
{"type":"ready"}
{"type":"observation","job":"j_…","id":"o1","source_url":"https://lemmy.world/api/v3/user?username=jmarlow","content_type":"application/json","raw":"{…}"}
{"type":"entity","job":"j_…","ref":"e1","entity":{"type":"account","value":"lemmy:jmarlow@lemmy.world"},"evidence":["o1"]}
{"type":"attr","job":"j_…","target":"e1","name":"display_name","value":{"text":"Jordan M."},"evidence":["o1"]}
{"type":"attr","job":"j_…","target":"e1","name":"last_active","value":{"time":"2026-10-16T09:12:00Z"},"event":true,"evidence":["o1"]}
{"type":"edge","job":"j_…","src":"$input","predicate":"has_account","dst":"e1","evidence":["o1"]}
{"type":"observation","job":"j_…","id":"o2","source_url":"https://files.example/avatar.png","content_type":"image/png","raw_b64":"iVBORw0…"}
{"type":"entity","job":"j_…","ref":"e2","entity":{"type":"image","blob":"o2","value":"avatar.png"},"evidence":["o2"]}
{"type":"entity","job":"j_…","ref":"e3","entity":{"type":"organization","value":"Kestrel Labs LLC","dedup_id":"KL-1234"},"evidence":["o1"]}
{"type":"review","job":"j_…","kind":"sweep_unsure","payload":{"site":"example-forum","url":"…","reason":"login wall"},"evidence":["o1"]}
{"type":"progress","job":"j_…","pct":60,"note":"120/300 sites"}
{"type":"log","job":"j_…","level":"info","msg":"rate limited by host, backing off"}
{"type":"done","job":"j_…"}
{"type":"error","job":"j_…","code":"rate_limited","message":"429 from host","retryable":true,"retry_after":"32s"}
```

Rules, all enforced by the host:

- **`raw` is a UTF-8 string;** use `raw_b64` for binary content. The host computes the SHA-256 over the decoded bytes.
- **Every `entity`, `edge` and `attr` must cite at least one observation id** that was emitted earlier in the same job. A message without valid evidence is rejected and logged, and the job continues.
- **References:** `$input` is the job's input entity. `ref` names are local to one job.
- **Image entities** must name the observation holding their bytes (`"blob"`). The store derives the key from that blob (DATA_MODEL §1.5).
- **Person and organization entities** may carry `dedup_id` (DATA_MODEL §1.3).
- **Values are validated** against the vocabulary and the manifest's declared `produces`. A module may not emit a predicate it didn't declare.
- **Error codes:** `rate_limited`, `not_found`, `auth`, `upstream`, `parse`, `timeout`, `internal`. Only `rate_limited` and `upstream` are retried automatically, with backoff and at most 2 retries.
- **Per-job limits:** 10,000 messages and 50 MiB of raw data, both configurable. Exceeding a limit fails the job, but ingestion up to that point is kept.
- **Exit codes:** a non-zero exit without `done` marks the job failed.

The protocol is published as JSON Schema (`schemas/exec-protocol.v1.json`, `schemas/manifest.v1.json`), generated from the Rust types with `schemars` via `cargo xtask schemas`.

## 4. Native module trait

```rust
// spektr-engine::module
pub trait Module: Send + Sync + 'static {
    fn manifest(&self) -> &Manifest;
    fn run<'a>(&'a self, input: JobInput, cx: ModuleCx) -> BoxFuture<'a, Result<(), ModuleError>>;
}

pub struct JobInput {
    pub entity: EntityView,               // type, key, value, attrs
    pub blob: Option<std::sync::Arc<[u8]>>, // image bytes when entity.type == image
    pub depth: u32,
}

pub struct ModuleCx {
    pub emit: Emitter,                    // sends ModuleMessage into the ingestion pipeline
    pub config: serde_json::Value,
    pub secrets: Secrets,                 // required + optional secrets that are set
    pub cancel: CancellationToken,
}
```

```rust
// spektr-modules::net, shared by every built-in module and by capture
pub struct Net { pub http: Http, pub dns: Dns }   // Transport: Live | Record(path) | Replay(fixtures)
pub fn builtins(net: Net) -> Vec<Box<dyn spektr_engine::Module>>;   // each module holds a Net clone
```

- `BoxFuture` (from `futures`) keeps the trait object-safe without `async-trait`.
- **The engine knows nothing about HTTP.** Each built-in module is constructed with a clone of `Net`, which provides per-host rate limiting, the user agent, retries and record/replay. Built-ins must make every request through `Net`. That's what makes fixtures, the tutor and rate limiting work for every module at once.
- The engine doesn't depend on `spektr-modules`. The binary builds `Net` (`Replay` for tests and the tutor, `Live` otherwise) and registers `spektr_modules::builtins(net)` into the engine at startup.

## 5. Engine pipeline

**Eligibility.** A job is queued only if all of these hold:

1. The module is enabled in this case, and its required `secrets` are present. Optional secrets never block a job.
2. The input matches `consumes`, either by type or as `account:<platform>`.
3. The rules profile allows the module's `technique`, and doesn't block the module by name.
4. The trigger is one of:
   - **automatic:** the module is `auto = true`, the input's depth is **≤ the case depth**, and the input is in scope and not a hub
   - **manual:** the analyst asked
   - **playbook:** a playbook step asked. This works like manual: `auto = false` modules run and the depth limit doesn't apply, but scope and hubs do (PLAYBOOKS §2)

   **Depth:** seeds are depth 0. An entity created by a job gets the input's depth + 1, or keeps its existing depth if that's lower. With the default depth 2, a seed username (0) is swept, its accounts (1) get profile modules, and what those find (2), such as linked URLs and avatars, get enriched. Entities at depth 3 are created but not auto-expanded.
5. There's no cached observation for `(module, version, input key)` within `cache_ttl`, unless this is an explicit rerun.
6. The budget isn't exhausted.

A job that fails a check is recorded as `blocked` (rules) or `cached`, with the reason, in the activity rail and the audit log.

**Ingestion.** For each batch of messages, in one transaction:

1. Validate the message.
2. Normalize values and upsert entities.
3. Store blobs.
4. Upsert edges and attach evidence.
5. Write the audit rows.

Then emit `EngineEvent`s, run the correlators on the changed entities, and enqueue follow-up automatic jobs.

**Concurrency:**

- A global job semaphore (default 16).
- The module's own `concurrency` limit.
- Per-host rate limiting using the manifest's `rate_limit`:
  - **built-ins:** enforced inside `spektr-modules::net` by `governor`, keyed by destination host
  - **exec modules:** the engine paces their job starts, since it can't see their traffic

## 6. Built-in modules (v1)

All built-ins are native, `technique = passive`, and recorded as fixtures for tests and the tutor.

| Module | Consumes | Produces | Auto | Cost | Source |
|---|---|---|---|---|---|
| `username-sweep` | username | `has_account` → account; review items | yes | free | ~300 public profile URLs (site list) |
| `github` | username, `account:github` | `has_account` (username → account); attrs (display name, bio, created, last active, avatar URL, blog); `links_to` url; `has_email` (public profile email only); `located_in` (stated) | yes | free; optional secret `GITHUB_TOKEN` raises limits | GitHub REST API |
| `bluesky` | username, `account:bluesky` | `has_account` (username → account); attrs (display name, bio, created, avatar URL, DID); `links_to` url | yes | free | Public AT Protocol AppView (`app.bsky.actor.getProfile`) |
| `gitlab` | username, `account:gitlab` | `has_account` (username → account); attrs (display name, bio, created, avatar URL, website); `links_to` url | yes | free | GitLab public users API (`/api/v4/users?username=`) on gitlab.com |
| `mastodon` | username, `account:mastodon` | `has_account` (username → `mastodon:user@instance`); attrs (display name, bio, created, last status time as an event, avatar URL); `links_to` profile fields' URLs | yes | free | WebFinger plus the public account lookup API, on each instance in config `instances` (default `["mastodon.social"]`); platform rules for `user@instance` handles live in `platforms.rs` |
| `hackernews` | username, `account:hackernews` | `has_account`; attrs (created, karma, about text); `links_to` URLs found in "about" | yes | free | Hacker News public Firebase API (`/v0/user/<id>.json`) |
| `gravatar` | email | account (`gravatar:<hash>`) + `has_email` (account → the input email); attrs (avatar URL, display name); `links_to` verified accounts' URLs | yes | free | Gravatar public profile by email hash |
| `email-check` | email | attrs (syntax valid, MX present, provider class: freemail, corporate, disposable) | yes | free | DNS MX only. **Never** SMTP, signup or reset probing |
| `hibp` | email | `appears_in_breach` → breach (names, dates, data classes; never breach content) | no | keyed, `daily_budget` from config | HIBP API v3 |
| `phone-parse` | phone | attrs (country, region, line type, validity) | yes | local | libphonenumber metadata, offline |
| `exif` | image | attrs (device, software, `taken_at` event); `taken_at` → location | yes | local | The image bytes (`JobInput.blob`) |
| `avatar-hash` | account | image entity (the avatar) + `posted` (account → image, attr `role = avatar`); attr `avatar_phash` (64-bit dHash) on the account | yes | free | Fetches the account's `avatar_url` |
| `http-fingerprint` | url, domain, subdomain | attrs (title, server, `favicon_mmh3`, `analytics_ids`); `links_to` outbound social URLs; `has_email` for mailto links | yes | free | One GET of the page plus its favicon |
| `wayback` | url, domain | attrs `archived_first` and `archived_last` (events); `links_to` archived URLs (capped at 50) | no | free | Wayback CDX API |
| `dns` | domain, subdomain | `resolves_to` ip; `mail_handled_by` | yes | free | System resolver (hickory) |
| `rdns` | ip | `reverse_resolves_to`; `hosted_in_asn` (origin ASN) | yes | free | PTR lookup, plus Team Cymru's IP-to-ASN DNS service (`origin.asn.cymru.com` TXT); DNS only |
| `rdap` | domain, ip, asn | `registered_by` (organization/person with `dedup_id` = RDAP handle, or email); `operated_by` (asn → organization); attrs `registered_at`, `expires_at`, network name | yes | free | RDAP bootstrap (IANA). RDAP does not reliably return an IP's origin ASN; `rdns` provides that |
| `crtsh` | domain | certificate + `issued_for`; new subdomains | yes | free | crt.sh JSON |

**`username-sweep` in detail:**

- **The site list** is embedded at `spektr-modules/data/sites.json`. A user file at `<config>/sites.json` overrides it, merged by site id. Each site has:
  - id, name
  - platform id (matching the platform rules in `spektr-model`)
  - URL template
  - a detection method: `status`, `absent_text`, `present_text` or `redirect`
  - optional unsure conditions (login wall, rate-limit page)
- **Results:**
  - **found:** an account entity plus `has_account`, at strength single
  - **unsure:** a `review_item` of kind `sweep_unsure`
  - **not found:** counted in the observation summary only, with no entities created
- **Limits:** 16 sites in flight, and 1 request per second per host.

**Web capture is not a module.** It's a service (`spektr-modules::capture`, injected into the engine as its `CaptureService`) that writes `kind = capture` observations. It does:

- one GET, saving HTML, headers, final URL and status
- optionally, a screenshot by running Chrome/Chromium headless as a subprocess (`--headless --screenshot --window-size=1280,2400`)
- finding the browser via `which` and common install paths, or using the copy that `spektr setup capture` downloads

Attaching a local file creates a `kind = attachment` observation. If the file is an image, it also creates an `image` entity.

## 7. Technique classes and rules profiles

**Technique classes:**

| Class | Meaning | Examples |
|---|---|---|
| `passive` | Reads public data the way any visitor would | Profile pages, public APIs, DNS, certificate transparency |
| `probing` | Interacts with a service to learn something it doesn't publish | Checking whether an email has an account via signup or reset forms |
| `active` | Sends traffic that a target could notice as reconnaissance | Port scans, directory brute-forcing |

**Rules profiles** are TOML files: built-ins are embedded, and user profiles go in `<config>/profiles/`.

```toml
[profile]
name = "tracelabs"
description = "Trace Labs Search Party: passive only, Trace Labs categories and lints"
techniques = ["passive"]
blocked_modules = []
default_depth = 2
auto = true

categories = [
  { id = "friends",    name = "Friends",                points = 10 },
  { id = "family",     name = "Family",                 points = 10 },
  { id = "partners",   name = "Romantic Partners",      points = 50 },
  { id = "employment", name = "Employment",             points = 50 },
  { id = "basic",      name = "Basic Subject Info",     points = 50 },
  { id = "advanced",   name = "Advanced Subject Info",  points = 250 },
  { id = "dls",        name = "Day Last Seen",          points = 250 },
  { id = "timeline",   name = "Advancing the Timeline", points = 1000, needs_approval = "staff coach" },
  { id = "location",   name = "Location",               points = 5000, needs_approval = "leadership" },
]

[lints]
require_claim = true
require_relevance = true
require_url_or_screenshot = true
no_duplicate_across_categories = true
blocked_source_domains_file = "news-and-aggregators.txt"   # resolved next to the profile; the built-in copy is embedded from spektr-engine/data/profiles/

[export]
format = "tracelabs"     # paste-ready: category, claim, relevance, URLs, capture hashes
```

**Built-in profiles:**

- **`default`:** passive only; a single "General" category; no point values; lints for claim and evidence only.
- **`tracelabs`:** as above.

Categories and points are data, so when Trace Labs changes its scoring, the fix is an edit to this file.

## 8. Pivot launchers

Launchers open hand-off searches in the system browser for things spektr shouldn't automate. They're configured in TOML (built-in plus `<config>/launchers.toml`):

```toml
[[launcher]]
id = "google-exact"
name = "Google: exact match"
for = ["username", "email", "phone", "person"]
url = "https://www.google.com/search?q=%22{value}%22"   # {value} is URL-encoded by spektr

[[launcher]]
id = "lens"
name = "Google Lens"
for = ["image", "url"]
url = "https://lens.google.com/uploadbyurl?url={url}"   # {url}: the entity's URL or the image's source URL
```

The TUI exposes them as `:open <id>` and as an "Open in browser ▸" context-menu submenu. **Opening a launcher records an analyst audit row**, so the search strategy is documented.

**Built-in launchers (v1):**

| Group | Launchers | For |
|---|---|---|
| Exact-phrase search | `google-exact`, `bing-exact`, `duckduckgo-exact` | username, email, phone, person, organization |
| Reverse image search | `lens`, `yandex-images`, `tineye`, `bing-visual` | image, url. Needs a source URL: disabled, with the reason, for local-only images |
| Site search | `google-site` (`site:<domain>`) | domain, subdomain |
| Archives | `wayback-web` (the Wayback calendar page) | url, domain |

Playbooks (`PLAYBOOKS.md`) refer to launchers by id.

## 9. Building your own module

```
spektr module new <name> --lang python|rust|shell   # scaffold: module.toml, script, fixture dir, README
spektr module test <name> --input <type>:<value>    # live run; shows validated messages and what would be ingested
spektr module test <name> --record                  # saves stdout messages as a fixture
spektr module test <name> --fixture <file>          # replays a fixture through validation
spektr module check <name>                          # manifest lint, schema validation, vocabulary,
                                                    # timeout/cancel behaviour, secret-echo check
spektr module install <path|git-url>                # copies into the modules dir; shows hosts + secrets;
                                                    # asks to confirm; pins a tree hash
spektr module install --update <name>              # re-approve a module whose files changed
spektr module list | info <name> | enable <name> | disable <name>
```

- **Modules dir:** `<config>/modules/<name>/`.
- **Tree-hash pinning:** if a module's files change after install, the module is disabled until the user re-approves it with `spektr module install --update`.
- **Templates:**
  - Python: standard library only
  - Rust: depends only on the `spektr-protocol` crate
  - shell: `jq`-based, for trivial cases
- **Trust model:** exec modules run with the user's permissions, so installing one means trusting its code. The install confirmation says so plainly. WASM sandboxing is planned for v2.

## 10. Fixtures (built-in modules)

- **Location:** `crates/spektr-modules/fixtures/<module>/<case>.jsonl`. Each line is a recorded exchange:
  - HTTP: `{kind:"http", method, url, status, headers, body_b64}`
  - DNS: `{kind:"dns", name, rtype, answers}`
- **Replay:** the `Replay` transport in `spektr-modules::net` matches requests by method plus normalized URL, or by name plus record type for DNS. **A request with no match fails the test.**
- **Recording:** `cargo xtask record <module> <input>` records live, with explicit opt-in and the `live` feature.
- The tutor case is built entirely from fixtures. Its network recordings live in `crates/spektr-modules/fixtures/tutor/`, and its lesson script in `crates/spektr-tui/src/tutor/`. **Both are embedded in the binary** (`include_bytes!`, exposed as `spektr_modules::tutor_fixtures()`), so an installed spektr runs the tutor offline with no repo checkout. Other test fixtures are not embedded. Its data is fictional and uses `.example` domains and documentation IP ranges.

## 11. Deferred (v1.1+)

- **Recipe runtime (v1.1):** declarative TOML for HTTP APIs and command-line tools. A recipe sets the request template and auth, extracts values with JSONPath, and maps them to entities and edges. Recipes compile to the same `ModuleMessage` stream.
- **WASM runtime (v2):** the same message schema, running in wasmtime with host-provided `fetch` limited to the declared `hosts`.
