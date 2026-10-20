//! `spektr run <case>`: seed, expand, run to quiescence, summarize.

use std::collections::BTreeMap;

use anyhow::{Context, Result, bail};
use serde_json::json;
use spektr_engine::{ActionOutcome, EngineAction, JobState, Query, QueryResult};
use spektr_model::EntityRef;
use tokio::sync::broadcast::error::RecvError;

use crate::cli::RunArgs;
use crate::fmt::{entity, event_line};
use crate::wiring::Ctx;

pub async fn run(ctx: &Ctx, a: RunArgs) -> Result<()> {
    let timeout = spektr_protocol::parse_duration(&a.timeout)
        .with_context(|| format!("--timeout `{}` is not a duration (e.g. 10m)", a.timeout))?;
    let seeds: Vec<EntityRef> = a
        .seeds
        .iter()
        .map(|s| {
            s.parse::<EntityRef>()
                .with_context(|| format!("--seed `{s}`"))
        })
        .collect::<Result<_>>()?;
    if !a.case.exists() {
        bail!("{} not found", a.case.display());
    }
    let modules = if a.modules.is_empty() {
        crate::wiring::Modules::All
    } else {
        crate::wiring::Modules::Only(&a.modules)
    };
    let h = ctx.open(&a.case, a.depth, modules).await?;
    let known: Vec<String> = h.modules().into_iter().map(|m| m.name).collect();
    for m in &a.modules {
        if !known.contains(m) {
            bail!("unknown module `{m}`");
        }
    }
    let mut rx = h.subscribe();
    let print = |ev: &spektr_engine::EngineEvent| {
        if a.json {
            if let Ok(s) = serde_json::to_string(ev) {
                println!("{s}");
            }
        } else if let Some(line) = event_line(ev) {
            eprintln!("{line}");
        }
    };
    for s in &seeds {
        let out = h
            .act(EngineAction::Seed { entity: s.clone() })
            .await
            .with_context(|| format!("seeding {s}"))?;
        if let ActionOutcome::Seeded {
            entity: e, created, ..
        } = out
            && !a.json
        {
            eprintln!(
                "seed {}{}",
                entity(&e),
                if created {
                    ""
                } else {
                    " (already in the case)"
                }
            );
        }
    }
    let idle = h.wait_idle(timeout);
    tokio::pin!(idle);
    loop {
        tokio::select! {
            r = &mut idle => { r?; break; }
            ev = rx.recv() => match ev {
                Ok(ev) => print(&ev),
                Err(RecvError::Lagged(n)) => eprintln!("({n} events not shown)"),
                Err(RecvError::Closed) => break,
            }
        }
    }
    while let Ok(ev) = rx.try_recv() {
        print(&ev);
    }
    h.store().flush().await?;

    let QueryResult::Status(s) = h.query(Query::Status).await? else {
        bail!("unexpected status reply")
    };
    let QueryResult::Jobs(jobs) = h.query(Query::Jobs(vec![])).await? else {
        bail!("unexpected jobs reply")
    };
    let mut by_state: BTreeMap<String, u64> = BTreeMap::new();
    for j in &jobs {
        *by_state.entry(j.state.to_string()).or_default() += 1;
    }
    let verify = h.store().verify().await?;
    if a.json {
        println!(
            "{}",
            json!({ "summary": {
                "case": s.name, "profile": s.profile, "depth": s.depth,
                "seeds": a.seeds, "entities": s.entities, "edges": s.edges, "observations": s.observations,
                "review_pending": s.review_pending, "jobs": by_state, "audit_rows": verify.audit_rows, "chain_ok": verify.ok(),
            }})
        );
    } else {
        let state = |st: JobState| by_state.get(st.as_str()).copied().unwrap_or(0);
        eprintln!();
        println!(
            "spektr run · {} · rules {} · depth {}",
            s.name, s.profile, s.depth
        );
        println!(
            "  jobs    done {} · cached {} · blocked {} · failed {}",
            state(JobState::Done),
            state(JobState::Cached),
            state(JobState::Blocked),
            state(JobState::Failed)
        );
        println!(
            "  graph   {} entities · {} edges · {} observations",
            s.entities, s.edges, s.observations
        );
        println!("  review  {} pending", s.review_pending);
        println!(
            "  chain   {} {} rows",
            if verify.ok() { "✓" } else { "✕" },
            verify.audit_rows
        );
    }
    Ok(())
}
