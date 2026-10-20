//! The async store: writer thread plus read pool (ARCHITECTURE §3).

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod support;

use spektr_model::Actor;
use spektr_store::{CaseInit, Db, EgoParams, Store};
use support::*;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn writes_and_concurrent_reads() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("actor.spektr");
    let store = Store::create(&path, &CaseInit::named("actor"), opts()).unwrap();
    let synth = store
        .write(|tx| spektr_store::synth::generate(tx, 400, 3))
        .await
        .unwrap();
    let seed = synth.seed;

    let mut handles = Vec::new();
    for i in 0..32 {
        let s = store.clone();
        handles.push(tokio::spawn(async move {
            if i % 4 == 0 {
                s.write(move |tx| {
                    let o = obs(tx, "github", None, "api.github.com");
                    ent(
                        tx,
                        "username",
                        &format!("concurrent{i}"),
                        1,
                        o,
                        "github.com",
                    );
                    Ok(())
                })
                .await
                .unwrap();
            } else {
                let ego = s
                    .read(move |r| r.ego(seed, &EgoParams::default()))
                    .await
                    .unwrap();
                assert!(!ego.groups.is_empty());
            }
        }));
    }
    for h in handles {
        h.await.unwrap();
    }
    let n = store.read(|r| r.entity_count()).await.unwrap();
    assert_eq!(n, synth.entities + 8);
    store.flush().await.unwrap();
    assert!(store.verify().await.unwrap().ok());
    drop(store);
    let mut db = Db::open(&path, opts()).unwrap();
    assert!(db.verify().unwrap().ok());
    let _ = Actor::Analyst;
}

#[test]
fn blocking_api_without_a_runtime() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("blocking.spektr");
    let store = Store::create(&path, &CaseInit::named("blocking"), opts()).unwrap();
    store
        .write_blocking(|tx| {
            let o = obs(tx, "github", None, "api.github.com");
            ent(tx, "username", "jmarlow.wav", 0, o, "github.com");
            Ok(())
        })
        .unwrap();
    assert_eq!(store.read_blocking(|r| r.entity_count()).unwrap(), 1);
}
