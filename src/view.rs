//! Viewport: zoom/pan state and the image↔screen coordinate transforms.

use eframe::egui::{Pos2, Rect, Vec2};

#[derive(Clone, Copy)]
pub struct View {
    pub zoom: f32,
    /// Screen-space offset of the image's top-left from the canvas top-left.
    pub pan: Vec2,
    /// False until the first fit (or after a reset that wants a refit).
    pub fitted: bool,
}

impl View {
    pub fn new() -> Self {
        Self { zoom: 1.0, pan: Vec2::ZERO, fitted: false }
    }

    pub fn to_screen(self, canvas: Rect, p: Pos2) -> Pos2 {
        canvas.min + self.pan + p.to_vec2() * self.zoom
    }

    pub fn to_image(self, canvas: Rect, p: Pos2) -> Pos2 {
        Pos2::ZERO + (p - canvas.min - self.pan) / self.zoom
    }

    pub fn rect_to_screen(&self, canvas: Rect, r: Rect) -> Rect {
        Rect::from_min_max(self.to_screen(canvas, r.min), self.to_screen(canvas, r.max))
    }

    /// Center `rect` (image coords) in the canvas at the largest zoom that
    /// fits it (capped at 1:1 so small targets don't blow up). Content
    /// outside the rect stays reachable by panning/zooming.
    pub fn frame(&mut self, rect: Rect, canvas_size: Vec2) {
        if rect.width() <= 0.0
            || rect.height() <= 0.0
            || canvas_size.x <= 0.0
            || canvas_size.y <= 0.0
        {
            return;
        }
        self.zoom = (canvas_size.x / rect.width())
            .min(canvas_size.y / rect.height())
            .clamp(0.02, 1.0);
        self.pan = canvas_size * 0.5 - rect.center().to_vec2() * self.zoom;
        self.fitted = true;
    }

    /// Frame the whole image.
    pub fn fit(&mut self, img: Vec2, canvas_size: Vec2) {
        self.frame(Rect::from_min_size(Pos2::ZERO, img), canvas_size);
    }

    /// Multiply the zoom by `factor`, keeping the point under `pointer`
    /// stationary.
    pub fn zoom_about(&mut self, canvas: Rect, pointer: Pos2, factor: f32) {
        let new_zoom = (self.zoom * factor).clamp(0.02, 16.0);
        let factor = new_zoom / self.zoom;
        let rel = pointer - canvas.min - self.pan;
        self.pan += rel * (1.0 - factor);
        self.zoom = new_zoom;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn screen_image_roundtrip() {
        let mut v = View::new();
        v.zoom = 0.5;
        v.pan = Vec2::new(30.0, 40.0);
        let canvas = Rect::from_min_max(Pos2::new(10.0, 20.0), Pos2::new(810.0, 620.0));
        let p = Pos2::new(123.0, 456.0);
        let back = v.to_image(canvas, v.to_screen(canvas, p));
        assert!((back - p).length() < 1e-3);
    }

    #[test]
    fn fit_centers_and_caps_at_one() {
        let mut v = View::new();
        v.fit(Vec2::new(100.0, 100.0), Vec2::new(1000.0, 500.0));
        assert_eq!(v.zoom, 1.0); // capped, image smaller than canvas
        assert_eq!(v.pan, Vec2::new(450.0, 200.0));
        v.fit(Vec2::new(2000.0, 1000.0), Vec2::new(1000.0, 500.0));
        assert!((v.zoom - 0.5).abs() < 1e-6);
    }

    #[test]
    fn frame_centers_a_sub_rect() {
        let mut v = View::new();
        // Second monitor of a 3840×1080 desktop: frame its 1920×1080 rect.
        let rect = Rect::from_min_max(Pos2::new(1920.0, 0.0), Pos2::new(3840.0, 1080.0));
        v.frame(rect, Vec2::new(1920.0, 1080.0));
        assert_eq!(v.zoom, 1.0);
        // The rect's center lands on the canvas center.
        let canvas = Rect::from_min_size(Pos2::ZERO, Vec2::new(1920.0, 1080.0));
        assert_eq!(v.to_screen(canvas, rect.center()), Pos2::new(960.0, 540.0));
    }
}
