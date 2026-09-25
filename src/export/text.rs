//! CPU rendering from the same epaint layout and font atlas as the canvas.
//! This shares fallback selection, shaping, line spacing, and glyph metrics;
//! screen zoom and GPU sampling can still affect antialiasing in the preview.

use anyhow::{Context, Result};
use eframe::egui::{FontId, Pos2, epaint::text::Fonts};
use tiny_skia::{FilterQuality, Pixmap, PixmapPaint};

use crate::annotate::Style;

pub(super) fn draw(
    target: &mut Pixmap,
    fonts: &mut Fonts,
    pos: Pos2,
    text: &str,
    style: &Style,
    rotation: f32,
    centered: bool,
) -> Result<()> {
    // Image coordinates are physical pixels. A separate 1:1 layout avoids
    // exporting the current screen's zoom or display scaling.
    let galley = fonts.with_pixels_per_point(1.0).layout_no_wrap(
        text.to_owned(),
        FontId::proportional(style.font_size),
        style.color,
    );
    let pos = if centered {
        pos - galley.size() * 0.5
    } else {
        pos
    };
    let transform = super::rotation_transform(rotation, pos + galley.size() * 0.5);
    let atlas = fonts.texture_atlas().image();
    let [r, g, b, a] = style.color.to_srgba_unmultiplied();
    let paint = PixmapPaint {
        quality: FilterQuality::Bilinear,
        ..Default::default()
    };

    for row in &galley.rows {
        for glyph in &row.glyphs {
            let uv = glyph.uv_rect;
            if uv.is_nothing() {
                continue;
            }
            let width = u32::from(uv.max[0] - uv.min[0]);
            let height = u32::from(uv.max[1] - uv.min[1]);
            let mut bitmap = Pixmap::new(width, height).context("allocating text glyph")?;
            for (y, scanline) in bitmap
                .data_mut()
                .chunks_exact_mut(width as usize * 4)
                .enumerate()
            {
                for (x, pixel) in scanline.chunks_exact_mut(4).enumerate() {
                    let coverage =
                        atlas[(usize::from(uv.min[0]) + x, usize::from(uv.min[1]) + y)].a();
                    let alpha = multiply_u8(coverage, a);
                    pixel.copy_from_slice(&[
                        multiply_u8(r, alpha),
                        multiply_u8(g, alpha),
                        multiply_u8(b, alpha),
                        alpha,
                    ]);
                }
            }
            // Use epaint's pixel-snapped mesh origin rather than duplicating
            // its glyph offsets/rounding. Rotation uses the galley's center,
            // exactly as ui::paint does for text blocks.
            let origin = pos
                + row.pos.to_vec2()
                + row.visuals.mesh.vertices[glyph.first_vertex as usize]
                    .pos
                    .to_vec2();
            target.draw_pixmap(
                0,
                0,
                bitmap.as_ref(),
                &paint,
                transform.pre_translate(origin.x, origin.y),
                None,
            );
        }
    }
    Ok(())
}

fn multiply_u8(a: u8, b: u8) -> u8 {
    ((u16::from(a) * u16::from(b) + 127) / 255) as u8
}
