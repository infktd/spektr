//! Image previews in the inspector, the edge comparison and Review
//! (TUI_SPEC §9). Never in list rows.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::widgets::Widget;
use ratatui_image::Image;

use super::draw::{self, run};
use crate::glyph::Marker;
use crate::images::{Key, Slot};
use crate::layout;
use crate::state::{AppState, View, Workspace};

/// The inspector's preview area: at most 40% of its height.
pub fn preview_size(inner: Rect) -> (u16, u16) {
    (inner.width, (inner.height * 2 / 5).max(3))
}

/// Each side of a side-by-side comparison.
pub fn pair_size(width: u16, height: u16) -> (u16, u16) {
    (width.saturating_sub(2) / 2, (height * 2 / 5).clamp(3, 12))
}

pub fn key(sha: &str, (w, h): (u16, u16)) -> Key {
    Key {
        sha: sha.to_string(),
        w,
        h,
    }
}

/// Draw an image at `(x, y)` fitted into `size`; returns the rows used.
/// While it's encoding: `loading image…`; on failure: `▨ name · preview
/// unavailable`. Nothing when images are off.
pub fn draw(
    buf: &mut Buffer,
    s: &AppState,
    x: u16,
    y: u16,
    size: (u16, u16),
    sha: &str,
    name: &str,
) -> u16 {
    if !s.images.enabled || size.0 == 0 || size.1 == 0 {
        return 0;
    }
    let t = &s.theme;
    match s.images.get(&key(sha, size)) {
        Some(Slot::Ready(r)) => {
            let area = Rect::new(x, y, r.w.min(size.0), r.h.min(size.1)).intersection(buf.area);
            Image::new(&r.proto).render(area, buf);
            area.height
        }
        Some(Slot::Failed(_)) => {
            draw::put(
                buf,
                x,
                y,
                size.0,
                &[run(
                    format!(
                        "{} {name} {} preview unavailable",
                        s.glyphs.m(Marker::Image),
                        s.glyphs.m(Marker::Dot)
                    ),
                    t.muted(),
                )],
            );
            1
        }
        _ => {
            draw::put(buf, x, y, size.0, &[run("loading image…", t.muted())]);
            1
        }
    }
}

/// The inspector's rectangle, if it's on screen.
pub fn inspector_rect(s: &AppState, w: &Workspace) -> Option<Rect> {
    let l = layout::workspace(s.size.0, s.size.1);
    if l.inspector.width > 0 {
        Some(l.inspector)
    } else if w.insp_overlay {
        Some(layout::inspector_overlay(&l))
    } else {
        None
    }
}

/// The images the current frame shows, so the runtime can encode them.
pub fn wanted(s: &AppState) -> Vec<Key> {
    let mut out = Vec::new();
    if !s.images.enabled {
        return out;
    }
    let Some(w) = s.workspace() else { return out };
    match w.view {
        View::Main => {
            let Some(r) = inspector_rect(s, w) else {
                return out;
            };
            let inner = layout::inner(r);
            if let Some(e) = w.edge.as_ref().filter(|_| w.inspect_edge.is_some()) {
                let size = pair_size(inner.width, inner.height);
                for sha in [&e.a_image, &e.b_image].into_iter().flatten() {
                    out.push(key(sha, size));
                }
            } else if let Some(sha) = w.detail.as_ref().and_then(|d| d.image.as_ref()) {
                out.push(key(sha, preview_size(inner)));
            }
        }
        View::Review => {
            if let Some((_, Some(a), Some(b))) = &w.review.compare
                && let (Some(x), Some(y)) = (&a.image, &b.image)
            {
                let (_, right) = layout::review(s.size.0, s.size.1);
                let inner = layout::inner(right);
                let size = pair_size(inner.width, inner.height);
                out.push(key(x, size));
                out.push(key(y, size));
            }
        }
        View::Checklist => {}
    }
    out
}
