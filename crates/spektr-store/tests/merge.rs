//! Merge / unmerge (DATA_MODEL §6). Round trips leave the graph exactly as
//! before (Phase 1 exit criterion).

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod support;

use spektr_model::{Actor, CorePredicate, EntityId, EntityType};
use spektr_store::{Db, EgoParams, EgoView, EntityRow};
use support::*;

/// Everything a reader can see about the graph, excluding the merge log
/// and the audit chain (which record history, not state).
#[derive(Debug, PartialEq)]
struct Snapshot {
    entities: Vec<EntityRow>,
    edges: Vec<spektr_store::EdgeRow>,
    evidence: Vec<(spektr_model::EdgeId, Vec<spektr_store::EvidenceRow>)>,
    egos: Vec<EgoView>,
    roots: Vec<spektr_store::EntitySummary>,
}

fn snapshot(db: &Db) -> Snapshot {
    let r = db.reader();
    let conn = db.raw_connection();
    let ids: Vec<EntityId> = conn
        .prepare("SELECT id FROM entity ORDER BY id")
        .unwrap()
        .query_map([], |row| row.get::<_, Vec<u8>>(0))
        .unwrap()
        .map(|b| EntityId::from_slice(&b.unwrap()).unwrap())
        .collect();
    let entities = ids
        .iter()
        .map(|id| r.entity(*id).unwrap().unwrap())
        .collect();
    let mut edges = r.edges_touching(&ids).unwrap();
    edges.sort_by_key(|e| e.id);
    let evidence = edges
        .iter()
        .map(|e| (e.id, r.evidence(e.id).unwrap()))
        .collect();
    let roots = r.entities().unwrap();
    let egos = roots
        .iter()
        .map(|s| {
            r.ego(
                s.id,
                &EgoParams {
                    include_rejected: true,
                    ..EgoParams::default()
                },
            )
            .unwrap()
        })
        .collect();
    Snapshot {
        entities,
        edges,
        evidence,
        egos,
        roots,
    }
}

struct G {
    u: [EntityId; 4],
    a: [EntityId; 3],
}

fn graph(c: &mut Case) -> G {
    c.db.write(|tx| {
        let o1 = obs(tx, "github", None, "api.github.com");
        let o2 = obs(tx, "gitlab", None, "gitlab.com");
        let u = [
            ent(tx, "username", "jmarlow.wav", 0, o1, "github.com"),
            ent(tx, "username", "jm4rlow", 1, o1, "github.com"),
            ent(tx, "username", "jordanmarlow", 1, o2, "gitlab.com"),
            ent(tx, "username", "j.marlow", 2, o2, "gitlab.com"),
        ];
        let a = [
            ent(tx, "account", "github:jmarlow-wav", 1, o1, "github.com"),
            ent(tx, "account", "gitlab:jm4rlow", 1, o2, "gitlab.com"),
            ent(tx, "account", "reddit:jordanmarlow", 2, o2, "gitlab.com"),
        ];
        edge(
            tx,
            u[0],
            CorePredicate::HasAccount,
            a[0],
            &[(o1, "github.com")],
        );
        edge(
            tx,
            u[1],
            CorePredicate::HasAccount,
            a[1],
            &[(o2, "gitlab.com")],
        );
        edge(
            tx,
            u[1],
            CorePredicate::HasAccount,
            a[0],
            &[(o2, "gitlab.com")],
        );
        edge(
            tx,
            u[2],
            CorePredicate::HasAccount,
            a[2],
            &[(o2, "gitlab.com")],
        );
        edge(
            tx,
            u[0],
            CorePredicate::VariantOf,
            u[1],
            &[(o1, "github.com")],
        );
        edge(
            tx,
            u[2],
            CorePredicate::VariantOf,
            u[3],
            &[(o2, "gitlab.com")],
        );
        Ok(G { u, a })
    })
    .unwrap()
}

#[test]
fn merge_folds_edges_and_unmerge_restores_exactly() {
    let mut c = new_case();
    let g = graph(&mut c);
    let before = snapshot(&c.db);

    let m =
        c.db.write(|tx| tx.merge(&Actor::Analyst, g.u[0], g.u[1]))
            .unwrap();
    let r = c.db.reader();
    assert_eq!(r.resolve(g.u[1]).unwrap(), g.u[0]);
    let ego = r.ego(g.u[1], &EgoParams::default()).unwrap();
    assert_eq!(ego.focus.id, g.u[0], "reads resolve to the root");
    let has_account = ego
        .groups
        .iter()
        .find(|g| g.key.predicate.as_str() == "has_account")
        .unwrap();
    // a[0] was reached by both usernames; it folds into one row with both sources
    assert_eq!(has_account.total, 2);
    let a0 = has_account
        .rows
        .iter()
        .find(|row| row.entity.id == g.a[0])
        .unwrap();
    assert_eq!(a0.edges.len(), 2);
    assert_eq!(a0.sources, 2);
    assert_eq!(a0.strength, Some(spektr_model::Strength::Corroborated));
    // the variant_of edge between the two became a self-loop and is hidden
    assert!(
        ego.groups
            .iter()
            .all(|g| g.key.predicate.as_str() != "variant_of")
    );
    assert!(r.entities().unwrap().iter().all(|e| e.id != g.u[1]));

    c.db.write(|tx| tx.unmerge(&Actor::Analyst, m)).unwrap();
    assert_eq!(snapshot(&c.db), before);
    assert!(c.db.verify().unwrap().ok());
}

#[test]
fn chained_merges_repoint_and_undo_newest_first() {
    let mut c = new_case();
    let g = graph(&mut c);
    let before = snapshot(&c.db);

    // u1 → u0, then u0 → u2: u1 must be re-pointed straight at the root u2.
    let m1 =
        c.db.write(|tx| tx.merge(&Actor::Analyst, g.u[0], g.u[1]))
            .unwrap();
    let after_m1 = snapshot(&c.db);
    let m2 =
        c.db.write(|tx| tx.merge(&Actor::Analyst, g.u[2], g.u[0]))
            .unwrap();
    let r = c.db.reader();
    assert_eq!(
        r.entity(g.u[1]).unwrap().unwrap().merged_into,
        Some(g.u[2]),
        "merged_into always points at the root"
    );
    assert_eq!(r.entity(g.u[0]).unwrap().unwrap().merged_into, Some(g.u[2]));
    let row = r.merge_row(m2).unwrap().unwrap();
    assert_eq!(row.repointed, vec![(g.u[1], g.u[0])]);
    // merging into a merged entity resolves it to its root first
    let m3 =
        c.db.write(|tx| tx.merge(&Actor::Analyst, g.u[1], g.u[3]))
            .unwrap();
    assert_eq!(
        c.db.reader().merge_row(m3).unwrap().unwrap().survivor,
        g.u[2]
    );

    // out of order is refused
    let err =
        c.db.write(|tx| tx.unmerge(&Actor::Analyst, m1))
            .unwrap_err();
    assert!(err.to_string().contains("later merge"), "{err}");

    c.db.write(|tx| tx.unmerge(&Actor::Analyst, m3)).unwrap();
    c.db.write(|tx| tx.unmerge(&Actor::Analyst, m2)).unwrap();
    assert_eq!(snapshot(&c.db), after_m1);
    c.db.write(|tx| tx.unmerge(&Actor::Analyst, m1)).unwrap();
    assert_eq!(snapshot(&c.db), before);
    let err =
        c.db.write(|tx| tx.unmerge(&Actor::Analyst, m1))
            .unwrap_err();
    assert!(err.to_string().contains("already undone"), "{err}");
    assert!(c.db.verify().unwrap().ok());
}

#[test]
fn merge_rules() {
    let mut c = new_case();
    let g = graph(&mut c);
    let err =
        c.db.write(|tx| tx.merge(&Actor::Analyst, g.u[0], g.a[0]))
            .unwrap_err();
    assert!(err.to_string().contains("can't merge"), "{err}");
    let err =
        c.db.write(|tx| tx.merge(&Actor::Analyst, g.u[0], g.u[0]))
            .unwrap_err();
    assert!(err.to_string().contains("already the same"), "{err}");
    c.db.write(|tx| tx.merge(&Actor::Analyst, g.u[0], g.u[1]))
        .unwrap();
    // re-ingesting the merged key finds that row; reads resolve it
    let id =
        c.db.write(|tx| {
            let o = obs(tx, "x", None, "x.example");
            Ok(ent(tx, "username", "JM4RLOW", 1, o, "x.example"))
        })
        .unwrap();
    assert_eq!(id, g.u[1]);
    let r = c.db.reader();
    assert_eq!(
        r.find(&"username".parse::<EntityType>().unwrap(), "jm4rlow")
            .unwrap(),
        Some(g.u[0])
    );
    // merge + unmerge each wrote exactly one audit row
    assert_eq!(r.audit_by_action("entity.merge").unwrap().len(), 1);
}
