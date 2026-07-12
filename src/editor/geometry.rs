//! Pure geometry shared by hit-testing, dragging, and chrome painting.

use eframe::egui::{self, Pos2, Rect, Vec2};

use crate::annotate::Shape;

/// Screen-px side length of a region handle.
pub const REGION_HANDLE_SIZE: f32 = 18.0;
/// Screen-px side length of an item handle.
pub const ITEM_HANDLE_SIZE: f32 = 14.0;
/// Screen-px radius of the rotate/move knobs.
pub const KNOB_RADIUS: f32 = 12.0;
/// Screen-px half-width of the box around a handle that grabs it.
pub const HANDLE_HIT: f32 = 18.0;

/// Which edges of a box a resize drag moves.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ResizeEdges {
    pub left: bool,
    pub right: bool,
    pub top: bool,
    pub bottom: bool,
}

/// Corner handles first so they win over edge handles on small boxes.
pub fn selection_handles(r: Rect) -> [(Pos2, ResizeEdges); 8] {
    let edges = |left, right, top, bottom| ResizeEdges { left, right, top, bottom };
    [
        (r.left_top(), edges(true, false, true, false)),
        (r.right_top(), edges(false, true, true, false)),
        (r.left_bottom(), edges(true, false, false, true)),
        (r.right_bottom(), edges(false, true, false, true)),
        (r.center_top(), edges(false, false, true, false)),
        (r.center_bottom(), edges(false, false, false, true)),
        (r.left_center(), edges(true, false, false, false)),
        (r.right_center(), edges(false, true, false, false)),
    ]
}

/// The region handle under `pointer` (both in screen coords), if any.
pub fn region_handle_at(sel_screen: Rect, pointer: Pos2) -> Option<ResizeEdges> {
    selection_handles(sel_screen)
        .into_iter()
        .find(|(pos, _)| (pointer - *pos).abs().max_elem() <= HANDLE_HIT)
        .map(|(_, edges)| edges)
}

/// Screen-px radius of the region's move grip.
pub const REGION_GRIP_RADIUS: f32 = 11.0;

/// Center (screen coords) of the region's move grip: on the top edge, just
/// left of the top-right resize handle. It is the way to move the region now
/// that a plain drag inside it rubber-band-selects instead.
pub fn region_move_grip(sel_screen: Rect) -> Pos2 {
    Pos2::new(sel_screen.right() - REGION_HANDLE_SIZE - 14.0, sel_screen.top())
}

/// Whether `pointer` (screen coords) is on the region's move grip. Checked
/// before the resize handles, so its small zone wins where the two touch.
pub fn region_grip_at(sel_screen: Rect, pointer: Pos2) -> bool {
    (pointer - region_move_grip(sel_screen)).abs().max_elem() <= 14.0
}

pub fn edges_cursor(e: ResizeEdges) -> egui::CursorIcon {
    use egui::CursorIcon::*;
    match (e.left, e.right, e.top, e.bottom) {
        (true, _, true, _) | (_, true, _, true) => ResizeNwSe,
        (true, _, _, true) | (_, true, true, _) => ResizeNeSw,
        (true, ..) | (_, true, ..) => ResizeHorizontal,
        _ => ResizeVertical,
    }
}

/// Rebuild the box from its pre-drag rect with the dragged edges following
/// the pointer. Always derived from `start`, so dragging an edge across the
/// opposite one inverts cleanly instead of flipping which edge is held.
pub fn resize_rect(start: Rect, edges: ResizeEdges, p: Pos2) -> Rect {
    let (x0, x1) = if edges.left {
        (p.x, start.max.x)
    } else if edges.right {
        (start.min.x, p.x)
    } else {
        (start.min.x, start.max.x)
    };
    let (y0, y1) = if edges.top {
        (p.y, start.max.y)
    } else if edges.bottom {
        (start.min.y, p.y)
    } else {
        (start.min.y, start.max.y)
    };
    Rect::from_two_pos(Pos2::new(x0, y0), Pos2::new(x1, y1))
}

/// Translate `r` the minimal amount to sit inside `bounds` (assumes it fits).
pub fn clamp_rect_within(r: Rect, bounds: Rect) -> Rect {
    let mut off = Vec2::ZERO;
    if r.min.x < bounds.min.x {
        off.x = bounds.min.x - r.min.x;
    } else if r.max.x > bounds.max.x {
        off.x = bounds.max.x - r.max.x;
    }
    if r.min.y < bounds.min.y {
        off.y = bounds.min.y - r.min.y;
    } else if r.max.y > bounds.max.y {
        off.y = bounds.max.y - r.max.y;
    }
    r.translate(off)
}

pub fn rotate_around(p: Pos2, c: Pos2, angle: f32) -> Pos2 {
    let (sin, cos) = angle.sin_cos();
    let v = p - c;
    c + Vec2::new(v.x * cos - v.y * sin, v.x * sin + v.y * cos)
}

/// Distance from `p` to the segment `a`–`b`.
pub fn seg_dist(p: Pos2, a: Pos2, b: Pos2) -> f32 {
    let ab = b - a;
    let len_sq = ab.length_sq();
    if len_sq <= f32::EPSILON {
        return (p - a).length();
    }
    let t = ((p - a).dot(ab) / len_sq).clamp(0.0, 1.0);
    (a + ab * t - p).length()
}

pub fn translate_shape(shape: &mut Shape, d: Vec2) {
    match shape {
        Shape::Pen { points } => {
            for p in points {
                *p += d;
            }
        }
        Shape::Line { a, b } | Shape::Arrow { a, b } => {
            *a += d;
            *b += d;
        }
        Shape::Rect { rect }
        | Shape::Ellipse { rect }
        | Shape::Highlight { rect }
        | Shape::Pixelate { rect } => *rect = rect.translate(d),
        Shape::Text { pos, .. } => *pos += d,
        Shape::Marker { pos, target, .. } => {
            *pos += d;
            if let Some(target) = target {
                *target += d;
            }
        }
    }
}

/// The up-to-four rectangles of `outer` not covered by `inner`.
pub fn subtract_rect(outer: Rect, inner: Rect) -> Vec<Rect> {
    let inner = inner.intersect(outer);
    if !inner.is_positive() {
        return vec![outer];
    }
    let mut parts = Vec::new();
    let top = Rect::from_min_max(outer.min, Pos2::new(outer.max.x, inner.min.y));
    let bottom = Rect::from_min_max(Pos2::new(outer.min.x, inner.max.y), outer.max);
    let left = Rect::from_min_max(
        Pos2::new(outer.min.x, inner.min.y),
        Pos2::new(inner.min.x, inner.max.y),
    );
    let right = Rect::from_min_max(
        Pos2::new(inner.max.x, inner.min.y),
        Pos2::new(outer.max.x, inner.max.y),
    );
    for r in [top, bottom, left, right] {
        if r.is_positive() {
            parts.push(r);
        }
    }
    parts
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resize_rect_moves_only_grabbed_edges() {
        let start = Rect::from_min_max(Pos2::new(10.0, 10.0), Pos2::new(50.0, 40.0));
        let edges = ResizeEdges { left: false, right: true, top: false, bottom: true };
        let r = resize_rect(start, edges, Pos2::new(80.0, 70.0));
        assert_eq!(r, Rect::from_min_max(Pos2::new(10.0, 10.0), Pos2::new(80.0, 70.0)));
        // Dragging an edge across the opposite one inverts cleanly.
        let r = resize_rect(start, edges, Pos2::new(0.0, 0.0));
        assert_eq!(r, Rect::from_min_max(Pos2::new(0.0, 0.0), Pos2::new(10.0, 10.0)));
    }

    #[test]
    fn rotate_around_quarter_turn() {
        let p = rotate_around(Pos2::new(2.0, 0.0), Pos2::ZERO, std::f32::consts::FRAC_PI_2);
        assert!((p.x - 0.0).abs() < 1e-4 && (p.y - 2.0).abs() < 1e-4);
        // Inverse rotation returns the original point.
        let back = rotate_around(p, Pos2::ZERO, -std::f32::consts::FRAC_PI_2);
        assert!((back.x - 2.0).abs() < 1e-4 && back.y.abs() < 1e-4);
    }

    #[test]
    fn seg_dist_basics() {
        let (a, b) = (Pos2::new(0.0, 0.0), Pos2::new(10.0, 0.0));
        assert!((seg_dist(Pos2::new(5.0, 3.0), a, b) - 3.0).abs() < 1e-5);
        assert!((seg_dist(Pos2::new(-4.0, 0.0), a, b) - 4.0).abs() < 1e-5);
        assert!((seg_dist(Pos2::new(1.0, 1.0), a, a) - 2f32.sqrt()).abs() < 1e-5);
    }

    #[test]
    fn clamp_rect_within_bounds() {
        let bounds = Rect::from_min_max(Pos2::ZERO, Pos2::new(100.0, 100.0));
        let r = Rect::from_min_max(Pos2::new(-10.0, 95.0), Pos2::new(20.0, 125.0));
        let c = clamp_rect_within(r, bounds);
        assert_eq!(c.min, Pos2::new(0.0, 70.0));
        assert_eq!(c.size(), r.size());
    }

    #[test]
    fn translate_shape_moves_everything() {
        let d = Vec2::new(5.0, -3.0);
        let mut line = Shape::Line { a: Pos2::ZERO, b: Pos2::new(1.0, 1.0) };
        translate_shape(&mut line, d);
        assert_eq!(line, Shape::Line { a: Pos2::new(5.0, -3.0), b: Pos2::new(6.0, -2.0) });
        let mut rect = Shape::Rect { rect: Rect::from_min_max(Pos2::ZERO, Pos2::new(2.0, 2.0)) };
        translate_shape(&mut rect, d);
        assert_eq!(
            rect,
            Shape::Rect { rect: Rect::from_min_max(Pos2::new(5.0, -3.0), Pos2::new(7.0, -1.0)) }
        );
    }
}
