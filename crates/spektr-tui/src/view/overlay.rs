//! Overlays: command palette (artboard 03), context menu and submenu
//! (artboard 07), help, dialogs (TUI_SPEC §5, §7).

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use super::draw::{self, Run, run};
use super::{Hits, caret, hint_runs};
use crate::glyph::{Marker, display_value};
use crate::keymap::Ctx;
use crate::layout;
use crate::state::{AppState, Dialog, Field, Help, Hit, Menu, MenuItem, Palette, Section};
use crate::text;

/// Word-wrap to `w` cells.
pub fn wrap(s: &str, w: usize) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    for word in s.split_whitespace() {
        if !cur.is_empty() && text::width(&cur) + 1 + text::width(word) > w {
            out.push(std::mem::take(&mut cur));
        }
        if !cur.is_empty() {
            cur.push(' ');
        }
        cur.push_str(word);
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

pub fn palette(buf: &mut Buffer, area: Rect, s: &AppState, p: &Palette, hits: &mut Hits) {
    let t = &s.theme;
    let g = s.glyphs;
    let r = layout::palette(area.width, area.height);
    draw::shadow(buf, r, t);
    draw::pane(
        buf,
        t,
        r,
        true,
        &[run("Command", t.title(true))],
        &[],
        t.raised(),
    );
    hits.push((r, Hit::Overlay));
    let inner = layout::inner(r);
    let raised = |st: ratatui::style::Style| st.bg(t.color(crate::theme::Token::BgRaised));
    // prompt
    let mut prompt = vec![
        run(": ", raised(t.accent())),
        run(p.input.clone(), raised(t.text())),
    ];
    prompt.push(caret(s));
    if let Some(tg) = &p.target {
        let target = vec![
            run("target ", raised(t.muted())),
            run(format!("{} ", g.entity(&tg.ty)), raised(t.muted())),
            run(display_value(tg, g), raised(t.text())),
        ];
        let tw = draw::width(&target).min(inner.width / 2);
        draw::put_right(buf, inner.right(), inner.y, tw, &target);
        draw::put(
            buf,
            inner.x,
            inner.y,
            inner.width.saturating_sub(tw + 2),
            &prompt,
        );
    } else {
        draw::put(buf, inner.x, inner.y, inner.width, &prompt);
    }
    draw::hrule(
        buf,
        inner.x,
        inner.y + 1,
        inner.width,
        g.m(Marker::Rule),
        raised(t.fg(crate::theme::Token::Border)),
    );

    // detail + footer take the bottom 7 rows
    let footer_y = inner.bottom().saturating_sub(1);
    let detail_y = footer_y.saturating_sub(6);
    let list_y = inner.y + 3;
    let list_h = detail_y.saturating_sub(list_y + 1) as usize;
    // grouped rows: (section header | item index)
    let mut rows: Vec<Result<usize, Option<Section>>> = Vec::new();
    let mut last: Option<Section> = None;
    for (i, it) in p.items.iter().enumerate() {
        if last != Some(it.section) {
            if last.is_some() {
                rows.push(Err(None));
            }
            rows.push(Err(Some(it.section)));
            last = Some(it.section);
        }
        rows.push(Ok(i));
    }
    let cursor_row = rows.iter().position(|r| *r == Ok(p.sel)).unwrap_or(0);
    let start = if rows.len() <= list_h {
        0
    } else {
        cursor_row
            .saturating_sub(list_h.saturating_sub(1))
            .min(rows.len() - list_h)
    };
    let sig_x = inner.x + 2 + 24;
    let info_x = sig_x + 18;
    for (row, entry) in rows.iter().enumerate().skip(start).take(list_h) {
        let y = list_y + (row - start) as u16;
        match entry {
            Err(None) => {}
            Err(Some(sec)) => {
                draw::put(
                    buf,
                    inner.x + 2,
                    y,
                    inner.width,
                    &[run(sec.title(), raised(t.muted()))],
                );
            }
            Ok(i) => {
                let it = &p.items[*i];
                let rect = Rect::new(inner.x, y, inner.width, 1);
                let sel = *i == p.sel;
                if sel || s.hover == Some(Hit::PaletteItem(*i)) {
                    draw::bg(buf, rect, t.cursor_row());
                }
                if sel {
                    draw::put(
                        buf,
                        inner.x,
                        y,
                        1,
                        &[run(g.m(Marker::Cursor), t.cursor_bar())],
                    );
                }
                let tok = if it.disabled.is_some() {
                    t.muted()
                } else {
                    t.text()
                };
                draw::put(
                    buf,
                    inner.x + 2,
                    y,
                    23,
                    &[run(it.line.trim_end().to_string(), tok)],
                );
                draw::put(buf, sig_x, y, 17, &[run(it.signature.clone(), t.muted())]);
                draw::put(
                    buf,
                    info_x,
                    y,
                    inner.right().saturating_sub(info_x),
                    &[run(it.info.clone(), t.muted())],
                );
                hits.push((rect, Hit::PaletteItem(*i)));
            }
        }
    }
    if p.items.is_empty() {
        let msg = if p.input.trim().is_empty() {
            "type a command"
        } else {
            "no match · ↵ runs what you typed"
        };
        draw::put(
            buf,
            inner.x + 2,
            list_y,
            inner.width,
            &[run(msg, raised(t.muted()))],
        );
    }
    // detail
    draw::hrule(
        buf,
        inner.x,
        detail_y,
        inner.width,
        g.m(Marker::Rule),
        raised(t.fg(crate::theme::Token::Border)),
    );
    if let Some(e) = &p.error {
        draw::put(
            buf,
            inner.x,
            detail_y + 1,
            inner.width,
            &[run(e.clone(), raised(t.hostile()))],
        );
    } else if let Some(it) = p.items.get(p.sel) {
        let (title, sub, body) = &it.detail;
        let mut head = vec![run(title.clone(), raised(t.focus_text()))];
        if !sub.is_empty() {
            head.push(run(format!(" {sub}"), raised(t.muted())));
        }
        draw::put(buf, inner.x, detail_y + 1, inner.width, &head);
        for (i, l) in wrap(body, inner.width as usize)
            .into_iter()
            .take(3)
            .enumerate()
        {
            draw::put(
                buf,
                inner.x,
                detail_y + 2 + i as u16,
                inner.width,
                &[run(l, raised(t.muted()))],
            );
        }
    }
    let hints = s.keymap.hints(&[Ctx::Palette]);
    let count = format!("{} of {}", p.items.len(), p.total);
    draw::put(
        buf,
        inner.x,
        footer_y,
        inner.width.saturating_sub(12),
        &hint_runs(s, &hints)
            .into_iter()
            .map(|(x, st)| (x, raised(st)))
            .collect::<Vec<_>>(),
    );
    draw::put_right(
        buf,
        inner.right(),
        footer_y,
        text::width(&count) as u16,
        &[run(count, raised(t.muted()))],
    );
}

#[allow(clippy::too_many_arguments)]
fn menu_box(
    buf: &mut Buffer,
    s: &AppState,
    rect: Rect,
    header: &[Vec<Run>],
    items: &[MenuItem],
    sel: usize,
    active: bool,
    hit: fn(usize) -> Hit,
    hits: &mut Hits,
) {
    let t = &s.theme;
    let g = s.glyphs;
    let raised = |st: ratatui::style::Style| st.bg(t.color(crate::theme::Token::BgRaised));
    draw::shadow(buf, rect, t);
    draw::pane(buf, t, rect, false, &[], &[], t.raised());
    hits.push((rect, Hit::Overlay));
    let x = rect.x + 2;
    let w = rect.width.saturating_sub(4);
    let mut y = rect.y + 1;
    for h in header {
        draw::put(
            buf,
            x,
            y,
            w,
            &h.iter()
                .map(|(s, st)| (s.clone(), raised(*st)))
                .collect::<Vec<_>>(),
        );
        y += 1;
    }
    if !header.is_empty() {
        draw::hrule(
            buf,
            rect.x + 1,
            y,
            rect.width - 2,
            g.m(Marker::Rule),
            raised(t.fg(crate::theme::Token::Border)),
        );
        y += 1;
    }
    for (i, it) in items.iter().enumerate() {
        if it.rule && i > 0 {
            draw::hrule(
                buf,
                rect.x + 1,
                y,
                rect.width - 2,
                g.m(Marker::Rule),
                raised(t.fg(crate::theme::Token::Border)),
            );
            y += 1;
        }
        let row = Rect::new(rect.x + 1, y, rect.width - 2, 1);
        let is_sel = active && i == sel || !active && i == sel;
        if is_sel || s.hover == Some(hit(i)) {
            draw::bg(buf, row, t.cursor_row());
        }
        if is_sel && active {
            draw::put(
                buf,
                rect.x + 1,
                y,
                1,
                &[run(g.m(Marker::Cursor), t.cursor_bar())],
            );
        }
        let tok = if it.disabled.is_some() {
            t.muted()
        } else {
            t.text()
        };
        let rw = text::width(&it.right) as u16;
        draw::put(
            buf,
            x,
            y,
            w.saturating_sub(rw + 1),
            &[run(it.label.clone(), tok)],
        );
        draw::put_right(buf, x + w, y, rw, &[run(it.right.clone(), t.muted())]);
        hits.push((row, hit(i)));
        y += 1;
    }
}

fn menu_height(header: usize, items: &[MenuItem]) -> u16 {
    let rules = items.iter().skip(1).filter(|i| i.rule).count();
    (header + usize::from(header > 0) + items.len() + rules + 2) as u16
}

fn menu_width(header: &[Vec<Run>], items: &[MenuItem]) -> u16 {
    let h = header.iter().map(|l| draw::width(l)).max().unwrap_or(0);
    let i = items
        .iter()
        .map(|i| text::width(&i.label) as u16 + text::width(&i.right) as u16 + 4)
        .max()
        .unwrap_or(0);
    h.max(i).max(24) + 4
}

pub fn menu(buf: &mut Buffer, area: Rect, s: &AppState, m: &Menu, hits: &mut Hits) {
    let t = &s.theme;
    let g = s.glyphs;
    let header = vec![
        vec![
            run(format!("{} ", g.entity(&m.target.ty)), t.muted()),
            run(display_value(&m.target, g), t.focus_text()),
        ],
        vec![run(m.subtitle.clone(), t.muted())],
    ];
    let w = menu_width(&header, &m.items).min(area.width);
    let h = menu_height(header.len(), &m.items).min(area.height.saturating_sub(1));
    let x = (m.anchor.0 + 1).min(area.right().saturating_sub(w + 1));
    let y = (m.anchor.1 + 1).min(area.bottom().saturating_sub(h + 1));
    let rect = Rect::new(x, y, w, h);
    menu_box(
        buf,
        s,
        rect,
        &header,
        &m.items,
        m.sel,
        m.sub.is_none(),
        Hit::MenuItem,
        hits,
    );
    if let Some(sub) = &m.sub {
        let sh_header = vec![vec![run(sub.title.clone(), t.muted())]];
        let sw = menu_width(&sh_header, &sub.items).min(area.width);
        let shh = menu_height(1, &sub.items).min(area.height.saturating_sub(1));
        // the parent item's row
        let rules_before = m
            .items
            .iter()
            .take(m.sel + 1)
            .skip(1)
            .filter(|i| i.rule)
            .count() as u16;
        let row_y = rect.y + 1 + header.len() as u16 + 1 + m.sel as u16 + rules_before;
        // Right of the menu, leaving its shadow column; flip left if needed.
        let sx = if rect.right() + 1 + sw <= area.right() {
            rect.right() + 1
        } else {
            rect.x.saturating_sub(sw + 1)
        };
        let sy = row_y
            .saturating_sub(1)
            .min(area.bottom().saturating_sub(shh + 1));
        menu_box(
            buf,
            s,
            Rect::new(sx, sy, sw, shh),
            &sh_header,
            &sub.items,
            sub.sel,
            true,
            Hit::SubItem,
            hits,
        );
    }
}

pub fn help(buf: &mut Buffer, area: Rect, s: &AppState, h: &Help, hits: &mut Hits) {
    let t = &s.theme;
    let raised = |st: ratatui::style::Style| st.bg(t.color(crate::theme::Token::BgRaised));
    let r = Rect::new(area.x, area.y, area.width, area.height.saturating_sub(1));
    draw::pane(
        buf,
        t,
        r,
        true,
        &[run("Help", t.title(true))],
        &[],
        t.raised(),
    );
    hits.push((r, Hit::Overlay));
    let inner = layout::inner(r);
    let mut search = vec![run("/ ", raised(t.accent()))];
    if h.search.is_empty() && !h.searching {
        search = vec![run("/ search", raised(t.muted()))];
    } else {
        search.push(run(h.search.clone(), raised(t.text())));
        if h.searching {
            search.push(caret(s));
        }
    }
    draw::put(buf, inner.x, inner.y, inner.width, &search);
    if s.mouse {
        draw::put_right(
            buf,
            inner.right(),
            inner.y,
            inner.width / 2,
            &[run(
                "mouse capture is on: shift-drag selects text in most terminals",
                raised(t.muted()),
            )],
        );
    }
    let q = h.search.to_lowercase();
    let mut lines: Vec<Vec<Run>> = Vec::new();
    for (ctx, rows) in s.keymap.help() {
        let rows: Vec<_> = rows
            .into_iter()
            .filter(|(k, d)| {
                q.is_empty() || k.to_lowercase().contains(&q) || d.to_lowercase().contains(&q)
            })
            .collect();
        if rows.is_empty() {
            continue;
        }
        lines.push(vec![run(ctx.label(), raised(t.accent()))]);
        for (k, d) in rows {
            lines.push(vec![
                run(format!("  {}", text::fit(&k, 16)), raised(t.text())),
                run(d, raised(t.muted())),
            ]);
        }
        lines.push(Vec::new());
    }
    let body_y = inner.y + 2;
    let body_h = inner.height.saturating_sub(2) as usize;
    let max_scroll = lines.len().saturating_sub(body_h);
    let scroll = h.scroll.min(max_scroll);
    for (i, l) in lines.iter().skip(scroll).take(body_h).enumerate() {
        draw::put(buf, inner.x, body_y + i as u16, inner.width, l);
    }
}

pub fn dialog(buf: &mut Buffer, area: Rect, s: &AppState, d: &Dialog, hits: &mut Hits) {
    let t = &s.theme;
    let g = s.glyphs;
    let raised = |st: ratatui::style::Style| st.bg(t.color(crate::theme::Token::BgRaised));
    let w = 64.min(area.width.saturating_sub(4));
    let iw = w.saturating_sub(4) as usize;
    let body: Vec<String> = d.body.iter().flat_map(|b| wrap(b, iw)).collect();
    let picks = d.picks.len().min(8);
    let h = (body.len()
        + usize::from(!body.is_empty())
        + d.fields.len()
        + picks
        + usize::from(picks > 0)
        + 2
        + 2
        + 1) as u16;
    let r = layout::centered(area, w, h.min(area.height.saturating_sub(2)));
    draw::shadow(buf, r, t);
    draw::pane(
        buf,
        t,
        r,
        true,
        &[run(d.title.clone(), t.title(true))],
        &[],
        t.raised(),
    );
    hits.push((r, Hit::Overlay));
    let inner = layout::inner(r);
    let mut y = inner.y;
    for l in &body {
        draw::put(
            buf,
            inner.x,
            y,
            inner.width,
            &[run(l.clone(), raised(t.text()))],
        );
        y += 1;
    }
    if !body.is_empty() {
        y += 1;
    }
    for (i, f) in d.fields.iter().enumerate() {
        let focus = i == d.focus;
        let mut runs: Vec<Run> = vec![run(
            text::fit(f.label(), 11),
            raised(if focus { t.accent() } else { t.muted() }),
        )];
        match f {
            Field::Text { value, hint, .. } => {
                runs.push(run(value.clone(), raised(t.text())));
                if focus {
                    runs.push(caret(s));
                }
                if value.is_empty() {
                    runs.push(run(format!(" {hint}"), raised(t.muted())));
                }
            }
            Field::Choice { options, idx, .. } => {
                let v = options.get(*idx).cloned().unwrap_or_default();
                runs.push(run(
                    format!("‹ {v} ›"),
                    raised(if focus { t.text() } else { t.muted() }),
                ));
                if focus {
                    runs.push(run("  ← → to change", raised(t.muted())));
                }
            }
        }
        draw::put(buf, inner.x, y, inner.width, &runs);
        hits.push((Rect::new(inner.x, y, inner.width, 1), Hit::DialogItem(i)));
        y += 1;
    }
    if picks > 0 {
        y += 1;
        let start = d.pick_sel.saturating_sub(picks - 1);
        for (row, (i, e)) in d
            .picks
            .iter()
            .enumerate()
            .skip(start)
            .take(picks)
            .enumerate()
        {
            let yy = y + row as u16;
            let rect = Rect::new(r.x + 1, yy, r.width - 2, 1);
            if i == d.pick_sel {
                draw::bg(buf, rect, t.cursor_row());
                draw::put(
                    buf,
                    r.x + 1,
                    yy,
                    1,
                    &[run(g.m(Marker::Cursor), t.cursor_bar())],
                );
            }
            draw::put(
                buf,
                inner.x,
                yy,
                inner.width,
                &[
                    run(format!("{} ", g.entity(&e.ty)), t.muted()),
                    run(display_value(e, g), t.text()),
                ],
            );
            hits.push((rect, Hit::DialogItem(i)));
        }
        y += picks as u16;
    }
    y += 1;
    if let Some(e) = &d.error {
        draw::put(
            buf,
            inner.x,
            y,
            inner.width,
            &[run(e.clone(), raised(t.hostile()))],
        );
    }
    let mut hints = s.keymap.hints(&[Ctx::Dialog]);
    if d.destructive {
        hints.retain(|(k, _)| k != "↵");
        if let Some(h) = s.keymap.hint_for(crate::keymap::Action::Yes, Ctx::Dialog) {
            hints.insert(0, (h.0, "yes"));
        }
    }
    if d.fields.len() < 2 {
        hints.retain(|(_, h)| *h != "field");
    }
    let runs: Vec<Run> = hint_runs(s, &hints)
        .into_iter()
        .map(|(x, st)| (x, raised(st)))
        .collect();
    draw::put(
        buf,
        inner.x,
        inner.bottom().saturating_sub(1),
        inner.width,
        &runs,
    );
}

/// The inline finding editor (`e`, mode INSERT): title, claim and relevance
/// as wrapped text areas in a raised box.
pub fn editor(
    buf: &mut Buffer,
    area: Rect,
    s: &AppState,
    ed: &crate::state::Editor,
    hits: &mut Hits,
) {
    use ratatui::widgets::Widget;
    let t = &s.theme;
    let raised = |st: ratatui::style::Style| st.bg(t.color(crate::theme::Token::BgRaised));
    let r = layout::editor(area.width, area.height);
    draw::shadow(buf, r, t);
    draw::pane(
        buf,
        t,
        r,
        true,
        &[run(format!("Edit F-{}", ed.id), t.title(true))],
        &[],
        t.raised(),
    );
    hits.push((r, Hit::Overlay));
    let inner = layout::inner(r);
    // title 1 row, claim 4, relevance the rest
    let avail = inner.height.saturating_sub(2 + 3 + 3);
    let heights = [1u16, 4, avail.saturating_sub(5).max(2)];
    let mut y = inner.y;
    for (i, f) in ed.fields.iter().enumerate() {
        let focus = i == ed.focus;
        draw::put(
            buf,
            inner.x,
            y,
            inner.width,
            &[run(
                crate::state::Editor::LABELS[i],
                raised(if focus { t.accent() } else { t.muted() }),
            )],
        );
        y += 1;
        let h = heights[i].min(inner.bottom().saturating_sub(y + 2));
        let rect = Rect::new(inner.x, y, inner.width, h);
        let mut ta = f.clone();
        ta.set_style(raised(t.text()));
        ta.set_cursor_line_style(raised(t.text()));
        ta.set_cursor_style(if focus { t.caret() } else { raised(t.text()) });
        (&ta).render(rect, buf);
        hits.push((
            Rect::new(inner.x, y - 1, inner.width, h + 1),
            Hit::EditorField(i),
        ));
        y += h + 1;
    }
    let hints = s.keymap.hints(&[crate::keymap::Ctx::Dialog]);
    let mut runs = super::hint_runs(s, &hints);
    for r in &mut runs {
        r.1 = raised(r.1);
    }
    draw::put(
        buf,
        inner.x,
        inner.bottom().saturating_sub(1),
        inner.width,
        &runs,
    );
}
