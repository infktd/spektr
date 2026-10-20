//! Correlation (DATA_MODEL §5) runs after every ingestion batch, on the
//! entities the batch changed. Identity merge happens at insert (the
//! normalized-key upsert), so it needs no correlator. The artifact rules
//! are `avatar_phash`, `analytics_id`, `favicon_mmh3`, `registrant_email`,
//! `cert_san` and `ip`; same-person candidates go to the review queue.
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

/// Text values of an attribute (every source's).
fn attr_texts(e: &spektr_store::EntityRow, name: &str) -> Vec<String> {
    use spektr_model::AttrValue as V;
    e.attrs
        .get(name)
        .map(|vs| {
            vs.iter()
                .filter_map(|a| match &a.value {
                    V::Text(s) | V::Url(s) => Some(s.clone()),
                    V::Int(i) => Some(i.to_string()),
                    _ => None,
                })
                .collect()
        })
        .unwrap_or_default()
}

fn is_host(ty: &spektr_model::EntityType) -> bool {
    matches!(
        ty.core(),
        Some(CoreType::Url | CoreType::Domain | CoreType::Subdomain)
    )
}

/// Roots of the changed entities.
fn roots(tx: &Tx<'_>, changed: &BTreeSet<EntityId>) -> StoreResult<BTreeSet<EntityId>> {
    let r = tx.reader();
    changed.iter().map(|id| r.resolve(*id)).collect()
}

/// Two accounts whose `avatar_phash` values are within Hamming distance 6
/// (64-bit dHash).
pub struct AvatarPhashCorrelator;

/// dHash hex → bits.
fn phash(s: &str) -> Option<u64> {
    u64::from_str_radix(s.trim(), 16).ok()
}

/// The largest distance that counts as the same avatar.
pub const AVATAR_MAX_DISTANCE: u32 = 6;

impl Correlator for AvatarPhashCorrelator {
    fn rule(&self) -> &'static str {
        "avatar_phash"
    }

    fn correlate(
        &self,
        tx: &mut Tx<'_>,
        changed: &BTreeSet<EntityId>,
    ) -> StoreResult<Vec<EventKind>> {
        let changed = roots(tx, changed)?;
        let all: Vec<(spektr_store::EntityRow, u64)> = tx
            .reader()
            .entities_with_attr("avatar_phash")?
            .into_iter()
            .filter(|e| e.ty.is(CoreType::Account) && !e.flags.contains(EntityFlags::HUB))
            .filter_map(|e| {
                let h = attr_texts(&e, "avatar_phash")
                    .first()
                    .and_then(|s| phash(s))?;
                Some((e, h))
            })
            .collect();
        let mut events = Vec::new();
        for (i, (a, ha)) in all.iter().enumerate() {
            for (b, hb) in &all[i + 1..] {
                if !changed.contains(&a.id) && !changed.contains(&b.id) {
                    continue;
                }
                let d = (ha ^ hb).count_ones();
                if d > AVATAR_MAX_DISTANCE {
                    continue;
                }
                // One edge per pair: the value names both hashes, in order.
                let (lo, hi) = if ha <= hb { (ha, hb) } else { (hb, ha) };
                let value = format!("{lo:016x}~{hi:016x}");
                events.extend(link_pairs(
                    tx,
                    self.rule(),
                    ArtifactKind::AvatarPhash,
                    &value,
                    &[a.summary(), b.summary()],
                    json!({ "distance": d, "bits": 64 }),
                )?);
            }
        }
        Ok(events)
    }
}

/// Two urls, domains or subdomains sharing a GA, GTM or AdSense id.
pub struct AnalyticsIdCorrelator;

impl Correlator for AnalyticsIdCorrelator {
    fn rule(&self) -> &'static str {
        "analytics_id"
    }

    fn correlate(
        &self,
        tx: &mut Tx<'_>,
        changed: &BTreeSet<EntityId>,
    ) -> StoreResult<Vec<EventKind>> {
        let changed = roots(tx, changed)?;
        let rows: Vec<spektr_store::EntityRow> = tx
            .reader()
            .entities_with_attr("analytics_ids")?
            .into_iter()
            .filter(|e| is_host(&e.ty) && !e.flags.contains(EntityFlags::HUB))
            .collect();
        let mut by_id: std::collections::BTreeMap<String, Vec<&spektr_store::EntityRow>> =
            Default::default();
        for e in &rows {
            for id in attr_texts(e, "analytics_ids") {
                by_id.entry(id).or_default().push(e);
            }
        }
        let mut events = Vec::new();
        for (id, members) in by_id {
            if members.len() < 2 || !members.iter().any(|m| changed.contains(&m.id)) {
                continue;
            }
            let ms: Vec<EntitySummary> = members.iter().map(|m| m.summary()).collect();
            events.extend(link_pairs(
                tx,
                self.rule(),
                ArtifactKind::AnalyticsId,
                &id,
                &ms,
                json!({ "id": id }),
            )?);
        }
        Ok(events)
    }
}

/// Two hosts with the same favicon hash, only if that hash is on ≤ 20
/// hosts in the case (a framework's default favicon isn't a link).
pub struct FaviconCorrelator;

pub const FAVICON_MAX_HOSTS: usize = 20;

impl Correlator for FaviconCorrelator {
    fn rule(&self) -> &'static str {
        "favicon_mmh3"
    }

    fn correlate(
        &self,
        tx: &mut Tx<'_>,
        changed: &BTreeSet<EntityId>,
    ) -> StoreResult<Vec<EventKind>> {
        let changed = roots(tx, changed)?;
        let rows: Vec<spektr_store::EntityRow> = tx
            .reader()
            .entities_with_attr("favicon_mmh3")?
            .into_iter()
            .filter(|e| is_host(&e.ty) && !e.flags.contains(EntityFlags::HUB))
            .collect();
        let mut by_hash: std::collections::BTreeMap<String, Vec<&spektr_store::EntityRow>> =
            Default::default();
        for e in &rows {
            if let Some(h) = attr_texts(e, "favicon_mmh3").into_iter().next() {
                by_hash.entry(h).or_default().push(e);
            }
        }
        let mut events = Vec::new();
        for (h, members) in by_hash {
            if members.len() < 2
                || members.len() > FAVICON_MAX_HOSTS
                || !members.iter().any(|m| changed.contains(&m.id))
            {
                continue;
            }
            let ms: Vec<EntitySummary> = members.iter().map(|m| m.summary()).collect();
            events.extend(link_pairs(
                tx,
                self.rule(),
                ArtifactKind::FaviconMmh3,
                &h,
                &ms,
                json!({ "mmh3": h, "hosts": members.len() }),
            )?);
        }
        Ok(events)
    }
}

/// The roots on one side of a predicate around an entity.
fn neighbours(
    tx: &Tx<'_>,
    id: EntityId,
    pred: CorePredicate,
    dir: Direction,
) -> StoreResult<Vec<EntitySummary>> {
    let all = EgoParams {
        page_size: usize::MAX,
        ..EgoParams::default()
    };
    Ok(tx
        .reader()
        .ego(id, &all)?
        .groups
        .into_iter()
        .filter(|g| g.key.predicate == Predicate::Core(pred) && g.key.direction == dir)
        .flat_map(|g| g.rows)
        .map(|r| r.entity)
        .collect())
}

/// Two domains with `registered_by` → the same email.
pub struct RegistrantEmailCorrelator;

impl Correlator for RegistrantEmailCorrelator {
    fn rule(&self) -> &'static str {
        "registrant_email"
    }

    fn correlate(
        &self,
        tx: &mut Tx<'_>,
        changed: &BTreeSet<EntityId>,
    ) -> StoreResult<Vec<EventKind>> {
        let mut emails: BTreeSet<EntityId> = BTreeSet::new();
        for id in roots(tx, changed)? {
            let Some(e) = tx.reader().entity(id)? else {
                continue;
            };
            match e.ty.core() {
                Some(CoreType::Email) => {
                    emails.insert(id);
                }
                Some(CoreType::Domain) => {
                    for m in neighbours(tx, id, CorePredicate::RegisteredBy, Direction::Out)? {
                        if m.ty.is(CoreType::Email) {
                            emails.insert(m.id);
                        }
                    }
                }
                _ => {}
            }
        }
        let mut events = Vec::new();
        for email in emails {
            let Some(row) = tx.reader().entity(email)? else {
                continue;
            };
            if row.flags.contains(EntityFlags::HUB) {
                continue;
            }
            let domains: Vec<EntitySummary> =
                neighbours(tx, email, CorePredicate::RegisteredBy, Direction::In)?
                    .into_iter()
                    .filter(|d| d.ty.is(CoreType::Domain) && !d.flags.contains(EntityFlags::HUB))
                    .collect();
            if domains.len() >= 2 {
                events.extend(link_pairs(
                    tx,
                    self.rule(),
                    ArtifactKind::RegistrantEmail,
                    &row.key,
                    &domains,
                    json!({ "email": row.key }),
                )?);
            }
        }
        Ok(events)
    }
}

/// Two domains or subdomains in the SANs of the same certificate. Hosts
/// under the same registrable domain are skipped: `subdomain_of` already
/// links them, and every certificate would otherwise link a site to its
/// own `www`.
pub struct CertSanCorrelator;

fn registrable(e: &EntitySummary) -> String {
    let parts: Vec<&str> = e.key.rsplitn(3, '.').collect();
    match e.ty.core() {
        Some(CoreType::Domain) => e.key.clone(),
        _ if parts.len() >= 2 => format!("{}.{}", parts[1], parts[0]),
        _ => e.key.clone(),
    }
}

impl Correlator for CertSanCorrelator {
    fn rule(&self) -> &'static str {
        "cert_san"
    }

    fn correlate(
        &self,
        tx: &mut Tx<'_>,
        changed: &BTreeSet<EntityId>,
    ) -> StoreResult<Vec<EventKind>> {
        let mut certs: BTreeSet<EntityId> = BTreeSet::new();
        for id in roots(tx, changed)? {
            let Some(e) = tx.reader().entity(id)? else {
                continue;
            };
            match e.ty.core() {
                Some(CoreType::Certificate) => {
                    certs.insert(id);
                }
                Some(CoreType::Domain | CoreType::Subdomain) => {
                    for c in neighbours(tx, id, CorePredicate::IssuedFor, Direction::In)? {
                        certs.insert(c.id);
                    }
                }
                _ => {}
            }
        }
        let mut events = Vec::new();
        for cert in certs {
            let Some(row) = tx.reader().entity(cert)? else {
                continue;
            };
            let hosts: Vec<EntitySummary> =
                neighbours(tx, cert, CorePredicate::IssuedFor, Direction::Out)?
                    .into_iter()
                    .filter(|h| !h.flags.contains(EntityFlags::HUB))
                    .collect();
            // Keep one host per registrable domain, then link across them.
            let mut seen: BTreeSet<String> = BTreeSet::new();
            let distinct: Vec<EntitySummary> = hosts
                .into_iter()
                .filter(|h| seen.insert(registrable(h)))
                .collect();
            if distinct.len() >= 2 {
                events.extend(link_pairs(
                    tx,
                    self.rule(),
                    ArtifactKind::CertSan,
                    &row.key,
                    &distinct,
                    json!({ "certificate": row.value }),
                )?);
            }
        }
        Ok(events)
    }
}

/// Two `person` entities linked to the same email, phone or account at
/// strength corroborated or higher: a review item, never an edge or a merge.
pub struct SamePersonCandidates;

impl Correlator for SamePersonCandidates {
    fn rule(&self) -> &'static str {
        "same_person"
    }

    fn correlate(
        &self,
        tx: &mut Tx<'_>,
        changed: &BTreeSet<EntityId>,
    ) -> StoreResult<Vec<EventKind>> {
        use spektr_model::Strength;
        let all = EgoParams {
            page_size: usize::MAX,
            ..EgoParams::default()
        };
        // The shared identifiers to look at: changed ones, and those linked
        // to changed people.
        let mut shared: BTreeSet<EntityId> = BTreeSet::new();
        for id in roots(tx, changed)? {
            let Some(e) = tx.reader().entity(id)? else {
                continue;
            };
            match e.ty.core() {
                Some(CoreType::Email | CoreType::Phone | CoreType::Account) => {
                    shared.insert(id);
                }
                Some(CoreType::Person) => {
                    for g in tx.reader().ego(id, &all)?.groups {
                        shared.extend(
                            g.rows
                                .iter()
                                .filter(|r| {
                                    matches!(
                                        r.entity.ty.core(),
                                        Some(CoreType::Email | CoreType::Phone | CoreType::Account)
                                    )
                                })
                                .map(|r| r.entity.id),
                        );
                    }
                }
                _ => {}
            }
        }
        let existing: Vec<serde_json::Value> = tx
            .reader()
            .review_items(None)?
            .into_iter()
            .filter(|r| r.kind == spektr_store::ReviewKind::SamePerson)
            .map(|r| r.payload)
            .collect();
        let mut events = Vec::new();
        for id in shared {
            let ego = tx.reader().ego(id, &all)?;
            let people: Vec<(EntitySummary, Strength)> = ego
                .groups
                .iter()
                .filter(|g| g.key.direction == Direction::In && g.key.artifact.is_none())
                .flat_map(|g| g.rows.iter())
                .filter(|r| r.entity.ty.is(CoreType::Person))
                .filter_map(|r| {
                    r.strength
                        .filter(|s| *s >= Strength::Corroborated)
                        .map(|s| (r.entity.clone(), s))
                })
                .collect();
            for (i, (a, _)) in people.iter().enumerate() {
                for (b, _) in &people[i + 1..] {
                    let (x, y) = if a.id.as_bytes() <= b.id.as_bytes() {
                        (a, b)
                    } else {
                        (b, a)
                    };
                    let payload = json!({ "a": x.id, "b": y.id, "via": id, "a_value": x.value, "b_value": y.value, "via_value": ego.focus.value, "via_type": ego.focus.ty });
                    let dup = existing
                        .iter()
                        .any(|p| p.get("a") == payload.get("a") && p.get("b") == payload.get("b"));
                    if dup {
                        continue;
                    }
                    let rid = tx.add_review_item(
                        &Actor::Correlator(self.rule().into()),
                        spektr_store::ReviewKind::SamePerson,
                        &payload,
                    )?;
                    events.push(EventKind::ReviewAdded {
                        id: rid,
                        kind: spektr_store::ReviewKind::SamePerson,
                        module: format!("correlator:{}", self.rule()),
                    });
                }
            }
        }
        Ok(events)
    }
}
