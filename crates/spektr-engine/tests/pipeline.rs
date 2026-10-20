//! The engine end to end with scripted native modules: automatic expansion,
//! depth, ingestion validation, cache, retries, failures, cancellation,
//! correlation, hubs, scope, budgets, review items.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod support;

use std::sync::atomic::Ordering;
use std::time::Duration;

use spektr_engine::{ActionOutcome, EngineAction, PlanResult, Query, QueryResult};
use spektr_model::{EntityFlags, EntityRef, EntityType};
use spektr_protocol::{AttrValue, ErrorCode, ModuleMessage};
use spektr_store::{EgoParams, JobState};
use support::*;

const WAIT: Duration = Duration::from_secs(10);

fn sweep_module() -> Scripted {
    Scripted::new(
        manifest(
            r#"name = "sweep"
technique = "passive"
consumes = ["username"]
produces = [["has_account", "account"]]
"#,
        ),
        |input, _| {
            let h = input.entity.key.clone();
            Ok(vec![
                obs("o1", &format!("https://github.com/{h}")),
                entity("e1", "account", &format!("github:{h}"), &["o1"]),
                edge("$input", "has_account", "e1", &["o1"]),
            ])
        },
    )
}

fn profile_module() -> Scripted {
    Scripted::new(
        manifest(
            r#"name = "profile"
technique = "passive"
consumes = ["account:github"]
produces = [["links_to", "url"], ["has_email", "email"]]
"#,
        ),
        |input, _| {
            let h = input.entity.key.split_once(':').unwrap().1.to_string();
            Ok(vec![
                obs("o1", "https://api.github.com/users/x"),
                attr(
                    "$input",
                    "display_name",
                    AttrValue::Text("Jordan M.".into()),
                    false,
                    &["o1"],
                ),
                attr(
                    "$input",
                    "last_active",
                    AttrValue::Time("2026-10-16T09:12:00Z".into()),
                    true,
                    &["o1"],
                ),
                entity("u", "url", &format!("https://{h}.example/"), &["o1"]),
                edge("$input", "links_to", "u", &["o1"]),
                entity("m", "email", &format!("{h}@mail.example"), &["o1"]),
                edge("$input", "has_email", "m", &["o1"]),
            ])
        },
    )
}

fn site_module() -> Scripted {
    Scripted::new(
        manifest(
            r#"name = "site"
technique = "passive"
consumes = ["url"]
produces = [["has_email", "email"]]
"#,
        ),
        |_, _| Ok(vec![obs("o1", "https://site.example/")]),
    )
}

#[tokio::test]
async fn seed_expands_automatically_to_the_depth_limit() {
    let mut t = case(|e| {
        e.register(Box::new(sweep_module())).unwrap();
        e.register(Box::new(profile_module())).unwrap();
        e.register(Box::new(site_module())).unwrap();
    })
    .await;
    let out = t
        .handle
        .act(EngineAction::Seed {
            entity: "username:JMarlow-wav".parse().unwrap(),
        })
        .await
        .unwrap();
    let ActionOutcome::Seeded {
        entity,
        created,
        jobs,
    } = out
    else {
        panic!("{out:?}")
    };
    assert!(created);
    assert_eq!(entity.key, "jmarlow-wav");
    assert!(entity.flags.contains(EntityFlags::SEED));
    assert_eq!(jobs.len(), 1, "{jobs:?}");
    t.handle.wait_idle(WAIT).await.unwrap();

    let r = t.handle.store().read(|r| {
        let ty = |s: &str| s.parse::<EntityType>().unwrap();
        Ok((
            r.find(&ty("account"), "github:jmarlow-wav")?
                .and_then(|id| r.entity(id).ok().flatten()),
            r.find(&ty("url"), "https://jmarlow-wav.example/")?
                .and_then(|id| r.entity(id).ok().flatten()),
            r.jobs(&[])?,
        ))
    });
    let (account, url, jobs) = r.await.unwrap();
    let account = account.unwrap();
    assert_eq!(account.depth, 1);
    assert_eq!(
        account.attrs["display_name"][0].value,
        spektr_model::AttrValue::Text("Jordan M.".into())
    );
    let url = url.unwrap();
    assert_eq!(url.depth, 2, "found by a depth-1 job");
    // depth 0 sweep, depth 1 profile, depth 2 site; the url's derived domain
    // (depth 3) is past the limit and not expanded
    let mut ran: Vec<(String, JobState)> =
        jobs.iter().map(|j| (j.module.clone(), j.state)).collect();
    ran.sort();
    // `site` runs on both depth-2 urls: the profile's link and the account's
    // implied profile URL (https://github.com/jmarlow-wav)
    assert_eq!(
        ran,
        vec![
            ("profile".into(), JobState::Done),
            ("site".into(), JobState::Done),
            ("site".into(), JobState::Done),
            ("sweep".into(), JobState::Done)
        ]
    );

    let ev = names(&drain(&mut t.events));
    for e in [
        "job_queued",
        "job_started",
        "job_finished",
        "entity_added",
        "edge_added",
    ] {
        assert!(ev.iter().any(|n| n == e), "missing {e} in {ev:?}");
    }
    let report = t.handle.store().verify().await.unwrap();
    assert!(report.ok(), "{:?}", report.failure);
}

#[tokio::test]
async fn depth_override_stops_expansion_earlier() {
    let t = case_with(
        spektr_store::CaseInit::named("d"),
        |c| c.depth_override = Some(0),
        |e| {
            e.register(Box::new(sweep_module())).unwrap();
            e.register(Box::new(profile_module())).unwrap();
        },
    )
    .await;
    t.handle
        .act(EngineAction::Seed {
            entity: "username:jmarlow".parse().unwrap(),
        })
        .await
        .unwrap();
    t.handle.wait_idle(WAIT).await.unwrap();
    let jobs = t.handle.store().read(|r| r.jobs(&[])).await.unwrap();
    assert_eq!(
        jobs.iter().map(|j| j.module.as_str()).collect::<Vec<_>>(),
        ["sweep"]
    );
}

#[tokio::test]
async fn invalid_messages_are_rejected_and_the_job_continues() {
    let bad = Scripted::new(
        manifest(
            r#"name = "sloppy"
technique = "passive"
consumes = ["username"]
produces = [["has_account", "account"]]
"#,
        ),
        |_, _| {
            Ok(vec![
                obs("o1", "https://example.example/"),
                entity("a", "account", "github:no-evidence", &[]), // no evidence
                entity("b", "account", "github:bad-evidence", &["o9"]), // unknown evidence
                entity("c", "email", "x@mail.example", &["o1"]),   // type not declared
                entity("d", "account", "github:bad handle", &["o1"]), // fails normalization
                entity("e", "account", "github:fine", &["o1"]),
                edge("$input", "knows", "e", &["o1"]), // not a predicate
                edge("$input", "links_to", "e", &["o1"]), // undeclared + wrong types
                edge("$input", "has_account", "zz", &["o1"]), // unknown ref
                edge("$input", "has_account", "e", &[]), // no evidence
                attr("e", "Bad-Name", AttrValue::Int(1), false, &["o1"]), // bad attr name
                attr(
                    "e",
                    "created_at",
                    AttrValue::Time("yesterday".into()),
                    true,
                    &["o1"],
                ), // bad time
                obs("o1", "https://dup.example/"),     // duplicate id
                edge("$input", "has_account", "e", &["o1"]), // finally valid
            ])
        },
    );
    let mut t = case(|e| e.register(Box::new(bad)).unwrap()).await;
    t.handle
        .act(EngineAction::Seed {
            entity: "username:jmarlow".parse().unwrap(),
        })
        .await
        .unwrap();
    t.handle.wait_idle(WAIT).await.unwrap();
    let events = drain(&mut t.events);
    let rejected: Vec<String> = events
        .iter()
        .filter_map(|e| match &e.kind {
            spektr_engine::EventKind::MessageRejected { reason, .. } => Some(reason.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(rejected.len(), 11, "{rejected:#?}");
    for needle in [
        "must cite at least one observation",
        "cites unknown observation `o9`",
        "doesn't declare that it produces `email`",
        "invalid account",
        "unknown predicate `knows`",
        "doesn't declare that it produces `links_to`",
        "unknown entity ref `zz`",
        "attr name `Bad-Name`",
        "bad time `yesterday`",
        "used twice",
    ] {
        assert!(
            rejected.iter().any(|r| r.contains(needle)),
            "no rejection mentioning {needle:?} in {rejected:#?}"
        );
    }
    let (jobs, edges) = t
        .handle
        .store()
        .read(|r| Ok((r.jobs(&[])?, r.edge_count()?)))
        .await
        .unwrap();
    assert_eq!(
        jobs[0].state,
        JobState::Done,
        "rejections don't fail the job"
    );
    // has_account + the account's derived links_to/on_host
    assert_eq!(edges, 3);
}

#[tokio::test]
async fn cache_rerun_and_joining() {
    let m = sweep_module();
    let runs = m.runs.clone();
    let t = case(|e| e.register(Box::new(m)).unwrap()).await;
    let seed = t
        .handle
        .act(EngineAction::Seed {
            entity: "username:jmarlow".parse().unwrap(),
        })
        .await
        .unwrap();
    let ActionOutcome::Seeded { entity, .. } = seed else {
        panic!()
    };
    t.handle.wait_idle(WAIT).await.unwrap();
    assert_eq!(runs.load(Ordering::SeqCst), 1);

    let again = t
        .handle
        .act(EngineAction::Run {
            module: "sweep".into(),
            entity: entity.id,
            rerun: false,
        })
        .await
        .unwrap();
    let ActionOutcome::Planned { jobs } = again else {
        panic!()
    };
    assert!(
        matches!(jobs[0].result, PlanResult::Cached { .. }),
        "{jobs:?}"
    );
    let rerun = t
        .handle
        .act(EngineAction::Run {
            module: "sweep".into(),
            entity: entity.id,
            rerun: true,
        })
        .await
        .unwrap();
    let ActionOutcome::Planned { jobs } = rerun else {
        panic!()
    };
    assert!(
        matches!(jobs[0].result, PlanResult::Queued { .. }),
        "{jobs:?}"
    );
    t.handle.wait_idle(WAIT).await.unwrap();
    assert_eq!(runs.load(Ordering::SeqCst), 2);
    let err = t
        .handle
        .act(EngineAction::Run {
            module: "nope".into(),
            entity: entity.id,
            rerun: false,
        })
        .await;
    assert!(err.is_err());
}

#[tokio::test]
async fn retries_then_success_and_hard_failures() {
    let flaky = Scripted::new(
        manifest("name = \"flaky\"\ntechnique = \"passive\"\nconsumes = [\"username\"]\n"),
        |input, n| {
            if input.entity.key == "gone" {
                return Err(err(ErrorCode::NotFound, "no such user"));
            }
            if n < 2 {
                Err(spektr_engine::ModuleError::Code {
                    code: ErrorCode::RateLimited,
                    message: "429 from host".into(),
                    retryable: true,
                    retry_after: Some(Duration::from_millis(10)),
                })
            } else {
                Ok(vec![])
            }
        },
    );
    let mut t = case(|e| e.register(Box::new(flaky)).unwrap()).await;
    t.handle
        .act(EngineAction::Seed {
            entity: "username:jmarlow".parse().unwrap(),
        })
        .await
        .unwrap();
    t.handle.wait_idle(WAIT).await.unwrap();
    let events = drain(&mut t.events);
    let retries = events
        .iter()
        .filter(|e| {
            matches!(
                &e.kind,
                spektr_engine::EventKind::JobFinished {
                    retry_in_secs: Some(_),
                    ..
                }
            )
        })
        .count();
    assert_eq!(retries, 2);
    t.handle
        .act(EngineAction::Seed {
            entity: "username:gone".parse().unwrap(),
        })
        .await
        .unwrap();
    t.handle.wait_idle(WAIT).await.unwrap();
    let jobs = t.handle.store().read(|r| r.jobs(&[])).await.unwrap();
    assert_eq!(jobs[0].state, JobState::Done);
    assert_eq!(jobs[1].state, JobState::Failed);
    assert_eq!(jobs[1].error.as_deref(), Some("not_found: no such user"));
}

#[tokio::test]
async fn timeouts_and_cancellation() {
    let mut slow = Scripted::new(
        manifest(
            "name = \"slow\"\ntechnique = \"passive\"\nconsumes = [\"username\"]\ntimeout = \"1s\"\n",
        ),
        |_, _| Ok(vec![]),
    );
    slow.delay = Duration::from_secs(30);
    let t = case(|e| e.register(Box::new(slow)).unwrap()).await;
    t.handle
        .act(EngineAction::Seed {
            entity: "username:a".parse().unwrap(),
        })
        .await
        .unwrap();
    t.handle.wait_idle(WAIT).await.unwrap();
    let ActionOutcome::Seeded { entity, .. } = t
        .handle
        .act(EngineAction::Seed {
            entity: "username:b".parse().unwrap(),
        })
        .await
        .unwrap()
    else {
        panic!()
    };
    let job = t
        .handle
        .store()
        .read(move |r| r.active_job("slow", entity.id))
        .await
        .unwrap()
        .unwrap();
    t.handle.act(EngineAction::Cancel { job }).await.unwrap();
    t.handle.wait_idle(WAIT).await.unwrap();
    let jobs = t.handle.store().read(|r| r.jobs(&[])).await.unwrap();
    assert!(
        jobs[0]
            .error
            .as_deref()
            .unwrap()
            .contains("timed out after 1s"),
        "{:?}",
        jobs[0].error
    );
    assert_eq!(jobs[1].state, JobState::Failed);
    assert_eq!(jobs[1].error.as_deref(), Some("cancelled"));
}

#[tokio::test]
async fn ip_correlation_skips_hubs() {
    let dns = Scripted::new(
        manifest(
            r#"name = "fakedns"
technique = "passive"
consumes = ["domain", "subdomain"]
produces = [["resolves_to", "ip"]]
"#,
        ),
        |input, _| {
            let ip = if input.entity.key.contains("cdn") {
                "104.16.1.1"
            } else {
                "203.0.113.24"
            };
            Ok(vec![
                obs("o1", "https://dns.example/"),
                entity("ip", "ip", ip, &["o1"]),
                edge("$input", "resolves_to", "ip", &["o1"]),
            ])
        },
    );
    let t = case(|e| e.register(Box::new(dns)).unwrap()).await;
    for d in [
        "kestrel.example",
        "vpn.kestrel.example",
        "kestrel-cdn.example",
        "cdn-edge.example",
    ] {
        t.handle
            .act(EngineAction::Seed {
                entity: EntityRef {
                    ty: "domain".parse().unwrap(),
                    value: d.into(),
                },
            })
            .await
            .unwrap();
    }
    t.handle.wait_idle(WAIT).await.unwrap();
    let ty = |s: &str| s.parse::<EntityType>().unwrap();
    let view = t
        .handle
        .store()
        .read(move |r| {
            let k = r.find(&ty("domain"), "kestrel.example")?.unwrap();
            let cdn = r.find(&ty("domain"), "kestrel-cdn.example")?.unwrap();
            Ok((
                r.ego(k, &EgoParams::default())?,
                r.ego(cdn, &EgoParams::default())?,
            ))
        })
        .await
        .unwrap();
    let correlated: Vec<_> = view
        .0
        .groups
        .iter()
        .filter(|g| g.key.is_correlated())
        .collect();
    assert_eq!(correlated.len(), 1);
    let g = correlated[0];
    assert_eq!(g.key.artifact, Some(spektr_model::ArtifactKind::Ip));
    // kestrel.example ≈ vpn.kestrel.example (203.0.113.24); its subdomain_of
    // edge is a separate group
    assert_eq!(
        g.rows
            .iter()
            .map(|r| r.entity.key.clone())
            .collect::<Vec<_>>(),
        ["vpn.kestrel.example"]
    );
    assert_eq!(g.rows[0].artifact_value.as_deref(), Some("203.0.113.24"));
    assert_eq!(g.rows[0].strength, Some(spektr_model::Strength::Single));
    // 104.16.1.1 is a Cloudflare hub: no correlation
    assert!(view.1.groups.iter().all(|g| !g.key.is_correlated()));
}

#[tokio::test]
async fn hubs_scope_budgets_and_review() {
    let m = Scripted::new(
        manifest(
            r#"name = "keyed"
technique = "passive"
consumes = ["domain", "username"]
produces = []
cost = { class = "keyed", daily_budget = 1 }
"#,
        ),
        |input, _| {
            let mut v = vec![obs("o1", "https://keyed.example/")];
            if input.entity.key == "unsure" {
                v.push(ModuleMessage::Review {
                    job: None,
                    kind: "sweep_unsure".into(),
                    payload: serde_json::json!({"site": "forum.example", "reason": "login wall"}),
                    evidence: vec!["o1".into()],
                });
            }
            Ok(v)
        },
    );
    let mut t = case(|e| e.register(Box::new(m)).unwrap()).await;
    // a known hub isn't expanded, and says so once
    t.handle
        .act(EngineAction::Seed {
            entity: "domain:gmail.com".parse().unwrap(),
        })
        .await
        .unwrap();
    // scope: denied domains aren't expanded
    t.handle
        .act(EngineAction::AddScope(
            "deny domain:cdn-edge.example".into(),
        ))
        .await
        .unwrap();
    let ActionOutcome::Seeded { entity, .. } = t
        .handle
        .act(EngineAction::Seed {
            entity: "domain:cdn-edge.example".parse().unwrap(),
        })
        .await
        .unwrap()
    else {
        panic!()
    };
    assert!(entity.flags.contains(EntityFlags::OUT_OF_SCOPE));
    t.handle.wait_idle(WAIT).await.unwrap();
    let events = drain(&mut t.events);
    assert!(events.iter().any(|e| matches!(&e.kind, spektr_engine::EventKind::HubSuppressed { entity, .. } if entity.key == "gmail.com")));
    // budget: one run a day
    t.handle
        .act(EngineAction::Seed {
            entity: "username:unsure".parse().unwrap(),
        })
        .await
        .unwrap();
    t.handle.wait_idle(WAIT).await.unwrap();
    let ActionOutcome::Seeded { jobs, .. } = t
        .handle
        .act(EngineAction::Seed {
            entity: "username:second".parse().unwrap(),
        })
        .await
        .unwrap()
    else {
        panic!()
    };
    assert!(
        jobs.iter().all(
            |j| matches!(&j.result, PlanResult::Ineligible { reason } if reason.contains("budget"))
        ),
        "auto candidates over budget are ineligible, not blocked: {jobs:?}"
    );
    let ActionOutcome::Seeded { entity: second, .. } = t
        .handle
        .act(EngineAction::Seed {
            entity: "username:second".parse().unwrap(),
        })
        .await
        .unwrap()
    else {
        panic!()
    };
    let ActionOutcome::Planned { jobs } = t
        .handle
        .act(EngineAction::Run {
            module: "keyed".into(),
            entity: second.id,
            rerun: false,
        })
        .await
        .unwrap()
    else {
        panic!()
    };
    assert!(
        matches!(&jobs[0].result, PlanResult::Blocked { reason, .. } if reason == "daily budget exhausted (1/1)"),
        "{jobs:?}"
    );
    t.handle.wait_idle(WAIT).await.unwrap();
    let QueryResult::Review(items) = t.handle.query(Query::Review(None)).await.unwrap() else {
        panic!()
    };
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].payload["detail"]["reason"], "login wall");
    let QueryResult::Status(s) = t.handle.query(Query::Status).await.unwrap() else {
        panic!()
    };
    assert_eq!(s.review_pending, 1);
    assert!(t.handle.store().verify().await.unwrap().ok());
}
