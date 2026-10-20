//! Correlation (DATA_MODEL §5) runs after every ingestion batch, on the
//! entities the batch changed. Identity merge happens at insert (the
//! normalized-key upsert), so it needs no correlator. Phase 2 ships the `ip`
//! rule; the other artifact rules arrive with the modules that produce their
//! inputs (Phase 4).
//!
//! Hubs are excluded from every artifact rule. Correlator edges carry a
//! synthetic observation (`kind = correlator`) as evidence, with source key
//! `correlator:<rule>`.

use std::collections::BTreeSet;

use serde_json::json;
use spektr_model::{
    Actor, ArtifactKind, CorePredicate, CoreType, EntityFlags, EntityId, Predicate,
};
use spektr_store::{
    Direction, EdgeAttrs, EgoParams, EntitySummary, NewEdge, NewObservation, ObservationKind,
    StoreResult, Tx,
};

use crate::events::EventKind;

pub trait Correlator: Send + Sync {
    fn rule(&self) -> &'static str;
    fn correlate(
        &self,
        tx: &mut Tx<'_>,
        changed: &BTreeSet<EntityId>,
    ) -> StoreResult<Vec<EventKind>>;
}

/// Write `shares_artifact` edges for each new pair among `members`, with one
/// correlator observation as their evidence. Returns the events.
pub(crate) fn link_pairs(
    tx: &mut Tx<'_>,
    rule: &str,
    artifact: ArtifactKind,
    value: &str,
    members: &[EntitySummary],
    detail: serde_json::Value,
) -> StoreResult<Vec<EventKind>> {
    let disc = artifact.discriminator(value);
    let mut new_pairs = Vec::new();
    for (i, a) in members.iter().enumerate() {
        let linked: BTreeSet<EntityId> = {
            let ego = tx.reader().ego(
                a.id,
                &EgoParams {
                    page_size: usize::MAX,
                    include_rejected: true,
                    ..EgoParams::default()
                },
            )?;
            ego.groups
                .iter()
                .filter(|g| g.key.artifact == Some(artifact))
                .flat_map(|g| g.rows.iter())
                .filter(|r| r.artifact_value.as_deref() == Some(value))
                .map(|r| r.entity.id)
                .collect()
        };
        for b in &members[i + 1..] {
            if !linked.contains(&b.id) {
                new_pairs.push((a.clone(), b.clone()));
            }
        }
    }
    if new_pairs.is_empty() {
        return Ok(Vec::new());
    }
    let actor = Actor::Correlator(rule.to_string());
    let obs = tx.add_observation(
        &actor,
        NewObservation {
            job: None,
            module: format!("correlator:{rule}"),
            module_version: env!("CARGO_PKG_VERSION").into(),
            manifest_hash: "builtin".into(),
            input_entity: None,
            source_url: None,
            content_type: None,
            blob: None,
            summary: json!({
                "artifact": artifact.as_str(),
                "value": value,
                "entities": members.iter().map(|m| m.id).collect::<Vec<_>>(),
                "detail": detail,
            }),
            kind: ObservationKind::Correlator,
        },
    )?;
    let source_key = format!("correlator:{rule}");
    let mut events = Vec::new();
    for (a, b) in new_pairs {
        let mut attrs = EdgeAttrs::new();
        attrs.insert("artifact".into(), json!(artifact.as_str()));
        attrs.insert("value".into(), json!(value));
        let (edge, _) = tx.upsert_edge(
            &actor,
            NewEdge {
                src: a.id,
                predicate: Predicate::Core(CorePredicate::SharesArtifact),
                dst: b.id,
                discriminator: disc.clone(),
                attrs,
                origin: source_key.clone(),
            },
        )?;
        tx.add_evidence(&actor, edge, obs, &source_key)?;
        events.push(EventKind::Correlation {
            rule: rule.to_string(),
            artifact,
            value: value.to_string(),
            a,
            b,
        });
    }
    Ok(events)
}

/// Two domains or subdomains resolving to the same **non-hub** IP.
pub struct IpCorrelator;

impl Correlator for IpCorrelator {
    fn rule(&self) -> &'static str {
        "ip"
    }

    fn correlate(
        &self,
        tx: &mut Tx<'_>,
        changed: &BTreeSet<EntityId>,
    ) -> StoreResult<Vec<EventKind>> {
        let resolves_to = Predicate::Core(CorePredicate::ResolvesTo);
        let all = EgoParams {
            page_size: usize::MAX,
            ..EgoParams::default()
        };
        let mut ips: BTreeSet<EntityId> = BTreeSet::new();
        for id in changed {
            let r = tx.reader();
            let root = r.resolve(*id)?;
            let Some(e) = r.entity(root)? else { continue };
            match e.ty.core() {
                Some(CoreType::Ip) => {
                    ips.insert(root);
                }
                Some(CoreType::Domain | CoreType::Subdomain) => {
                    for g in r.ego(root, &all)?.groups {
                        if g.key.predicate == resolves_to && g.key.direction == Direction::Out {
                            ips.extend(g.rows.iter().map(|row| row.entity.id));
                        }
                    }
                }
                _ => {}
            }
        }
        let mut events = Vec::new();
        for ip in ips {
            let r = tx.reader();
            let Some(ip_row) = r.entity(ip)? else {
                continue;
            };
            if ip_row.flags.contains(EntityFlags::HUB) {
                continue;
            }
            let hosts: Vec<EntitySummary> = r
                .ego(ip, &all)?
                .groups
                .into_iter()
                .filter(|g| g.key.predicate == resolves_to && g.key.direction == Direction::In)
                .flat_map(|g| g.rows)
                .map(|row| row.entity)
                .filter(|e| !e.flags.contains(EntityFlags::HUB))
                .collect();
            if hosts.len() < 2 {
                continue;
            }
            events.extend(link_pairs(
                tx,
                self.rule(),
                ArtifactKind::Ip,
                &ip_row.key,
                &hosts,
                json!({ "ip": ip_row.key }),
            )?);
        }
        Ok(events)
    }
}
