//! The Findings screen (artboard 09): findings grouped by the profile's
//! categories with filter tabs on the left; the selected finding's claim,
//! relevance, supporting entities, checks and evidence on the right, with
//! the selected capture's screenshot.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Modifier;
use spektr_engine::export::thousands;
use spektr_engine::findings::{CheckLevel, FindingDetail};

use super::Hits;
use super::draw::{self, Run, run};
use crate::glyph::{GlyphSet, Marker, display_value, strength_token};
use crate::keymap::{Action, Ctx};
use crate::layout;
use crate::state::{AppState, FindingFilter, FindingRowItem, FindingsPane, Hit, Workspace};
use crate::text;
use crate::theme::{Theme, Token};

pub fn render(buf: &mut Buffer, area: Rect, s: &AppState, w: &Workspace, hits: &mut Hits) {
    super::workspace::top_bar(
        buf,
        Rect::new(area.x, area.y, area.width, 1),
        s,
        w,
        Some("findings"),
    );
    let (left, right) = layout::findings(area.width, area.height);
    list(buf, left, s, w, hits);
    detail(buf, right, s, w, hits);
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

fn status_run(t: &Theme, g: GlyphSet, status: &str) -> Run {
    match status {
        "sent" => run(format!("{} sent", g.m(Marker::Ok)), t.ok()),
        "ready" => run("ready", t.accent()),
        other => run(other.to_string(), t.muted()),
    }
}

fn list(buf: &mut Buffer, r: Rect, s: &AppState, w: &Workspace, hits: &mut Hits) {
    let t = &s.theme;
    let g = s.glyphs;
    let f = &w.findings;
    let focused = f.pane == FindingsPane::List;
    let n = f.view.as_ref().map_or(0, |v| v.findings.len());
    // filter tabs
    let mut right: Vec<Run> = Vec::new();
    let mut spans = Vec::new();
    let mut x = 0u16;
    for (i, ff) in FindingFilter::ALL.iter().enumerate() {
        if i > 0 {
            right.push(run("  ", t.muted()));
            x += 2;
        }
        let txt = if *ff == f.filter {
            format!("[{}]", ff.label())
        } else {
            ff.label().to_string()
        };
        let wd = text::width(&txt) as u16;
        right.push(run(
            txt,
            if *ff == f.filter {
                t.accent()
            } else {
                t.muted()
            },
        ));
        spans.push((*ff, x, wd));
        x += wd;
    }
    draw::pane(
        buf,
        t,
        r,
        focused,
        &[
            run("Findings", t.title(focused)),
            run(format!(" {n}"), t.text()),
        ],
        &right,
        t.base(),
    );
    let tx = r.right().saturating_sub(1 + x + 2);
    for (ff, off, wd) in spans {
        hits.push((
            Rect::new(tx + 1 + off, r.y, wd, 1),
            Hit::FindingFilterTab(ff),
        ));
    }
    let inner = layout::inner(r);
    let Some(v) = &f.view else {
        draw::put(
            buf,
            inner.x,
            inner.y,
            inner.width,
            &[run("loading…", t.muted())],
        );
        return;
    };
    let pts_w = 8usize;
    let found_w = 7usize;
    let name_w = (inner.width as usize).saturating_sub(pts_w + found_w + 4);
    draw::put(
        buf,
        inner.x + 2,
        inner.y,
        inner.width,
        &[
            run(text::fit("category", name_w), t.muted()),
            run(format!("{:>pts_w$}", "pts"), t.muted()),
            run(format!("{:>found_w$}", "found"), t.muted()),
        ],
    );
    let rows = f.rows();
    let footer_h = 4u16;
    let list_h = inner.height.saturating_sub(2 + footer_h) as usize;
    let start = f.sel.saturating_sub(list_h.saturating_sub(1));
    let mut y = inner.y + 2;
    let mut prev_cat = false;
    for (i, row) in rows.iter().enumerate().skip(start).take(list_h) {
        let rect = Rect::new(r.x + 1, y, r.width.saturating_sub(2), 1);
        let cursor = i == f.sel;
        match row {
            FindingRowItem::Category { id, open } => {
                if prev_cat || i == start {
                    // categories with findings get a blank line before them
                }
                let c = v.categories.iter().find(|c| &c.id == id);
                let name = c.map_or(id.clone(), |c| c.name.clone());
                let count = v.findings.iter().filter(|x| &x.category == id).count();
                let pts = c.and_then(|c| c.points).map(thousands).unwrap_or_default();
                if cursor {
                    draw::bg(
                        buf,
                        rect,
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
                }
                let tok = if count > 0 { t.text() } else { t.muted() };
                draw::put(
                    buf,
                    inner.x + 2,
                    y,
                    inner.width.saturating_sub(2),
                    &[
                        run(
                            format!(
                                "{} ",
                                g.m(if *open {
                                    Marker::Expanded
                                } else {
                                    Marker::Collapsed
                                })
                            ),
                            t.muted(),
                        ),
                        run(text::fit(&name, name_w.saturating_sub(2)), tok),
                        run(format!("{pts:>pts_w$}"), t.muted()),
                        run(format!("{count:>found_w$}"), tok),
                    ],
                );
                prev_cat = true;
            }
            FindingRowItem::Finding(id) => {
                let Some(fs) = v.findings.iter().find(|x| x.id == *id) else {
                    continue;
                };
                if cursor {
                    draw::bg(
                        buf,
                        rect,
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
                } else if s.hover == Some(Hit::FindingRow(i)) {
                    draw::bg(buf, rect, t.hover_row());
                }
                let label = format!("F-{}  ", fs.id);
                let title_w = name_w.saturating_sub(4 + text::width(&label));
                let mut runs = vec![
                    run(format!("   {label}"), t.text()),
                    run(text::fit(&fs.title, title_w), t.text()),
                    run("  ", t.text()),
                    status_run(t, g, &fs.status),
                ];
                if let Some(st) = fs.strength {
                    runs.push(run(
                        format!("  {}", g.strength(st)),
                        t.fg(strength_token(st)),
                    ));
                }
                draw::put(buf, inner.x + 1, y, inner.width.saturating_sub(1), &runs);
                prev_cat = false;
            }
        }
        hits.push((rect, Hit::FindingRow(i)));
        y += 1;
    }
    // footer: counts and the open value
    let count = |st: &str| v.findings.iter().filter(|x| x.status == st).count();
    let open_value: u32 = v
        .findings
        .iter()
        .filter(|x| x.status != "sent")
        .filter_map(|x| {
            v.categories
                .iter()
                .find(|c| c.id == x.category)
                .and_then(|c| c.points)
        })
        .sum();
    let fy = inner.bottom().saturating_sub(3);
    draw::hrule(buf, inner.x, fy, inner.width, g.m(Marker::Rule), t.faint());
    let mut runs = vec![
        run("sent ", t.muted()),
        run(count("sent").to_string(), t.text()),
        run("   ready ", t.muted()),
        run(count("ready").to_string(), t.text()),
        run("   draft ", t.muted()),
        run(count("draft").to_string(), t.text()),
    ];
    if v.categories.iter().any(|c| c.points.is_some()) {
        runs.push(run(
            format!("   {} open value ", g.m(Marker::Dot)),
            t.muted(),
        ));
        runs.push(run(format!("{} pts", thousands(open_value)), t.text()));
    }
    draw::put(buf, inner.x, fy + 1, inner.width, &runs);
    draw::put(
        buf,
        inner.x,
        fy + 2,
        inner.width,
        &[run(
            format!("Categories come from the {} rules profile.", w.profile()),
            t.muted(),
        )],
    );
}

fn detail(buf: &mut Buffer, r: Rect, s: &AppState, w: &Workspace, hits: &mut Hits) {
    let t = &s.theme;
    let g = s.glyphs;
    let f = &w.findings;
    let focused = f.pane == FindingsPane::Detail;
    let selected = f.selected_finding();
    let d = f.detail.as_ref().filter(|d| Some(d.id) == selected);
    let title = d.map_or("Finding".to_string(), |d| format!("F-{}", d.id));
    draw::pane(
        buf,
        t,
        r,
        focused,
        &[run(title, t.title(focused))],
        &[],
        t.base(),
    );
    let inner = layout::inner(r);
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
        Action::FindingEdit,
        Action::FindingAttach,
        Action::FindingReady,
        Action::FindingSent,
        Action::FindingExport,
    ] {
        if let Some(h) = s.keymap.hint_for(a, Ctx::Findings) {
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
    let Some(d) = d else {
        let msg = match &f.view {
            Some(v) if v.findings.is_empty() => {
                let k = s
                    .keymap
                    .hint_for(Action::FindingNew, Ctx::Findings)
                    .map(|h| h.0)
                    .unwrap_or_default();
                format!(
                    "No findings yet. {k} creates one; f on an entity in the graph files it with the entity."
                )
            }
            Some(_) => "select a finding".into(),
            None => "loading…".into(),
        };
        for (i, l) in super::wrap(&msg, inner.width as usize)
            .into_iter()
            .enumerate()
        {
            draw::put(
                buf,
                inner.x,
                inner.y + i as u16,
                inner.width,
                &[run(l, t.muted())],
            );
        }
        return;
    };
    let stop = footer_y.saturating_sub(1);
    let mut y = inner.y;
    // header: category · pts · status · edited
    let cat = f
        .view
        .as_ref()
        .and_then(|v| v.categories.iter().find(|c| c.id == d.category));
    let mut head = vec![run(
        cat.map_or(d.category.clone(), |c| c.name.clone()),
        t.text().add_modifier(Modifier::BOLD),
    )];
    let dot = g.m(Marker::Dot);
    if let Some(p) = cat.and_then(|c| c.points) {
        head.push(run(format!("  {dot} {} pts", thousands(p)), t.muted()));
    }
    head.push(run(format!("  {dot} "), t.muted()));
    head.push(status_run(t, g, &d.status));
    head.push(run(
        format!("  {dot} edited {}", d.updated_at.strftime("%H:%M")),
        t.muted(),
    ));
    draw::put(buf, inner.x, y, inner.width, &head);
    y += 1;
    // A title written by hand; the automatic one is the claim's start.
    let auto =
        !d.claim.trim().is_empty() && d.claim.trim().starts_with(d.title.trim_end_matches('…'));
    if !auto {
        draw::put(
            buf,
            inner.x,
            y,
            inner.width,
            &[run(d.title.clone(), t.text())],
        );
        y += 1;
    }
    y += 1;
    for (label, body) in [("Claim", &d.claim), ("Relevance", &d.relevance)] {
        if y >= stop {
            break;
        }
        draw::put(buf, inner.x, y, inner.width, &[run(label, t.muted())]);
        y += 1;
        let text = if body.trim().is_empty() {
            "—".to_string()
        } else {
            body.trim().to_string()
        };
        for l in super::wrap(&text, inner.width as usize).into_iter().take(4) {
            if y >= stop {
                break;
            }
            draw::put(buf, inner.x, y, inner.width, &[run(l, t.text())]);
            y += 1;
        }
        y += 1;
    }
    // supporting entities (left) and checks (right)
    let half = inner.width / 2;
    let top = y;
    if y < stop {
        section(
            buf,
            t,
            g,
            inner.x,
            y,
            half.saturating_sub(2),
            "Supporting entities",
        );
        section(
            buf,
            t,
            g,
            inner.x + half + 2,
            y,
            inner.width.saturating_sub(half + 2),
            "Checks",
        );
    }
    y += 1;
    let mut ly = y;
    for (i, (e, st)) in d.entities.iter().enumerate() {
        if ly >= stop {
            break;
        }
        let rect = Rect::new(inner.x, ly, half.saturating_sub(2), 1);
        if focused && f.dsel == i {
            draw::bg(buf, rect, t.cursor_row());
            draw::put(
                buf,
                inner.x.saturating_sub(1),
                ly,
                1,
                &[run(g.m(Marker::Cursor), t.cursor_bar())],
            );
        }
        let val = format!("{} {}", g.entity(&e.ty), display_value(e, g));
        let right = match st {
            Some(st) => run(g.strength(*st).to_string(), t.fg(strength_token(*st))),
            None if e.flags.contains(spektr_model::EntityFlags::SUBJECT) => {
                run("subject", t.muted())
            }
            None => run("—", t.muted()),
        };
        let vw = (half as usize).saturating_sub(12);
        draw::put(
            buf,
            inner.x + 1,
            ly,
            half.saturating_sub(3),
            &[run(text::fit(&val, vw), t.text()), right],
        );
        hits.push((rect, Hit::FindingDetailRow(i)));
        ly += 1;
    }
    if d.entities.is_empty() && ly < stop {
        let k = s
            .keymap
            .hint_for(Action::Attach, Ctx::Entity)
            .map(|h| h.0)
            .unwrap_or_default();
        draw::put(
            buf,
            inner.x + 1,
            ly,
            half,
            &[run(format!("none · {k} on an entity adds it"), t.muted())],
        );
        ly += 1;
    }
    let mut ry = y;
    for c in &d.checks {
        if ry >= stop {
            break;
        }
        let (m, tok) = match c.level {
            CheckLevel::Pass => (g.m(Marker::Ok), Token::Ok),
            CheckLevel::Warn => (g.m(Marker::Attention), Token::Warn),
            CheckLevel::Fail => (g.m(Marker::Failed), Token::Hostile),
        };
        draw::put(
            buf,
            inner.x + half + 2,
            ry,
            inner.width.saturating_sub(half + 2),
            &[
                run(format!("{m} "), t.fg(tok)),
                run(c.text.clone(), t.text()),
            ],
        );
        ry += 1;
    }
    y = ly.max(ry).max(top + 2) + 1;
    // evidence
    if y < stop {
        section(
            buf,
            t,
            g,
            inner.x,
            y,
            inner.width,
            &format!("Evidence {}", d.evidence.len()),
        );
    }
    y += 1;
    let base = d.entities.len();
    for (i, e) in d.evidence.iter().enumerate() {
        if y >= stop {
            break;
        }
        let rect = Rect::new(inner.x, y, inner.width, 1);
        if focused && f.dsel == base + i {
            draw::bg(buf, rect, t.cursor_row());
            draw::put(
                buf,
                inner.x.saturating_sub(1),
                y,
                1,
                &[run(g.m(Marker::Cursor), t.cursor_bar())],
            );
        }
        let what = e
            .url
            .as_deref()
            .map(|u| {
                u.trim_start_matches("https://")
                    .trim_start_matches("http://")
                    .to_string()
            })
            .unwrap_or_else(|| e.observation.kind.clone());
        let kind = if e.screenshot.is_some() {
            "html+png".to_string()
        } else {
            e.observation
                .content_type
                .clone()
                .unwrap_or_else(|| e.observation.module_version.clone())
        };
        let hash = e
            .screenshot
            .as_deref()
            .or(e.observation.blob.as_deref())
            .map(|h| {
                format!(
                    "{}…{}",
                    &h[..4.min(h.len())],
                    &h[h.len().saturating_sub(2)..]
                )
            })
            .unwrap_or_default();
        let ww = (inner.width as usize).saturating_sub(10 + 12 + 8 + 10);
        draw::put(
            buf,
            inner.x + 1,
            y,
            inner.width.saturating_sub(1),
            &[
                run(text::fit(&e.observation.module, 9), t.text()),
                run(" ", t.text()),
                run(text::fit(&what, ww), t.text()),
                run(format!(" {}", text::fit(&kind, 10)), t.muted()),
                run(
                    format!(" {}  ", e.observation.created_at.strftime("%H:%M")),
                    t.muted(),
                ),
                run(hash, t.muted()),
            ],
        );
        hits.push((rect, Hit::FindingDetailRow(base + i)));
        y += 1;
    }
    // the selected (or first) capture's screenshot and facts
    let pick = f
        .dsel
        .checked_sub(base)
        .and_then(|i| d.evidence.get(i))
        .or_else(|| d.evidence.iter().find(|e| e.screenshot.is_some()));
    if let Some(e) = pick
        && y + 3 < stop
    {
        y += 1;
        capture_block(buf, s, inner, y, stop, d, e);
    }
}

fn capture_block(
    buf: &mut Buffer,
    s: &AppState,
    inner: Rect,
    y: u16,
    stop: u16,
    _d: &FindingDetail,
    e: &spektr_engine::findings::EvidenceItem,
) {
    let t = &s.theme;
    let g = s.glyphs;
    let size = super::image::shot_size(inner.width, 10);
    let mut x = inner.x;
    if let Some(sha) = &e.screenshot {
        let used = super::image::draw(buf, s, inner.x, y, size, sha, "screenshot");
        if used > 0 {
            x = inner.x + size.0 + 2;
        }
    }
    let facts: Vec<(&str, String)> = vec![
        (
            "captured",
            e.observation
                .created_at
                .strftime("%Y-%m-%d %H:%M:%S UTC")
                .to_string(),
        ),
        ("url", e.url.clone().unwrap_or_else(|| "—".into())),
        (
            "sha256",
            e.screenshot
                .clone()
                .or_else(|| e.observation.blob.clone())
                .map(|h| {
                    format!(
                        "{}…{}",
                        &h[..6.min(h.len())],
                        &h[h.len().saturating_sub(6)..]
                    )
                })
                .unwrap_or_else(|| "—".into()),
        ),
        (
            "audit",
            e.audit_seq.map_or("—".into(), |n| format!("row #{n}")),
        ),
    ];
    let fw = inner.right().saturating_sub(x);
    for (i, (k, v)) in facts.into_iter().enumerate() {
        let yy = y + i as u16;
        if yy >= stop {
            break;
        }
        draw::put(
            buf,
            x,
            yy,
            fw,
            &[run(text::fit(k, 10), t.muted()), run(v, t.text())],
        );
    }
    let k = s
        .keymap
        .hint_for(Action::Activate, Ctx::Findings)
        .map(|h| h.0)
        .unwrap_or_default();
    let yy = y + 5;
    if yy < stop {
        draw::put(
            buf,
            x,
            yy,
            fw,
            &[
                run(format!("{k} "), t.text()),
                run(
                    if e.screenshot.is_some() {
                        "open screenshot"
                    } else {
                        "open raw response"
                    },
                    t.muted(),
                ),
                run(format!("   {} ", g.m(Marker::Dot)), t.muted()),
                run("chain verified by spektr verify", t.muted()),
            ],
        );
    }
}
