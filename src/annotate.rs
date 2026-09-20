//! Annotation model and the geometry shared by the on-screen (egui) and
//! export (tiny-skia) renderers. All coordinates are in image-pixel space.

use eframe::egui::{Color32, Pos2, Rect, Vec2};
use image::RgbaImage;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tool {
    /// Adjust the capture region (move / resize / redraw) instead of drawing.
    Select,
    Pen,
    Line,
    Arrow,
    Rect,
    Ellipse,
    Highlight,
    Pixelate,
    Text,
    Marker,
}

impl Tool {
    pub fn label(self) -> &'static str {
        match self {
            Tool::Select => "Select",
            Tool::Pen => "Pen",
            Tool::Line => "Line",
            Tool::Arrow => "Arrow",
            Tool::Rect => "Box",
            Tool::Ellipse => "Ellipse",
            Tool::Highlight => "Highlight",
            Tool::Pixelate => "Blur",
            Tool::Text => "Text",
            Tool::Marker => "Marker",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Style {
    pub color: Color32,
    pub width: f32,
    pub font_size: f32,
}

#[derive(Clone, PartialEq, Debug)]
pub enum Shape {
    Pen { points: Vec<Pos2> },
    Line { a: Pos2, b: Pos2 },
    Arrow { a: Pos2, b: Pos2 },
    Rect { rect: Rect },
    Ellipse { rect: Rect },
    Highlight { rect: Rect },
    Pixelate { rect: Rect },
    /// Never soft-wrapped: lines break only at typed newlines.
    Text { pos: Pos2, text: String },
    /// Numbered circle; `target` grows an arrow out of it (one object — an
    /// arrow with a fat numbered tail; each end drags independently).
    Marker { pos: Pos2, number: u32, target: Option<Pos2> },
}

#[derive(Clone, PartialEq, Debug)]
pub struct Annotation {
    pub shape: Shape,
    pub style: Style,
    /// Radians, applied around the shape's bounding-box center. Only
    /// rect-like shapes and text store an angle; pen/line/arrow bake
    /// rotation into their points, and pixelate/marker never rotate.
    pub rotation: f32,
}

impl Annotation {
    pub fn new(shape: Shape, style: Style) -> Self {
        Self { shape, style, rotation: 0.0 }
    }
}

/// Alpha used for highlight fills; the stored color keeps full alpha so the
/// swatch UI stays readable.
pub fn highlight_color(color: Color32) -> Color32 {
    Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), 70)
}

pub struct ArrowGeometry {
    /// Shaft endpoint pulled back so it doesn't poke through the head.
    pub shaft_end: Pos2,
    pub head: [Pos2; 3],
}

pub fn arrow_geometry(a: Pos2, b: Pos2, stroke_width: f32) -> ArrowGeometry {
    let dir = b - a;
    let len = dir.length().max(0.001);
    let dir = dir / len;
    // Head ≈ 6× stroke width, at least 14px, but never longer than the shaft
    // (chained min/max instead of clamp: `len` may be tiny mid-drag).
    let head_len = (stroke_width * 6.0).max(14.0).min(len);
    let head_width = head_len * 0.7;
    let base = b - dir * head_len;
    let normal = Vec2::new(-dir.y, dir.x);
    ArrowGeometry {
        shaft_end: b - dir * (head_len * 0.6),
        head: [b, base + normal * head_width * 0.5, base - normal * head_width * 0.5],
    }
}

pub fn marker_radius(style: &Style) -> f32 {
    style.font_size * 0.9
}

/// A marker drag too short to clear the circle is a plain drop. One rule for
/// the live preview and the committed shape, so they can't disagree.
pub fn marker_target(pos: Pos2, target: Option<Pos2>, style: &Style) -> Option<Pos2> {
    let min_len = marker_radius(style) * 1.6;
    target.filter(|t| (*t - pos).length() >= min_len)
}

/// Block size for pixelation, derived from the image so the effect reads the
/// same on HiDPI screenshots and small crops alike.
pub fn pixelate_block(img_w: u32, img_h: u32) -> u32 {
    (img_w.min(img_h) / 120).clamp(8, 48)
}

/// Clamp an f32 image coordinate to a valid pixel index in `0..=limit`.
pub fn clamp_px(v: f32, limit: u32) -> u32 {
    if v <= 0.0 {
        return 0;
    }
    let limit_f = limit.min(1 << 24);
    if v >= limit_f as f32 { limit_f } else { v as u32 }
}

/// Mosaic `rect` (image coords) in place. Shared by the display compositor
/// and the PNG exporter so both surfaces show identical pixels.
pub fn apply_pixelate(img: &mut RgbaImage, rect: Rect, block: u32) {
    let (w, h) = img.dimensions();
    let x0 = clamp_px(rect.min.x.floor(), w);
    let y0 = clamp_px(rect.min.y.floor(), h);
    let x1 = clamp_px(rect.max.x.ceil(), w);
    let y1 = clamp_px(rect.max.y.ceil(), h);
    if x1 <= x0 || y1 <= y0 {
        return;
    }
    let mut by = y0;
    while by < y1 {
        let bh = block.min(y1 - by);
        let mut bx = x0;
        while bx < x1 {
            let bw = block.min(x1 - bx);
            // Average premultiplied colors so hidden RGB in transparent
            // pixels cannot darken or tint the visible mosaic. Return straight
            // RGBA for the shared display/export image buffer.
            let mut sum = [0u64; 3];
            let mut alpha = 0_u64;
            for y in by..by + bh {
                for x in bx..bx + bw {
                    let p = img.get_pixel(x, y).0;
                    let a = u64::from(p[3]);
                    sum[0] += u64::from(p[0]) * a;
                    sum[1] += u64::from(p[1]) * a;
                    sum[2] += u64::from(p[2]) * a;
                    alpha += a;
                }
            }
            let n = u64::from(bw) * u64::from(bh);
            let channel = |s: u64| u8::try_from(s / alpha.max(1)).unwrap_or(u8::MAX);
            let avg = image::Rgba([
                channel(sum[0]),
                channel(sum[1]),
                channel(sum[2]),
                u8::try_from((alpha + n / 2) / n).unwrap_or(u8::MAX),
            ]);
            for y in by..by + bh {
                for x in bx..bx + bw {
                    img.put_pixel(x, y, avg);
                }
            }
            bx += bw;
        }
        by += bh;
    }
}

/// Apply every pixelate annotation onto `img`.
pub fn composite_pixelates<'a>(
    img: &mut RgbaImage,
    annotations: impl IntoIterator<Item = &'a Annotation>,
) {
    let (w, h) = img.dimensions();
    let block = pixelate_block(w, h);
    for ann in annotations {
        if let Shape::Pixelate { rect } = &ann.shape {
            apply_pixelate(img, *rect, block);
        }
    }
}

/// The pixelate regions currently in `annotations`, used to detect when the
/// composited base texture must be rebuilt.
pub fn pixelate_rects<'a>(annotations: impl IntoIterator<Item = &'a Annotation>) -> Vec<Rect> {
    annotations
        .into_iter()
        .filter_map(|a| match &a.shape {
            Shape::Pixelate { rect } => Some(*rect),
            _ => None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pixelation_preserves_transparency_and_ignores_hidden_colors() {
        let mut img = RgbaImage::from_fn(4, 2, |x, _| match x {
            0 => image::Rgba([255, 0, 0, 0]),
            1 => image::Rgba([0, 0, 255, 255]),
            _ => image::Rgba([90, 120, 180, 0]),
        });
        apply_pixelate(
            &mut img,
            Rect::from_min_max(Pos2::ZERO, Pos2::new(4.0, 2.0)),
            2,
        );
        for y in 0..2 {
            for x in 0..2 {
                assert_eq!(*img.get_pixel(x, y), image::Rgba([0, 0, 255, 128]));
            }
            for x in 2..4 {
                assert_eq!(*img.get_pixel(x, y), image::Rgba([0, 0, 0, 0]));
            }
        }
    }

    #[test]
    fn opaque_pixelation_keeps_existing_average_and_region_bounds() {
        let mut img = RgbaImage::from_fn(3, 2, |x, _| match x {
            0 => image::Rgba([20, 40, 60, 255]),
            1 => image::Rgba([31, 51, 71, 255]),
            _ => image::Rgba([255, 0, 0, 255]),
        });
        apply_pixelate(
            &mut img,
            Rect::from_min_max(Pos2::ZERO, Pos2::new(2.0, 2.0)),
            2,
        );
        for y in 0..2 {
            assert_eq!(*img.get_pixel(0, y), image::Rgba([25, 45, 65, 255]));
            assert_eq!(*img.get_pixel(1, y), image::Rgba([25, 45, 65, 255]));
            assert_eq!(*img.get_pixel(2, y), image::Rgba([255, 0, 0, 255]));
        }
    }
}
