//! The Review screen (artboard 05's layout): the queue of `sweep_unsure`
//! and `same_person` items with filter tabs on the left; the selected
//! item's detail on the right, with a side-by-side comparison (and
//! avatars) for same-person items.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Modifier;
use spektr_engine::{EntityDetail, ReviewView};
use spektr_model::{CoreType, EntityType};

use super::Hits;
use super::draw::{self, Run, run};
use crate::glyph::{GlyphSet, Marker};
use crate::keymap::{Action, Ctx};
use crate::layout;
use crate::state::{AppState, Hit, ReviewFilter, Workspace};
use crate::text;
use crate::theme::{Theme, Token};

pub fn render(buf: &mut Buffer, area: Rect, s: &AppState, w: &Workspace, hits: &mut Hits) {
    super::workspace::top_bar(
        buf,
        Rect::new(area.x, area.y, area.width, 1),
        s,
        w,
        Some("review"),
    );
    let (left, right) = layout::review(area.width, area.height);
    queue(buf, left, s, w, hits);
    detail(buf, right, s, w);
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

fn glyph(ty: &str, g: GlyphSet) -> char {
    match CoreType::parse(ty) {
        Some(c) => g.entity(&EntityType::from(c)),
        None => '·',
    }
}

/// `(kind, rule, link)` for a queue row.
fn row_text(i: &ReviewView, g: GlyphSet) -> (String, String, String) {
    let p = &i.payload;
    let d = &p["detail"];
    let str_of = |v: &serde_json::Value| v.as_str().unwrap_or("").to_string();
    let either = |a: String, b: String| if a.is_empty() { b } else { a };
    match i.kind.as_str() {
        "sweep_unsure" => (
            "sweep".into(),
            either(str_of(&d["name"]), str_of(&d["site"])),
            format!(
                "{} {}  {}  {} {} {} {}",
                glyph("username", g),
                str_of(&d["username"]),
                g.m(Marker::Out),
                glyph("account", g),
                str_of(&d["platform"]),
                g.m(Marker::Dot),
                either(str_of(&d["handle"]), str_of(&d["username"])),
            ),
        ),
        "same_person" => (
            "merge".into(),
            "same-person".into(),
            format!(
                "{} {}  {}  {} {}",
                glyph("person", g),
                str_of(&p["a_value"]),
                g.m(Marker::Sym),
                glyph("person", g),
                str_of(&p["b_value"]),
            ),
        ),
        other => (other.into(), String::new(), String::new()),
    }
}

fn queue(buf: &mut Buffer, r: Rect, s: &AppState, w: &Workspace, hits: &mut Hits) {
    let t = &s.theme;
    let g = s.glyphs;
    let rv = &w.review;
    let mut right: Vec<Run> = Vec::new();
    let mut spans = Vec::new();
    let mut x = 0u16;
    for (i, f) in ReviewFilter::ALL.iter().enumerate() {
        if i > 0 {
            right.push(run("  ", t.muted()));
            x += 2;
        }
        let label = format!("{} {}", f.label(), rv.count(*f));
        let txt = if *f == rv.filter {
            format!("[{label}]")
        } else {
            label
        };
        let wd = text::width(&txt) as u16;
        right.push(run(
            txt,
            if *f == rv.filter {
                t.accent()
            } else {
                t.muted()
            },
        ));
        spans.push((*f, x, wd));
        x += wd;
    }
    let mut title = vec![run("Review", t.title(true))];
    if let Some(e) = rv.only.and_then(|id| w.entity(id)) {
        title.push(run(format!(" {} {}", g.entity(&e.ty), e.value), t.text()));
    }
    draw::pane(buf, t, r, true, &title, &right, t.base());
    let tx = r.right().saturating_sub(1 + x + 2);
    for (f, off, wd) in spans {
        hits.push((Rect::new(tx + 1 + off, r.y, wd, 1), Hit::ReviewFilterTab(f)));
    }
    let inner = layout::inner(r);
    let (kind_w, rule_w) = (8usize, 22usize);
    draw::put(
        buf,
        inner.x + 2,
        inner.y,
        inner.width,
        &[
            run(text::fit("kind", kind_w), t.muted()),
            run(text::fit("site", rule_w), t.muted()),
            run("link", t.muted()),
        ],
    );
    let visible = rv.visible();
    if visible.is_empty() {
        let msg = if rv.loaded {
            format!("nothing {}", rv.filter.label())
        } else {
            "loading…".into()
        };
        draw::put(
            buf,
            inner.x + 2,
            inner.y + 2,
            inner.width,
            &[run(msg, t.muted())],
        );
    }
    let list_h = inner.height.saturating_sub(5) as usize;
    let start = rv.sel.saturating_sub(list_h.saturating_sub(1));
    for (y, (i, it)) in (inner.y + 2..).zip(visible.iter().enumerate().skip(start).take(list_h)) {
        let row = Rect::new(r.x + 1, y, r.width.saturating_sub(2), 1);
        if i == rv.sel {
            draw::bg(buf, row, t.cursor_row());
            draw::put(
                buf,
                inner.x,
                y,
                1,
                &[run(g.m(Marker::Cursor), t.cursor_bar())],
            );
        } else if s.hover == Some(Hit::ReviewRow(i)) {
            draw::bg(buf, row, t.hover_row());
        }
        let (kind, rule, link) = row_text(it, g);
        let ktok = if kind == "merge" {
            Token::Warn
        } else {
            Token::Fg
        };
        let reason = it.payload["detail"]["reason"]
            .as_str()
            .unwrap_or("")
            .to_string();
        let link_w = (inner.width as usize).saturating_sub(kind_w + rule_w + 2 + 18);
        draw::put(
            buf,
            inner.x + 2,
            y,
            inner.width.saturating_sub(2),
            &[
                run(text::fit(&kind, kind_w), t.fg(ktok)),
                run(text::fit(&rule, rule_w), t.text()),
                run(text::fit(&link, link_w), t.text()),
                run("  ", t.text()),
                run(reason, t.muted()),
            ],
        );
        hits.push((row, Hit::ReviewRow(i)));
    }
    let note = "Unsure sweep hits and same-person suggestions wait here. Nothing becomes an account or merges without you.";
    let lines = super::wrap(note, inner.width as usize);
    let ny = inner.bottom().saturating_sub(lines.len() as u16);
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

fn detail(buf: &mut Buffer, r: Rect, s: &AppState, w: &Workspace) {
    let t = &s.theme;
    let g = s.glyphs;
    draw::pane(
        buf,
        t,
        r,
        false,
        &[run("Detail", t.title(false))],
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
        Action::ReviewAccept,
        Action::ReviewReject,
        Action::ReviewLater,
        Action::OpenUrl,
    ] {
        if let Some(h) = s.keymap.hint_for(a, Ctx::Review) {
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
    let Some(it) = w.review.selected() else {
        draw::put(
            buf,
            inner.x,
            inner.y,
            inner.width,
            &[run("nothing selected", t.muted())],
        );
        return;
    };
    let stop = footer_y.saturating_sub(1);
    let mut y = inner.y;
    let line = |buf: &mut Buffer, y: &mut u16, runs: &[Run]| {
        if *y < stop {
            draw::put(buf, inner.x, *y, inner.width, runs);
        }
        *y += 1;
    };
    let p = &it.payload;
    let dot = g.m(Marker::Dot);
    let bold = t.text().add_modifier(Modifier::BOLD);
    match it.kind.as_str() {
        "sweep_unsure" => {
            let d = &p["detail"];
            let sv = |k: &str| d[k].as_str().unwrap_or("").to_string();
            line(
                buf,
                &mut y,
                &[
                    run("account ", t.warn()),
                    run(
                        format!(
                            "{} {} {dot} {}",
                            glyph("account", g),
                            sv("platform"),
                            sv("handle")
                        ),
                        bold,
                    ),
                ],
            );
            line(
                buf,
                &mut y,
                &[run(
                    format!(
                        "unsure sweep hit {dot} {} {dot} {}",
                        sv("name"),
                        sv("reason")
                    ),
                    t.muted(),
                )],
            );
            y += 1;
            for (label, v) in [
                ("username", sv("username")),
                ("site", sv("site")),
                ("url", sv("url")),
                ("reason", sv("reason")),
                ("module", p["module"].as_str().unwrap_or("").to_string()),
            ] {
                line(
                    buf,
                    &mut y,
                    &[run(text::fit(label, 14), t.muted()), run(v, t.text())],
                );
            }
            y += 1;
            let n = p["evidence"].as_array().map_or(0, Vec::len);
            let note = format!(
                "Accepting adds the account, with the sweep's response as evidence ({n} observation{}).",
                if n == 1 { "" } else { "s" }
            );
            for l in super::wrap(&note, inner.width as usize) {
                line(buf, &mut y, &[run(l, t.muted())]);
            }
        }
        "same_person" => {
            let sv = |k: &str| p[k].as_str().unwrap_or("?").to_string();
            line(
                buf,
                &mut y,
                &[
                    run("merge ", t.warn()),
                    run(format!("{} {}", glyph("person", g), sv("a_value")), bold),
                    run(format!("  {}  ", g.m(Marker::Sym)), t.muted()),
                    run(format!("{} {}", glyph("person", g), sv("b_value")), bold),
                ],
            );
            line(
                buf,
                &mut y,
                &[run(
                    format!(
                        "rule same-person {dot} both link to {} {}",
                        glyph(p["via_type"].as_str().unwrap_or(""), g),
                        sv("via_value")
                    ),
                    t.muted(),
                )],
            );
            y += 1;
            let (a, b) = match &w.review.compare {
                Some((id, a, b)) if *id == it.id => (a.as_ref(), b.as_ref()),
                _ => (None, None),
            };
            if let (Some(a), Some(b)) = (a, b) {
                compare(buf, s, inner, &mut y, stop, a, b);
            } else {
                line(buf, &mut y, &[run("loading…", t.muted())]);
            }
        }
        other => line(buf, &mut y, &[run(other.to_string(), t.text())]),
    }
}

/// Avatars side by side (when both have one), then a field table with
/// `=` same, `≠` different, `·` one side only.
fn compare(
    buf: &mut Buffer,
    s: &AppState,
    inner: Rect,
    y: &mut u16,
    stop: u16,
    a: &EntityDetail,
    b: &EntityDetail,
) {
    let t = &s.theme;
    let g = s.glyphs;
    if let (Some(x), Some(z)) = (&a.image, &b.image) {
        let size = super::image::pair_size(inner.width, inner.height);
        let h1 = super::image::draw(buf, s, inner.x, *y, size, x, &a.entity.summary.value);
        let h2 = super::image::draw(
            buf,
            s,
            inner.x + size.0 + 2,
            *y,
            size,
            z,
            &b.entity.summary.value,
        );
        *y += h1.max(h2) + 1;
    }
    let col = 16usize;
    let vw = ((inner.width as usize).saturating_sub(col + 2) / 2).max(8);
    if *y < stop {
        draw::put(
            buf,
            inner.x,
            *y,
            inner.width,
            &[
                run(" ".repeat(col + 2), t.muted()),
                run(text::fit(&a.entity.summary.value, vw), t.muted()),
                run(text::fit(&b.entity.summary.value, vw), t.muted()),
            ],
        );
    }
    *y += 1;
    let first = |d: &EntityDetail, k: &str| -> Option<String> {
        d.entity
            .attrs
            .get(k)
            .and_then(|v| v.first())
            .map(|a| match &a.value {
                spektr_model::AttrValue::Text(s) | spektr_model::AttrValue::Url(s) => s.clone(),
                other => format!("{other:?}"),
            })
    };
    let mut keys: Vec<&String> = a.entity.attrs.keys().chain(b.entity.attrs.keys()).collect();
    keys.sort();
    keys.dedup();
    for k in keys {
        if *y >= stop {
            break;
        }
        let (x, z) = (first(a, k), first(b, k));
        let (m, tok) = match (&x, &z) {
            (Some(p), Some(q)) if p == q => ("=", Token::Ok),
            (Some(_), Some(_)) => ("≠", Token::Hostile),
            _ => (g.m(Marker::Dot), Token::FgMuted),
        };
        draw::put(
            buf,
            inner.x,
            *y,
            inner.width,
            &[
                run(format!("{m} "), t.fg(tok)),
                run(text::fit(&k.replace('_', " "), col), t.text()),
                run(text::fit(x.as_deref().unwrap_or("—"), vw), t.text()),
                run(text::fit(z.as_deref().unwrap_or("—"), vw), t.text()),
            ],
        );
        *y += 1;
    }
    *y += 1;
    if *y < stop {
        section(buf, t, g, inner.x, *y, inner.width, "Evidence");
    }
    *y += 1;
    for o in a
        .observations
        .iter()
        .take(3)
        .chain(b.observations.iter().take(3))
    {
        if *y >= stop {
            break;
        }
        draw::put(
            buf,
            inner.x,
            *y,
            inner.width,
            &[
                run(text::fit(&o.module, 16), t.text()),
                run(format!("{} ", text::fit(&o.module_version, 8)), t.muted()),
                run(o.created_at.strftime("%H:%M:%S").to_string(), t.muted()),
            ],
        );
        *y += 1;
    }
}
