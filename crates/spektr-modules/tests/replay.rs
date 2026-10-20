//! Built-in modules through the real engine against replay fixtures
//! (MODULES §10). No network.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::PathBuf;
use std::time::Duration;

use spektr_engine::{
    ActionOutcome, CaseInit, Engine, EngineAction, EngineConfig, EngineHandle, EventKind,
    OpenOptions,
};
use spektr_model::{Clock, EntityType, IdGen};
use spektr_modules::{Fixtures, Net};
use spektr_store::{EgoParams, JobState};

const WAIT: Duration = Duration::from_secs(30);

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures")
}

/// An engine with only `only` registered (all built-ins when empty), so a
/// test exercises one module without the others' auto-expansion.
async fn engine(net: Net, profile: &str, only: &[&str]) -> (tempfile::TempDir, EngineHandle) {
    let dir = tempfile::tempdir().unwrap();
    let opts = OpenOptions::new(
        Clock::Fixed("2026-10-20T18:14:22Z".parse().unwrap()),
        IdGen::seeded(1),
    )
    .unwrap();
    let mut cfg = EngineConfig::new(opts);
    cfg.batch_window = Duration::from_millis(5);
    let mut init = CaseInit::named("replay");
    init.rules_profile = profile.into();
    let e = Engine::create(&dir.path().join("case.spektr"), &init, cfg)
        .await
        .unwrap();
    for m in spektr_modules::builtins(net) {
        if only.is_empty() || only.contains(&m.manifest().name.as_str()) {
            e.register(m).unwrap();
        }
    }
    (dir, e.start())
}

fn ty(s: &str) -> EntityType {
    s.parse().unwrap()
}

#[tokio::test]
async fn username_sweep_finds_accounts_and_queues_unsure_hits() {
    let net = Net::replay(Fixtures::load_dir(&fixtures_dir()).unwrap());
    let (_dir, h) = engine(net.clone(), "tracelabs", &["username-sweep"]).await;
    let mut events = h.subscribe();
    h.act(EngineAction::Seed {
        entity: "username:jmarlow.wav".parse().unwrap(),
    })
    .await
    .unwrap();
    h.wait_idle(WAIT).await.unwrap();
    assert!(
        net.misses().is_empty(),
        "unmatched requests: {:?}",
        net.misses()
    );

    let (accounts, reviews, jobs) = h
        .store()
        .read(|r| {
            let u = r.find(&ty("username"), "jmarlow.wav")?.unwrap();
            let ego = r.ego(
                u,
                &EgoParams {
                    page_size: 50,
                    ..EgoParams::default()
                },
            )?;
            let mut accounts: Vec<String> = ego
                .groups
                .iter()
                .filter(|g| g.key.predicate.as_str() == "has_account")
                .flat_map(|g| {
                    g.rows
                        .iter()
                        .map(|row| format!("{} {:?}", row.entity.key, row.strength))
                })
                .collect();
            accounts.sort();
            Ok((accounts, r.review_items(None)?, r.jobs(&[])?))
        })
        .await
        .unwrap();
    assert_eq!(
        accounts,
        [
            "aboutme:jmarlow.wav Some(Single)",
            "bluesky:jmarlow.wav.bsky.social Some(Single)",
            "gitlab:jmarlow.wav Some(Single)",
            "instagram:jmarlow.wav Some(Single)",
            "mastodon:jmarlow.wav@mastodon.social Some(Single)",
            "medium:jmarlow.wav Some(Single)",
            "tiktok:jmarlow.wav Some(Single)",
        ]
    );
    let mut reasons: Vec<String> = reviews
        .iter()
        .map(|r| {
            format!(
                "{} {}",
                r.payload["detail"]["site"].as_str().unwrap(),
                r.payload["detail"]["reason"].as_str().unwrap()
            )
        })
        .collect();
    reasons.sort();
    assert_eq!(
        reasons,
        [
            "facebook login wall",
            "okru blocked by the site (HTTP 403)",
            "snapchat login wall",
            "threads login wall",
            "vk blocked by the site (HTTP 403)",
        ]
    );
    let sweep: Vec<_> = jobs
        .iter()
        .filter(|j| j.module == "username-sweep")
        .collect();
    assert_eq!(sweep.len(), 1);
    assert_eq!(sweep[0].state, JobState::Done);
    let note = std::iter::from_fn(|| events.try_recv().ok())
        .find_map(|e| match e.kind {
            EventKind::JobFinished { module, note, .. } if module == "username-sweep" => note,
            _ => None,
        })
        .unwrap();
    let total = spektr_modules::builtins::sweep::embedded_sites().len();
    assert_eq!(
        note,
        format!("{total} sites · 7 found · 5 unsure · {} none", total - 12)
    );
}

#[tokio::test]
async fn github_profile_from_an_account() {
    let net = Net::replay(Fixtures::load_dir(&fixtures_dir()).unwrap());
    let (_dir, h) = engine(net.clone(), "default", &["github"]).await;
    h.act(EngineAction::Seed {
        entity: "username:JMarlow-WAV".parse().unwrap(),
    })
    .await
    .unwrap();
    h.wait_idle(WAIT).await.unwrap();
    assert!(
        net.misses().is_empty(),
        "unmatched requests: {:?}",
        net.misses()
    );
    let (acct, ego) = h
        .store()
        .read(|r| {
            let a = r.find(&ty("account"), "github:jmarlow-wav")?.unwrap();
            Ok((r.entity(a)?.unwrap(), r.ego(a, &EgoParams::default())?))
        })
        .await
        .unwrap();
    let text = |k: &str| acct.attrs[k][0].value.display();
    assert_eq!(text("display_name"), "Jordan M.");
    assert_eq!(text("bio"), "synths, trails, coffee");
    assert_eq!(text("created_at"), "2019-03-02 17:40");
    assert_eq!(text("last_active"), "2026-10-16 09:12");
    assert!(acct.attrs["last_active"][0].event);
    let mut linked: Vec<String> = ego
        .groups
        .iter()
        .flat_map(|g| {
            g.rows
                .iter()
                .map(move |r| format!("{} {}", g.key.predicate, r.entity.key))
        })
        .collect();
    linked.sort();
    assert!(
        linked.contains(&"has_email jordan.marlow@mail.example".to_string()),
        "{linked:?}"
    );
    assert!(
        linked.contains(&"links_to https://jmarlow.example/".to_string()),
        "{linked:?}"
    );
    assert!(
        linked.contains(&"located_in place:portland, or".to_string()),
        "{linked:?}"
    );
    assert!(
        linked.contains(&"has_account jmarlow-wav".to_string()),
        "{linked:?}"
    );
    // github ran on the username (depth 0) and again on the account (depth 1); both done
    let jobs = h.store().read(|r| r.jobs(&[])).await.unwrap();
    let gh: Vec<_> = jobs
        .iter()
        .filter(|j| j.module == "github")
        .map(|j| j.state)
        .collect();
    assert_eq!(gh, [JobState::Done, JobState::Done]);
}

#[tokio::test]
async fn dns_records_and_ip_correlation() {
    let net = Net::replay(Fixtures::load_dir(&fixtures_dir()).unwrap());
    let (_dir, h) = engine(net.clone(), "default", &["dns"]).await;
    for d in [
        "domain:kestrel.example",
        "domain:vpn.kestrel.example",
        "domain:gone.example",
    ] {
        h.act(EngineAction::Seed {
            entity: d.parse().unwrap(),
        })
        .await
        .unwrap();
    }
    h.wait_idle(WAIT).await.unwrap();
    assert!(
        net.misses().is_empty(),
        "unmatched requests: {:?}",
        net.misses()
    );
    let ego = h
        .store()
        .read(|r| {
            let d = r.find(&ty("domain"), "kestrel.example")?.unwrap();
            r.ego(d, &EgoParams::default())
        })
        .await
        .unwrap();
    let mut rows: Vec<String> = ego
        .groups
        .iter()
        .flat_map(|g| {
            g.rows
                .iter()
                .map(move |r| format!("{} {}", g.key.predicate, r.entity.key))
        })
        .collect();
    rows.sort();
    assert_eq!(
        rows,
        [
            "mail_handled_by mx1.mailhost.example",
            "resolves_to 2001:db8::24",
            "resolves_to 203.0.113.24",
            "resolves_to 203.0.113.25",
            "shares_artifact vpn.kestrel.example",
            "subdomain_of vpn.kestrel.example",
        ]
    );
    let jobs = h.store().read(|r| r.jobs(&[])).await.unwrap();
    assert!(jobs.iter().all(|j| j.state == JobState::Done), "{jobs:?}");
}

/// The negative test (Phase 2 exit criterion): with replay, a request that
/// has no fixture fails the job and is reported as a miss.
#[tokio::test]
async fn unmatched_request_fails() {
    let mut fx = Fixtures::default();
    fx.add_jsonl("inline", r#"{"kind":"http","method":"GET","url":"https://api.github.com/users/someone-else","status":404}"#).unwrap();
    let net = Net::replay(fx);
    let (_dir, h) = engine(net.clone(), "default", &["github"]).await;
    let ActionOutcome::Seeded { .. } = h
        .act(EngineAction::Seed {
            entity: "username:nofixture".parse().unwrap(),
        })
        .await
        .unwrap()
    else {
        panic!()
    };
    h.wait_idle(WAIT).await.unwrap();
    let jobs = h.store().read(|r| r.jobs(&[])).await.unwrap();
    let gh = jobs.iter().find(|j| j.module == "github").unwrap();
    assert_eq!(gh.state, JobState::Failed);
    assert_eq!(
        gh.error.as_deref(),
        Some("no fixture for GET https://api.github.com/users/nofixture")
    );
    assert!(
        net.misses()
            .contains(&"GET https://api.github.com/users/nofixture".to_string()),
        "{:?}",
        net.misses()
    );
    // a fixture miss is not retried
    assert_eq!(jobs.iter().filter(|j| j.module == "github").count(), 1);
}

/// The tutor storyline (J2 steps 2–4 and 6), with every built-in
/// registered: the sweep and the profile modules find the accounts, the
/// avatars are fetched and hashed, and three of them correlate.
#[tokio::test]
async fn tutor_handle_expands_and_avatars_correlate() {
    let net = Net::replay(Fixtures::load_dir(&fixtures_dir()).unwrap());
    let (_dir, h) = engine(net.clone(), "tracelabs", &[]).await;
    h.act(EngineAction::Seed {
        entity: "username:jmarlow.wav".parse().unwrap(),
    })
    .await
    .unwrap();
    h.wait_idle(WAIT).await.unwrap();
    assert!(
        net.misses().is_empty(),
        "unmatched requests: {:?}",
        net.misses()
    );

    let (gh, avatar_edges, jobs, site) = h
        .store()
        .read(|r| {
            let gh = r
                .entity_by_key(&ty("account"), "github:jmarlow-wav")?
                .unwrap();
            let edges: Vec<(String, String)> = r
                .all_edges()?
                .into_iter()
                .filter(|e| e.discriminator.starts_with("avatar_phash:"))
                .map(|e| {
                    let k = |id| {
                        r.entity(id)
                            .ok()
                            .flatten()
                            .map(|x| x.key)
                            .unwrap_or_default()
                    };
                    let (a, b) = (k(e.src), k(e.dst));
                    if a < b { (a, b) } else { (b, a) }
                })
                .collect();
            let site = r
                .entity_by_key(&ty("url"), "https://jmarlow.example/")?
                .unwrap();
            Ok((gh, edges, r.jobs(&[])?, site))
        })
        .await
        .unwrap();

    // GitHub: the variant handle, with last activity after the last-seen date.
    let last = &gh.attrs["last_active"][0].value;
    assert_eq!(
        *last,
        spektr_model::AttrValue::Time("2026-10-16T09:12:00Z".parse().unwrap())
    );
    assert!(gh.attrs.contains_key("matched_variant"));

    // Three avatars are the same picture; the Mastodon one isn't.
    let mut pairs = avatar_edges.clone();
    pairs.sort();
    assert_eq!(
        pairs,
        [
            (
                "bluesky:jmarlow.wav.bsky.social".to_string(),
                "github:jmarlow-wav".to_string()
            ),
            (
                "bluesky:jmarlow.wav.bsky.social".to_string(),
                "gitlab:jmarlow.wav".to_string()
            ),
            (
                "github:jmarlow-wav".to_string(),
                "gitlab:jmarlow.wav".to_string()
            ),
        ]
    );

    // The site was fingerprinted: title and the analytics id.
    assert!(site.attrs.contains_key("title"));
    assert_eq!(
        site.attrs["analytics_ids"][0].value,
        spektr_model::AttrValue::Text("G-JM4RL0W2K6".into())
    );

    // Nothing failed; wayback (auto = false) and hibp (keyed, no secret) never ran.
    let failed: Vec<_> = jobs
        .iter()
        .filter(|j| j.state == JobState::Failed)
        .map(|j| (&j.module, &j.error))
        .collect();
    assert!(failed.is_empty(), "failed jobs: {failed:?}");
    assert!(
        !jobs
            .iter()
            .any(|j| j.module == "wayback" || j.module == "hibp")
    );
    for m in [
        "username-sweep",
        "github",
        "bluesky",
        "gitlab",
        "mastodon",
        "avatar-hash",
        "http-fingerprint",
        "email-check",
        "gravatar",
        "dns",
        "crtsh",
        "rdap",
        "exif",
    ] {
        assert!(
            jobs.iter()
                .any(|j| j.module == m && j.state == JobState::Done),
            "{m} never ran to done"
        );
    }
}
