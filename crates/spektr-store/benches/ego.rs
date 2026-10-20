//! Ego-query benchmark (Phase 1 exit criterion: < 20 ms on a generated
//! 2,000-entity case). Run with `cargo bench -p spektr-store --bench ego`.
//! Exits non-zero if any scenario's p95 is 20 ms or more.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::print_stdout)]

use std::time::{Duration, Instant};

use spektr_model::{Clock, IdGen};
use spektr_store::{CaseInit, Db, EgoParams, OpenOptions};

const RUNS: usize = 200;
const LIMIT: Duration = Duration::from_millis(20);

fn opts() -> OpenOptions {
    OpenOptions::new(
        Clock::Fixed("2026-10-20T18:14:22Z".parse().unwrap()),
        IdGen::seeded(1),
    )
    .unwrap()
}

fn stats(mut xs: Vec<Duration>) -> (Duration, Duration, Duration, Duration) {
    xs.sort();
    let p = |q: f64| xs[((xs.len() as f64 - 1.0) * q).round() as usize];
    (xs[0], p(0.5), p(0.95), xs[xs.len() - 1])
}

fn main() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("bench.spektr");
    let t = Instant::now();
    let synth = {
        let mut db = Db::create(&path, &CaseInit::named("bench"), opts()).unwrap();
        db.write(|tx| spektr_store::synth::generate(tx, 2000, 42))
            .unwrap()
    };
    println!(
        "generated {} entities, {} edges in {:.2?} (profile: {})",
        synth.entities,
        synth.edges,
        t.elapsed(),
        if cfg!(debug_assertions) {
            "dev"
        } else {
            "release"
        }
    );

    // A fresh connection, as the read pool would have.
    let mut db = Db::open_read_only(&path, opts()).unwrap();
    let seed_ego = db
        .read(|r| r.ego(synth.seed, &EgoParams::default()))
        .unwrap();
    let neighbours: usize = seed_ego.groups.iter().map(|g| g.total).sum();
    let mut late_page = EgoParams::default();
    if let Some(g) = seed_ego.groups.first() {
        late_page.pages.insert(g.key.clone(), g.pages - 1);
    }

    let scenarios: [(&str, spektr_model::EntityId, &EgoParams); 3] = [
        ("seed (largest ego)", synth.seed, &EgoParams::default()),
        ("typical account", synth.typical, &EgoParams::default()),
        ("seed, last page", synth.seed, &late_page),
    ];
    println!(
        "seed ego: {neighbours} neighbours in {} groups",
        seed_ego.groups.len()
    );
    let mut failed = false;
    for (name, focus, params) in scenarios {
        for _ in 0..10 {
            db.read(|r| r.ego(focus, params)).unwrap();
        }
        let times: Vec<Duration> = (0..RUNS)
            .map(|_| {
                let t = Instant::now();
                let v = db.read(|r| r.ego(focus, params)).unwrap();
                std::hint::black_box(v);
                t.elapsed()
            })
            .collect();
        let (min, p50, p95, max) = stats(times);
        let ok = p95 < LIMIT;
        failed |= !ok;
        println!(
            "{name:<22} runs {RUNS}  min {min:>9.3?}  p50 {p50:>9.3?}  p95 {p95:>9.3?}  max {max:>9.3?}  {}",
            if ok {
                "ok (< 20 ms)"
            } else {
                "FAIL (>= 20 ms)"
            }
        );
    }
    if failed {
        std::process::exit(1);
    }
}
