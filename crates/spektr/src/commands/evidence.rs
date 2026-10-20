//! `spektr capture` and `spektr attach`.

use anyhow::{Context, Result, bail};
use spektr_engine::{ActionOutcome, EngineAction, EngineHandle, Query, QueryResult};
use spektr_model::{EntityId, EntityRef};

use crate::cli::{AttachArgs, CaptureArgs};
use crate::wiring::{Ctx, Modules};

async fn find(h: &EngineHandle, s: &str) -> Result<EntityId> {
    let r: EntityRef = s.parse().with_context(|| format!("--entity `{s}`"))?;
    match h.query(Query::Find(r)).await? {
        QueryResult::Found(Some(e)) => Ok(e.id),
        _ => bail!("--entity `{s}` isn't in the case"),
    }
}

pub async fn capture(ctx: &Ctx, a: CaptureArgs) -> Result<()> {
    if !a.case.exists() {
        bail!("{} not found", a.case.display());
    }
    let h = ctx.open(&a.case, None, Modules::None).await?;
    let entity = match &a.entity {
        Some(s) => Some(find(&h, s).await?),
        None => None,
    };
    let out = h
        .act(EngineAction::Capture {
            entity,
            url: a.url.clone(),
            screenshot: !a.no_screenshot,
        })
        .await?;
    h.store().flush().await?;
    let ActionOutcome::Captured { observation } = out else {
        bail!("unexpected reply")
    };
    let raw = match h.query(Query::Raw(observation)).await? {
        QueryResult::Raw(Some(r)) => r,
        _ => bail!("the capture wasn't stored"),
    };
    let s = &raw.summary;
    println!(
        "captured {} · HTTP {}",
        s["final_url"].as_str().unwrap_or(&a.url),
        s["status"]
    );
    println!(
        "  html   {}",
        raw.observation.blob.as_deref().unwrap_or("—")
    );
    match (s["screenshot"].as_str(), s["screenshot_note"].as_str()) {
        (Some(sha), _) => println!("  png    {sha}"),
        (None, Some(why)) => println!("  png    none · {why}"),
        _ => {}
    }
    Ok(())
}

pub async fn attach(ctx: &Ctx, a: AttachArgs) -> Result<()> {
    if !a.case.exists() {
        bail!("{} not found", a.case.display());
    }
    let bytes = std::fs::read(&a.path).with_context(|| format!("reading {}", a.path.display()))?;
    let name = a
        .path
        .file_name()
        .map_or_else(|| "attachment".into(), |n| n.to_string_lossy().into_owned());
    let h = ctx.open(&a.case, None, Modules::All).await?;
    let entity = match &a.entity {
        Some(s) => Some(find(&h, s).await?),
        None => None,
    };
    let out = h
        .act(EngineAction::Attach {
            entity,
            name: name.clone(),
            bytes,
            note: a.note.clone(),
        })
        .await?;
    // An image's exif runs now.
    h.wait_idle(std::time::Duration::from_secs(120)).await?;
    h.store().flush().await?;
    let ActionOutcome::Attached { image, .. } = out else {
        bail!("unexpected reply")
    };
    match image {
        Some(img) => println!("attached {name} · image {}", crate::fmt::entity(&img)),
        None => println!("attached {name}"),
    }
    Ok(())
}
