//! The Checklist screen (artboard 10): playbook runs, the selected run's
//! steps and the available playbooks on the left; checklist items with
//! filter tabs and the selected item's detail on the right.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Modifier;
use spektr_engine::{ItemView, RunView, StepView};
use spektr_model::{CoreType, EntityType};

use super::Hits;
use super::draw::{self, Run, run};
use crate::glyph::{GlyphSet, Marker, display_value};
use crate::keymap::{Action, Ctx};
use crate::layout;
use crate::state::{AppState, ChecklistPane, Hit, ItemFilter, Workspace};
use crate::text;
use crate::theme::{Theme, Token};

pub fn render(buf: &mut Buffer, area: Rect, s: &AppState, w: &Workspace, hits: &mut Hits) {
    super::workspace::top_bar(
        buf,
        Rect::new(area.x, area.y, area.width, 1),
        s,
        w,
        Some("playbooks"),
    );
    let (left, right) = layout::checklist(area.width, area.height);
    runs_pane(buf, left, s, w, hits);
    items_pane(buf, right, s, w, hits);
}

fn section(buf: &mut Buffer, t: &Theme, g: GlyphSet, x: u16, y: u16, w: u16, title: &[Run]) {
    let tw = draw::put(buf, x, y, w, title);
    draw::hrule(
        buf,
        x + tw + 1,
        y,
        w.saturating_sub(tw + 1),
        g.m(Marker::Rule),
        t.faint(),
    );
}

/// `▸` running (accent), `✓` done, `⊘` stopped, `✕` failed.
fn run_mark(state: &str, g: GlyphSet) -> (&'static str, Token) {
    match state {
        "running" => (g.m(Marker::Running), Token::Accent),
        "done" => (g.m(Marker::Ok), Token::Ok),
        "failed" => (g.m(Marker::Failed), Token::Hostile),
        _ => (g.m(Marker::Blocked), Token::FgMuted),
    }
}

fn step_mark(state: &str, g: GlyphSet) -> (&'static str, Token) {
    match state {
        "done" => (g.m(Marker::Ok), Token::Ok),
        "running" => (g.m(Marker::Running), Token::Accent),
        "stopped" => (g.m(Marker::Blocked), Token::FgMuted),
        _ => (g.m(Marker::Pending), Token::FgMuted),
    }
}

fn type_glyph(ty: &str, g: GlyphSet) -> char {
    match CoreType::parse(ty) {
        Some(c) => g.entity(&EntityType::from(c)),
        None => '·',
    }
}

/// `on seed`, `on 9 ◐ account`, `on ▨ image`, `—`.
fn step_on(st: &StepView, g: GlyphSet) -> String {
    let on = st.info.on.as_str();
    if on == "—" || on.is_empty() {
        return "—".into();
    }
    let first = on.split([' ', ',']).next().unwrap_or(on);
    if first == "seed" {
        return "on seed".into();
    }
    let glyph = type_glyph(first, g);
    if st.selected > 0 {
        format!("on {} {glyph} {first}", st.selected)
    } else {
        format!("on {glyph} {first}")
    }
}

/// A step's result: job counts, items, nested runs.
fn step_result(st: &StepView) -> (String, Token) {
    let r = &st.result;
    if let Some(jobs) = r.get("jobs").and_then(|j| j.as_object()) {
        let n = |k: &str| jobs.get(k).and_then(|v| v.as_u64()).unwrap_or(0);
        let finished = n("done") + n("cached");
        let total: u64 = jobs.values().filter_map(|v| v.as_u64()).sum();
        let mut out = if st.state == "running" {
            format!("{finished}/{total} jobs")
        } else {
            format!("{total} job{}", if total == 1 { "" } else { "s" })
        };
        if n("failed") > 0 {
            out.push_str(&format!(" · {} failed", n("failed")));
            return (out, Token::Hostile);
        }
        if n("blocked") > 0 {
            out.push_str(&format!(" · {} blocked", n("blocked")));
        }
        return (out, Token::FgMuted);
    }
    if let Some(n) = r.get("items").and_then(|v| v.as_u64()) {
        return (
            format!("{n} item{}", if n == 1 { "" } else { "s" }),
            Token::FgMuted,
        );
    }
    if let Some(c) = r.get("children").and_then(|v| v.as_array()) {
        return (
            format!("{} run{}", c.len(), if c.len() == 1 { "" } else { "s" }),
            Token::FgMuted,
        );
    }
    if st.capped > 0 {
        return (format!("+{} over the cap", st.capped), Token::Warn);
    }
    (String::new(), Token::FgMuted)
}

/// `run username-sweep`, `check reverse image search`, `playbook …`.
fn step_text(st: &StepView) -> String {
    match st.info.kind.as_str() {
        "check" => format!("check {}", st.info.title.to_lowercase()),
        _ => st.info.detail.clone(),
    }
}

fn run_status(r: &RunView) -> (String, Token) {
    match r.state.as_str() {
        "running" => (
            format!("step {}/{}", r.current_step.max(1), r.total_steps),
            Token::Accent,
        ),
        "done" => (
            format!("done {}/{}", r.total_steps, r.total_steps),
            Token::FgMuted,
        ),
        other => (
            format!("{other} {}/{}", r.current_step, r.total_steps),
            Token::FgMuted,
        ),
    }
}

fn runs_pane(buf: &mut Buffer, r: Rect, s: &AppState, w: &Workspace, hits: &mut Hits) {
    let t = &s.theme;
    let g = s.glyphs;
    let c = &w.checklist;
    let focused = c.pane == ChecklistPane::Runs;
    let n = c.runs.len();
    draw::pane(
        buf,
        t,
        r,
        focused,
        &[
            run("Playbooks", t.title(focused)),
            run(
                format!(" {n} run{}", if n == 1 { "" } else { "s" }),
                t.text(),
            ),
        ],
        &[],
        t.base(),
    );
    let inner = layout::inner(r);
    let bottom = inner.bottom();
    let mut y = inner.y;
    let name_w = 22usize;
    let seed_w = (inner.width as usize)
        .saturating_sub(name_w + 16)
        .clamp(12, 30);
    if c.runs.is_empty() {
        let msg = if c.loaded {
            "no runs yet · p on an entity runs a playbook"
        } else {
            "loading…"
        };
        draw::put(buf, inner.x + 2, y, inner.width, &[run(msg, t.muted())]);
        y += 1;
    }
    // runs: at most a third of the pane
    let max_runs = (inner.height / 3).max(3) as usize;
    let start = c.run_sel.saturating_sub(max_runs.saturating_sub(1));
    for (i, rv) in c.runs.iter().enumerate().skip(start).take(max_runs) {
        if y >= bottom {
            break;
        }
        let row = Rect::new(r.x + 1, y, r.width.saturating_sub(2), 1);
        let cursor = i == c.run_sel;
        if cursor {
            draw::bg(
                buf,
                row,
                if focused {
                    t.cursor_row()
                } else {
                    t.hover_row()
                },
            );
            if focused {
                draw::put(
                    buf,
                    inner.x,
                    y,
                    1,
                    &[run(g.m(Marker::Cursor), t.cursor_bar())],
                );
            }
        } else if s.hover == Some(Hit::RunRow(i)) {
            draw::bg(buf, row, t.hover_row());
        }
        let indent = if rv.parent.is_some() { 2 } else { 0 };
        let (m, mt) = run_mark(&rv.state, g);
        let seed = rv
            .seed
            .as_ref()
            .map(|e| format!("{} {}", g.entity(&e.ty), display_value(e, g)))
            .unwrap_or_default();
        let (status, st) = run_status(rv);
        let name = text::fit(&rv.playbook, name_w.saturating_sub(indent));
        draw::put(
            buf,
            inner.x + 1 + indent as u16,
            y,
            inner.width.saturating_sub(1),
            &[
                run(format!("{m} "), t.fg(mt)),
                run(name, t.text()),
                run(" ", t.text()),
                run(text::fit(&seed, seed_w), t.text()),
                run(" ", t.text()),
                run(status, t.fg(st)),
            ],
        );
        hits.push((row, Hit::RunRow(i)));
        y += 1;
    }
    // steps of the selected run
    if let Some(rv) = c.selected_run() {
        y += 1;
        if y < bottom {
            section(
                buf,
                t,
                g,
                inner.x,
                y,
                inner.width,
                &[run(
                    format!("Steps {} {}", g.m(Marker::Dot), rv.playbook),
                    t.muted(),
                )],
            );
        }
        y += 1;
        let text_w = 34usize
            .min((inner.width as usize).saturating_sub(34))
            .max(16);
        let on_w = 16usize;
        for st in &rv.steps {
            if y >= bottom {
                break;
            }
            let (m, mt) = step_mark(&st.state, g);
            let lines = super::wrap(&step_text(st), text_w);
            let (res, rt) = step_result(st);
            let on = step_on(st, g);
            let on_tok = if st.selected > 0 || on == "on seed" {
                Token::Fg
            } else {
                Token::FgMuted
            };
            draw::put(
                buf,
                inner.x,
                y,
                inner.width,
                &[
                    run(format!("{m} "), t.fg(mt)),
                    run(format!("{:<2} ", st.step), t.text()),
                    run(
                        text::fit(lines.first().map_or("", String::as_str), text_w),
                        t.text(),
                    ),
                    run(" ", t.text()),
                    run(text::fit(&on, on_w), t.fg(on_tok)),
                    run(" ", t.text()),
                    run(res, t.fg(rt)),
                ],
            );
            y += 1;
            for l in lines.iter().skip(1).take(2) {
                if y >= bottom {
                    break;
                }
                draw::put(
                    buf,
                    inner.x + 5,
                    y,
                    text_w as u16 + 2,
                    &[run(format!("  {l}"), t.muted())],
                );
                y += 1;
            }
        }
    }
    // available playbooks
    y += 1;
    let note_h = 3u16;
    if y + 2 < bottom.saturating_sub(note_h) {
        section(
            buf,
            t,
            g,
            inner.x,
            y,
            inner.width,
            &[run("Available", t.muted())],
        );
        y += 1;
        for p in &c.playbooks {
            if y >= bottom.saturating_sub(note_h) {
                break;
            }
            let seed = p
                .seed
                .iter()
                .map(|ty| format!("{} {ty}", type_glyph(ty, g)))
                .collect::<Vec<_>>()
                .join(", ");
            let mut src = if p.source == "built-in" {
                "built-in".to_string()
            } else {
                let path = std::path::Path::new(&p.source);
                text::tilde(path.parent().unwrap_or(path), s.settings.home.as_deref())
            };
            if !p.requires_profile.is_empty() {
                src.push_str(&format!(" {} {}", g.m(Marker::Dot), p.requires_profile));
            }
            let mut runs = vec![
                run(format!("  {}", text::fit(&p.name, name_w)), t.text()),
                run(" ", t.text()),
                run(text::fit(&seed, 14), t.muted()),
                run(" ", t.text()),
                run(src, t.muted()),
            ];
            if !p.errors.is_empty() {
                runs.push(run(
                    format!("  {} {} problems", g.m(Marker::Failed), p.errors.len()),
                    t.hostile(),
                ));
            }
            draw::put(buf, inner.x, y, inner.width, &runs);
            y += 1;
        }
    }
    // rules note
    let profile = w.profile();
    let note = if profile == "tracelabs" {
        "Rules still apply inside a playbook: the tracelabs profile blocks anything that isn't passive.".to_string()
    } else {
        format!("Rules still apply inside a playbook: every step runs under the {profile} profile.")
    };
    let lines = super::wrap(&note, inner.width as usize);
    let ny = bottom.saturating_sub(lines.len() as u16);
    for (i, l) in lines.iter().enumerate() {
        draw::put(
            buf,
            inner.x,
            ny + i as u16,
            inner.width,
            &[run(l.clone(), t.muted())],
        );
    }
}

/// One line of the items list.
enum Line<'a> {
    Header(&'a ItemView),
    Item(usize, &'a ItemView),
    Blank,
}

fn launcher_short(id: &str) -> &str {
    id.split('-').next().unwrap_or(id)
}

fn item_right(i: &ItemView, g: GlyphSet) -> (String, Token) {
    let dot = g.m(Marker::Dot);
    match i.state.as_str() {
        "done" => {
            let at = i
                .resolved_at
                .map(|t| t.strftime("%H:%M").to_string())
                .unwrap_or_default();
            let note = if i.note.is_some() {
                format!(" {dot} note")
            } else {
                String::new()
            };
            (format!("done {at}{note}"), Token::FgMuted)
        }
        "skipped" => {
            let why = i
                .note
                .as_deref()
                .map(|n| format!(" {dot} {n}"))
                .unwrap_or_default();
            (format!("skipped{why}"), Token::FgMuted)
        }
        _ => match i.screen.as_deref() {
            Some("review") => ("opens Review".into(), Token::FgMuted),
            Some(other) => (format!("opens {other}"), Token::FgMuted),
            None => (
                i.launchers
                    .iter()
                    .map(|l| launcher_short(&l.id))
                    .collect::<Vec<_>>()
                    .join(&format!(" {dot} ")),
                Token::FgMuted,
            ),
        },
    }
}

fn items_pane(buf: &mut Buffer, r: Rect, s: &AppState, w: &Workspace, hits: &mut Hits) {
    let t = &s.theme;
    let g = s.glyphs;
    let c = &w.checklist;
    let focused = c.pane == ChecklistPane::Items;
    // filter tabs in the top border
    let mut right: Vec<Run> = Vec::new();
    let mut spans = Vec::new();
    let mut x = 0u16;
    for (i, f) in ItemFilter::ALL.iter().enumerate() {
        if i > 0 {
            right.push(run("  ", t.muted()));
            x += 2;
        }
        let txt = if *f == c.filter {
            format!("[{}]", f.label())
        } else {
            f.label().to_string()
        };
        let wd = text::width(&txt) as u16;
        right.push(run(
            txt,
            if *f == c.filter {
                t.accent()
            } else {
                t.muted()
            },
        ));
        spans.push((*f, x, wd));
        x += wd;
    }
    draw::pane(
        buf,
        t,
        r,
        focused,
        &[
            run("Checklist", t.title(focused)),
            run(format!(" {} open", c.open_count()), t.text()),
        ],
        &right,
        t.base(),
    );
    let tx = r.right().saturating_sub(1 + x + 2);
    for (f, off, wd) in spans {
        hits.push((Rect::new(tx + 1 + off, r.y, wd, 1), Hit::ItemFilterTab(f)));
    }
    let inner = layout::inner(r);
    let visible = c.visible();
    // footer hints
    let footer_y = inner.bottom().saturating_sub(1);
    draw::hrule(
        buf,
        inner.x,
        footer_y.saturating_sub(1),
        inner.width,
        g.m(Marker::Rule),
        t.faint(),
    );
    let mut hints = Vec::new();
    for a in [
        Action::ItemDone,
        Action::ItemSkip,
        Action::ItemNote,
        Action::ItemFinding,
        Action::Activate,
    ] {
        if let Some(h) = s.keymap.hint_for(a, Ctx::Checklist) {
            hints.push(h);
        }
    }
    draw::put(
        buf,
        inner.x,
        footer_y,
        inner.width,
        &super::hint_runs(s, &hints),
    );
    if visible.is_empty() {
        let msg = if !c.loaded {
            "loading…".to_string()
        } else if c.items.is_empty() {
            "no checklist items · playbooks add them for manual steps".to_string()
        } else {
            format!("nothing {}", c.filter.label())
        };
        draw::put(buf, inner.x, inner.y, inner.width, &[run(msg, t.muted())]);
        return;
    }
    // lines: a header per (run, step) group
    let mut lines: Vec<Line> = Vec::new();
    let mut last: Option<(spektr_model::PlaybookRunId, u32)> = None;
    for (i, it) in visible.iter().enumerate() {
        if last != Some((it.run, it.step)) {
            if last.is_some() {
                lines.push(Line::Blank);
            }
            lines.push(Line::Header(it));
            last = Some((it.run, it.step));
        }
        lines.push(Line::Item(i, it));
    }
    let sel = c.item_sel.min(visible.len() - 1);
    let detail_h = 8u16;
    let list_h = inner.height.saturating_sub(detail_h + 3).max(3) as usize;
    let cur_line = lines
        .iter()
        .position(|l| matches!(l, Line::Item(i, _) if *i == sel))
        .unwrap_or(0);
    let start = if lines.len() <= list_h {
        0
    } else {
        cur_line
            .saturating_sub(list_h - 1)
            .min(lines.len() - list_h)
    };
    let mut y = inner.y;
    let val_w = (inner.width as usize).saturating_sub(4).min(38);
    for l in lines.iter().skip(start).take(list_h) {
        match l {
            Line::Header(it) => {
                draw::put(
                    buf,
                    inner.x,
                    y,
                    inner.width,
                    &[
                        run(it.step_title.clone(), t.text().add_modifier(Modifier::BOLD)),
                        run(
                            format!("  {} {} step {}", it.playbook, g.m(Marker::Dot), it.step),
                            t.muted(),
                        ),
                    ],
                );
            }
            Line::Item(i, it) => {
                let row = Rect::new(r.x + 1, y, r.width.saturating_sub(2), 1);
                let cursor = *i == sel;
                if cursor {
                    draw::bg(
                        buf,
                        row,
                        if focused {
                            t.cursor_row()
                        } else {
                            t.hover_row()
                        },
                    );
                    if focused {
                        draw::put(
                            buf,
                            inner.x,
                            y,
                            1,
                            &[run(g.m(Marker::Cursor), t.cursor_bar())],
                        );
                    }
                } else if s.hover == Some(Hit::ItemRow(*i)) {
                    draw::bg(buf, row, t.hover_row());
                }
                let (m, mt) = match it.state.as_str() {
                    "done" => (g.m(Marker::Ok), Token::Ok),
                    "skipped" => (g.m(Marker::Skipped), Token::FgMuted),
                    _ => (g.m(Marker::Pending), Token::FgMuted),
                };
                // An avatar reads by its account; an item without an entity
                // (a step with no `on`) shows its run's seed.
                let who = match (&it.label, &it.entity) {
                    (Some(l), Some(e)) => format!("{} {l}", g.entity(&e.ty)),
                    _ => it
                        .entity
                        .as_ref()
                        .or_else(|| {
                            c.runs
                                .iter()
                                .find(|r| r.id == it.run)
                                .and_then(|r| r.seed.as_ref())
                        })
                        .map_or_else(
                            || "—".into(),
                            |e| format!("{} {}", g.entity(&e.ty), display_value(e, g)),
                        ),
                };
                let (rt, rtok) = item_right(it, g);
                draw::put(
                    buf,
                    inner.x + 1,
                    y,
                    inner.width.saturating_sub(1),
                    &[
                        run(format!("{m} "), t.fg(mt)),
                        run(text::fit(&who, val_w), t.text()),
                        run("  ", t.text()),
                        run(rt, t.fg(rtok)),
                    ],
                );
                hits.push((row, Hit::ItemRow(*i)));
            }
            Line::Blank => {}
        }
        y += 1;
    }
    // selected item
    let Some(it) = visible.get(sel) else { return };
    y += 1;
    let stop = footer_y.saturating_sub(1);
    if y >= stop {
        return;
    }
    section(
        buf,
        t,
        g,
        inner.x,
        y,
        inner.width,
        &[run("Selected", t.muted())],
    );
    y += 1;
    for (i, l) in super::wrap(&it.text, inner.width as usize)
        .into_iter()
        .enumerate()
    {
        if y >= stop {
            return;
        }
        draw::put(
            buf,
            inner.x,
            y,
            inner.width,
            &[run(l, if i == 0 { t.text() } else { t.muted() })],
        );
        y += 1;
    }
    if let Some(n) = &it.note {
        if y < stop {
            draw::put(
                buf,
                inner.x,
                y,
                inner.width,
                &[run("note  ", t.muted()), run(n.clone(), t.text())],
            );
        }
        y += 1;
    }
    if it.launchers.is_empty() || y + 1 >= stop {
        return;
    }
    y += 1;
    let (k, label) = s
        .keymap
        .hint_for(Action::ItemOpen, Ctx::Checklist)
        .unwrap_or_default();
    let mut runs = vec![
        run(format!("{k} "), t.text()),
        run(format!("{label}   "), t.muted()),
    ];
    for (i, l) in it.launchers.iter().enumerate() {
        if i > 0 {
            runs.push(run(format!("   {}   ", g.m(Marker::Dot)), t.muted()));
        }
        runs.push(run(
            l.name.clone(),
            if l.disabled.is_some() {
                t.muted()
            } else {
                t.text()
            },
        ));
    }
    draw::put(buf, inner.x, y, inner.width, &runs);
    y += 1;
    if y < stop {
        let note = match (
            &it.local_image,
            it.launchers.iter().find_map(|l| l.disabled.as_ref()),
        ) {
            (Some(_), Some(why)) => format!("{why} · {k} reveals the file instead"),
            _ => "Opening a launcher is recorded in the audit log.".into(),
        };
        draw::put(buf, inner.x, y, inner.width, &[run(note, t.muted())]);
    }
}
