//! J3 (PRD §5): with the `tracelabs` profile, a user-installed module of
//! class `probing` never runs. The activity shows `⊘ rules … blocked` and the
//! audit log records the reason.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod support;

use std::path::PathBuf;
use std::time::Duration;

use spektr_engine::{ActionOutcome, EngineAction, EventKind, PlanResult};
use spektr_store::{CaseInit, JobState};
use support::*;

#[tokio::test]
async fn probing_module_is_blocked_under_tracelabs() {
    let marker = tempfile::NamedTempFile::new().unwrap();
    let marker_path = marker.path().to_path_buf();
    let mut init = CaseInit::named("tl-ctf-1101");
    init.rules_profile = "tracelabs".into();
    let mut t = case_with(
        init,
        move |c| {
            c.exec_env
                .insert("FAKE_MARKER".into(), marker_path.display().to_string());
        },
        |e| {
            let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/probing");
            assert!(e.load_exec_modules(&dir).is_empty());
        },
    )
    .await;

    // automatic: the new email triggers the probing module, which is blocked
    let ActionOutcome::Seeded { entity, jobs, .. } = t
        .handle
        .act(EngineAction::Seed {
            entity: "email:jordan.marlow@mail.example".parse().unwrap(),
        })
        .await
        .unwrap()
    else {
        panic!()
    };
    let blocked: Vec<_> = jobs
        .iter()
        .filter(|j| j.module == "email-account-probe")
        .collect();
    assert_eq!(blocked.len(), 1);
    assert!(
        matches!(&blocked[0].result, PlanResult::Blocked { reason, .. } if reason == "tracelabs forbids probing"),
        "{blocked:?}"
    );
    // and manually
    let ActionOutcome::Planned { jobs } = t
        .handle
        .act(EngineAction::Run {
            module: "email-account-probe".into(),
            entity: entity.id,
            rerun: true,
        })
        .await
        .unwrap()
    else {
        panic!()
    };
    assert!(
        matches!(&jobs[0].result, PlanResult::Blocked { reason, .. } if reason == "tracelabs forbids probing")
    );
    t.handle.wait_idle(Duration::from_secs(10)).await.unwrap();

    // the activity rail's ⊘ row
    let events = drain(&mut t.events);
    let rail: Vec<String> = events
        .iter()
        .filter_map(|e| match &e.kind {
            EventKind::JobBlocked { module, reason, .. } => {
                Some(format!("⊘ rules {module} blocked · {reason}"))
            }
            _ => None,
        })
        .collect();
    assert_eq!(
        rail,
        ["⊘ rules email-account-probe blocked · tracelabs forbids probing"; 2]
    );

    // the job rows and the audit log
    let (jobs, audit) = t
        .handle
        .store()
        .read(|r| Ok((r.jobs(&[])?, r.audit_by_action("job.add")?)))
        .await
        .unwrap();
    assert!(jobs.iter().all(|j| j.state == JobState::Blocked
        && j.reason.as_deref() == Some("tracelabs forbids probing")));
    let probe_rows: Vec<_> = audit
        .iter()
        .filter(|a| a.payload.contains("email-account-probe"))
        .collect();
    assert_eq!(probe_rows.len(), 2);
    for row in probe_rows {
        let p: serde_json::Value = serde_json::from_str(&row.payload).unwrap();
        assert_eq!(p["state"], "blocked");
        assert_eq!(p["reason"], "tracelabs forbids probing");
    }
    // it never ran
    assert_eq!(std::fs::read_to_string(marker.path()).unwrap(), "");
    assert!(t.handle.store().verify().await.unwrap().ok());
}
