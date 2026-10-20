//! Shared helpers for the store's integration tests.

#![allow(dead_code, clippy::unwrap_used, clippy::expect_used)]

use std::path::PathBuf;

use serde_json::json;
use spektr_model::{
    Actor, Clock, CorePredicate, EdgeId, EntityId, EntityType, IdGen, KeyHint, NormalizeCtx,
    ObservationId, Vocabulary, normalize,
};
use spektr_store::{
    CaseInit, Db, EdgeAttrs, NewEdge, NewObservation, ObservationKind, OpenOptions, Tx,
};
use tempfile::TempDir;

pub const NOW: &str = "2026-10-20T18:14:22Z";

pub fn opts() -> OpenOptions {
    OpenOptions::new(Clock::Fixed(NOW.parse().unwrap()), IdGen::seeded(1)).unwrap()
}

pub struct Case {
    pub dir: TempDir,
    pub path: PathBuf,
    pub db: Db,
}

pub fn new_case() -> Case {
    new_case_with(&CaseInit::named("test-case"))
}

pub fn new_case_with(init: &CaseInit) -> Case {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("case.spektr");
    let db = Db::create(&path, init, opts()).unwrap();
    Case { dir, path, db }
}

pub fn module(name: &str) -> Actor {
    Actor::Module {
        name: name.into(),
        version: "1.0.0".into(),
    }
}

/// A module observation with a source URL on `host`.
pub fn obs(
    tx: &mut Tx<'_>,
    module_name: &str,
    input: Option<EntityId>,
    host: &str,
) -> ObservationId {
    tx.add_observation(
        &module(module_name),
        NewObservation {
            job: None,
            module: module_name.into(),
            module_version: "1.0.0".into(),
            manifest_hash: "test".into(),
            input_entity: input,
            source_url: Some(format!("https://{host}/x")),
            content_type: Some("application/json".into()),
            blob: None,
            summary: json!({}),
            kind: ObservationKind::Module,
        },
    )
    .unwrap()
}

pub fn ent(
    tx: &mut Tx<'_>,
    ty: &str,
    value: &str,
    depth: u32,
    evidence: ObservationId,
    key: &str,
) -> EntityId {
    let v = Vocabulary::builtin();
    let n = normalize(
        &ty.parse::<EntityType>().unwrap(),
        value,
        KeyHint::None,
        NormalizeCtx {
            vocab: &v,
            default_region: Some("US"),
        },
    )
    .unwrap();
    tx.ingest_entity(&Actor::Analyst, &n, depth, Some((evidence, key)))
        .unwrap()
        .id
}

pub fn edge(
    tx: &mut Tx<'_>,
    src: EntityId,
    p: CorePredicate,
    dst: EntityId,
    evidence: &[(ObservationId, &str)],
) -> EdgeId {
    edge_d(tx, src, p, dst, "", evidence)
}

pub fn edge_d(
    tx: &mut Tx<'_>,
    src: EntityId,
    p: CorePredicate,
    dst: EntityId,
    disc: &str,
    evidence: &[(ObservationId, &str)],
) -> EdgeId {
    let (id, _) = tx
        .upsert_edge(
            &Actor::Analyst,
            NewEdge {
                src,
                predicate: p.into(),
                dst,
                discriminator: disc.into(),
                attrs: EdgeAttrs::new(),
                origin: "test".into(),
            },
        )
        .unwrap();
    for (o, k) in evidence {
        tx.add_evidence(&Actor::Analyst, id, *o, k).unwrap();
    }
    id
}
