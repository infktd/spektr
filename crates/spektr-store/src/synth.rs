//! Generated cases for tests, benchmarks and QA (e.g. the 2,000-entity case
//! in Phase 1 and Phase 3). All data is fictional: `.example` domains,
//! documentation IP ranges, invented handles.

use serde_json::json;
use spektr_model::{
    Actor, Attr, AttrValue, CorePredicate, CoreType, EdgeId, EntityFlags, EntityId, EntityType,
    KeyHint, NormalizeCtx, ObservationId, Vocabulary, normalize,
};

use crate::error::StoreResult;
use crate::rows::{EdgeAttrs, ObservationKind};
use crate::tx::{NewEdge, NewObservation, Tx};

/// What [`generate`] built.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SynthCase {
    pub entities: u64,
    pub edges: u64,
    /// The seed username; it has the largest ego (hundreds of neighbours).
    pub seed: EntityId,
    /// An ordinary account with a handful of neighbours.
    pub typical: EntityId,
}

const PLATFORMS: &[&str] = &[
    "github",
    "gitlab",
    "bluesky",
    "reddit",
    "soundcloud",
    "strava",
    "keybase",
    "devto",
    "lastfm",
    "steam",
    "twitch",
    "medium",
];

/// xorshift64*: deterministic and dependency-free.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn chance(&mut self, one_in: u64) -> bool {
        self.next().is_multiple_of(one_in)
    }
}

/// Generate at least `target` root entities into a fresh case, in this
/// transaction.
pub fn generate(tx: &mut Tx<'_>, target: u64, seed: u64) -> StoreResult<SynthCase> {
    let vocab = Vocabulary::builtin();
    let ctx = NormalizeCtx {
        vocab: &vocab,
        default_region: Some("US"),
    };
    let mut rng = Rng(seed.max(1));
    let analyst = Actor::Analyst;
    let base: jiff::Timestamp = "2024-01-01T00:00:00Z".parse().unwrap_or(tx.now());

    let ingest =
        |tx: &mut Tx<'_>, ty: CoreType, value: &str, depth: u32, obs: ObservationId, key: &str| {
            let n = normalize(&EntityType::Core(ty), value, KeyHint::None, ctx)?;
            tx.ingest_entity(&analyst, &n, depth, Some((obs, key)))
                .map(|e| e.id)
        };
    let observe = |tx: &mut Tx<'_>, module: &str, input: Option<EntityId>, host: &str| {
        tx.add_observation(
            &Actor::Module {
                name: module.into(),
                version: "1.0.0".into(),
            },
            NewObservation {
                job: None,
                module: module.into(),
                module_version: "1.0.0".into(),
                manifest_hash: "synth".into(),
                input_entity: input,
                source_url: Some(format!("https://{host}/synth")),
                content_type: Some("application/json".into()),
                blob: None,
                summary: json!({ "synth": true }),
                kind: ObservationKind::Module,
            },
        )
    };
    let link = |tx: &mut Tx<'_>,
                src: EntityId,
                p: CorePredicate,
                dst: EntityId,
                disc: &str,
                ev: &[(ObservationId, &str)]|
     -> StoreResult<EdgeId> {
        let (edge, _) = tx.upsert_edge(
            &analyst,
            NewEdge {
                src,
                predicate: p.into(),
                dst,
                discriminator: disc.into(),
                attrs: EdgeAttrs::new(),
                origin: "synth".into(),
            },
        )?;
        for (o, k) in ev {
            tx.add_evidence(&analyst, edge, *o, k)?;
        }
        Ok(edge)
    };

    let sweep = observe(tx, "username-sweep", None, "sweep.example")?;
    let seed_id = ingest(
        tx,
        CoreType::Username,
        "synth.seed",
        0,
        sweep,
        "username-sweep",
    )?;
    tx.set_flags(&analyst, seed_id, EntityFlags::SEED, EntityFlags::NONE)?;

    let mut typical = seed_id;
    let mut last_account = seed_id;
    let mut accounts: Vec<EntityId> = Vec::new();
    let mut i: u64 = 0;
    loop {
        if i.is_multiple_of(16) && tx.reader().entity_count()? >= target {
            break;
        }
        let platform = PLATFORMS[(i as usize) % PLATFORMS.len()];
        let host = format!("api.{platform}.example");
        match i % 8 {
            0..=2 => {
                let obs = observe(tx, platform, Some(seed_id), &host)?;
                let acct = ingest(
                    tx,
                    CoreType::Account,
                    &format!("{platform}:user{i:04}"),
                    1,
                    obs,
                    platform,
                )?;
                let mut ev = vec![(sweep, "username-sweep")];
                if rng.chance(2) {
                    ev.push((obs, "platform.example"));
                }
                let edge = link(tx, seed_id, CorePredicate::HasAccount, acct, "", &ev)?;
                if rng.chance(17) {
                    tx.confirm_edge(&analyst, edge)?;
                }
                tx.add_attr(
                    &analyst,
                    acct,
                    "created_at",
                    Attr {
                        value: AttrValue::Time(
                            base + jiff::SignedDuration::from_hours(24 * i as i64),
                        ),
                        observation: obs,
                        event: true,
                    },
                )?;
                tx.add_attr(
                    &analyst,
                    acct,
                    "display_name",
                    Attr {
                        value: AttrValue::Text(format!("Synth User {i}")),
                        observation: obs,
                        event: false,
                    },
                )?;
                if let Some(prev) = accounts.last().copied()
                    && i.is_multiple_of(5)
                {
                    link(
                        tx,
                        prev,
                        CorePredicate::SharesArtifact,
                        acct,
                        &format!("avatar_phash:{:016x}", rng.next()),
                        &[(obs, "correlator:avatar_phash")],
                    )?;
                }
                accounts.push(acct);
                last_account = acct;
                if i == 9 {
                    typical = acct;
                }
            }
            3 => {
                let obs = observe(tx, "variants", Some(seed_id), "variants.example")?;
                let u = ingest(
                    tx,
                    CoreType::Username,
                    &format!("user{i:04}"),
                    1,
                    obs,
                    "variants",
                )?;
                link(
                    tx,
                    seed_id,
                    CorePredicate::VariantOf,
                    u,
                    "",
                    &[(obs, "variants")],
                )?;
                let a = ingest(
                    tx,
                    CoreType::Account,
                    &format!("{platform}:user{i:04}"),
                    2,
                    obs,
                    "variants",
                )?;
                link(
                    tx,
                    u,
                    CorePredicate::HasAccount,
                    a,
                    "",
                    &[(obs, "variants")],
                )?;
            }
            4 => {
                let obs = observe(tx, platform, Some(last_account), &host)?;
                let e = ingest(
                    tx,
                    CoreType::Email,
                    &format!("user{i:04}@mail{}.example", i % 20),
                    2,
                    obs,
                    platform,
                )?;
                link(
                    tx,
                    last_account,
                    CorePredicate::HasEmail,
                    e,
                    "",
                    &[(obs, "platform.example")],
                )?;
            }
            5 => {
                let obs = observe(tx, platform, Some(last_account), &host)?;
                let u = ingest(
                    tx,
                    CoreType::Url,
                    &format!("https://site{}.example/p{i}", i % 60),
                    2,
                    obs,
                    platform,
                )?;
                link(
                    tx,
                    last_account,
                    CorePredicate::LinksTo,
                    u,
                    "",
                    &[(obs, "platform.example")],
                )?;
            }
            6 => {
                let obs = observe(tx, "dns", None, "dns.example")?;
                let h = ingest(
                    tx,
                    CoreType::Subdomain,
                    &format!("h{i}.site{}.example", i % 60),
                    2,
                    obs,
                    "dns",
                )?;
                let net = ["192.0.2", "198.51.100", "203.0.113"][(i % 3) as usize];
                let ip = ingest(
                    tx,
                    CoreType::Ip,
                    &format!("{net}.{}", (i / 3) % 250 + 1),
                    3,
                    obs,
                    "dns",
                )?;
                link(tx, h, CorePredicate::ResolvesTo, ip, "", &[(obs, "dns")])?;
            }
            _ => {
                let obs = observe(tx, "avatar-hash", Some(last_account), &host)?;
                let sha = tx.put_blob(&analyst, format!("synthetic image {i}").as_bytes())?;
                let hex = sha.hex();
                let n = normalize(
                    &CoreType::Image.into(),
                    &format!("img{i}.png"),
                    KeyHint::Sha256(&hex),
                    ctx,
                )?;
                let img = tx.ingest_entity(&analyst, &n, 2, None)?.id;
                link(
                    tx,
                    last_account,
                    CorePredicate::Posted,
                    img,
                    "",
                    &[(obs, "platform.example")],
                )?;
                tx.add_attr(
                    &analyst,
                    img,
                    "taken_at",
                    Attr {
                        value: AttrValue::Time(
                            base + jiff::SignedDuration::from_hours(12 * i as i64),
                        ),
                        observation: obs,
                        event: true,
                    },
                )?;
            }
        }
        i += 1;
    }
    let r = tx.reader();
    Ok(SynthCase {
        entities: r.entity_count()?,
        edges: r.edge_count()?,
        seed: seed_id,
        typical,
    })
}
