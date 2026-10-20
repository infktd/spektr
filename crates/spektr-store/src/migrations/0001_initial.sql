-- spektr case file, schema version 1 (DATA_MODEL §8, PLAYBOOKS §3).
-- IDs are UUIDv7 as 16-byte BLOBs, except finding.id and audit.seq.

CREATE TABLE case_meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);  -- JSON values

CREATE TABLE entity (
  id BLOB PRIMARY KEY, type TEXT NOT NULL, key TEXT NOT NULL, value TEXT NOT NULL,
  attrs TEXT NOT NULL DEFAULT '{}', flags INTEGER NOT NULL DEFAULT 0, depth INTEGER NOT NULL,
  merged_into BLOB REFERENCES entity(id), created_at TEXT NOT NULL, created_by TEXT NOT NULL,
  UNIQUE (type, key));
CREATE INDEX entity_merged_into ON entity(merged_into) WHERE merged_into IS NOT NULL;
CREATE INDEX entity_flags ON entity(flags) WHERE flags != 0;

CREATE TABLE edge (
  id BLOB PRIMARY KEY, src BLOB NOT NULL REFERENCES entity(id), dst BLOB NOT NULL REFERENCES entity(id),
  predicate TEXT NOT NULL, discriminator TEXT NOT NULL DEFAULT '', attrs TEXT NOT NULL DEFAULT '{}',
  state TEXT NOT NULL DEFAULT 'active', confirmed_at TEXT, origin TEXT NOT NULL, created_at TEXT NOT NULL,
  UNIQUE (src, predicate, dst, discriminator));
CREATE INDEX edge_src ON edge(src); CREATE INDEX edge_dst ON edge(dst);

CREATE TABLE blob (sha256 BLOB PRIMARY KEY, size INTEGER NOT NULL, compression TEXT NOT NULL, data BLOB NOT NULL);
-- sha256 is over the UNCOMPRESSED bytes, computed by spektr-store on insert, never by a module.

CREATE TABLE playbook_run (id BLOB PRIMARY KEY, playbook TEXT NOT NULL, playbook_version TEXT NOT NULL,
  playbook_hash TEXT NOT NULL, seed_entity BLOB NOT NULL REFERENCES entity(id), parent_run BLOB REFERENCES playbook_run(id),
  state TEXT NOT NULL,          -- running | done | stopped | failed
  current_step INTEGER NOT NULL, started_at TEXT NOT NULL, finished_at TEXT);
CREATE INDEX playbook_run_parent ON playbook_run(parent_run);

CREATE TABLE playbook_step (run_id BLOB NOT NULL REFERENCES playbook_run(id), step INTEGER NOT NULL,
  kind TEXT NOT NULL,           -- run | playbook | check
  state TEXT NOT NULL,          -- pending | running | done | stopped
  selected INTEGER NOT NULL DEFAULT 0, capped INTEGER NOT NULL DEFAULT 0,
  result TEXT NOT NULL DEFAULT '{}',
  started_at TEXT, finished_at TEXT, PRIMARY KEY (run_id, step));

CREATE TABLE job (id BLOB PRIMARY KEY, module TEXT NOT NULL, input_entity BLOB NOT NULL REFERENCES entity(id),
  trigger TEXT NOT NULL,                 -- auto | manual | playbook | tutor
  run_id BLOB REFERENCES playbook_run(id),
  cached_observation BLOB REFERENCES observation(id),
  joined_job BLOB REFERENCES job(id),
  depth INTEGER NOT NULL, state TEXT NOT NULL,  -- queued|running|done|failed|blocked|cached
  reason TEXT, error TEXT, queued_at TEXT NOT NULL, started_at TEXT, finished_at TEXT);
CREATE INDEX job_state ON job(state);
CREATE INDEX job_run ON job(run_id) WHERE run_id IS NOT NULL;
CREATE INDEX job_input ON job(module, input_entity);

CREATE TABLE observation (
  id BLOB PRIMARY KEY, job_id BLOB REFERENCES job(id), module TEXT NOT NULL, module_version TEXT NOT NULL,
  manifest_hash TEXT NOT NULL, input_entity BLOB REFERENCES entity(id), source_url TEXT,
  content_type TEXT, blob_sha256 BLOB REFERENCES blob(sha256), summary TEXT NOT NULL DEFAULT '{}',
  kind TEXT NOT NULL DEFAULT 'module',   -- module | capture | attachment | analyst | correlator
  created_at TEXT NOT NULL);
CREATE INDEX obs_cache ON observation(module, module_version, input_entity, created_at);
CREATE INDEX obs_job ON observation(job_id) WHERE job_id IS NOT NULL;

CREATE TABLE edge_evidence (edge_id BLOB NOT NULL REFERENCES edge(id),
  observation_id BLOB NOT NULL REFERENCES observation(id), source_key TEXT NOT NULL,
  PRIMARY KEY (edge_id, observation_id));
CREATE INDEX edge_evidence_obs ON edge_evidence(observation_id);

CREATE TABLE finding (id INTEGER PRIMARY KEY AUTOINCREMENT, title TEXT NOT NULL, category TEXT NOT NULL,
  claim TEXT NOT NULL DEFAULT '', relevance TEXT NOT NULL DEFAULT '', status TEXT NOT NULL DEFAULT 'draft',
  created_at TEXT NOT NULL, updated_at TEXT NOT NULL);
CREATE TABLE finding_entity (finding_id INTEGER REFERENCES finding(id), entity_id BLOB REFERENCES entity(id),
  PRIMARY KEY (finding_id, entity_id));
CREATE INDEX finding_entity_entity ON finding_entity(entity_id);
CREATE TABLE finding_evidence (finding_id INTEGER REFERENCES finding(id), observation_id BLOB REFERENCES observation(id),
  PRIMARY KEY (finding_id, observation_id));

CREATE TABLE note (id BLOB PRIMARY KEY, entity_id BLOB REFERENCES entity(id), finding_id INTEGER REFERENCES finding(id),
  body TEXT NOT NULL, created_at TEXT NOT NULL);
CREATE INDEX note_entity ON note(entity_id);

CREATE TABLE scope_rule (id BLOB PRIMARY KEY, kind TEXT NOT NULL,   -- allow|deny
  matcher TEXT NOT NULL,                                              -- domain_suffix|cidr|platform|regex
  pattern TEXT NOT NULL, created_at TEXT NOT NULL);

CREATE TABLE module_setting (module TEXT PRIMARY KEY, enabled INTEGER NOT NULL, config TEXT NOT NULL DEFAULT '{}');

CREATE TABLE review_item (id BLOB PRIMARY KEY, kind TEXT NOT NULL,  -- sweep_unsure|same_person
  payload TEXT NOT NULL, state TEXT NOT NULL DEFAULT 'pending', created_at TEXT NOT NULL, decided_at TEXT);
CREATE INDEX review_item_state ON review_item(state);

CREATE TABLE merge (id BLOB PRIMARY KEY, survivor BLOB NOT NULL REFERENCES entity(id), merged BLOB NOT NULL REFERENCES entity(id),
  repointed TEXT NOT NULL DEFAULT '[]',
  created_at TEXT NOT NULL, undone_at TEXT);

CREATE TABLE checklist_item (id BLOB PRIMARY KEY, run_id BLOB NOT NULL REFERENCES playbook_run(id), step INTEGER NOT NULL,
  entity_id BLOB REFERENCES entity(id), text TEXT NOT NULL, launchers TEXT NOT NULL DEFAULT '[]', screen TEXT,
  state TEXT NOT NULL DEFAULT 'open',   -- open | done | skipped
  note TEXT, resolved_at TEXT);
CREATE INDEX checklist_item_run ON checklist_item(run_id);
CREATE INDEX checklist_item_entity ON checklist_item(entity_id) WHERE entity_id IS NOT NULL;

CREATE TABLE audit (seq INTEGER PRIMARY KEY, ts TEXT NOT NULL, actor TEXT NOT NULL, action TEXT NOT NULL,
  payload TEXT NOT NULL, prev_hash BLOB NOT NULL, hash BLOB NOT NULL);
