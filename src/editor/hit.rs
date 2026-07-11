//! Hit-testing annotations and their manipulation handles.
//!
//! Text extent depends on font layout, which lives in egui — callers inject
//! a [`Measure`] closure so this module (and the editor FSM) stays testable
//! without a UI context.

use eframe::egui::{Pos2, Rect, Vec2};

use crate::annotate::{Annotation, Shape, marker_radius};
use crate::document::{AnnotationId, Document};
use crate::editor::geometry::{HANDLE_HIT, rotate_around, seg_dist, selection_handles};
use crate::editor::state::ItemDragKind;
use crate::view::View;

/// Returns the laid-out size of `text` at `font_size` (image pixels).
pub type Measure<'a> = &'a dyn Fn(&str, f32) -> Vec2;

/// Bounding box of an annotation in image coords, ignoring its rotation.
pub fn annotation_bbox(ann: &Annotation, measure: Measure) -> Rect {
    match &ann.shape {
        Shape::Pen { points } => {
            let mut r = points.first().map_or(Rect::ZERO, |p| Rect::from_min_max(*p, *p));
            for p in points {
                r.min = r.min.min(*p);
                r.max = r.max.max(*p);
            }
            r.expand(ann.style.width * 0.5)
        }
        Shape::Line { a, b } | Shape::Arrow { a, b } => {
            Rect::from_two_pos(*a, *b).expand(ann.style.width * 0.5)
        }
        Shape::Rect { rect }
        | Shape::Ellipse { rect }
        | Shape::Highlight { rect }
        | Shape::Pixelate { rect } => *rect,
        Shape::Text { pos, text } => {
            Rect::from_min_size(*pos, measure(text, ann.style.font_size))
        }
        Shape::Marker { pos, target, .. } => {
            let mut bbox =
                Rect::from_center_size(*pos, Vec2::splat(marker_radius(&ann.style) * 2.0));
            if let Some(target) = target {
                bbox = bbox.union(Rect::from_center_size(*target, Vec2::splat(ann.style.width)));
            }
            bbox
        }
    }
}

/// Whether `p` (image coords) lands on the annotation's visible geometry.
pub fn hit_annotation(p: Pos2, ann: &Annotation, bbox: Rect, zoom: f32) -> bool {
    let tol = ann.style.width * 0.5 + 6.0 / zoom;
    let pl = rotate_around(p, bbox.center(), -ann.rotation);
    match &ann.shape {
        Shape::Pen { points } => match points.len() {
            0 => false,
            1 => (pl - points[0]).length() <= tol,
            _ => points.windows(2).any(|w| seg_dist(pl, w[0], w[1]) <= tol),
        },
        Shape::Line { a, b } | Shape::Arrow { a, b } => seg_dist(pl, *a, *b) <= tol,
        Shape::Rect { rect } => {
            let c = [rect.left_top(), rect.right_top(), rect.right_bottom(), rect.left_bottom()];
            (0..4).any(|i| seg_dist(pl, c[i], c[(i + 1) % 4]) <= tol)
        }
        Shape::Ellipse { rect } => {
            let r = (rect.size() * 0.5).max(Vec2::splat(0.001));
            let v = pl - rect.center();
            let k = ((v.x / r.x).powi(2) + (v.y / r.y).powi(2)).sqrt();
            (k - 1.0).abs() * r.min_elem() <= tol
        }
        Shape::Highlight { rect } | Shape::Pixelate { rect } => rect.contains(pl),
        Shape::Text { .. } => bbox.contains(pl),
        Shape::Marker { pos, target, .. } => {
            (pl - *pos).length() <= marker_radius(&ann.style) + tol
                || target.is_some_and(|t| seg_dist(pl, *pos, t) <= tol)
        }
    }
}

/// Topmost annotation under `p`, optionally restricted to text.
pub fn topmost_hit(
    doc: &Document,
    p: Pos2,
    text_only: bool,
    zoom: f32,
    measure: Measure,
) -> Option<AnnotationId> {
    for (id, ann) in doc.annotations().iter().rev() {
        if text_only && !matches!(ann.shape, Shape::Text { .. }) {
            continue;
        }
        let bbox = annotation_bbox(ann, measure);
        if hit_annotation(p, ann, bbox, zoom) {
            return Some(*id);
        }
    }
    None
}

/// Manipulation handles for an annotation, in screen coords.
pub fn item_handles(
    ann: &Annotation,
    view: &View,
    canvas: Rect,
    measure: Measure,
) -> Vec<(Pos2, ItemDragKind)> {
    let mut out = Vec::new();
    if let Shape::Line { a, b } | Shape::Arrow { a, b } = &ann.shape {
        out.push((view.to_screen(canvas, *a), ItemDragKind::Endpoint { second: false }));
        out.push((view.to_screen(canvas, *b), ItemDragKind::Endpoint { second: true }));
        return out;
    }
    // A marker with an arrow behaves like an arrow: two draggable ends.
    if let Shape::Marker { pos, target: Some(target), .. } = &ann.shape {
        out.push((view.to_screen(canvas, *pos), ItemDragKind::Endpoint { second: false }));
        out.push((view.to_screen(canvas, *target), ItemDragKind::Endpoint { second: true }));
        return out;
    }
    let bbox = annotation_bbox(ann, measure);
    let c = bbox.center();
    // Markers scale their font size from a corner instead of stretching
    // — no distortion. Text has no resize handles at all: its size comes
    // from the Text size slider, leaving just rotate + move knobs.
    let is_text = matches!(ann.shape, Shape::Text { .. });
    let corners_only = matches!(ann.shape, Shape::Marker { .. });
    if !is_text {
        for (pos, edges) in selection_handles(bbox) {
            let corner = (edges.left || edges.right) && (edges.top || edges.bottom);
            if corners_only && !corner {
                continue;
            }
            let kind = if corners_only {
                ItemDragKind::ScaleUniform
            } else {
                ItemDragKind::Resize { edges }
            };
            out.push((view.to_screen(canvas, rotate_around(pos, c, ann.rotation)), kind));
        }
    }
    let rotatable = matches!(
        ann.shape,
        Shape::Rect { .. }
            | Shape::Ellipse { .. }
            | Shape::Highlight { .. }
            | Shape::Pen { .. }
            | Shape::Text { .. }
    );
    // A knob floats 30px outward from the midpoint of a (rotated) edge.
    let knob = |edge_mid: Pos2, fallback: Vec2| {
        let c_screen = view.to_screen(canvas, c);
        let edge = view.to_screen(canvas, rotate_around(edge_mid, c, ann.rotation));
        let dir = edge - c_screen;
        let dir = if dir.length() > 0.1 { dir.normalized() } else { fallback };
        edge + dir * 30.0
    };
    if rotatable {
        out.push((knob(bbox.center_top(), Vec2::new(0.0, -1.0)), ItemDragKind::Rotate));
    }
    // Text gets an explicit move knob (below the box, opposite the rotate
    // knob) since its corners are taken by font scaling.
    if is_text {
        out.push((knob(bbox.center_bottom(), Vec2::new(0.0, 1.0)), ItemDragKind::Move));
    }
    out
}

/// The handle under `pointer` (screen coords), if any.
pub fn handle_at(handles: &[(Pos2, ItemDragKind)], pointer: Pos2) -> Option<ItemDragKind> {
    handles
        .iter()
        .find(|(pos, _)| (pointer - *pos).abs().max_elem() <= HANDLE_HIT)
        .map(|(_, kind)| *kind)
}
