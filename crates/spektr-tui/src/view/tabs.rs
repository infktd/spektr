//! The graph pane's timeline and path tabs (TUI_SPEC §4.3).

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use spektr_engine::Direction;

use super::Hits;
use super::draw::{self, run};
use crate::glyph::{Marker, display_value, strength_token};
use crate::state::{AppState, Hit, Pane, Workspace};
use crate::text;

pub fn timeline(buf: &mut Buffer, inner: Rect, s: &AppState, w: &Workspace, hits: &mut Hits) {
    let t = &s.theme;
    let g = s.glyphs;
    let tl = &w.timeline;
    let Some(all) = &tl.events else {
        draw::put(
            buf,
            inner.x,
            inner.y,
            inner.width,
            &[run("loading…", t.muted())],
        );
        return;
    };
    let ev = tl.visible();
    let kind = tl.kind.as_deref().unwrap_or("all");
    let e_key = s
        .keymap
        .hint_for(crate::keymap::Action::EventFilter, crate::keymap::Ctx::Nav)
        .map(|(k, _)| k)
        .unwrap_or_default();
    draw::put(
        buf,
        inner.x,
        inner.y,
        inner.width,
        &[
            run(format!("{} of {} events", ev.len(), all.len()), t.muted()),
            run(format!("   {e_key} kind: "), t.muted()),
            run(kind.to_string(), t.text()),
        ],
    );
    if ev.is_empty() {
        draw::put(
            buf,
            inner.x,
            inner.y + 2,
            inner.width,
            &[run("no dated attributes yet", t.muted())],
        );
        return;
    }
    let last_seen = w
        .status
        .as_ref()
        .and_then(|s| s.last_seen.as_deref())
        .and_then(|d| d.parse::<jiff::civil::Date>().ok());
    let after = |e: &spektr_engine::TimelineEvent| {
        last_seen.is_some_and(|ls| e.at.to_zoned(jiff::tz::TimeZone::UTC).date() > ls)
    };
    let late = |e: &spektr_engine::TimelineEvent| after(e) && crate::tabs::marks_activity(&e.attr);
    // rows, with the last-seen divider before the first event after it
    enum Row<'a> {
        Ev(usize, &'a spektr_engine::TimelineEvent),
        Divider,
    }
    let mut rows = Vec::new();
    let mut divided = false;
    for (i, e) in ev.iter().enumerate() {
        if !divided && after(e) {
            rows.push(Row::Divider);
            divided = true;
        }
        rows.push(Row::Ev(i, e));
    }
    if !divided && last_seen.is_some() {
        rows.push(Row::Divider);
    }
    let h = inner.height.saturating_sub(2) as usize;
    let cur = rows
        .iter()
        .position(|r| matches!(r, Row::Ev(i, _) if *i == tl.cursor))
        .unwrap_or(0);
    let start = if rows.len() <= h {
        0
    } else {
        cur.saturating_sub(h.saturating_sub(1)).min(rows.len() - h)
    };
    let focused = w.pane == Pane::Graph;
    let ent_w = (inner.width as usize)
        .saturating_sub(16 + 2 + 20 + 16)
        .clamp(14, 40);
    for (n, r) in rows.iter().skip(start).take(h).enumerate() {
        let y = inner.y + 2 + n as u16;
        match r {
            Row::Divider => {
                if let Some(ls) = last_seen {
                    let label = format!(" last seen {ls} ");
                    let side = (inner.width as usize).saturating_sub(text::width(&label)) / 2;
                    let rule = g.m(Marker::Rule).repeat(side);
                    draw::put(
                        buf,
                        inner.x,
                        y,
                        inner.width,
                        &[run(format!("{rule}{label}{rule}"), t.warn())],
                    );
                }
            }
            Row::Ev(i, e) => {
                let row = Rect::new(inner.x.saturating_sub(1), y, inner.width + 2, 1);
                if *i == tl.cursor && focused {
                    draw::bg(buf, row, t.cursor_row());
                    draw::put(
                        buf,
                        inner.x.saturating_sub(1),
                        y,
                        1,
                        &[run(g.m(Marker::Cursor), t.cursor_bar())],
                    );
                } else if s.hover == Some(Hit::TimeRow(*i)) {
                    draw::bg(buf, row, t.hover_row());
                }
                let late = late(e);
                let who = format!("{} {}", g.entity(&e.entity.ty), display_value(&e.entity, g));
                draw::put(
                    buf,
                    inner.x,
                    y,
                    inner.width,
                    &[
                        run(
                            if late {
                                format!("{} ", g.m(Marker::AfterLastSeen))
                            } else {
                                "  ".into()
                            },
                            t.warn(),
                        ),
                        run(e.at.strftime("%Y-%m-%d %H:%M").to_string(), t.muted()),
                        run("  ", t.text()),
                        run(text::fit(&who, ent_w), t.text()),
                        run("  ", t.text()),
                        run(text::fit(&e.attr.replace('_', " "), 18), t.text()),
                        run("  ", t.text()),
                        run(e.module.clone(), t.muted()),
                    ],
                );
                hits.push((row, Hit::TimeRow(*i)));
            }
        }
    }
}

pub fn path(buf: &mut Buffer, inner: Rect, s: &AppState, w: &Workspace, hits: &mut Hits) {
    let t = &s.theme;
    let g = s.glyphs;
    let p = &w.path_tab;
    let Some(target) = &p.target else {
        draw::put(
            buf,
            inner.x,
            inner.y,
            inner.width,
            &[run(
                "press 3 to pick a target, or :path <type>:<value>",
                t.muted(),
            )],
        );
        return;
    };
    let from = p
        .from
        .as_ref()
        .map(|e| format!("{} {}", g.entity(&e.ty), display_value(e, g)))
        .unwrap_or_else(|| "the focus".into());
    draw::put(
        buf,
        inner.x,
        inner.y,
        inner.width,
        &[
            run("path ", t.muted()),
            run(from, t.text()),
            run(format!(" {} ", g.m(Marker::Out)), t.muted()),
            run(
                format!("{} {}", g.entity(&target.ty), display_value(target, g)),
                t.text(),
            ),
        ],
    );
    let view = match &p.view {
        None => {
            draw::put(
                buf,
                inner.x,
                inner.y + 2,
                inner.width,
                &[run("loading…", t.muted())],
            );
            return;
        }
        Some(None) => {
            draw::put(
                buf,
                inner.x,
                inner.y + 2,
                inner.width,
                &[run(
                    format!("No path within {} hops", crate::tabs::MAX_HOPS),
                    t.muted(),
                )],
            );
            return;
        }
        Some(Some(v)) => v,
    };
    let focused = w.pane == Pane::Graph;
    let mut y = inner.y + 2;
    for (i, st) in view.steps.iter().enumerate() {
        if let Some((_, pred, dir, strength)) = &st.via {
            if y >= inner.bottom() {
                break;
            }
            let arrow = match dir {
                Direction::Out => g.m(Marker::Out),
                Direction::In => g.m(Marker::In),
                Direction::Sym => g.m(Marker::Sym),
            };
            let mut runs = vec![
                run(format!("{} ", g.m(Marker::VRule)), t.faint()),
                run(format!("{arrow} {}", pred.as_str()), t.muted()),
            ];
            if let Some(sv) = strength {
                runs.push(run(
                    format!(" {}", g.strength(*sv)),
                    t.fg(strength_token(*sv)),
                ));
            }
            draw::put(buf, inner.x, y, inner.width, &runs);
            y += 1;
        }
        if y >= inner.bottom() {
            break;
        }
        let row = Rect::new(inner.x.saturating_sub(1), y, inner.width + 2, 1);
        if i == p.cursor && focused {
            draw::bg(buf, row, t.cursor_row());
            draw::put(
                buf,
                inner.x.saturating_sub(1),
                y,
                1,
                &[run(g.m(Marker::Cursor), t.cursor_bar())],
            );
        }
        draw::put(
            buf,
            inner.x,
            y,
            inner.width,
            &[
                run(format!("{} ", g.entity(&st.entity.ty)), t.muted()),
                run(display_value(&st.entity, g), t.text()),
            ],
        );
        hits.push((row, Hit::TimeRow(i)));
        y += 1;
    }
}
