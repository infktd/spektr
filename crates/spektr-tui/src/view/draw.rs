//! Low-level drawing: styled runs, rounded panes, rules. Every helper
//! clips to the width it's given.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;

use crate::text;
use crate::theme::{Theme, Token};

/// A styled run of text.
pub type Run = (String, Style);

pub fn run(s: impl Into<String>, st: Style) -> Run {
    (s.into(), st)
}

/// Draw runs from `(x, y)`, clipped to `max` cells; returns the cells used.
pub fn put(buf: &mut Buffer, x: u16, y: u16, max: u16, runs: &[Run]) -> u16 {
    let area = buf.area;
    if y < area.y || y >= area.bottom() || x >= area.right() {
        return 0;
    }
    let max = max.min(area.right() - x);
    let mut used = 0u16;
    for (s, st) in runs {
        if used >= max {
            break;
        }
        let room = (max - used) as usize;
        let t = if text::width(s) > room {
            text::truncate(s, room)
        } else {
            s.clone()
        };
        let (nx, _) = buf.set_stringn(x + used, y, &t, room, *st);
        used = nx - x;
    }
    used
}

/// Total width of runs.
pub fn width(runs: &[Run]) -> u16 {
    runs.iter().map(|(s, _)| text::width(s) as u16).sum()
}

/// Draw runs right-aligned so they end at `right` (exclusive).
pub fn put_right(buf: &mut Buffer, right: u16, y: u16, max: u16, runs: &[Run]) -> u16 {
    let w = width(runs).min(max);
    put(buf, right.saturating_sub(w), y, w, runs)
}

pub fn fill(buf: &mut Buffer, area: Rect, st: Style) {
    let area = area.intersection(buf.area);
    for y in area.top()..area.bottom() {
        for x in area.left()..area.right() {
            if let Some(c) = buf.cell_mut((x, y)) {
                c.reset();
                c.set_symbol(" ");
                c.set_style(st);
            }
        }
    }
}

/// Apply a background (keeping text) across a row segment.
pub fn bg(buf: &mut Buffer, area: Rect, st: Style) {
    buf.set_style(area.intersection(buf.area), st);
}

/// A horizontal rule.
pub fn hrule(buf: &mut Buffer, x: u16, y: u16, w: u16, glyph: &str, st: Style) {
    if w == 0 {
        return;
    }
    put(buf, x, y, w, &[(glyph.repeat(w as usize), st)]);
}

/// A rounded pane (TUI_SPEC §2): border, title inset in the top border,
/// optional right-hand title. Fills the inside with `fill_st`.
pub fn pane(
    buf: &mut Buffer,
    t: &Theme,
    area: Rect,
    focused: bool,
    title: &[Run],
    right: &[Run],
    fill_st: Style,
) {
    let area = area.intersection(buf.area);
    if area.width < 2 || area.height < 2 {
        return;
    }
    fill(buf, area, fill_st);
    let b = t
        .border(focused)
        .bg(fill_st.bg.unwrap_or(t.color(Token::Bg)));
    let (x0, y0, x1, y1) = (area.x, area.y, area.right() - 1, area.bottom() - 1);
    let w = area.width - 2;
    put(buf, x0, y0, 1, &[run("╭", b)]);
    hrule(buf, x0 + 1, y0, w, "─", b);
    put(buf, x1, y0, 1, &[run("╮", b)]);
    put(buf, x0, y1, 1, &[run("╰", b)]);
    hrule(buf, x0 + 1, y1, w, "─", b);
    put(buf, x1, y1, 1, &[run("╯", b)]);
    for y in y0 + 1..y1 {
        put(buf, x0, y, 1, &[run("│", b)]);
        put(buf, x1, y, 1, &[run("│", b)]);
    }
    if !title.is_empty() && area.width > 6 {
        let mut runs = vec![run(" ", b)];
        runs.extend(title.iter().cloned());
        runs.push(run(" ", b));
        put(buf, x0 + 1, y0, area.width - 4, &runs);
    }
    if !right.is_empty() && area.width > 10 {
        let mut runs = vec![run(" ", b)];
        runs.extend(right.iter().cloned());
        runs.push(run(" ", b));
        put_right(buf, x1, y0, area.width / 2, &runs);
    }
}

/// Dim everything inside `area` (the workspace behind the palette).
pub fn dim(buf: &mut Buffer, area: Rect, t: &Theme) {
    let area = area.intersection(buf.area);
    for y in area.top()..area.bottom() {
        for x in area.left()..area.right() {
            if let Some(c) = buf.cell_mut((x, y)) {
                let st = t.dimmed(c.style());
                c.set_style(st);
                if t.mode != crate::theme::ColorMode::Mono {
                    c.modifier = ratatui::style::Modifier::empty();
                }
            }
        }
    }
}

/// A drop shadow emulated by a `bg` margin around an overlay (TUI_SPEC §7).
pub fn shadow(buf: &mut Buffer, area: Rect, t: &Theme) {
    let r = Rect::new(
        area.x.saturating_sub(1),
        area.y,
        area.width + 2,
        area.height + 1,
    )
    .intersection(buf.area);
    fill(buf, r, t.base());
}
