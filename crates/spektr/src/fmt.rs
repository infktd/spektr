//! Text formatting for the headless CLI: activity lines in the rail's
//! format (TUI_SPEC §4.5), entity labels, tables.

use std::collections::BTreeMap;

use spektr_engine::{EngineEvent, EntitySummary, EventKind, JobState};

/// `◐ github · jmarlow-wav`, `◆ kestrel.example`.
pub fn entity(e: &EntitySummary) -> String {
    format!("{} {}", e.ty.glyph(), label(e))
}

/// An entity's display text without the glyph.
pub fn label(e: &EntitySummary) -> String {
    if e.ty.as_str() == "account"
        && let Some((p, h)) = e.key.split_once(':')
    {
        return format!("{p} · {h}");
    }
    e.value.clone()
}

fn found(f: &BTreeMap<String, u32>) -> String {
    f.iter()
        .map(|(ty, n)| {
            let g = ty
                .parse::<spektr_model::EntityType>()
                .map(|t| t.glyph())
                .unwrap_or('·');
            format!("+{n} {g} {ty}")
        })
        .collect::<Vec<_>>()
        .join("  ")
}

/// `HH:MM:SS <glyph> <module, padded to 15> <summary>`; `None` for events
/// the rail doesn't show.
pub fn event_line(ev: &EngineEvent) -> Option<String> {
    let t = ev.at.strftime("%H:%M:%S");
    let row = |glyph: &str, who: &str, text: String| Some(format!("{t} {glyph} {who:<15} {text}"));
    match &ev.kind {
        EventKind::JobStarted {
            module, entity: e, ..
        } => row("▸", module, format!("running  on {}", entity(e))),
        EventKind::JobFinished {
            module,
            entity: e,
            state,
            found: f,
            note,
            error,
            retry_in_secs,
            attempts_left,
            ..
        } => match state {
            JobState::Done => {
                let what = match (note, f.is_empty()) {
                    (Some(n), _) => n.clone(),
                    (None, false) => found(f),
                    (None, true) => "nothing new".into(),
                };
                row("✓", module, format!("{what}  ← {}", entity(e)))
            }
            JobState::Queued => row(
                "✕",
                module,
                format!(
                    "{}  retry in {}s · {} attempt{} left",
                    error.clone().unwrap_or_default(),
                    retry_in_secs.unwrap_or(0),
                    attempts_left.unwrap_or(0),
                    if attempts_left == &Some(1) { "" } else { "s" }
                ),
            ),
            _ => row(
                "✕",
                module,
                format!(
                    "{}  ← {}",
                    error.clone().unwrap_or_else(|| "failed".into()),
                    entity(e)
                ),
            ),
        },
        EventKind::JobBlocked { module, reason, .. } => {
            row("⊘", "rules", format!("{module} blocked · {reason}"))
        }
        EventKind::JobCached {
            module, entity: e, ..
        } => row("✓", module, format!("cached  ← {}", entity(e))),
        EventKind::HubSuppressed { entity: e, degree } => row(
            "⊛",
            "hub",
            format!("{} degree {degree} · auto-expansion suppressed", entity(e)),
        ),
        EventKind::Correlation { artifact, a, b, .. } => row(
            "≈",
            "correlate",
            format!("{}  {} ↔ {}", artifact.label(), entity(a), entity(b)),
        ),
        EventKind::IngestFailed { message } => row("✕", "ingest", message.clone()),
        EventKind::MessageRejected { module, reason, .. } => {
            row("!", module, format!("rejected: {reason}"))
        }
        _ => None,
    }
}
