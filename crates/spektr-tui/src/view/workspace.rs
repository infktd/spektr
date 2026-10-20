//! The workspace (TUI_SPEC §4): top bar, entities pane, graph pane (ego
//! view), inspector, activity rail.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use spektr_engine::{Direction, EgoGroup, EgoView, EntitySummary};
use spektr_model::{AttrValue, EntityFlags};

use super::draw::{self, Run, run};
use super::{Hits, caret};
use crate::ego::{self, Col, Item};
use crate::glyph::{self, GlyphSet, Marker, display_value, strength_token};
use crate::host::Chain;
use crate::keymap::{Action, Ctx};
use crate::layout;
use crate::state::{AppState, EntItem, Hit, Pane, Tab, Workspace};
use crate::text;
use crate::theme::{Theme, Token};

pub fn render(buf: &mut Buffer, area: Rect, s: &AppState, w: &Workspace, hits: &mut Hits) {
    let l = layout::workspace(area.width, area.height);
    top_bar(buf, l.top, s, w);
    entities(buf, l.entities, s, w, hits);
    graph(buf, l.graph, s, w, hits);
    if l.inspector.width > 0 {
        inspector(buf, l.inspector, s, w, hits, false);
    }
    activity(buf, l.activity, s, w, hits);
    if l.inspector.width == 0 && w.insp_overlay {
        let r = layout::inspector_overlay(&l);
        draw::shadow(buf, r, &s.theme);
        inspector(buf, r, s, w, hits, true);
    }
}

fn top_bar(buf: &mut Buffer, r: Rect, s: &AppState, w: &Workspace) {
    let t = &s.theme;
    let g = s.glyphs;
    let dot = g.m(Marker::Dot);
    let mut left: Vec<Run> = vec![
        run(
            "spektr",
            t.accent().add_modifier(ratatui::style::Modifier::BOLD),
        ),
        run("  ", t.text()),
        run(g.m(Marker::VRule), t.faint()),
        run("  ", t.text()),
    ];
    match &w.status {
        Some(st) => {
            left.push(run(st.name.clone(), t.text()));
            left.push(run("  ", t.text()));
            if let Some(sub) = &st.subject {
                left.push(run("subject ", t.muted()));
                left.push(run(
                    format!("{} {}", g.entity(&sub.ty), sub.value),
                    t.text(),
                ));
                if let Some(ls) = &st.last_seen {
                    left.push(run(format!(" {dot} last seen {ls}"), t.muted()));
                }
            } else {
                left.push(run(
                    text::tilde(&w.path, s.settings.home.as_deref()),
                    t.muted(),
                ));
            }
        }
        None => left.push(run(
            text::tilde(&w.path, s.settings.home.as_deref()),
            t.muted(),
        )),
    }
    // Right-hand items, most important last; the first ones drop when
    // the bar is too narrow.
    let mut items: Vec<Vec<Run>> = Vec::new();
    if let Some(st) = &w.status {
        items.push(vec![
            run("rules ", t.muted()),
            run(st.profile.clone(), t.text()),
        ]);
        items.push(vec![
            run("findings ", t.muted()),
            run(st.findings.to_string(), t.text()),
        ]);
        if st.review_pending > 0 {
            items.push(vec![run(format!("review {}", st.review_pending), t.warn())]);
        }
        let dot_tok = if w.running > 0 { t.accent() } else { t.muted() };
        items.push(vec![
            run(g.m(Marker::Bullet), dot_tok),
            run(format!(" {} running", w.running), t.text()),
            run(format!(" {dot} {} queued", w.queued), t.muted()),
        ]);
        let chain = match &w.chain {
            Chain::Ok { .. } => run(format!("{} {}", g.m(Marker::Ok), st.audit_rows), t.ok()),
            Chain::Broken { at } => run(format!("{} {at}", g.m(Marker::Failed)), t.hostile()),
            Chain::Missing(_) => run(g.m(Marker::Failed), t.hostile()),
            Chain::Unknown => run(format!("{} {}", dot, st.audit_rows), t.muted()),
        };
        items.push(vec![run("chain ", t.muted()), chain]);
    }
    let left_min = draw::width(&left).min(36);
    let join = |items: &[Vec<Run>]| -> Vec<Run> {
        let mut v = Vec::new();
        for (i, it) in items.iter().enumerate() {
            if i > 0 {
                v.push(run("   ", t.text()));
            }
            v.extend(it.iter().cloned());
        }
        v
    };
    let mut from = 0;
    let mut right = join(&items);
    while from < items.len() && draw::width(&right) + left_min + 4 > r.width {
        from += 1;
        right = join(&items[from..]);
    }
    let rw = draw::width(&right);
    draw::put_right(buf, r.right().saturating_sub(1), r.y, rw, &right);
    draw::put(buf, r.x + 1, r.y, r.width.saturating_sub(rw + 4), &left);
}

/// The window start that keeps `cursor` inside `h` rows.
fn window(cursor: usize, len: usize, h: usize) -> usize {
    if len <= h || h == 0 {
        0
    } else {
        cursor.saturating_sub(h - 1).min(len - h)
    }
}

fn entities(buf: &mut Buffer, r: Rect, s: &AppState, w: &Workspace, hits: &mut Hits) {
    let t = &s.theme;
    let g = s.glyphs;
    let focused = w.pane == Pane::Entities;
    hits.push((r, Hit::Pane(Pane::Entities)));
    let count = w.entities.len();
    draw::pane(
        buf,
        t,
        r,
        focused,
        &[
            run("Entities ", t.title(focused)),
            run(count.to_string(), t.title(focused)),
        ],
        &[],
        t.base(),
    );
    let inner = layout::inner(r);
    if inner.height < 3 {
        return;
    }
    // filter line
    match &w.filter {
        Some(f) => {
            let mut runs = vec![run("/ ", t.accent()), run(f.text.clone(), t.text())];
            if f.editing {
                runs.push(caret(s));
            }
            let m = format!("{} match", f.matches);
            draw::put_right(
                buf,
                inner.right(),
                inner.y,
                text::width(&m) as u16,
                &[run(m.clone(), t.muted())],
            );
            draw::put(
                buf,
                inner.x,
                inner.y,
                inner.width.saturating_sub(text::width(&m) as u16 + 1),
                &runs,
            );
        }
        None => {
            draw::put(
                buf,
                inner.x,
                inner.y,
                inner.width,
                &[run("/ filter", t.muted())],
            );
        }
    }
    let list_y = inner.y + 2;
    let h = inner.height.saturating_sub(2) as usize;
    if w.entities.is_empty() {
        let key = s
            .keymap
            .hint_for(Action::Palette, Ctx::Global)
            .map(|(k, _)| k)
            .unwrap_or_default();
        draw::put(
            buf,
            inner.x,
            list_y,
            inner.width,
            &[run("no entities yet", t.muted())],
        );
        draw::put(
            buf,
            inner.x,
            list_y + 1,
            inner.width,
            &[run(key, t.text()), run(" seed <type>:<value>", t.muted())],
        );
        return;
    }
    let start = window(w.ent_cursor, w.ent_items.len(), h);
    let count_right = inner.right().saturating_sub(9);
    for (row, (i, item)) in w
        .ent_items
        .iter()
        .enumerate()
        .skip(start)
        .take(h)
        .enumerate()
    {
        let y = list_y + row as u16;
        let line = Rect::new(r.x + 1, y, r.width.saturating_sub(2), 1);
        let cursor = i == w.ent_cursor;
        let hover = s.hover == Some(Hit::EntRow(i));
        if (cursor && !matches!(item, EntItem::Blank)) || hover && crate::entities::selectable(item)
        {
            draw::bg(buf, line, t.cursor_row());
        }
        if cursor && focused {
            draw::put(
                buf,
                r.x + 1,
                y,
                1,
                &[run(g.m(Marker::Cursor), t.cursor_bar())],
            );
        }
        match item {
            EntItem::Header {
                ty,
                count,
                expanded,
            } => {
                let arrow = if *expanded {
                    g.m(Marker::Expanded)
                } else {
                    g.m(Marker::Collapsed)
                };
                draw::put(
                    buf,
                    inner.x,
                    y,
                    inner.width,
                    &[run(
                        format!("{arrow} {} {}", g.entity(ty), ty.as_str()),
                        t.muted(),
                    )],
                );
                let c = count.to_string();
                draw::put_right(
                    buf,
                    count_right,
                    y,
                    text::width(&c) as u16,
                    &[run(c, t.muted())],
                );
            }
            EntItem::Row(idx) => {
                let Some(e) = w.entities.get(*idx) else {
                    continue;
                };
                let is_focus = w.focus == Some(e.id);
                let marker = if w.fresh.contains(&e.id) {
                    Some((g.m(Marker::New).to_string(), Token::Accent))
                } else {
                    glyph::row_marker(e.flags, g)
                };
                let mw = marker
                    .as_ref()
                    .map_or(0, |(m, _)| text::width(m) as u16 + 1);
                if is_focus {
                    draw::put(
                        buf,
                        inner.x + 2,
                        y,
                        1,
                        &[run(g.m(Marker::Running), t.accent())],
                    );
                }
                let vw = inner.width.saturating_sub(4 + mw);
                let tok = if e.flags.contains(EntityFlags::OUT_OF_SCOPE) {
                    Token::FgMuted
                } else {
                    Token::Fg
                };
                draw::put(
                    buf,
                    inner.x + 4,
                    y,
                    vw,
                    &[run(display_value(e, g), t.fg(tok))],
                );
                if let Some((m, tok)) = marker {
                    draw::put_right(buf, inner.right(), y, mw, &[run(m, t.fg(tok))]);
                }
            }
            EntItem::More { hidden, .. } => {
                draw::put(
                    buf,
                    inner.x + 4,
                    y,
                    inner.width.saturating_sub(4),
                    &[run(
                        format!("{} {hidden} more", g.m(Marker::More)),
                        t.muted(),
                    )],
                );
            }
            EntItem::Hidden(n) => {
                draw::put(
                    buf,
                    inner.x + 2,
                    y,
                    inner.width.saturating_sub(2),
                    &[run(
                        format!(
                            "{} {n} out of scope {} hidden",
                            g.m(Marker::Blocked),
                            g.m(Marker::Dot)
                        ),
                        t.muted(),
                    )],
                );
            }
            EntItem::Blank => {}
        }
        if crate::entities::selectable(item) {
            hits.push((line, Hit::EntRow(i)));
        }
    }
}

fn graph(buf: &mut Buffer, r: Rect, s: &AppState, w: &Workspace, hits: &mut Hits) {
    let t = &s.theme;
    let focused = w.pane == Pane::Graph;
    hits.push((r, Hit::Pane(Pane::Graph)));
    // tabs: [1 ego] 2 timeline 3 path
    let tabs = [
        (Tab::Ego, Action::TabEgo),
        (Tab::Timeline, Action::TabTimeline),
        (Tab::Path, Action::TabPath),
    ];
    let mut right: Vec<Run> = Vec::new();
    let mut spans: Vec<(Tab, u16, u16)> = Vec::new();
    let mut x = 0u16;
    for (i, (tab, a)) in tabs.iter().enumerate() {
        let (k, label) = s.keymap.hint_for(*a, Ctx::Global).unwrap_or_default();
        if i > 0 {
            right.push(run("  ", t.muted()));
            x += 2;
        }
        let txt = if w.tab == *tab {
            format!("[{k} {label}]")
        } else {
            format!("{k} {label}")
        };
        let wdt = text::width(&txt) as u16;
        if w.tab == *tab {
            right.push(run(txt, t.accent()));
        } else {
            right.push(run(format!("{k} "), t.text()));
            right.push(run(label, t.muted()));
        }
        spans.push((*tab, x, wdt));
        x += wdt;
    }
    draw::pane(
        buf,
        t,
        r,
        focused,
        &[run("Graph", t.title(focused))],
        &right,
        t.base(),
    );
    let total = x + 2;
    let tx = r.right().saturating_sub(1 + total);
    for (tab, off, wdt) in spans {
        hits.push((Rect::new(tx + 1 + off, r.y, wdt, 1), Hit::Tab(tab)));
    }
    let inner = layout::inner(r);
    match w.tab {
        Tab::Ego => ego_view(buf, inner, s, w, hits),
        Tab::Timeline | Tab::Path => {
            let name = if w.tab == Tab::Timeline {
                "timeline"
            } else {
                "path"
            };
            let msg = format!("{name}: {}", crate::palette::NOT_YET);
            let c = layout::centered(inner, text::width(&msg) as u16, 1);
            draw::put(buf, c.x, c.y, c.width, &[run(msg, t.muted())]);
        }
    }
}

/// A group's display rows, with the account platform column width.
fn account_col(g: &EgoGroup) -> Option<usize> {
    let parts: Vec<_> = g
        .rows
        .iter()
        .map(|r| glyph::account_parts(&r.entity))
        .collect();
    if parts.is_empty() || parts.iter().any(Option::is_none) {
        return None;
    }
    parts
        .iter()
        .flatten()
        .map(|(p, _)| text::width(p))
        .max()
        .map(|m| m.min(12))
}

fn ego_view(buf: &mut Buffer, inner: Rect, s: &AppState, w: &Workspace, hits: &mut Hits) {
    let t = &s.theme;
    let g = s.glyphs;
    let Some(ego) = &w.ego else {
        let (line1, line2) = if w.focus.is_some() {
            ("loading…".to_string(), String::new())
        } else {
            (
                "no focus yet".to_string(),
                ": seed <type>:<value> to start".to_string(),
            )
        };
        let c = layout::centered(
            inner,
            text::width(&line2).max(text::width(&line1)) as u16,
            2,
        );
        draw::put(buf, c.x, c.y, c.width, &[run(line1, t.muted())]);
        draw::put(buf, c.x, c.y + 1, c.width, &[run(line2, t.muted())]);
        return;
    };
    if inner.height < 6 || inner.width < 40 {
        return;
    }
    // breadcrumb
    let n = w.crumbs.len();
    let mut x = inner.x;
    let first = n.saturating_sub(4);
    if first > 0 {
        x += draw::put(
            buf,
            x,
            inner.y,
            4,
            &[run(format!("… {} ", g.m(Marker::Crumb)), t.muted())],
        );
    }
    for (i, c) in w.crumbs.iter().enumerate().skip(first) {
        let last = i + 1 == n;
        let label = format!("{} {}", g.entity(&c.ty), display_value(c, g));
        let st = if last { t.text() } else { t.muted() };
        let room = inner.right().saturating_sub(x);
        let used = draw::put(buf, x, inner.y, room, &[run(label, st)]);
        hits.push((Rect::new(x, inner.y, used, 1), Hit::Crumb(i)));
        x += used;
        if !last {
            x += draw::put(
                buf,
                x,
                inner.y,
                5,
                &[run(format!("  {}  ", g.m(Marker::Crumb)), t.faint())],
            );
        }
    }

    let footer_y = inner.bottom().saturating_sub(1);
    let cols_y = inner.y + 2;
    let cols_h = footer_y.saturating_sub(cols_y + 1);
    let card_w = 20u16;
    let gap = 1u16;
    let (lg, rg) = ego::sides(ego);
    // Both sides share the width; a lone side gets three quarters of it.
    let sides_w = inner.width.saturating_sub(card_w + 2 * gap);
    let left_w = match (lg.is_empty(), rg.is_empty()) {
        (true, false) => sides_w / 4,
        (false, true) => sides_w * 3 / 4,
        _ => sides_w / 2,
    };
    let left = Rect::new(inner.x, cols_y, left_w, cols_h);
    let card_x = inner.x + left_w + gap;
    let right_x = card_x + card_w + gap;
    let right = Rect::new(
        right_x,
        cols_y,
        inner.right().saturating_sub(right_x),
        cols_h,
    );
    let focused = w.pane == Pane::Graph;
    column(buf, left, s, w, ego, &lg, Col::Left, focused, hits);
    column(buf, right, s, w, ego, &rg, Col::Right, focused, hits);
    card(
        buf,
        Rect::new(card_x, cols_y, card_w, cols_h),
        s,
        w,
        ego,
        hits,
    );

    // footer: hints left, strength legend right
    let mut hints: Vec<(String, &str)> = Vec::new();
    for (a, c) in [
        (Action::Activate, Ctx::Nav),
        (Action::Back, Ctx::Nav),
        (Action::Capture, Ctx::Entity),
        (Action::NewFinding, Ctx::Entity),
    ] {
        if let Some(h) = s.keymap.hint_for(a, c) {
            hints.push(h);
        }
    }
    let legend: Vec<Run> = [
        (spektr_model::Strength::Single, "1 source"),
        (spektr_model::Strength::Corroborated, "corroborated"),
        (spektr_model::Strength::Confirmed, "confirmed"),
    ]
    .iter()
    .enumerate()
    .flat_map(|(i, (st, label))| {
        let mut v = Vec::new();
        if i > 0 {
            v.push(run("   ", t.muted()));
        }
        v.push(run(g.strength(*st), t.fg(strength_token(*st))));
        v.push(run(format!(" {label}"), t.muted()));
        v
    })
    .collect();
    // The legend stays; hints drop from the end until both fit.
    let lw = draw::width(&legend);
    let mut hint_runs = super::hint_runs(s, &hints);
    while !hints.is_empty() && draw::width(&hint_runs) + lw + 3 > inner.width {
        hints.pop();
        hint_runs = super::hint_runs(s, &hints);
    }
    draw::put(buf, inner.x, footer_y, inner.width, &hint_runs);
    if lw + 2 <= inner.width {
        draw::put_right(buf, inner.right(), footer_y, lw, &legend);
    }
}

#[allow(clippy::too_many_arguments)]
fn column(
    buf: &mut Buffer,
    r: Rect,
    s: &AppState,
    w: &Workspace,
    ego: &EgoView,
    groups: &[usize],
    col: Col,
    focused: bool,
    hits: &mut Hits,
) {
    let t = &s.theme;
    let g = s.glyphs;
    let right_side = col == Col::Right;
    // (runs, right runs, item, is_row)
    struct L {
        runs: Vec<Run>,
        right: Vec<Run>,
        item: Option<usize>,
        rule: Option<ratatui::style::Style>,
    }
    let mut lines: Vec<L> = Vec::new();
    let mut idx = 0usize;
    let items = ego::items(ego, groups);
    for &gi in groups {
        let grp = &ego.groups[gi];
        let n = grp.total;
        let corr = grp.key.artifact;
        let pred = grp.key.predicate.as_str().to_string();
        let arrow = match grp.key.direction {
            Direction::In => g.m(Marker::In),
            Direction::Out => g.m(Marker::Out),
            Direction::Sym => g.m(Marker::Sym),
        };
        let (head, rule_st): (Vec<Run>, _) = match corr {
            Some(a) => (
                vec![run(
                    format!("{} {} {arrow} {n}", g.m(Marker::Correlated), a.label()),
                    t.accent(),
                )],
                t.accent(),
            ),
            None if right_side => (
                vec![
                    run(format!("{arrow} {n} "), t.text()),
                    run(pred.clone(), t.muted()),
                ],
                t.faint(),
            ),
            None => (
                vec![
                    run(format!("{pred} "), t.muted()),
                    run(format!("{arrow} {n}"), t.text()),
                ],
                t.faint(),
            ),
        };
        lines.push(L {
            runs: head,
            right: Vec::new(),
            item: None,
            rule: Some(rule_st),
        });
        let acol = account_col(grp);
        let has_more = grp.pages > 1;
        for (ri, row) in grp.rows.iter().enumerate() {
            let last = ri + 1 == grp.rows.len() && !has_more;
            let tree = if last {
                g.m(Marker::TreeEnd)
            } else {
                g.m(Marker::TreeMid)
            };
            let mut runs = vec![
                run(tree, t.faint()),
                run(" ", t.text()),
                run(g.entity(&row.entity.ty).to_string(), t.muted()),
                run(" ", t.text()),
            ];
            match (acol, glyph::account_parts(&row.entity)) {
                (Some(wd), Some((p, h))) => {
                    runs.push(run(text::fit(p, wd), t.text()));
                    runs.push(run(format!(" {h}"), t.text()));
                }
                _ => runs.push(run(display_value(&row.entity, g), t.text())),
            }
            if w.fresh.contains(&row.entity.id) {
                runs.push(run(format!(" {}", g.m(Marker::New)), t.accent()));
            }
            let right = match (row.strength, corr) {
                (Some(st), None) => vec![run(g.strength(st), t.fg(strength_token(st)))],
                _ => Vec::new(),
            };
            let it = items
                .iter()
                .position(|x| *x == Item::Row { group: gi, row: ri });
            lines.push(L {
                runs,
                right,
                item: it,
                rule: None,
            });
            idx += 1;
        }
        if has_more {
            let more = ego::remaining(grp);
            let it = items.iter().position(|x| *x == Item::More { group: gi });
            lines.push(L {
                runs: vec![
                    run(g.m(Marker::TreeEnd), t.faint()),
                    run(format!(" {} {more} more", g.m(Marker::More)), t.muted()),
                ],
                right: vec![run(format!("] {}/{}", grp.page + 1, grp.pages), t.muted())],
                item: it,
                rule: None,
            });
        }
        if let Some(a) = corr {
            lines.push(L {
                runs: vec![run(
                    format!(
                        "  {} {} {n} sources",
                        ego::artifact_note(a),
                        g.m(Marker::Dot)
                    ),
                    t.muted(),
                )],
                right: Vec::new(),
                item: None,
                rule: None,
            });
        }
        lines.push(L {
            runs: Vec::new(),
            right: Vec::new(),
            item: None,
            rule: None,
        });
    }
    let _ = idx;
    let cursor_item = (w.ego_col == col).then_some(w.ego_idx);
    let cursor_line = cursor_item
        .and_then(|ci| lines.iter().position(|l| l.item == Some(ci)))
        .unwrap_or(0);
    let h = r.height as usize;
    let start = window(cursor_line, lines.len(), h);
    for (row, line) in lines.iter().skip(start).take(h).enumerate() {
        let y = r.y + row as u16;
        let rect = Rect::new(r.x, y, r.width, 1);
        let is_cursor = line.item.is_some() && line.item == cursor_item;
        let hover = line.item.is_some_and(|i| s.hover == Some(Hit::Ego(col, i)));
        if is_cursor || hover {
            draw::bg(buf, rect, t.cursor_row());
        }
        let mut runs = line.runs.clone();
        if is_cursor && focused && !runs.is_empty() {
            runs[0] = run(g.m(Marker::Cursor), t.cursor_bar());
        }
        if let Some(rule_st) = line.rule {
            let hw = draw::width(&runs);
            let rule_w = r.width.saturating_sub(hw + 1);
            if right_side {
                draw::hrule(buf, r.x, y, rule_w, g.m(Marker::Rule), rule_st);
                draw::put_right(buf, r.right(), y, hw, &runs);
            } else {
                draw::put(buf, r.x, y, hw, &runs);
                draw::hrule(buf, r.x + hw + 1, y, rule_w, g.m(Marker::Rule), rule_st);
            }
            continue;
        }
        let rw = draw::width(&line.right);
        draw::put(buf, r.x, y, r.width.saturating_sub(rw + 1), &runs);
        if rw > 0 {
            draw::put_right(buf, r.right(), y, rw, &line.right);
        }
        if let Some(i) = line.item {
            hits.push((rect, Hit::Ego(col, i)));
        }
    }
}

fn card(buf: &mut Buffer, r: Rect, s: &AppState, w: &Workspace, ego: &EgoView, hits: &mut Hits) {
    let t = &s.theme;
    let g = s.glyphs;
    let f = &ego.focus;
    // counts: the two highest-priority labelled predicates, then observations
    let mut counts: Vec<(usize, &'static str, usize)> = Vec::new();
    for grp in &ego.groups {
        if let Some(label) = ego::count_label(&grp.key.predicate) {
            let rank = ego::predicate_rank(&grp.key.predicate);
            match counts.iter_mut().find(|c| c.1 == label) {
                Some(c) => c.2 += grp.total,
                None => counts.push((rank, label, grp.total)),
            }
        }
    }
    counts.sort();
    counts.truncate(2);
    let mut lines: Vec<Vec<Run>> = vec![
        vec![run(
            format!("{} {}", g.entity(&f.ty), f.ty.as_str()),
            t.muted(),
        )],
        vec![run(display_value(f, g), t.focus_text())],
        Vec::new(),
    ];
    for (_, label, n) in &counts {
        lines.push(vec![
            run(format!("{label:<6}"), t.muted()),
            run(n.to_string(), t.text()),
        ]);
    }
    lines.push(vec![
        run(format!("{:<6}", "obs"), t.muted()),
        run(ego.observations.to_string(), t.text()),
    ]);
    let status = if f.flags.contains(EntityFlags::SUBJECT) {
        Some(format!("{} subject", g.m(Marker::Seed)))
    } else if f.flags.contains(EntityFlags::SEED) {
        Some(format!("{} seed", g.m(Marker::Seed)))
    } else if f.flags.contains(EntityFlags::HUB) {
        Some(format!("{} hub", g.m(Marker::Hub)))
    } else if f.flags.contains(EntityFlags::OUT_OF_SCOPE) {
        Some(format!("{} out of scope", g.m(Marker::Blocked)))
    } else {
        None
    };
    if let Some(st) = status {
        lines.push(Vec::new());
        let tok = if f.flags.contains(EntityFlags::HUB) {
            t.warn()
        } else {
            t.muted()
        };
        lines.push(vec![run(st, tok)]);
    }
    let h = (lines.len() as u16 + 4).min(r.height);
    let y = r.y + r.height.saturating_sub(h) / 3;
    let rect = Rect::new(r.x, y, r.width, h);
    let on = w.ego_col == Col::Card;
    let focused = on && w.pane == Pane::Graph;
    draw::pane(buf, t, rect, true, &[], &[], t.base());
    if !focused {
        // The card is always drawn in accent; brighter when it has the cursor.
        let _ = focused;
    }
    if on {
        draw::bg(
            buf,
            Rect::new(rect.x + 1, rect.y + 1, rect.width - 2, rect.height - 2),
            t.cursor_row(),
        );
    }
    for (i, l) in lines.iter().enumerate() {
        draw::put(
            buf,
            rect.x + 2,
            rect.y + 2 + i as u16,
            rect.width.saturating_sub(4),
            l,
        );
    }
    hits.push((rect, Hit::Card));
}

fn attr_text(v: &AttrValue, g: GlyphSet) -> String {
    match v {
        AttrValue::Text(s) => s.clone(),
        AttrValue::Int(i) => i.to_string(),
        AttrValue::Bool(b) => {
            if *b {
                "yes".into()
            } else {
                "no".into()
            }
        }
        AttrValue::Time(ts) => ts.strftime("%Y-%m-%d %H:%M").to_string(),
        AttrValue::Date(d) => d.to_string(),
        AttrValue::Geo { lat, lon } => format!("{lat:.2}, {lon:.2}"),
        AttrValue::Url(u) => format!(
            "{} {}",
            g.m(Marker::Link),
            u.trim_start_matches("https://")
                .trim_start_matches("http://")
        ),
    }
}

fn section(buf: &mut Buffer, t: &Theme, g: GlyphSet, x: u16, y: u16, w: u16, title: &str) {
    let tw = text::width(title) as u16;
    draw::put(buf, x, y, w, &[run(title, t.muted())]);
    draw::hrule(
        buf,
        x + tw + 1,
        y,
        w.saturating_sub(tw + 1),
        g.m(Marker::Rule),
        t.faint(),
    );
}

fn inspector(
    buf: &mut Buffer,
    r: Rect,
    s: &AppState,
    w: &Workspace,
    hits: &mut Hits,
    overlay: bool,
) {
    let t = &s.theme;
    let g = s.glyphs;
    let focused = w.pane == Pane::Inspector;
    hits.push((r, Hit::Pane(Pane::Inspector)));
    let fill = if overlay { t.raised() } else { t.base() };
    draw::pane(
        buf,
        t,
        r,
        focused || overlay,
        &[run("Inspector", t.title(focused))],
        &[],
        fill,
    );
    let inner = layout::inner(r);
    let Some(d) = &w.detail else {
        draw::put(
            buf,
            inner.x,
            inner.y,
            inner.width,
            &[run("nothing under the cursor", t.muted())],
        );
        return;
    };
    let e = &d.entity.summary;
    let bottom = inner.bottom().saturating_sub(2);
    let mut y = inner.y;
    let line = |buf: &mut Buffer, y: &mut u16, runs: &[Run]| {
        if *y < bottom {
            draw::put(buf, inner.x, *y, inner.width, runs);
        }
        *y += 1;
    };
    line(
        buf,
        &mut y,
        &[
            run(format!("{} ", g.entity(&e.ty)), t.muted()),
            run(display_value(e, g), t.focus_text()),
        ],
    );
    let sub = if e.key != e.value {
        format!("{} {} {}", e.ty.as_str(), g.m(Marker::Dot), e.key)
    } else {
        e.ty.as_str().to_string()
    };
    line(buf, &mut y, &[run(sub, t.muted())]);
    y += 1;
    // fields
    let last_seen = w
        .status
        .as_ref()
        .and_then(|s| s.last_seen.as_deref())
        .and_then(|d| d.parse::<jiff::civil::Date>().ok());
    let module_of = |obs| {
        d.observations
            .iter()
            .find(|o| o.id == obs)
            .map(|o| o.module.clone())
    };
    let label_w = 14usize;
    let mut any_field = false;
    for (name, vals) in &d.entity.attrs {
        for (i, a) in vals.iter().enumerate() {
            let label = if i == 0 {
                name.replace('_', " ")
            } else {
                String::new()
            };
            let mut runs = vec![
                run(text::fit(&label, label_w), t.muted()),
                run(attr_text(&a.value, g), t.text()),
            ];
            if vals.len() > 1
                && let Some(m) = module_of(a.observation)
            {
                runs.push(run(format!("  {m}"), t.muted()));
            }
            line(buf, &mut y, &runs);
            any_field = true;
            let after = a.event
                && last_seen.is_some_and(|ls| {
                    a.value
                        .instant()
                        .is_some_and(|ts| ts.to_zoned(jiff::tz::TimeZone::UTC).date() > ls)
                });
            if after {
                line(
                    buf,
                    &mut y,
                    &[
                        run(" ".repeat(label_w), t.muted()),
                        run(
                            format!("{} after last-seen date", g.m(Marker::AfterLastSeen)),
                            t.warn(),
                        ),
                    ],
                );
            }
        }
    }
    if any_field {
        y += 1;
    }
    // evidence
    if y < bottom {
        section(
            buf,
            t,
            g,
            inner.x,
            y,
            inner.width,
            &format!("Evidence {}", d.observations.len()),
        );
    }
    y += 1;
    for (i, o) in d.observations.iter().enumerate() {
        if y >= bottom {
            break;
        }
        let rect = Rect::new(r.x + 1, y, r.width.saturating_sub(2), 1);
        let cursor = focused && i == w.insp_cursor;
        if cursor || s.hover == Some(Hit::InspRow(i)) {
            draw::bg(buf, rect, t.cursor_row());
        }
        if cursor {
            draw::put(
                buf,
                r.x + 1,
                y,
                1,
                &[run(g.m(Marker::Cursor), t.cursor_bar())],
            );
        }
        let hash = o
            .blob
            .as_deref()
            .map(|h| {
                format!(
                    "{}…{}",
                    &h[..4.min(h.len())],
                    &h[h.len().saturating_sub(2)..]
                )
            })
            .unwrap_or_default();
        draw::put(
            buf,
            inner.x,
            y,
            inner.width,
            &[
                run(text::fit(&o.module, 15), t.text()),
                run(format!("{} ", text::fit(&o.module_version, 8)), t.muted()),
                run(format!("{}  ", o.created_at.strftime("%H:%M")), t.muted()),
                run(hash, t.muted()),
            ],
        );
        hits.push((rect, Hit::InspRow(i)));
        y += 1;
    }
    y += 1;
    // strength
    if y < bottom {
        section(buf, t, g, inner.x, y, inner.width, "Strength");
    }
    y += 1;
    match d.strength {
        Some(st) => line(
            buf,
            &mut y,
            &[
                run(g.strength(st), t.fg(strength_token(st))),
                run(format!(" {}", st.as_str()), t.text()),
            ],
        ),
        None if e.flags.contains(EntityFlags::SEED) || e.flags.contains(EntityFlags::SUBJECT) => {
            line(
                buf,
                &mut y,
                &[run(
                    format!(
                        "{} seed {} the case starts here",
                        g.m(Marker::Seed),
                        g.m(Marker::Dot)
                    ),
                    t.muted(),
                )],
            );
        }
        None => line(buf, &mut y, &[run("not linked to a seed", t.muted())]),
    }
    let deg = &d.degree;
    line(
        buf,
        &mut y,
        &[run(
            format!(
                "{} edges {} in {} {} out {}",
                deg.total,
                g.m(Marker::Dot),
                deg.inbound,
                g.m(Marker::Dot),
                deg.outbound
            ),
            t.muted(),
        )],
    );
    if !d.findings.is_empty() {
        y += 1;
        if y < bottom {
            section(buf, t, g, inner.x, y, inner.width, "Used in");
        }
        y += 1;
        for f in &d.findings {
            line(
                buf,
                &mut y,
                &[
                    run(format!("F-{}  ", f.id), t.text()),
                    run(format!("{}  ", f.title), t.text()),
                    run(f.status.clone(), t.muted()),
                ],
            );
        }
    }
    if !d.notes.is_empty() {
        y += 1;
        if y < bottom {
            section(
                buf,
                t,
                g,
                inner.x,
                y,
                inner.width,
                &format!("Notes {}", d.notes.len()),
            );
        }
        y += 1;
        for n in &d.notes {
            line(
                buf,
                &mut y,
                &[
                    run(format!("{}  ", n.created_at.strftime("%H:%M")), t.muted()),
                    run(n.body.clone(), t.text()),
                ],
            );
        }
    }
    // footer hints
    let mut hints = Vec::new();
    for a in [Action::OpenEvidence, Action::Copy, Action::NewFinding] {
        if let Some(h) = s.keymap.hint_for(a, Ctx::Entity) {
            hints.push(h);
        }
    }
    draw::put(
        buf,
        inner.x,
        inner.bottom().saturating_sub(1),
        inner.width,
        &super::hint_runs(s, &hints),
    );
}

fn activity(buf: &mut Buffer, r: Rect, s: &AppState, w: &Workspace, hits: &mut Hits) {
    let t = &s.theme;
    let g = s.glyphs;
    draw::pane(
        buf,
        t,
        r,
        false,
        &[run("Activity", t.title(false))],
        &[],
        t.base(),
    );
    let inner = layout::inner(r);
    hits.push((inner, Hit::Activity));
    let side_w = 30u16;
    let log_w = inner.width.saturating_sub(side_w + 3);
    let rule_x = inner.x + log_w + 1;
    for y in inner.y..inner.bottom() {
        draw::put(
            buf,
            rule_x,
            y,
            1,
            &[run(g.m(Marker::VRule), t.fg(Token::Border))],
        );
    }
    if w.activity.is_empty() {
        draw::put(
            buf,
            inner.x,
            inner.y,
            log_w,
            &[run("no activity yet", t.muted())],
        );
    }
    for (i, a) in w
        .activity
        .iter()
        .skip(w.act_scroll)
        .take(inner.height as usize)
        .enumerate()
    {
        let mut runs = vec![
            run(a.at.strftime("%H:%M:%S").to_string(), t.muted()),
            run(" ", t.text()),
            run(g.m(a.marker), t.fg(a.token)),
            run(" ", t.text()),
            run(text::fit(&a.module, 15), t.fg(a.module_token)),
            run(" ", t.text()),
        ];
        runs.extend(a.spans.iter().map(|(s, tok)| run(s.clone(), t.fg(*tok))));
        draw::put(buf, inner.x, inner.y + i as u16, log_w, &runs);
    }
    // right column
    let x = rule_x + 2;
    let cw = inner.right().saturating_sub(x);
    let pair = |a: &str, n: u64, b: &str, m: u64, mtok: Token| -> Vec<Run> {
        vec![
            run(format!("{a:<8}"), t.muted()),
            run(format!("{n:>3}"), t.text()),
            run(format!("   {b:<7}"), t.muted()),
            run(m.to_string(), t.fg(mtok)),
        ]
    };
    draw::put(
        buf,
        x,
        inner.y,
        cw,
        &pair("running", w.running, "queued", w.queued, Token::Fg),
    );
    let ftok = if w.failed > 0 {
        Token::Hostile
    } else {
        Token::Fg
    };
    draw::put(
        buf,
        x,
        inner.y + 1,
        cw,
        &pair("done", w.done, "failed", w.failed, ftok),
    );
    let mut y = inner.y + 2;
    if let Some(st) = &w.status {
        draw::put(
            buf,
            x,
            y,
            cw,
            &[
                run(format!("{:<9}", "rules"), t.muted()),
                run(st.profile.clone(), t.text()),
            ],
        );
        y += 1;
        for l in super::wrap(&st.profile_summary, cw.saturating_sub(9) as usize)
            .into_iter()
            .take(2)
        {
            draw::put(buf, x + 9, y, cw.saturating_sub(9), &[run(l, t.muted())]);
            y += 1;
        }
    }
    let blocked = w.modules.iter().filter(|m| m.blocked.is_some()).count();
    if blocked > 0 && y < inner.bottom() {
        draw::put(
            buf,
            x,
            y,
            cw,
            &[
                run(format!("{:<9}", "blocked"), t.muted()),
                run(
                    format!("{blocked} module{}", if blocked == 1 { "" } else { "s" }),
                    t.text(),
                ),
            ],
        );
    }
}

/// For tests: the visible entity rows.
#[allow(dead_code)]
pub fn visible_values(w: &Workspace) -> Vec<&EntitySummary> {
    w.ent_items
        .iter()
        .filter_map(|i| match i {
            EntItem::Row(i) => w.entities.get(*i),
            _ => None,
        })
        .collect()
}
