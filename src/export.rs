//! Rasterize annotations onto the screenshot for saving/copying. Geometry
//! comes from `annotate` so this stays in lockstep with the egui renderer;
//! text uses the same embedded font egui draws with on screen.

use ab_glyph::{Font, FontRef, PxScale, ScaleFont};
use anyhow::{Context, Result};
use eframe::egui::{Color32, Pos2};
use image::RgbaImage;
use tiny_skia::{
    FillRule, FilterQuality, LineCap, LineJoin, Paint, PathBuilder, Pixmap, PixmapPaint, Stroke,
    Transform,
};

use crate::annotate::{
    Annotation, Shape, arrow_geometry, clamp_px, composite_pixelates, highlight_color,
    marker_radius,
};

/// Save `img` into `dir` under the timestamped scrannotate name, creating
/// the directory if needed. Returns the written path.
pub fn save_timestamped(img: &RgbaImage, dir: &std::path::Path) -> Result<std::path::PathBuf> {
    std::fs::create_dir_all(dir)
        .with_context(|| format!("creating {}", dir.display()))?;
    let name = format!("scrannotate-{}.png", chrono::Local::now().format("%Y-%m-%d_%H%M%S"));
    let path = dir.join(name);
    img.save(&path).with_context(|| format!("saving {}", path.display()))?;
    Ok(path)
}

pub fn render_to_image<'a>(
    base: &RgbaImage,
    annotations: impl IntoIterator<Item = &'a Annotation> + Clone,
    crop: Option<eframe::egui::Rect>,
) -> Result<RgbaImage> {
    let mut img = base.clone();
    composite_pixelates(&mut img, annotations.clone());

    let (w, h) = img.dimensions();
    let size = tiny_skia::IntSize::from_wh(w, h).context("empty image")?;
    // The screenshot is fully opaque, so straight RGBA == premultiplied RGBA
    // and the buffer can round-trip through tiny-skia unchanged.
    let mut pixmap = Pixmap::from_vec(img.into_raw(), size).context("building pixmap")?;

    let font = FontRef::try_from_slice(epaint_default_fonts::UBUNTU_LIGHT)
        .context("loading embedded font")?;

    for ann in annotations {
        draw_annotation(&mut pixmap, &font, ann);
    }

    let full = RgbaImage::from_raw(w, h, pixmap.take())
        .context("pixmap buffer size mismatch")?;

    let Some(crop) = crop else { return Ok(full) };
    let x0 = clamp_px(crop.min.x.round(), w.saturating_sub(1));
    let y0 = clamp_px(crop.min.y.round(), h.saturating_sub(1));
    let cw = clamp_px(crop.width().round(), w - x0).max(1);
    let ch = clamp_px(crop.height().round(), h - y0).max(1);
    Ok(image::imageops::crop_imm(&full, x0, y0, cw, ch).to_image())
}

fn solid_paint(color: Color32) -> Paint<'static> {
    let [r, g, b, a] = color.to_srgba_unmultiplied();
    let mut paint = Paint::default();
    paint.set_color_rgba8(r, g, b, a);
    paint.anti_alias = true;
    paint
}

fn round_stroke(width: f32) -> Stroke {
    Stroke {
        width,
        line_cap: LineCap::Round,
        line_join: LineJoin::Round,
        ..Stroke::default()
    }
}

fn skia_rect(rect: eframe::egui::Rect) -> Option<tiny_skia::Rect> {
    tiny_skia::Rect::from_ltrb(rect.min.x, rect.min.y, rect.max.x, rect.max.y)
}

/// Rigid rotation around a shape's center (stroke widths are unaffected).
fn rotation_transform(rotation: f32, center: Pos2) -> Transform {
    if rotation == 0.0 {
        Transform::identity()
    } else {
        Transform::from_rotate_at(rotation.to_degrees(), center.x, center.y)
    }
}

fn draw_annotation(pixmap: &mut Pixmap, font: &FontRef<'_>, ann: &Annotation) {
    let style = &ann.style;
    let paint = solid_paint(style.color);
    let stroke = round_stroke(style.width);
    let identity = Transform::identity();

    match &ann.shape {
        Shape::Pen { points } => {
            if points.len() < 2 {
                if let Some(p) = points.first()
                    && let Some(dot) = PathBuilder::from_circle(p.x, p.y, style.width * 0.5)
                {
                    pixmap.fill_path(&dot, &paint, FillRule::Winding, identity, None);
                }
                return;
            }
            let mut pb = PathBuilder::new();
            pb.move_to(points[0].x, points[0].y);
            for p in &points[1..] {
                pb.line_to(p.x, p.y);
            }
            if let Some(path) = pb.finish() {
                pixmap.stroke_path(&path, &paint, &stroke, identity, None);
            }
        }
        Shape::Line { a, b } => {
            let mut pb = PathBuilder::new();
            pb.move_to(a.x, a.y);
            pb.line_to(b.x, b.y);
            if let Some(path) = pb.finish() {
                pixmap.stroke_path(&path, &paint, &stroke, identity, None);
            }
        }
        Shape::Arrow { a, b } => {
            draw_arrow(pixmap, &paint, &stroke, *a, *b, style.width);
        }
        Shape::Rect { rect } => {
            if let Some(r) = skia_rect(*rect) {
                let path = PathBuilder::from_rect(r);
                let transform = rotation_transform(ann.rotation, rect.center());
                pixmap.stroke_path(&path, &paint, &stroke, transform, None);
            }
        }
        Shape::Ellipse { rect } => {
            if let Some(r) = skia_rect(*rect)
                && let Some(path) = PathBuilder::from_oval(r)
            {
                let transform = rotation_transform(ann.rotation, rect.center());
                pixmap.stroke_path(&path, &paint, &stroke, transform, None);
            }
        }
        Shape::Highlight { rect } => {
            if let Some(r) = skia_rect(*rect) {
                let path = PathBuilder::from_rect(r);
                let fill = solid_paint(highlight_color(style.color));
                let transform = rotation_transform(ann.rotation, rect.center());
                pixmap.fill_path(&path, &fill, FillRule::Winding, transform, None);
            }
        }
        // Pixelation is baked into the base image before vector drawing.
        Shape::Pixelate { .. } => {}
        Shape::Text { pos, text } => {
            if ann.rotation == 0.0 {
                draw_text(pixmap, font, *pos, text, style.font_size, style.width, style.color);
            } else {
                draw_text_rotated(
                    pixmap,
                    font,
                    *pos,
                    text,
                    style.font_size,
                    style.width,
                    style.color,
                    ann.rotation,
                );
            }
        }
        Shape::Marker { pos, number, target } => {
            // Arrow first; the circle covers the shaft's root.
            if let Some(target) = target {
                draw_arrow(pixmap, &paint, &stroke, *pos, *target, style.width);
            }
            let radius = marker_radius(style);
            if let Some(circle) = PathBuilder::from_circle(pos.x, pos.y, radius) {
                pixmap.fill_path(&circle, &paint, FillRule::Winding, identity, None);
            }
            draw_text_centered(pixmap, font, *pos, &number.to_string(), style.font_size, Color32::WHITE);
        }
    }
}

/// Shaft + filled head from `a` to `b` (shared by arrows and markers).
fn draw_arrow(pixmap: &mut Pixmap, paint: &Paint<'_>, stroke: &Stroke, a: Pos2, b: Pos2, width: f32) {
    let geo = arrow_geometry(a, b, width);
    let identity = Transform::identity();
    let mut pb = PathBuilder::new();
    pb.move_to(a.x, a.y);
    pb.line_to(geo.shaft_end.x, geo.shaft_end.y);
    if let Some(path) = pb.finish() {
        pixmap.stroke_path(&path, paint, stroke, identity, None);
    }
    let mut pb = PathBuilder::new();
    pb.move_to(geo.head[0].x, geo.head[0].y);
    pb.line_to(geo.head[1].x, geo.head[1].y);
    pb.line_to(geo.head[2].x, geo.head[2].y);
    pb.close();
    if let Some(path) = pb.finish() {
        pixmap.fill_path(&path, paint, FillRule::Winding, identity, None);
    }
}

/// Match epaint's sizing: a FontId size is the em size in pixels, while
/// ab_glyph's PxScale is the full glyph-height, so convert via font metrics.
fn px_scale(font: &FontRef<'_>, size: f32) -> PxScale {
    let units_per_em = font.units_per_em().unwrap_or(font.height_unscaled());
    PxScale::from(size * font.height_unscaled() / units_per_em)
}

fn line_width(font: &FontRef<'_>, scale: PxScale, line: &str) -> f32 {
    let scaled = font.as_scaled(scale);
    let mut width = 0.0;
    let mut prev = None;
    for ch in line.chars() {
        let id = scaled.glyph_id(ch);
        if let Some(prev) = prev {
            width += scaled.kern(prev, id);
        }
        width += scaled.h_advance(id);
        prev = Some(id);
    }
    width
}

fn draw_text(
    pixmap: &mut Pixmap,
    font: &FontRef<'_>,
    pos: Pos2,
    text: &str,
    size: f32,
    width: f32,
    color: Color32,
) {
    let scale = px_scale(font, size);
    let scaled = font.as_scaled(scale);
    let line_height = scaled.height() + scaled.line_gap();
    // Fake bold: stamp each line over the bold offsets (matches the on-screen
    // renderer in ui/paint.rs).
    let offsets = crate::annotate::bold_offsets(crate::annotate::text_bold(width));
    let mut baseline = pos.y + scaled.ascent();
    for line in text.split('\n') {
        for (ox, oy) in &offsets {
            draw_text_line(
                pixmap,
                font,
                Pos2::new(pos.x + ox, baseline + oy),
                line,
                scale,
                color,
            );
        }
        baseline += line_height;
    }
}

/// Width/height of a text block, matching `draw_text`'s layout.
fn text_block_size(font: &FontRef<'_>, text: &str, size: f32) -> (f32, f32) {
    let scale = px_scale(font, size);
    let scaled = font.as_scaled(scale);
    let line_height = scaled.height() + scaled.line_gap();
    let mut width: f32 = 0.0;
    let mut lines: f32 = 0.0;
    for line in text.split('\n') {
        width = width.max(line_width(font, scale, line));
        lines += 1.0;
    }
    (width, line_height * lines)
}

/// ab_glyph can't rasterize at an angle, so rotated text renders into a
/// transparent scratch pixmap that gets blitted with a rotate transform.
#[allow(clippy::too_many_arguments)]
fn draw_text_rotated(
    pixmap: &mut Pixmap,
    font: &FontRef<'_>,
    pos: Pos2,
    text: &str,
    size: f32,
    width: f32,
    color: Color32,
    rotation: f32,
) {
    let (w, h) = text_block_size(font, text, size);
    // Padding for glyph overhang (italic-ish curves, descenders past the
    // metric box).
    let pad = (size * 0.5).ceil().max(4.0);
    let tw = clamp_px((w + pad * 2.0).ceil(), 1 << 14).max(1);
    let th = clamp_px((h + pad * 2.0).ceil(), 1 << 14).max(1);
    let Some(mut temp) = Pixmap::new(tw, th) else {
        draw_text(pixmap, font, pos, text, size, width, color);
        return;
    };
    draw_text(&mut temp, font, Pos2::new(pad, pad), text, size, width, color);
    let center = Pos2::new(pos.x + w * 0.5, pos.y + h * 0.5);
    let paint = PixmapPaint { quality: FilterQuality::Bilinear, ..PixmapPaint::default() };
    pixmap.draw_pixmap(
        (pos.x - pad).round() as i32,
        (pos.y - pad).round() as i32,
        temp.as_ref(),
        &paint,
        rotation_transform(rotation, center),
        None,
    );
}

fn draw_text_centered(
    pixmap: &mut Pixmap,
    font: &FontRef<'_>,
    center: Pos2,
    text: &str,
    size: f32,
    color: Color32,
) {
    let scale = px_scale(font, size);
    let scaled = font.as_scaled(scale);
    let width = line_width(font, scale, text);
    let baseline = center.y + (scaled.ascent() + scaled.descent()) * 0.5;
    draw_text_line(
        pixmap,
        font,
        Pos2::new(center.x - width * 0.5, baseline),
        text,
        scale,
        color,
    );
}

/// Rasterize one line with its baseline-left at `origin`.
fn draw_text_line(
    pixmap: &mut Pixmap,
    font: &FontRef<'_>,
    origin: Pos2,
    line: &str,
    scale: PxScale,
    color: Color32,
) {
    let scaled = font.as_scaled(scale);
    let [r, g, b, a] = color.to_srgba_unmultiplied();
    let (w, h) = (pixmap.width(), pixmap.height());
    let mut x = origin.x;
    let mut prev = None;
    for ch in line.chars() {
        let id = scaled.glyph_id(ch);
        if let Some(prev) = prev {
            x += scaled.kern(prev, id);
        }
        let glyph = id.with_scale_and_position(scale, ab_glyph::point(x, origin.y));
        x += scaled.h_advance(id);
        prev = Some(id);
        let Some(outlined) = font.outline_glyph(glyph) else { continue };
        let bounds = outlined.px_bounds();
        let data = pixmap.data_mut();
        outlined.draw(|gx, gy, coverage| {
            let px = bounds.min.x + gx as f32;
            let py = bounds.min.y + gy as f32;
            if px < 0.0 || py < 0.0 || px >= w as f32 || py >= h as f32 {
                return;
            }
            let idx = 4 * (clamp_px(py, h - 1) as usize * w as usize + clamp_px(px, w - 1) as usize);
            let alpha = coverage * (f32::from(a) / 255.0);
            if alpha <= 0.0 {
                return;
            }
            // Premultiplied source-over; also accumulates the alpha channel
            // so text works on the transparent scratch pixmap rotated text
            // renders through (a no-op on the opaque screenshot).
            let blend = |dst: u8, src: u8| -> u8 {
                let v = f32::from(src) * alpha + f32::from(dst) * (1.0 - alpha);
                u8::try_from((v.round().clamp(0.0, 255.0)) as i32).unwrap_or(u8::MAX)
            };
            data[idx] = blend(data[idx], r);
            data[idx + 1] = blend(data[idx + 1], g);
            data[idx + 2] = blend(data[idx + 2], b);
            data[idx + 3] = blend(data[idx + 3], 255);
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::annotate::Style;
    use eframe::egui::{Pos2, Rect};

    fn style(color: Color32) -> Style {
        Style { color, width: 4.0, font_size: 24.0 }
    }

    #[test]
    fn render_all_annotation_kinds() {
        let base = RgbaImage::from_fn(900, 600, |x, y| {
            let checker = if (x / 40 + y / 40) % 2 == 0 { 40 } else { 70 };
            image::Rgba([
                u8::try_from(x * 255 / 900).unwrap_or(255),
                checker,
                u8::try_from(y * 255 / 600).unwrap_or(255),
                255,
            ])
        });
        let red = Color32::from_rgb(0xe0, 0x2d, 0x2d);
        let yellow = Color32::from_rgb(0xf2, 0xd0, 0x2e);
        let blue = Color32::from_rgb(0x2f, 0x52, 0xe0);
        let annotations = vec![
            Annotation::new(
                Shape::Pen {
                    points: (0..60)
                        .map(|i| {
                            let t = i as f32 / 59.0;
                            Pos2::new(40.0 + t * 200.0, 60.0 + (t * 12.0).sin() * 25.0)
                        })
                        .collect(),
                },
                style(red),
            ),
            Annotation::new(
                Shape::Line { a: Pos2::new(40.0, 140.0), b: Pos2::new(240.0, 180.0) },
                style(blue),
            ),
            Annotation::new(
                Shape::Arrow { a: Pos2::new(40.0, 220.0), b: Pos2::new(240.0, 300.0) },
                style(red),
            ),
            Annotation::new(
                Shape::Rect {
                    rect: Rect::from_min_max(Pos2::new(300.0, 60.0), Pos2::new(480.0, 180.0)),
                },
                style(red),
            ),
            Annotation::new(
                Shape::Ellipse {
                    rect: Rect::from_min_max(Pos2::new(300.0, 220.0), Pos2::new(480.0, 330.0)),
                },
                style(blue),
            ),
            Annotation::new(
                Shape::Highlight {
                    rect: Rect::from_min_max(Pos2::new(520.0, 60.0), Pos2::new(860.0, 120.0)),
                },
                style(yellow),
            ),
            Annotation::new(
                Shape::Pixelate {
                    rect: Rect::from_min_max(Pos2::new(520.0, 160.0), Pos2::new(860.0, 300.0)),
                },
                style(red),
            ),
            Annotation::new(
                Shape::Text {
                    pos: Pos2::new(40.0, 380.0),
                    text: "Annotated with scrannotate\nsecond line".to_owned(),
                },
                style(Color32::WHITE),
            ),
            Annotation::new(
                Shape::Marker { pos: Pos2::new(600.0, 420.0), number: 1, target: None },
                style(red),
            ),
            Annotation::new(
                Shape::Marker {
                    pos: Pos2::new(660.0, 420.0),
                    number: 12,
                    target: Some(Pos2::new(760.0, 520.0)),
                },
                style(blue),
            ),
            Annotation {
                shape: Shape::Rect {
                    rect: Rect::from_min_max(Pos2::new(560.0, 380.0), Pos2::new(700.0, 460.0)),
                },
                style: style(yellow),
                rotation: 0.5,
            },
            Annotation {
                shape: Shape::Text {
                    pos: Pos2::new(80.0, 480.0),
                    text: "rotated text".to_owned(),
                },
                style: style(red),
                rotation: -0.4,
            },
        ];

        let out = render_to_image(&base, &annotations, None).expect("render");
        assert_eq!(out.dimensions(), (900, 600));

        // Inside the pixelate region every pixel of an 8×8 mosaic block is
        // identical, which is never true of the gradient base.
        let block: Vec<_> = (552..560)
            .flat_map(|x| (160..168).map(move |y| (x, y)))
            .map(|(x, y)| *out.get_pixel(x, y))
            .collect();
        assert!(block.windows(2).all(|w| w[0] == w[1]), "pixelate block not uniform");
        let base_block: Vec<_> = (552..560)
            .flat_map(|x| (160..168).map(move |y| (x, y)))
            .map(|(x, y)| *base.get_pixel(x, y))
            .collect();
        assert!(base_block.windows(2).any(|w| w[0] != w[1]), "base unexpectedly uniform");

        let cropped = render_to_image(
            &base,
            &annotations,
            Some(Rect::from_min_max(Pos2::new(100.0, 50.0), Pos2::new(500.0, 350.0))),
        )
        .expect("render cropped");
        assert_eq!(cropped.dimensions(), (400, 300));

        if let Some(dir) = std::env::var_os("SCRANNOTATE_TEST_OUT") {
            let dir = std::path::PathBuf::from(dir);
            out.save(dir.join("render_full.png")).expect("save full");
            cropped.save(dir.join("render_cropped.png")).expect("save cropped");
        }
    }
}
