//! Rendering: pure functions of `AppState` that draw into the frame and
//! return the hit regions for the mouse (TUI_SPEC §8, §11).

mod draw;
mod launcher;
mod overlay;
mod workspace;

pub use overlay::wrap;

use ratatui::Frame;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use crate::keymap::Ctx;
use crate::state::{AppState, Hit, Mode, Overlay, Screen};
use draw::{Run, run};

/// Hit regions, in drawing order (later ones are on top).
pub type Hits = Vec<(Rect, Hit)>;

pub fn render(f: &mut Frame<'_>, s: &AppState) -> Hits {
    let area = f.area();
    render_buf(f.buffer_mut(), area, s)
}

/// Render into any buffer (snapshot tests use a `TestBackend`).
pub fn render_buf(buf: &mut Buffer, area: Rect, s: &AppState) -> Hits {
    let t = &s.theme;
    let mut hits = Hits::new();
    draw::fill(buf, area, t.base());
    match &s.screen {
        Screen::Launcher(l) => launcher::render(buf, area, s, l, &mut hits),
        Screen::Opening(p) => {
            let msg = format!(
                "opening {}…",
                crate::text::tilde(p, s.settings.home.as_deref())
            );
            let r = crate::layout::centered(area, crate::text::width(&msg) as u16, 1);
            draw::put(buf, r.x, r.y, r.width, &[run(msg, t.muted())]);
        }
        Screen::Workspace(w) => {
            if crate::layout::too_small(area.width, area.height) {
                let msg = format!(
                    "spektr needs at least {}×{} (now {}×{})",
                    crate::layout::MIN_W,
                    crate::layout::MIN_H,
                    area.width,
                    area.height
                );
                let r = crate::layout::centered(area, crate::text::width(&msg) as u16, 1);
                draw::put(buf, r.x, r.y, r.width, &[run(msg, t.muted())]);
                return hits;
            }
            workspace::render(buf, area, s, w, &mut hits);
        }
    }
    match &s.overlay {
        Some(Overlay::Palette(p)) => {
            draw::dim(buf, area, t);
            overlay::palette(buf, area, s, p, &mut hits);
        }
        Some(Overlay::Menu(m)) => overlay::menu(buf, area, s, m, &mut hits),
        Some(Overlay::Help(h)) => overlay::help(buf, area, s, h, &mut hits),
        Some(Overlay::Dialog(d)) => overlay::dialog(buf, area, s, d, &mut hits),
        None => {}
    }
    if matches!(s.screen, Screen::Workspace(_)) || s.overlay.is_some() {
        status_line(buf, area, s, &mut hits);
    }
    hits
}

/// Which contexts' hints the status line shows.
fn hint_ctxs(s: &AppState) -> Vec<Ctx> {
    match &s.overlay {
        Some(Overlay::Palette(_)) => vec![Ctx::Palette],
        Some(Overlay::Menu(_)) => vec![Ctx::Menu],
        Some(Overlay::Help(_)) => vec![Ctx::Help],
        Some(Overlay::Dialog(_)) => vec![Ctx::Dialog],
        None => match s.mode() {
            Mode::Filter => vec![Ctx::Filter],
            _ => vec![Ctx::Entity, Ctx::Global],
        },
    }
}

fn hint_runs(s: &AppState, hints: &[(String, &str)]) -> Vec<Run> {
    let t = &s.theme;
    let mut runs = Vec::new();
    for (i, (k, h)) in hints.iter().enumerate() {
        if i > 0 {
            runs.push(run("   ", t.muted()));
        }
        runs.push(run(k.clone(), t.text()));
        runs.push(run(format!(" {h}"), t.muted()));
    }
    runs
}

/// Mode pill, hints, toast and the UTC clock (TUI_SPEC §4.1).
fn status_line(buf: &mut Buffer, area: Rect, s: &AppState, hits: &mut Hits) {
    let t = &s.theme;
    let y = area.bottom().saturating_sub(1);
    draw::fill(buf, Rect::new(area.x, y, area.width, 1), t.base());
    let mode = s.mode();
    let pill = format!(" {} ", mode.label());
    let pw = draw::put(
        buf,
        area.x + 1,
        y,
        area.width,
        &[run(pill, t.pill(mode.token()))],
    );
    hits.push((Rect::new(area.x + 1, y, pw, 1), Hit::Pill));
    let clock = format!("{} UTC", s.now.strftime("%H:%M:%S"));
    let clock_w = crate::text::width(&clock) as u16;
    let right = area.right().saturating_sub(1);
    draw::put_right(buf, right, y, clock_w, &[run(clock, t.muted())]);
    let mut avail = area.width.saturating_sub(pw + clock_w + 6);
    if let Some((msg, tok, _)) = &s.toast {
        let tw = (crate::text::width(msg) as u16).min(avail / 2);
        draw::put_right(
            buf,
            right - clock_w - 3,
            y,
            tw,
            &[run(msg.clone(), t.fg(*tok))],
        );
        avail = avail.saturating_sub(tw + 3);
    }
    let hints = s.keymap.hints(&hint_ctxs(s));
    draw::put(buf, area.x + 1 + pw + 2, y, avail, &hint_runs(s, &hints));
}

/// The text cursor: a reversed cell.
fn caret(s: &AppState) -> Run {
    run(" ", s.theme.caret())
}
