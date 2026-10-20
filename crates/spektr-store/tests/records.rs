//! Findings, notes, scope, settings, review items, jobs and playbook records.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod support;

use serde_json::json;
use spektr_model::{Actor, CorePredicate};
use spektr_store::{
    FindingStatus, FindingUpdate, ItemState, JobState, ReviewKind, ReviewState, RunState,
    ScopeKind, ScopeMatcher, StepKind, StepState, Trigger,
};
use support::*;

#[test]
fn records_round_trip_and_are_audited() {
    let mut c = new_case();
    let before = c.db.reader().audit_count().unwrap();
    let (u, f, o) =
        c.db.write(|tx| {
            let o = obs(tx, "github", None, "api.github.com");
            let u = ent(tx, "username", "jmarlow.wav", 0, o, "github.com");
            let f = tx.add_finding(
                &Actor::Analyst,
                "GitHub activity after last seen",
                "timeline",
                "",
                "",
            )?;
            tx.update_finding(
                &Actor::Analyst,
                f,
                &FindingUpdate {
                    claim: Some("Active on 2026-10-16.".into()),
                    status: Some(FindingStatus::Ready),
                    ..FindingUpdate::default()
                },
            )?;
            assert!(tx.finding_entity(&Actor::Analyst, f, u, true)?);
            assert!(!tx.finding_entity(&Actor::Analyst, f, u, true)?);
            tx.finding_evidence(&Actor::Analyst, f, o, true)?;
            tx.add_note(&Actor::Analyst, Some(u), None, "seen on three platforms")?;
            tx.add_scope_rule(
                &Actor::Analyst,
                ScopeKind::Deny,
                ScopeMatcher::DomainSuffix,
                "cdn-edge.example",
            )?;
            tx.set_module_setting(&Actor::Analyst, "hibp", false, &json!({}))?;
            let ri = tx.add_review_item(
                &module("username-sweep"),
                ReviewKind::SweepUnsure,
                &json!({"site": "example-forum"}),
            )?;
            tx.decide_review_item(&Actor::Analyst, ri, ReviewState::Rejected)?;
            Ok((u, f, o))
        })
        .unwrap();
    let r = c.db.reader();
    let finding = r.finding(f).unwrap().unwrap();
    assert_eq!(finding.status, FindingStatus::Ready);
    assert_eq!(finding.claim, "Active on 2026-10-16.");
    assert_eq!(finding.title, "GitHub activity after last seen");
    assert_eq!(r.finding_entities(f).unwrap(), vec![u]);
    assert_eq!(r.finding_evidence(f).unwrap(), vec![o]);
    assert_eq!(r.findings_for_entity(u).unwrap().len(), 1);
    assert_eq!(
        r.notes_for_entity(u).unwrap()[0].body,
        "seen on three platforms"
    );
    assert_eq!(r.scope_rules().unwrap()[0].pattern, "cdn-edge.example");
    assert!(!r.module_settings().unwrap()[0].enabled);
    assert!(
        r.review_items(Some(ReviewState::Pending))
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        r.review_items(None).unwrap()[0].state,
        ReviewState::Rejected
    );
    // obs, entity, finding add/update, entity link, evidence link, note, scope,
    // setting, review add/decide = 11 rows
    assert_eq!(r.audit_count().unwrap(), before + 11);

    c.db.write(|tx| tx.delete_finding(&Actor::Analyst, f))
        .unwrap();
    assert!(c.db.reader().finding(f).unwrap().is_none());
    assert!(c.db.verify().unwrap().ok());
    let _ = CorePredicate::HasAccount;
}

#[test]
fn jobs_and_playbooks() {
    let mut c = new_case();
    c.db.write(|tx| {
        let o = obs(tx, "seed", None, "seed.example");
        let u = ent(tx, "username", "jmarlow.wav", 0, o, "seed");
        let pb = Actor::Playbook {
            name: "person-from-username".into(),
            version: "1.0.0".into(),
        };
        let run = tx.add_playbook_run(&pb, "person-from-username", "1.0.0", "abc123", u, None)?;
        let child =
            tx.add_playbook_run(&pb, "person-from-email", "1.0.0", "def456", u, Some(run))?;
        let job = tx.add_job(
            &Actor::Engine,
            "username-sweep",
            u,
            Trigger::Playbook,
            Some(run),
            0,
            JobState::Queued,
            None,
        )?;
        tx.set_job_state(&Actor::Engine, job, JobState::Running, None, None)?;
        tx.set_job_state(&Actor::Engine, job, JobState::Done, None, None)?;
        let blocked = tx.add_job(
            &Actor::Engine,
            "email-account-probe",
            u,
            Trigger::Auto,
            None,
            0,
            JobState::Blocked,
            Some("tracelabs forbids probing"),
        )?;
        let j2 = tx.add_job(
            &Actor::Engine,
            "github",
            u,
            Trigger::Playbook,
            Some(run),
            0,
            JobState::Cached,
            None,
        )?;
        tx.link_job(&Actor::Engine, j2, Some(o), None)?;
        tx.set_step(
            &pb,
            run,
            1,
            StepKind::Run,
            StepState::Running,
            1,
            0,
            &json!({}),
        )?;
        tx.set_step(
            &pb,
            run,
            1,
            StepKind::Run,
            StepState::Done,
            1,
            0,
            &json!({"done": 1}),
        )?;
        let item = tx.add_checklist_item(
            &pb,
            run,
            5,
            Some(u),
            "Reverse image search",
            &["lens".into(), "tineye".into()],
            None,
        )?;
        tx.resolve_checklist_item(
            &Actor::Analyst,
            item,
            ItemState::Done,
            Some("nothing found"),
        )?;
        tx.set_run_state(&pb, child, RunState::Done, 5)?;
        tx.set_run_state(&pb, run, RunState::Running, 2)?;

        let r = tx.reader();
        let j = r.job(job)?.unwrap();
        assert_eq!(j.state, JobState::Done);
        assert!(j.started_at.is_some() && j.finished_at.is_some());
        let b = r.job(blocked)?.unwrap();
        assert_eq!(b.reason.as_deref(), Some("tracelabs forbids probing"));
        assert!(b.finished_at.is_some());
        assert_eq!(r.job(j2)?.unwrap().cached_observation, Some(o));
        assert_eq!(r.jobs_for_run(run)?.len(), 2);
        let steps = r.playbook_steps(run)?;
        assert_eq!(steps.len(), 1);
        assert_eq!(steps[0].state, StepState::Done);
        assert!(steps[0].started_at.is_some() && steps[0].finished_at.is_some());
        let it = r.checklist_item(item)?.unwrap();
        assert_eq!(
            (it.state, it.note.as_deref()),
            (ItemState::Done, Some("nothing found"))
        );
        assert_eq!(it.launchers, ["lens", "tineye"]);
        assert!(
            r.checklist_items_for_entity(u)?.is_empty(),
            "only open items"
        );
        let runs = r.playbook_runs()?;
        assert_eq!(runs.len(), 2);
        assert_eq!(runs[1].parent_run, Some(run));
        assert_eq!(r.job_counts()?.get(&JobState::Blocked), Some(&1));
        assert_eq!(r.audit_by_action("playbook.finish")?.len(), 1);
        Ok(())
    })
    .unwrap();
    assert!(c.db.verify().unwrap().ok());
}
