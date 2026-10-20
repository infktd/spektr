//! Graph queries: derived edges, strength, ego groups and paging, symmetric
//! storage, paths, k-hop, degree, hubs, timeline.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod support;

use std::collections::BTreeMap;

use spektr_model::{Actor, Attr, AttrValue, CorePredicate, EntityFlags, EntityType, Strength};
use spektr_store::{Direction, EgoParams};
use support::*;

fn ty(s: &str) -> EntityType {
    s.parse().unwrap()
}

#[test]
fn derived_edges_are_created_at_insert_with_evidence() {
    let mut c = new_case();
    let e =
        c.db.write(|tx| {
            let o = obs(tx, "gravatar", None, "api.gravatar.com");
            Ok(ent(
                tx,
                "email",
                "Ops@Mail.Kestrel.Example",
                1,
                o,
                "gravatar.com",
            ))
        })
        .unwrap();
    let r = c.db.reader();
    let sub = r
        .find(&ty("subdomain"), "mail.kestrel.example")
        .unwrap()
        .unwrap();
    let dom = r.find(&ty("domain"), "kestrel.example").unwrap().unwrap();
    assert_eq!(r.entity(sub).unwrap().unwrap().depth, 2);
    assert_eq!(r.entity(dom).unwrap().unwrap().depth, 3);
    let ego = r.ego(e, &EgoParams::default()).unwrap();
    let g = &ego.groups[0];
    assert_eq!(g.key.predicate.as_str(), "at_domain");
    assert_eq!(g.key.direction, Direction::Out);
    assert_eq!(g.rows[0].entity.id, sub);
    assert_eq!(g.rows[0].strength, Some(Strength::Single));
    let ego = r.ego(sub, &EgoParams::default()).unwrap();
    let preds: Vec<_> = ego
        .groups
        .iter()
        .map(|g| (g.key.predicate.as_str().to_string(), g.key.direction))
        .collect();
    assert!(preds.contains(&("subdomain_of".into(), Direction::Out)));
    assert!(preds.contains(&("at_domain".into(), Direction::In)));
}

#[test]
fn strength_levels_and_edge_states() {
    let mut c = new_case();
    let (u, a, e) =
        c.db.write(|tx| {
            let o1 = obs(tx, "username-sweep", None, "github.com");
            let u = ent(tx, "username", "jmarlow.wav", 0, o1, "username-sweep");
            let a = ent(tx, "account", "github:jmarlow-wav", 1, o1, "username-sweep");
            let e = edge(
                tx,
                u,
                CorePredicate::HasAccount,
                a,
                &[(o1, "username-sweep")],
            );
            Ok((u, a, e))
        })
        .unwrap();
    let strength = |c: &Case| c.db.reader().edge_strength(e).unwrap();
    assert_eq!(strength(&c), Some(Strength::Single));
    // a second observation from the same source key doesn't corroborate
    c.db.write(|tx| {
        let o = obs(tx, "username-sweep", None, "github.com");
        tx.add_evidence(&Actor::Analyst, e, o, "username-sweep")
            .map(|_| ())
    })
    .unwrap();
    assert_eq!(strength(&c), Some(Strength::Single));
    c.db.write(|tx| {
        let o = obs(tx, "github", Some(u), "api.github.com");
        tx.add_evidence(&Actor::Analyst, e, o, "github.com")
            .map(|_| ())
    })
    .unwrap();
    assert_eq!(strength(&c), Some(Strength::Corroborated));
    c.db.write(|tx| tx.confirm_edge(&Actor::Analyst, e))
        .unwrap();
    assert_eq!(strength(&c), Some(Strength::Confirmed));
    c.db.write(|tx| tx.unconfirm_edge(&Actor::Analyst, e))
        .unwrap();
    assert_eq!(strength(&c), Some(Strength::Corroborated));

    c.db.write(|tx| {
        tx.set_flags(&Actor::Analyst, u, EntityFlags::SEED, EntityFlags::NONE)
            .map(|_| ())
    })
    .unwrap();
    assert_eq!(
        c.db.reader().entity_strength(a).unwrap(),
        Some(Strength::Corroborated)
    );
    assert_eq!(c.db.reader().entity_strength(u).unwrap(), None);

    c.db.write(|tx| tx.reject_edge(&Actor::Analyst, e)).unwrap();
    let r = c.db.reader();
    assert_eq!(r.edge_strength(e).unwrap(), None);
    assert!(
        r.ego(u, &EgoParams::default())
            .unwrap()
            .groups
            .iter()
            .all(|g| g.key.predicate.as_str() != "has_account")
    );
    let with_rejected = r
        .ego(
            u,
            &EgoParams {
                include_rejected: true,
                ..EgoParams::default()
            },
        )
        .unwrap();
    let row = &with_rejected
        .groups
        .iter()
        .find(|g| g.key.predicate.as_str() == "has_account")
        .unwrap()
        .rows[0];
    assert!(row.rejected);
    assert_eq!(row.strength, None);
    assert_eq!(r.degree(u).unwrap().total, 0);
    assert_eq!(r.entity_strength(a).unwrap(), None);
    c.db.write(|tx| tx.restore_edge(&Actor::Analyst, e))
        .unwrap();
    assert_eq!(
        c.db.reader().edge_strength(e).unwrap(),
        Some(Strength::Corroborated)
    );
}

#[test]
fn symmetric_edges_are_stored_once() {
    let mut c = new_case();
    c.db.write(|tx| {
        let o = obs(tx, "variants", None, "variants.example");
        let a = ent(tx, "username", "jmarlow", 0, o, "variants");
        let b = ent(tx, "username", "jm4rlow", 0, o, "variants");
        let e1 = edge(tx, a, CorePredicate::VariantOf, b, &[]);
        let e2 = edge(tx, b, CorePredicate::VariantOf, a, &[]);
        assert_eq!(e1, e2);
        let row = tx.reader().edge(e1).unwrap().unwrap();
        assert!(row.src.as_bytes() < row.dst.as_bytes());
        // non-symmetric predicates keep their direction
        assert!(
            tx.upsert_edge(
                &Actor::Analyst,
                spektr_store::NewEdge {
                    src: a,
                    predicate: CorePredicate::VariantOf.into(),
                    dst: a,
                    discriminator: String::new(),
                    attrs: Default::default(),
                    origin: "t".into()
                }
            )
            .is_err(),
            "self-loops are refused"
        );
        Ok(())
    })
    .unwrap();
}

#[test]
fn ego_groups_page_and_correlations_group_by_artifact() {
    let mut c = new_case();
    let (seed, accounts) =
        c.db.write(|tx| {
            let o = obs(tx, "username-sweep", None, "sweep.example");
            let seed = ent(tx, "username", "jmarlow.wav", 0, o, "username-sweep");
            let mut accounts = Vec::new();
            for i in 0..12 {
                let a = ent(
                    tx,
                    "account",
                    &format!("somesite{i}:jmarlow"),
                    1,
                    o,
                    "username-sweep",
                );
                edge(
                    tx,
                    seed,
                    CorePredicate::HasAccount,
                    a,
                    &[(o, "username-sweep")],
                );
                accounts.push(a);
            }
            let oc = obs(tx, "avatar-hash", None, "img.example");
            edge_d(
                tx,
                accounts[0],
                CorePredicate::SharesArtifact,
                accounts[1],
                "avatar_phash:00ff00ff00ff00ff",
                &[(oc, "correlator:avatar_phash")],
            );
            edge_d(
                tx,
                accounts[0],
                CorePredicate::SharesArtifact,
                accounts[2],
                "avatar_phash:00ff00ff00ff00fe",
                &[(oc, "correlator:avatar_phash")],
            );
            edge_d(
                tx,
                accounts[0],
                CorePredicate::SharesArtifact,
                accounts[3],
                "ip:203.0.113.24",
                &[(oc, "correlator:ip")],
            );
            Ok((seed, accounts))
        })
        .unwrap();
    let r = c.db.reader();
    let ego = r.ego(seed, &EgoParams::default()).unwrap();
    let g = &ego.groups[0];
    assert_eq!((g.total, g.pages, g.page, g.rows.len()), (12, 3, 0, 5));
    let mut pages = BTreeMap::new();
    pages.insert(g.key.clone(), 2);
    let ego2 = r
        .ego(
            seed,
            &EgoParams {
                pages: pages.clone(),
                ..EgoParams::default()
            },
        )
        .unwrap();
    assert_eq!((ego2.groups[0].page, ego2.groups[0].rows.len()), (2, 2));
    pages.insert(g.key.clone(), 99);
    let ego3 = r
        .ego(
            seed,
            &EgoParams {
                pages,
                ..EgoParams::default()
            },
        )
        .unwrap();
    assert_eq!(
        ego3.groups[0].page, 2,
        "out-of-range pages clamp to the last"
    );
    assert_eq!(ego.degree.outbound, 12);
    assert!(ego.observations >= 1);

    let ego = r.ego(accounts[0], &EgoParams::default()).unwrap();
    let correlated: Vec<_> = ego
        .groups
        .iter()
        .filter(|g| g.key.is_correlated())
        .collect();
    assert_eq!(correlated.len(), 2, "one group per artifact kind");
    let avatar = correlated
        .iter()
        .find(|g| g.key.artifact == Some(spektr_model::ArtifactKind::AvatarPhash))
        .unwrap();
    assert_eq!(avatar.total, 2);
    assert_eq!(avatar.key.direction, Direction::Sym);
    assert!(avatar.rows.iter().all(|row| row.artifact_value.is_some()));

    // The seed's ego shows its accounts' correlations as `≈` groups too
    // (artboard 08), one row per account, acting on an account-account edge.
    let ego = r.ego(seed, &EgoParams::default()).unwrap();
    let avatar = ego
        .groups
        .iter()
        .find(|g| g.key.artifact == Some(spektr_model::ArtifactKind::AvatarPhash))
        .expect("the avatar cluster");
    let mut ids: Vec<_> = avatar.rows.iter().map(|r| r.entity.id).collect();
    ids.sort();
    let mut want = vec![accounts[0], accounts[1], accounts[2]];
    want.sort();
    assert_eq!(ids, want);
    let first = avatar
        .rows
        .iter()
        .find(|r| r.entity.id == accounts[0])
        .unwrap();
    assert_eq!(first.edges.len(), 2, "both of account 0's avatar edges");
    assert_eq!(first.strength, Some(Strength::Single));
    let ip = ego
        .groups
        .iter()
        .find(|g| g.key.artifact == Some(spektr_model::ArtifactKind::Ip))
        .expect("the ip cluster");
    assert_eq!(ip.total, 2);
    // the seed's own degree doesn't count the clusters
    assert_eq!(ego.degree.outbound, 12);
}

#[test]
fn paths_khop_and_hubs() {
    let mut c = new_case();
    let (u, a, url, lone) =
        c.db.write(|tx| {
            let o = obs(tx, "github", None, "api.github.com");
            let u = ent(tx, "username", "jmarlow.wav", 0, o, "github.com");
            let a = ent(tx, "account", "somesite:jmarlow", 1, o, "github.com");
            let url = ent(tx, "url", "https://jmarlow.example/", 2, o, "github.com");
            let lone = ent(tx, "username", "nobody", 0, o, "github.com");
            edge(tx, u, CorePredicate::HasAccount, a, &[(o, "github.com")]);
            edge(tx, a, CorePredicate::LinksTo, url, &[(o, "github.com")]);
            Ok((u, a, url, lone))
        })
        .unwrap();
    let r = c.db.reader();
    let p = r.shortest_path(u, url, 6).unwrap().unwrap();
    let names: Vec<_> = p.steps.iter().map(|s| s.entity.value.clone()).collect();
    // url → on_host → jmarlow.example comes for free from normalization
    assert_eq!(
        names,
        [
            "jmarlow.wav",
            "somesite:jmarlow",
            "https://jmarlow.example/"
        ]
    );
    assert_eq!(p.steps[1].via.as_ref().unwrap().1.as_str(), "has_account");
    let dom = r.find(&ty("domain"), "jmarlow.example").unwrap().unwrap();
    // reverse direction works too
    assert_eq!(r.shortest_path(dom, u, 6).unwrap().unwrap().steps.len(), 4);
    assert!(
        r.shortest_path(u, dom, 2).unwrap().is_none(),
        "max hops is respected"
    );
    assert!(r.shortest_path(u, lone, 6).unwrap().is_none());
    let k = r.khop(u, 2).unwrap();
    assert_eq!(
        k.iter().map(|(e, h)| (e.id, *h)).collect::<Vec<_>>(),
        vec![(a, 1), (url, 2)]
    );

    // known hubs are flagged at insert; degree hubs on refresh
    let gm =
        c.db.write(|tx| {
            let o = obs(tx, "x", None, "x.example");
            Ok(ent(tx, "domain", "gmail.com", 1, o, "x"))
        })
        .unwrap();
    assert!(
        c.db.reader()
            .entity(gm)
            .unwrap()
            .unwrap()
            .flags
            .contains(EntityFlags::HUB)
    );
    let is_hub =
        c.db.write(|tx| tx.refresh_hub(&Actor::Engine, a, 1))
            .unwrap();
    assert!(is_hub, "degree 2 > threshold 1");
    assert!(
        !c.db
            .write(|tx| tx.refresh_hub(&Actor::Engine, lone, 1))
            .unwrap()
    );
}

#[test]
fn timeline_lists_event_attributes_in_order() {
    let mut c = new_case();
    c.db.write(|tx| {
        let o = obs(tx, "github", None, "api.github.com");
        let a = ent(tx, "account", "github:jmarlow-wav", 1, o, "github.com");
        let b = ent(tx, "account", "gitlab:jmarlow", 1, o, "github.com");
        let ev = |v: &str| Attr {
            value: AttrValue::Time(v.parse().unwrap()),
            observation: o,
            event: true,
        };
        tx.add_attr(
            &Actor::Analyst,
            a,
            "last_active",
            ev("2026-10-16T09:12:00Z"),
        )
        .unwrap();
        tx.add_attr(
            &Actor::Analyst,
            a,
            "created_at",
            Attr {
                value: AttrValue::Date("2019-03-01".parse().unwrap()),
                observation: o,
                event: true,
            },
        )
        .unwrap();
        tx.add_attr(&Actor::Analyst, b, "created_at", ev("2021-05-05T00:00:00Z"))
            .unwrap();
        tx.add_attr(
            &Actor::Analyst,
            b,
            "bio",
            Attr {
                value: AttrValue::Text("not an event".into()),
                observation: o,
                event: false,
            },
        )
        .unwrap();
        Ok(())
    })
    .unwrap();
    let t = c.db.reader().timeline().unwrap();
    let rows: Vec<_> = t
        .iter()
        .map(|e| (e.at.to_string(), e.attr.as_str(), e.module.as_str()))
        .collect();
    assert_eq!(
        rows,
        [
            ("2019-03-01T00:00:00Z".to_string(), "created_at", "github"),
            ("2021-05-05T00:00:00Z".to_string(), "created_at", "github"),
            ("2026-10-16T09:12:00Z".to_string(), "last_active", "github"),
        ]
    );
}

#[test]
fn cache_lookup_and_entity_observations() {
    let mut c = new_case();
    let (u, o) =
        c.db.write(|tx| {
            let o0 = obs(tx, "seed", None, "seed.example");
            let u = ent(tx, "username", "jmarlow.wav", 0, o0, "seed");
            let o = obs(tx, "github", Some(u), "api.github.com");
            Ok((u, o))
        })
        .unwrap();
    let r = c.db.reader();
    let since: jiff::Timestamp = "2026-10-19T18:14:22Z".parse().unwrap();
    assert_eq!(
        r.cached_observation("github", "1.0.0", u, since).unwrap(),
        Some(o)
    );
    assert_eq!(
        r.cached_observation("github", "2.0.0", u, since).unwrap(),
        None
    );
    let later: jiff::Timestamp = "2026-10-21T00:00:00Z".parse().unwrap();
    assert_eq!(
        r.cached_observation("github", "1.0.0", u, later).unwrap(),
        None
    );
    let obs_ids: Vec<_> = r
        .entity_observations(u)
        .unwrap()
        .into_iter()
        .map(|o| o.id)
        .collect();
    assert!(obs_ids.contains(&o));
}

#[test]
fn synthetic_case_has_two_thousand_entities_and_a_large_ego() {
    let mut c = new_case();
    let s =
        c.db.write(|tx| spektr_store::synth::generate(tx, 2000, 7))
            .unwrap();
    assert!(s.entities >= 2000, "{s:?}");
    let r = c.db.reader();
    let ego = r.ego(s.seed, &EgoParams::default()).unwrap();
    let total: usize = ego.groups.iter().map(|g| g.total).sum();
    assert!(total > 300, "seed ego has {total} neighbours");
    assert!(ego.groups.iter().all(|g| g.rows.len() <= 5));
    assert!(c.db.verify().unwrap().ok());
}
