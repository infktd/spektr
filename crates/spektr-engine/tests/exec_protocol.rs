//! The exec runtime against a fake module script (Python, standard library
//! only, `tests/fixtures/fake.py`), in per-job and serve modes (Phase 2 exit
//! criterion).

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod support;

use std::path::PathBuf;
use std::time::{Duration, Instant};

use spektr_engine::{ActionOutcome, EngineAction, EventKind};
use spektr_model::EntityType;
use spektr_store::{EgoParams, JobState};
use support::*;

const WAIT: Duration = Duration::from_secs(20);

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

async fn exec_case(marker: &std::path::Path, dirs: &'static [&'static str]) -> TestCase {
    let marker = marker.to_path_buf();
    case_with(
        spektr_store::CaseInit::named("exec"),
        move |c| {
            c.secrets = secrets(&[("FAKE_TOKEN", "tok-4f1c9e-fictional")]);
            c.exec_env
                .insert("FAKE_MARKER".into(), marker.display().to_string());
        },
        |e| {
            for d in dirs {
                let warnings = e.load_exec_modules(&fixtures().join(d));
                assert!(warnings.is_empty(), "{warnings:?}");
            }
        },
    )
    .await
}

fn jobs_by_module(jobs: &[spektr_store::JobRow], m: &str) -> Vec<spektr_store::JobRow> {
    jobs.iter().filter(|j| j.module == m).cloned().collect()
}

#[tokio::test]
async fn both_modes_round_trip_every_message_type() {
    let marker = tempfile::NamedTempFile::new().unwrap();
    let mut t = exec_case(marker.path(), &["modules", "serve"]).await;
    t.handle
        .act(EngineAction::Seed {
            entity: "username:jmarlow".parse().unwrap(),
        })
        .await
        .unwrap();
    t.handle
        .act(EngineAction::Seed {
            entity: "username:jm4rlow".parse().unwrap(),
        })
        .await
        .unwrap();
    t.handle.wait_idle(WAIT).await.unwrap();

    let jobs = t.handle.store().read(|r| r.jobs(&[])).await.unwrap();
    for m in ["fake", "fake-serve"] {
        let js = jobs_by_module(&jobs, m);
        assert_eq!(js.len(), 2, "{m}: {js:?}");
        assert!(js.iter().all(|j| j.state == JobState::Done), "{m}: {js:?}");
    }
    let ty = |s: &str| s.parse::<EntityType>().unwrap();
    let (acct, inst, ego) = t
        .handle
        .store()
        .read(move |r| {
            let a = r
                .find(&ty("account"), "lemmy:jmarlow@lemmy.example")?
                .unwrap();
            let i = r.find(&ty("x.fake.instance"), "lemmy.example")?;
            Ok((r.entity(a)?.unwrap(), i, r.ego(a, &EgoParams::default())?))
        })
        .await
        .unwrap();
    assert_eq!(acct.depth, 1);
    assert_eq!(
        acct.attrs["display_name"][0].value,
        spektr_model::AttrValue::Text("Jordan M.".into())
    );
    assert!(acct.attrs["last_active"][0].event);
    assert_eq!(
        acct.attrs["token_seen"][0].value,
        spektr_model::AttrValue::Bool(true),
        "optional secret was passed in `run`"
    );
    assert!(
        inst.is_some(),
        "extension type normalized with its `lowercase` rule"
    );
    let preds: Vec<String> = ego
        .groups
        .iter()
        .map(|g| g.key.predicate.to_string())
        .collect();
    assert!(
        preds.contains(&"x.fake.member_of_instance".to_string()),
        "{preds:?}"
    );
    assert!(preds.contains(&"has_account".to_string()), "{preds:?}");
    // the declared platform's profile URL was derived
    assert!(preds.contains(&"links_to".to_string()), "{preds:?}");

    let events = drain(&mut t.events);
    assert!(events.iter().any(
        |e| matches!(&e.kind, EventKind::JobFinished { note: Some(n), .. } if n == "1/1 instances")
    ));
    // serve mode used one process for both jobs; per-job mode used two
    let lines = std::fs::read_to_string(marker.path()).unwrap();
    let pids = |m: &str| -> std::collections::BTreeSet<String> {
        lines
            .lines()
            .filter(|l| l.starts_with(&format!("{m} ")))
            .map(|l| l.split_whitespace().nth(1).unwrap().to_string())
            .collect()
    };
    assert_eq!(pids("fake-serve").len(), 1, "{lines}");
    assert_eq!(pids("fake").len(), 2, "{lines}");
    assert!(t.handle.store().verify().await.unwrap().ok());
}

#[tokio::test]
async fn module_errors_bad_lines_and_silent_exits_fail_the_job() {
    let marker = tempfile::NamedTempFile::new().unwrap();
    let t = exec_case(marker.path(), &["modules"]).await;
    for u in ["boom", "silent", "garbage"] {
        t.handle
            .act(EngineAction::Seed {
                entity: format!("username:{u}").parse().unwrap(),
            })
            .await
            .unwrap();
    }
    t.handle.wait_idle(WAIT).await.unwrap();
    let jobs = t.handle.store().read(|r| r.jobs(&[])).await.unwrap();
    let per_job = jobs_by_module(&jobs, "fake");
    let mut errors: Vec<String> = per_job
        .iter()
        .map(|j| format!("{:?}: {}", j.state, j.error.clone().unwrap_or_default()))
        .collect();
    errors.sort();
    assert_eq!(errors.len(), 3, "{errors:?}");
    assert_eq!(
        errors[0],
        "Failed: exited (exit status: 3) without sending done"
    );
    assert!(
        errors[1].starts_with("Failed: parse: bad protocol line ("),
        "{errors:?}"
    );
    assert!(errors[1].ends_with("): this is not json"), "{errors:?}");
    assert_eq!(errors[2], "Failed: upstream: upstream exploded");
}

#[tokio::test]
async fn cancellation_kills_the_process() {
    let marker = tempfile::NamedTempFile::new().unwrap();
    let t = exec_case(marker.path(), &["modules"]).await;
    let ActionOutcome::Seeded { entity, .. } = t
        .handle
        .act(EngineAction::Seed {
            entity: "username:slow".parse().unwrap(),
        })
        .await
        .unwrap()
    else {
        panic!()
    };
    // wait until the per-job process is running
    let started = Instant::now();
    loop {
        if std::fs::read_to_string(marker.path())
            .unwrap()
            .contains("fake pid=")
        {
            break;
        }
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "module never started"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let id = entity.id;
    let job = t
        .handle
        .store()
        .read(move |r| r.active_job("fake", id))
        .await
        .unwrap()
        .unwrap();
    let t0 = Instant::now();
    t.handle.act(EngineAction::Cancel { job }).await.unwrap();
    t.handle.wait_idle(WAIT).await.unwrap();
    assert!(
        t0.elapsed() < Duration::from_secs(5),
        "cancel took {:?}",
        t0.elapsed()
    );
    let row = t
        .handle
        .store()
        .read(move |r| r.job(job))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(row.state, JobState::Failed);
    assert_eq!(row.error.as_deref(), Some("cancelled"));
}

#[tokio::test]
async fn serve_mode_errors_and_process_exit() {
    let marker = tempfile::NamedTempFile::new().unwrap();
    let t = exec_case(marker.path(), &["serve"]).await;
    t.handle
        .act(EngineAction::Seed {
            entity: "username:boom".parse().unwrap(),
        })
        .await
        .unwrap();
    t.handle.wait_idle(WAIT).await.unwrap();
    t.handle
        .act(EngineAction::Seed {
            entity: "username:silent".parse().unwrap(),
        })
        .await
        .unwrap();
    t.handle.wait_idle(WAIT).await.unwrap();
    // the process is respawned for the next job
    t.handle
        .act(EngineAction::Seed {
            entity: "username:jmarlow".parse().unwrap(),
        })
        .await
        .unwrap();
    t.handle.wait_idle(WAIT).await.unwrap();
    let jobs = t.handle.store().read(|r| r.jobs(&[])).await.unwrap();
    let mut outcomes: Vec<String> = jobs
        .iter()
        .map(|j| format!("{:?} {}", j.state, j.error.clone().unwrap_or_default()))
        .collect();
    outcomes.sort();
    assert_eq!(
        outcomes,
        [
            "Done ",
            "Failed internal: module process exited",
            "Failed upstream: upstream exploded"
        ]
    );
    let lines = std::fs::read_to_string(marker.path()).unwrap();
    let pids: std::collections::BTreeSet<&str> = lines
        .lines()
        .map(|l| l.split_whitespace().nth(1).unwrap())
        .collect();
    assert_eq!(
        pids.len(),
        2,
        "one process before the exit, one after: {lines}"
    );
}
