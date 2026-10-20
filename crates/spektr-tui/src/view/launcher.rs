//! The launcher (artboard 01): wordmark, spectral strip, recent cases and
//! the doctor panel.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use super::draw::{self, Run, run};
use super::{Hits, hint_runs};
use crate::glyph::Marker;
use crate::host::{Chain, Level};
use crate::keymap::{Action, Ctx};
use crate::state::{AppState, Hit, Launcher};
use crate::text;
use crate::theme::{ColorMode, Token};

/// "2 min ago", "yesterday", "sep 27".
pub fn ago(now: jiff::Timestamp, then: jiff::Timestamp) -> String {
    let secs = now.as_second() - then.as_second();
    if secs < 60 {
        return "just now".into();
    }
    if secs < 3600 {
        return format!("{} min ago", secs / 60);
    }
    let tz = jiff::tz::TimeZone::UTC;
    let today = now.to_zoned(tz.clone()).date();
    let day = then.to_zoned(tz).date();
    if day == today {
        return format!("{} h ago", secs / 3600);
    }
    if today.yesterday().ok() == Some(day) {
        return "yesterday".into();
    }
    day.strftime("%b %-d").to_string().to_lowercase()
}

pub fn render(buf: &mut Buffer, area: Rect, s: &AppState, l: &Launcher, hits: &mut Hits) {
    let t = &s.theme;
    let g = s.glyphs;
    let bw = area.width.saturating_sub(8).min(100);
    let bx = area.x + (area.width - bw) / 2;
    let mut y = area.y + 3;

    // wordmark, strip, subtitle
    let mark = "s p e k t r";
    let mx = area.x + (area.width.saturating_sub(text::width(mark) as u16)) / 2;
    draw::put(buf, mx, y, area.width, &[run(mark, t.focus_text())]);
    y += 1;
    let strip_x = area.x + (area.width.saturating_sub(12)) / 2;
    for i in 0..6u16 {
        let st = if t.mode == ColorMode::Mono {
            t.text()
        } else {
            ratatui::style::Style::new().fg(t.spectrum(i as usize))
        };
        draw::put(buf, strip_x + i * 2, y, 2, &[run("▀▀", st)]);
    }
    y += 2;
    let sub = format!(
        "terminal osint workbench {} {}",
        g.m(Marker::Dot),
        s.settings.version
    );
    let sx = area.x + (area.width.saturating_sub(text::width(&sub) as u16)) / 2;
    draw::put(buf, sx, y, area.width, &[run(sub, t.muted())]);
    y += 3;

    // recent cases
    let recent = l.recent.as_deref().unwrap_or(&[]);
    let rows = recent.len().clamp(1, 8) as u16;
    let rh = rows + 6;
    let rect = Rect::new(bx, y, bw, rh);
    draw::pane(
        buf,
        t,
        rect,
        true,
        &[run("Recent cases", t.title(true))],
        &[],
        t.base(),
    );
    let ix = bx + 3;
    let iw = bw.saturating_sub(6);
    let cols = [0u16, 21, 31, 38, 54];
    let hy = y + 2;
    let header: [(&str, u16, bool); 5] = [
        ("case", cols[0], false),
        ("entities", cols[1], false),
        ("edges", cols[2], false),
        ("last opened", cols[3], false),
        ("chain", cols[4], false),
    ];
    for (h, c, _) in header {
        draw::put(buf, ix + c, hy, iw.saturating_sub(c), &[run(h, t.muted())]);
    }
    let ry = hy + 1;
    match &l.recent {
        None => {
            draw::put(buf, ix, ry, iw, &[run("loading…", t.muted())]);
        }
        Some(r) if r.is_empty() => {
            let key = s
                .keymap
                .hint_for(Action::LauncherNew, Ctx::Launcher)
                .map(|(k, _)| k)
                .unwrap_or_default();
            draw::put(
                buf,
                ix,
                ry,
                iw,
                &[
                    run("no cases yet ", t.muted()),
                    run(g.m(Marker::Dot), t.muted()),
                    run(" press ", t.muted()),
                    run(key, t.text()),
                    run(" to create one", t.muted()),
                ],
            );
        }
        Some(r) => {
            let offset = l.cursor.saturating_sub(rows as usize - 1);
            for (row, (i, c)) in r
                .iter()
                .enumerate()
                .skip(offset)
                .take(rows as usize)
                .enumerate()
            {
                let yy = ry + row as u16;
                let cursor = i == l.cursor;
                let hover = s.hover == Some(Hit::LauncherRow(i));
                let line = Rect::new(bx + 1, yy, bw - 2, 1);
                if cursor || hover {
                    draw::bg(buf, line, t.cursor_row());
                }
                if cursor {
                    draw::put(
                        buf,
                        bx + 2,
                        yy,
                        1,
                        &[run(g.m(Marker::Cursor), t.cursor_bar())],
                    );
                }
                let num = |n: Option<u64>, w: u16| {
                    text::right(&n.map_or("·".into(), |n| n.to_string()), w as usize)
                };
                draw::put(
                    buf,
                    ix,
                    yy,
                    20,
                    &[run(text::truncate(&c.name, 20), t.text())],
                );
                draw::put(
                    buf,
                    ix + cols[1],
                    yy,
                    8,
                    &[run(num(c.entities, 6), t.text())],
                );
                draw::put(buf, ix + cols[2], yy, 5, &[run(num(c.edges, 5), t.text())]);
                let opened = c
                    .last_opened
                    .map(|o| ago(s.now, o))
                    .unwrap_or_else(|| "never".into());
                draw::put(buf, ix + cols[3], yy, 15, &[run(opened, t.text())]);
                let chain: Vec<Run> = match &c.chain {
                    Chain::Ok { .. } => vec![run(g.m(Marker::Ok), t.ok())],
                    Chain::Broken { at } => vec![run(
                        format!("{} break at {at}", g.m(Marker::Failed)),
                        t.hostile(),
                    )],
                    Chain::Missing(why) => {
                        vec![run(format!("{} {why}", g.m(Marker::Failed)), t.hostile())]
                    }
                    Chain::Unknown => vec![run("…", t.muted())],
                };
                draw::put(buf, ix + cols[4], yy, iw.saturating_sub(cols[4]), &chain);
                hits.push((line, Hit::LauncherRow(i)));
            }
        }
    }
    // footer: path · size · seeds
    if let Some(c) = recent.get(l.cursor) {
        let mut parts = vec![text::tilde(&c.path, s.settings.home.as_deref())];
        if let Some(sz) = c.size {
            parts.push(text::bytes(sz));
        }
        if !c.seeds.is_empty() {
            let seeds: Vec<String> = c
                .seeds
                .iter()
                .take(3)
                .map(|(ty, v)| format!("{} {v}", g.entity(ty)))
                .collect();
            parts.push(format!("seeds: {}", seeds.join(", ")));
        }
        let sep = format!(" {} ", g.m(Marker::Dot));
        draw::put(buf, ix, y + rh - 2, iw, &[run(parts.join(&sep), t.muted())]);
    }
    if let Some(e) = &l.error {
        draw::put(buf, ix, y + rh, iw, &[run(e.clone(), t.hostile())]);
    }
    y += rh + 2;

    // doctor
    let mut checks = l.doctor.clone().unwrap_or_default();
    let mode = match t.mode {
        ColorMode::TrueColor => "truecolor",
        ColorMode::Indexed => "256-color",
        ColorMode::Mono => "no color",
    };
    checks.push(crate::host::DoctorLine {
        name: "terminal".into(),
        level: Level::Ok,
        detail: format!("{mode} {} {}×{}", g.m(Marker::Dot), s.size.0, s.size.1),
    });
    let per_col = checks.len().div_ceil(2).max(1) as u16;
    let room = area.bottom().saturating_sub(y + 4);
    let per_col = per_col.min(room.max(1));
    let dh = per_col + 3;
    if y + dh < area.bottom().saturating_sub(1) {
        let rect = Rect::new(bx, y, bw, dh);
        draw::pane(
            buf,
            t,
            rect,
            false,
            &[run("Doctor", t.title(false))],
            &[],
            t.base(),
        );
        let half = iw / 2;
        if l.doctor.is_none() {
            draw::put(buf, ix, y + 2, iw, &[run("checking…", t.muted())]);
        }
        let (left, right) = checks.split_at(checks.len().div_ceil(2));
        for (col, list) in [left, right].into_iter().enumerate() {
            for (i, c) in list.iter().take(per_col as usize).enumerate() {
                let (glyph, tok) = match c.level {
                    Level::Ok => (g.m(Marker::Ok), Token::Ok),
                    Level::Warn => (g.m(Marker::Attention), Token::Warn),
                    Level::Fail => (g.m(Marker::Failed), Token::Hostile),
                };
                let x = ix.saturating_sub(1) + col as u16 * half;
                draw::put(
                    buf,
                    x,
                    y + 2 + i as u16,
                    half.saturating_sub(1),
                    &[
                        run(glyph, t.fg(tok)),
                        run(" ", t.text()),
                        run(text::fit(&c.name, 11), t.text()),
                        run(c.detail.clone(), t.muted()),
                    ],
                );
            }
        }
    }

    // footer hints
    let fy = area.bottom().saturating_sub(1);
    let mut hints = s.keymap.hints(&[Ctx::Launcher]);
    if let Some((k, h)) = s.keymap.hint_for(Action::Quit, Ctx::Global) {
        hints.push((k, h));
    }
    let right = match &s.toast {
        Some((msg, tok, _)) => run(msg.clone(), t.fg(*tok)),
        None => run("spektr --help", t.muted()),
    };
    let rw = text::width(&right.0) as u16;
    draw::put_right(buf, area.right().saturating_sub(1), fy, rw, &[right]);
    draw::put(
        buf,
        area.x + 1,
        fy,
        area.width.saturating_sub(rw + 4),
        &hint_runs(s, &hints),
    );
}
