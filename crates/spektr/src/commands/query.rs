//! `spektr query <case>`: entities, neighbours, paths and findings, as a
//! table or JSON lines.

use anyhow::{Context, Result, bail};
use nucleo_matcher::pattern::{CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config, Matcher, Utf32Str};
use serde_json::json;
use spektr_engine::{EgoParams, Query, QueryResult};
use spektr_model::EntityRef;

use crate::cli::QueryArgs;
use crate::fmt::{entity, label};
use crate::wiring::Ctx;

async fn find(h: &spektr_engine::EngineHandle, s: &str) -> Result<spektr_engine::EntitySummary> {
    let r: EntityRef = s.parse().with_context(|| format!("`{s}`"))?;
    match h.query(Query::Find(r)).await? {
        QueryResult::Found(Some(e)) => Ok(e),
        _ => bail!("{s} is not in this case"),
    }
}

pub async fn run(ctx: &Ctx, a: QueryArgs) -> Result<()> {
    if !a.case.exists() {
        bail!("{} not found", a.case.display());
    }
    let h = ctx
        .open(&a.case, None, crate::wiring::Modules::None)
        .await?;

    if let Some(p) = &a.path {
        let (from, to) = (find(&h, &p[0]).await?, find(&h, &p[1]).await?);
        let QueryResult::Path(path) = h
            .query(Query::Path {
                from: from.id,
                to: to.id,
                max_hops: 6,
            })
            .await?
        else {
            bail!("unexpected reply")
        };
        let Some(path) = path else {
            if a.json {
                println!("{}", json!({ "path": null }));
            } else {
                println!("No path within 6 hops");
            }
            return Ok(());
        };
        for step in &path.steps {
            if a.json {
                println!("{}", serde_json::to_string(step)?);
                continue;
            }
            if let Some((_, pred, _, strength)) = &step.via {
                println!("│ {pred} {}", strength.map_or("", |s| s.bar()));
            }
            println!("{}", entity(&step.entity));
        }
        return Ok(());
    }

    if let Some(n) = &a.neighbors {
        let focus = find(&h, n).await?;
        let QueryResult::Ego(ego) = h
            .query(Query::Ego {
                focus: focus.id,
                params: EgoParams {
                    page_size: usize::MAX,
                    ..EgoParams::default()
                },
            })
            .await?
        else {
            bail!("unexpected reply")
        };
        for g in &ego.groups {
            if a.predicate
                .as_deref()
                .is_some_and(|p| p != g.key.predicate.as_str())
            {
                continue;
            }
            for row in &g.rows {
                if a.json {
                    println!(
                        "{}",
                        json!({
                            "predicate": g.key.predicate, "direction": g.key.direction, "artifact": g.key.artifact,
                            "entity": row.entity, "strength": row.strength, "sources": row.sources, "edges": row.edges,
                        })
                    );
                } else {
                    let arrow = match g.key.direction {
                        spektr_engine::Direction::In => "←",
                        spektr_engine::Direction::Out => "→",
                        spektr_engine::Direction::Sym => "↔",
                    };
                    let pred = match g.key.artifact {
                        Some(art) => format!("≈ {}", art.label()),
                        None => g.key.predicate.to_string(),
                    };
                    println!(
                        "{arrow} {pred:<20} {:<48} {}",
                        entity(&row.entity),
                        row.strength.map_or("", |s| s.bar())
                    );
                }
            }
        }
        return Ok(());
    }

    if a.findings {
        let rows = h.store().read(|r| r.findings()).await?;
        for f in rows {
            if a.json {
                println!(
                    "{}",
                    json!({ "id": f.id, "title": f.title, "category": f.category, "status": f.status, "claim": f.claim, "relevance": f.relevance })
                );
            } else {
                println!(
                    "F-{:<4} {:<24} {:<8} {}",
                    f.id, f.category, f.status, f.title
                );
            }
        }
        return Ok(());
    }

    let QueryResult::Entities(mut list) = h.query(Query::Entities).await? else {
        bail!("unexpected reply")
    };
    if let Some(t) = &a.ty {
        let t: spektr_model::EntityType = t.parse().with_context(|| format!("--type `{t}`"))?;
        list.retain(|e| e.ty == t);
    }
    if let Some(m) = &a.matching {
        let mut matcher = Matcher::new(Config::DEFAULT);
        let pattern = Pattern::parse(m, CaseMatching::Ignore, Normalization::Smart);
        let mut scored: Vec<(u32, spektr_engine::EntitySummary)> = list
            .into_iter()
            .filter_map(|e| {
                let mut buf = Vec::new();
                let hay = format!("{} {}", label(&e), e.key);
                pattern
                    .score(Utf32Str::new(&hay, &mut buf), &mut matcher)
                    .map(|s| (s, e))
            })
            .collect();
        scored.sort_by_key(|s| std::cmp::Reverse(s.0));
        list = scored.into_iter().map(|(_, e)| e).collect();
    }
    for e in &list {
        if a.json {
            println!("{}", serde_json::to_string(e)?);
        } else {
            println!("{:<14} {:<48} depth {}", e.ty.as_str(), entity(e), e.depth);
        }
    }
    if !a.json && list.is_empty() {
        println!("no entities match");
    }
    Ok(())
}
