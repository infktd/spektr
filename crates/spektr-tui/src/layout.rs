//! Screen geometry (TUI_SPEC §4.1), shared by `update` (page sizes, menu
//! anchors) and the render functions.

use ratatui::layout::Rect;

/// The size floor.
pub const MIN_W: u16 = 100;
pub const MIN_H: u16 = 30;
/// Inspector overlay width on narrow terminals.
pub const INSP_OVERLAY_W: u16 = 42;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Workspace {
    pub top: Rect,
    pub entities: Rect,
    pub graph: Rect,
    /// Zero-width below 130 columns (the inspector becomes an overlay).
    pub inspector: Rect,
    pub activity: Rect,
    pub status: Rect,
}

pub fn too_small(w: u16, h: u16) -> bool {
    w < MIN_W || h < MIN_H
}

/// `(W_e, W_i)` for a terminal width.
pub fn widths(w: u16) -> (u16, u16) {
    if w >= 170 {
        (38, 48)
    } else if w >= 130 {
        (32, 42)
    } else {
        (30, 0)
    }
}

pub fn workspace(w: u16, h: u16) -> Workspace {
    let (we, wi) = widths(w);
    let main_h = h.saturating_sub(10);
    let we = we.min(w);
    let wi = wi.min(w.saturating_sub(we));
    Workspace {
        top: Rect::new(0, 0, w, 1),
        entities: Rect::new(0, 1, we, main_h),
        graph: Rect::new(we, 1, w.saturating_sub(we + wi), main_h),
        inspector: Rect::new(w.saturating_sub(wi), 1, wi, main_h),
        activity: Rect::new(0, h.saturating_sub(9), w, 8),
        status: Rect::new(0, h.saturating_sub(1), w, 1),
    }
}

/// The inspector overlay's rectangle on narrow terminals.
pub fn inspector_overlay(l: &Workspace) -> Rect {
    let w = INSP_OVERLAY_W.min(l.graph.width + l.entities.width);
    Rect::new(
        l.graph.x + l.graph.width.saturating_sub(w),
        l.graph.y,
        w,
        l.graph.height,
    )
}

/// A pane's content area: inside the border, with one cell of padding on
/// each side (TUI_SPEC §2).
pub fn inner(r: Rect) -> Rect {
    Rect::new(
        r.x.saturating_add(2),
        r.y.saturating_add(1),
        r.width.saturating_sub(4),
        r.height.saturating_sub(2),
    )
}

/// Rows of the entities list (below the filter line and a gap).
pub fn entity_rows(l: &Workspace) -> u16 {
    inner(l.entities).height.saturating_sub(2)
}

/// Centered rectangle, clamped to `area`.
pub fn centered(area: Rect, w: u16, h: u16) -> Rect {
    let w = w.min(area.width);
    let h = h.min(area.height);
    Rect::new(
        area.x + (area.width - w) / 2,
        area.y + (area.height - h) / 2,
        w,
        h,
    )
}

/// The command palette: ≈ 96 × 22 at 1/6 of the height (TUI_SPEC §5).
pub fn palette(w: u16, h: u16) -> Rect {
    let pw = 96.min(w.saturating_sub(4));
    let ph = 22.min(h.saturating_sub(4));
    Rect::new((w - pw) / 2, (h / 6).min(h.saturating_sub(ph)), pw, ph)
}
