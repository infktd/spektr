//! Built-in playbooks through the real engine against replay fixtures
//! (PLAYBOOKS §2, J7 steps 1–3). No network.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::PathBuf;
use std::time::Duration;

use spektr_engine::{
    ActionOutcome, CaseInit, Engine, EngineAction, EngineConfig, EngineHandle, ItemState,
    OpenOptions, Query, QueryResult, RunView,
};
use spektr_model::{Clock, IdGen};
use spektr_modules::{Fixtures, Net};

const WAIT: Duration = Duration::from_secs(60);

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures")
}

async fn engine(net: Net, subject: Option<&str>) -> (tempfile::TempDir, EngineHandle) {
    let dir = tempfile::tempdir().unwrap();
    let opts = OpenOptions::new(
        Clock::Fixed("2026-10-20T18:14:22Z".parse().unwrap()),
        IdGen::seeded(1),
    )
    .unwrap();
    let mut cfg = EngineConfig::new(opts);
    cfg.batch_window = Duration::from_millis(5);
    let mut init = CaseInit::named("pb");
    init.rules_profile = "tracelabs".into();
    init.subject = subject.map(str::to_string);
    init.last_seen = Some("2026-09-28".parse().unwrap());
    let e = Engine::create(&dir.path().join("case.spektr"), &init, cfg)
        .await
        .unwrap();
    for m in spektr_modules::builtins(net) {
        e.register(m).unwrap();
    }
    (dir, e.start())
}

async fn runs(h: &EngineHandle) -> Vec<RunView> {
    match h.query(Query::Runs).await.unwrap() {
        QueryResult::Runs(r) => r,
        other => panic!("{other:?}"),
    }
}

async fn seed(h: &EngineHandle, s: &str) -> spektr_model::EntityId {
    match h
        .act(EngineAction::Seed {
            entity: s.parse().unwrap(),
        })
        .await
        .unwrap()
    {
        ActionOutcome::Seeded { entity, .. } => entity.id,
        other => panic!("{other:?}"),
    }
}

#[tokio::test]
async fn person_from_username_runs_every_step() {
    let net = Net::replay(Fixtures::load_dir(&fixtures_dir()).unwrap());
    let (_dir, h) = engine(net.clone(), Some("Jordan Marlow")).await;
    let u = seed(&h, "username:jmarlow.wav").await;
    h.wait_idle(WAIT).await.unwrap();
    h.act(EngineAction::StartPlaybook {
        name: "person-from-username".into(),
        entity: u,
    })
    .await
    .unwrap();
    h.wait_idle(WAIT).await.unwrap();
    assert!(
        net.misses().is_empty(),
        "unmatched requests: {:?}",
        net.misses()
    );

    let r = runs(&h).await;
    assert_eq!(r.len(), 1);
    let run = &r[0];
    assert_eq!(run.state, "done", "{run:#?}");
    let states: Vec<&str> = run.steps.iter().map(|s| s.state.as_str()).collect();
    assert_eq!(states, ["done"; 6]);
    // Avatar hashes ran on the run's accounts; reverse image search gets one
    // item per avatar image; the review step one item opening Review.
    assert!(run.steps[2].selected >= 4, "{:?}", run.steps[2]);
    let items = match h
        .query(Query::Checklist(Some(ItemState::Open)))
        .await
        .unwrap()
    {
        QueryResult::Checklist(i) => i,
        other => panic!("{other:?}"),
    };
    let images: Vec<_> = items.iter().filter(|i| i.step == 5).collect();
    assert_eq!(images.len(), 4, "{items:#?}");
    assert!(images.iter().all(|i| {
        i.launchers
            .iter()
            .map(|l| l.id.as_str())
            .collect::<Vec<_>>()
            == ["lens", "yandex-images", "tineye"]
    }));
    assert!(images.iter().all(|i| {
        i.launchers.iter().all(|l| {
            l.url.as_deref().is_some_and(|u| {
                u.contains("avatars.example")
                    || u.contains("bsky.example")
                    || u.contains("gitlab.example")
                    || u.contains("mastodon.example")
            })
        })
    }));
    let review: Vec<_> = items.iter().filter(|i| i.step == 6).collect();
    assert_eq!(review.len(), 1);
    assert_eq!(review[0].screen.as_deref(), Some("review"));
    assert!(review[0].entity.is_none());

    // Opening a launcher returns its URL and is audited.
    let img = images[0];
    let out = h
        .act(EngineAction::OpenLauncher {
            launcher: "lens".into(),
            entity: img.entity.as_ref().unwrap().id,
            item: Some(img.id),
        })
        .await
        .unwrap();
    let ActionOutcome::Launch { url } = out else {
        panic!("{out:?}")
    };
    assert!(
        url.starts_with("https://lens.google.com/uploadbyurl?url=https%3A%2F%2F"),
        "{url}"
    );
    let audited = h
        .store()
        .read(|r| r.audit_by_action("launcher.open"))
        .await
        .unwrap();
    assert_eq!(audited.len(), 1);

    // Done with a note.
    h.act(EngineAction::ResolveItem {
        item: img.id,
        state: ItemState::Done,
        note: Some("no other copies".into()),
    })
    .await
    .unwrap();
    let done = match h
        .query(Query::Checklist(Some(ItemState::Done)))
        .await
        .unwrap()
    {
        QueryResult::Checklist(i) => i,
        other => panic!("{other:?}"),
    };
    assert_eq!(done.len(), 1);
    assert_eq!(done[0].note.as_deref(), Some("no other copies"));
}

#[tokio::test]
async fn tracelabs_kickoff_nests_the_person_playbooks() {
    let net = Net::replay(Fixtures::load_dir(&fixtures_dir()).unwrap());
    let (_dir, h) = engine(net.clone(), Some("Jordan Marlow")).await;
    seed(&h, "username:jmarlow.wav").await;
    seed(&h, "email:jordan.marlow@mail.example").await;
    h.wait_idle(WAIT).await.unwrap();
    let subject = h.settings().subject.unwrap();
    h.act(EngineAction::StartPlaybook {
        name: "tracelabs-kickoff".into(),
        entity: subject,
    })
    .await
    .unwrap();
    // A second top-level run is refused while this one is going.
    let again = h
        .act(EngineAction::StartPlaybook {
            name: "tracelabs-kickoff".into(),
            entity: subject,
        })
        .await;
    assert!(again.is_err());
    h.wait_idle(WAIT).await.unwrap();
    assert!(
        net.misses().is_empty(),
        "unmatched requests: {:?}",
        net.misses()
    );

    let r = runs(&h).await;
    let top: Vec<_> = r.iter().filter(|x| x.parent.is_none()).collect();
    assert_eq!(top.len(), 1);
    assert_eq!(top[0].state, "done", "{:#?}", top[0]);
    let mut children: Vec<&str> = r
        .iter()
        .filter(|x| x.parent == Some(top[0].id))
        .map(|x| x.playbook.as_str())
        .collect();
    children.sort();
    assert_eq!(children, ["person-from-email", "person-from-username"]);
    assert!(r.iter().all(|x| x.state == "done"));
    // The category checks are on the subject.
    let items = match h
        .query(Query::Checklist(Some(ItemState::Open)))
        .await
        .unwrap()
    {
        QueryResult::Checklist(i) => i,
        other => panic!("{other:?}"),
    };
    let kickoff: Vec<&str> = items
        .iter()
        .filter(|i| i.playbook == "tracelabs-kickoff")
        .map(|i| i.step_title.as_str())
        .collect();
    assert_eq!(
        kickoff,
        [
            "Employment",
            "Family and friends",
            "Day last seen",
            "Advancing the timeline"
        ]
    );
}

#[tokio::test]
async fn stop_cancels_the_rest() {
    let net = Net::replay(Fixtures::load_dir(&fixtures_dir()).unwrap());
    let (_dir, h) = engine(net, None).await;
    let u = seed(&h, "username:jmarlow.wav").await;
    h.act(EngineAction::StartPlaybook {
        name: "person-from-username".into(),
        entity: u,
    })
    .await
    .unwrap();
    h.act(EngineAction::StopPlaybook).await.unwrap();
    h.wait_idle(WAIT).await.unwrap();
    let r = runs(&h).await;
    assert_eq!(r[0].state, "stopped", "{:#?}", r[0]);
    assert!(r[0].steps.iter().any(|s| s.state == "stopped"));
    assert!(
        h.act(EngineAction::StopPlaybook).await.is_err(),
        "nothing left to stop"
    );
}

async fn engine_profile(net: Net, profile: &str) -> (tempfile::TempDir, EngineHandle) {
    let dir = tempfile::tempdir().unwrap();
    let opts = OpenOptions::new(
        Clock::Fixed("2026-10-20T18:14:22Z".parse().unwrap()),
        IdGen::seeded(1),
    )
    .unwrap();
    let mut cfg = EngineConfig::new(opts);
    cfg.batch_window = Duration::from_millis(5);
    let mut init = CaseInit::named("pb");
    init.rules_profile = profile.into();
    let e = Engine::create(&dir.path().join("case.spektr"), &init, cfg)
        .await
        .unwrap();
    for m in spektr_modules::builtins(net) {
        e.register(m).unwrap();
    }
    (dir, e.start())
}

#[tokio::test]
async fn domain_recon_finds_the_infrastructure_and_correlates_it() {
    let net = Net::replay(Fixtures::load_dir(&fixtures_dir()).unwrap());
    let (_dir, h) = engine_profile(net.clone(), "default").await;
    let d = seed(&h, "domain:kestrel.example").await;
    h.wait_idle(WAIT).await.unwrap();
    h.act(EngineAction::StartPlaybook {
        name: "domain-recon".into(),
        entity: d,
    })
    .await
    .unwrap();
    h.wait_idle(WAIT).await.unwrap();
    assert!(
        net.misses().is_empty(),
        "unmatched requests: {:?}",
        net.misses()
    );
    let r = runs(&h).await;
    assert_eq!(r[0].state, "done", "{:#?}", r[0]);
    let artifacts: std::collections::BTreeSet<String> = h
        .store()
        .read(|r| {
            Ok(r.all_edges()?
                .into_iter()
                .filter_map(|e| e.discriminator.split_once(':').map(|(a, _)| a.to_string()))
                .collect())
        })
        .await
        .unwrap();
    for a in ["cert_san", "registrant_email", "analytics_id", "ip"] {
        assert!(artifacts.contains(a), "no {a} correlation: {artifacts:?}");
    }
}

#[tokio::test]
async fn image_triage_and_person_from_phone_run() {
    let net = Net::replay(Fixtures::load_dir(&fixtures_dir()).unwrap());
    let (_dir, h) = engine_profile(net.clone(), "tracelabs").await;
    let photo = std::fs::read(fixtures_dir().join("tutor/IMG_2214.jpg")).unwrap();
    let out = h
        .act(EngineAction::Attach {
            entity: None,
            name: "IMG_2214.jpg".into(),
            bytes: photo,
            note: None,
        })
        .await
        .unwrap();
    let ActionOutcome::Attached {
        image: Some(img), ..
    } = out
    else {
        panic!("{out:?}")
    };
    h.wait_idle(WAIT).await.unwrap();
    h.act(EngineAction::StartPlaybook {
        name: "image-triage".into(),
        entity: img.id,
    })
    .await
    .unwrap();
    h.wait_idle(WAIT).await.unwrap();
    let p = seed(&h, "phone:+1 503 555 0142").await;
    h.wait_idle(WAIT).await.unwrap();
    h.act(EngineAction::StartPlaybook {
        name: "person-from-phone".into(),
        entity: p,
    })
    .await
    .unwrap();
    h.wait_idle(WAIT).await.unwrap();
    assert!(
        net.misses().is_empty(),
        "unmatched requests: {:?}",
        net.misses()
    );
    let r = runs(&h).await;
    assert_eq!(
        r.iter().map(|x| x.state.as_str()).collect::<Vec<_>>(),
        ["done", "done"]
    );
    // EXIF: device, the time taken (after last-seen in the tutor), a location.
    let e = h
        .store()
        .read(move |r| r.entity(img.id))
        .await
        .unwrap()
        .unwrap();
    assert!(e.attrs.contains_key("taken_at") && e.attrs.contains_key("device"));
    // A local-only image: the reverse-image launchers are disabled, and the
    // item points at the file to reveal instead.
    let items = match h
        .query(Query::Checklist(Some(ItemState::Open)))
        .await
        .unwrap()
    {
        QueryResult::Checklist(i) => i,
        other => panic!("{other:?}"),
    };
    let rev = items
        .iter()
        .find(|i| i.step_title == "Reverse image search")
        .unwrap();
    assert!(rev.launchers.iter().all(|l| l.disabled.is_some()));
    assert!(rev.local_image.is_some());
}

#[tokio::test]
async fn every_builtin_playbook_checks_clean() {
    let net = Net::replay(Fixtures::default());
    let (_dir, h) = engine_profile(net, "tracelabs").await;
    let infos = match h.query(Query::Playbooks).await.unwrap() {
        QueryResult::Playbooks(p) => p,
        other => panic!("{other:?}"),
    };
    assert_eq!(infos.len(), 6);
    for p in infos {
        assert!(
            p.errors.is_empty() && p.warnings.is_empty(),
            "{}: {:?} {:?}",
            p.name,
            p.errors,
            p.warnings
        );
        assert_eq!(p.source, "built-in");
    }
}
