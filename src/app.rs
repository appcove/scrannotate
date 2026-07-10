//! Single fullscreen surface: the frozen capture fills the window; drag out a
//! region, adjust it with the handles, annotate in place, then copy (quits)
//! or save (stays). Placed annotations stay live: with the Select tool they
//! can be moved, resized, rotated, deleted, and text re-edited inline.

use std::path::PathBuf;

use eframe::egui::{
    self, Align2, Button, CentralPanel, Color32, ColorImage, Context, CornerRadius, FontId, Id,
    Key, KeyboardShortcut, Margin, Modifiers, PointerButton, Pos2, Rect, RichText, Sense,
    Shape as EguiShape, Slider, Stroke, StrokeKind, TextEdit, TextureHandle, TextureOptions, Ui,
    Vec2, ViewportCommand,
    epaint::{EllipseShape, TextShape},
};
use image::RgbaImage;

use crate::annotate::{
    Annotation, Shape, Style, Tool, arrow_geometry, composite_pixelates, highlight_color,
    marker_radius, pixelate_rects,
};
use crate::{capture, clipboard, export, prefs};

/// Starter palette for first runs; at runtime the palette is a
/// most-recently-used stack persisted via `prefs`.
const PALETTE: [Color32; 6] = [
    Color32::from_rgb(0xe0, 0x2d, 0x2d), // red
    Color32::from_rgb(0xf2, 0xd0, 0x2e), // yellow
    Color32::from_rgb(0x2f, 0xb3, 0x44), // green
    Color32::from_rgb(0x2f, 0x52, 0xe0), // blue
    Color32::WHITE,
    Color32::BLACK,
];

/// Accent for selection/hover chrome.
const ACCENT: Color32 = Color32::from_rgb(0x4c, 0x9e, 0xff);
/// Background fill of the active tool button.
const ACTIVE_TOOL_FILL: Color32 = Color32::from_rgb(0x1c, 0x63, 0x9e);

const TOOLS: [(Tool, Key); 10] = [
    (Tool::Select, Key::S),
    (Tool::Pen, Key::P),
    (Tool::Line, Key::L),
    (Tool::Arrow, Key::A),
    (Tool::Rect, Key::R),
    (Tool::Ellipse, Key::E),
    (Tool::Highlight, Key::H),
    (Tool::Pixelate, Key::B),
    (Tool::Text, Key::T),
    (Tool::Marker, Key::M),
];

struct Snapshot {
    annotations: Vec<Annotation>,
    marker_next: u32,
}

struct DragState {
    start: Pos2,
    current: Pos2,
    /// Pen accumulates every sampled point; other tools use start/current.
    points: Vec<Pos2>,
}

struct TextEditState {
    /// `Some` while re-editing an existing text annotation (hidden meanwhile).
    index: Option<usize>,
    pos: Pos2,
    buffer: String,
    style: Style,
    just_created: bool,
}

/// Which edges of a box a resize drag moves.
#[derive(Clone, Copy)]
struct ResizeEdges {
    left: bool,
    right: bool,
    top: bool,
    bottom: bool,
}

/// Active pointer interaction on the region box.
#[derive(Clone, Copy)]
enum SelectDrag {
    /// Rubber-band a new box out from `anchor`.
    Draw { anchor: Pos2 },
    /// Translate the whole box; `grab` is where the pointer picked it up.
    Move { start_rect: Rect, grab: Pos2 },
    /// Drag a handle; edges not in `edges` keep their `start_rect` position.
    Resize { start_rect: Rect, edges: ResizeEdges },
}

/// What an item-manipulation drag does to the grabbed annotation.
#[derive(Clone, Copy)]
enum ItemDragKind {
    Move,
    Resize { edges: ResizeEdges },
    /// Corner drag on text/markers: scales the font size, never stretches.
    ScaleUniform,
    Rotate,
    /// Line/arrow endpoint (`false` = start, `true` = end).
    Endpoint { second: bool },
}

/// Active drag on a placed annotation. Each frame rebuilds the annotation
/// from `start` + the pointer, so drags are stable and Esc can revert.
struct ItemDrag {
    index: usize,
    start: Annotation,
    /// `start`'s bounding box and its center (rotation/scale pivot).
    bbox0: Rect,
    center: Pos2,
    grab: Pos2,
    kind: ItemDragKind,
}

/// Screen-px side length of a region handle.
const REGION_HANDLE_SIZE: f32 = 18.0;
/// Screen-px side length of an item handle.
const ITEM_HANDLE_SIZE: f32 = 14.0;
/// Screen-px radius of the rotate/move knobs.
const KNOB_RADIUS: f32 = 12.0;
/// Screen-px half-width of the box around a handle that grabs it.
const HANDLE_HIT: f32 = 18.0;

/// Corner handles first so they win over edge handles on small boxes.
fn selection_handles(r: Rect) -> [(Pos2, ResizeEdges); 8] {
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

fn hit_handle(sel_screen: Rect, pointer: Pos2) -> Option<ResizeEdges> {
    selection_handles(sel_screen)
        .into_iter()
        .find(|(pos, _)| (pointer - *pos).abs().max_elem() <= HANDLE_HIT)
        .map(|(_, edges)| edges)
}

fn edges_cursor(e: ResizeEdges) -> egui::CursorIcon {
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
fn resize_rect(start: Rect, edges: ResizeEdges, p: Pos2) -> Rect {
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
fn clamp_rect_within(r: Rect, bounds: Rect) -> Rect {
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

fn rotate_around(p: Pos2, c: Pos2, angle: f32) -> Pos2 {
    let (sin, cos) = angle.sin_cos();
    let v = p - c;
    c + Vec2::new(v.x * cos - v.y * sin, v.x * sin + v.y * cos)
}

/// Distance from `p` to the segment `a`–`b`.
fn seg_dist(p: Pos2, a: Pos2, b: Pos2) -> f32 {
    let ab = b - a;
    let len_sq = ab.length_sq();
    if len_sq <= f32::EPSILON {
        return (p - a).length();
    }
    let t = ((p - a).dot(ab) / len_sq).clamp(0.0, 1.0);
    (a + ab * t - p).length()
}

fn translate_shape(shape: &mut Shape, d: Vec2) {
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

struct Toast {
    message: String,
    until: f64,
    is_error: bool,
}

pub struct ScreencapApp {
    base: RgbaImage,
    texture: Option<TextureHandle>,
    /// Pixelate rects currently baked into `texture`; rebuilt when they drift
    /// from the annotation list (add, move, undo, redo).
    baked_pixelates: Vec<Rect>,
    annotations: Vec<Annotation>,
    undo: Vec<Snapshot>,
    redo: Vec<Snapshot>,
    tool: Tool,
    style: Style,
    /// Most-recently-used colors, newest first; the head is the selected
    /// color (persisted across runs).
    palette: Vec<Color32>,
    /// Working color of the open custom-color picker.
    custom_picker: Option<Color32>,
    /// One undo step per continuous slider adjustment of a selected item.
    style_undo_pushed: bool,
    /// Width/size were deliberately set (this or a prior session), so they
    /// are a preference worth persisting — not just resolution defaults.
    persist_style: bool,
    /// A pan happened while space was held, so releasing space is the end
    /// of a pan gesture, not a tap-to-Select.
    space_panned: bool,
    /// Deadline of the "press Esc again to discard" window; a second Esc
    /// before it quits, any other input disarms it.
    discard_armed: Option<f64>,
    marker_next: u32,
    /// In-progress annotation drag (drawing tools).
    drag: Option<DragState>,
    /// In-progress region-box drag (Select tool, handles, or right-drag).
    select_drag: Option<SelectDrag>,
    /// In-progress manipulation of a placed annotation.
    item_drag: Option<ItemDrag>,
    /// Index of the annotation selected with the Select tool.
    selected: Option<usize>,
    /// The capture region (image coords) — also the export crop. Adjustable
    /// the whole session.
    selection: Option<Rect>,
    text_edit: Option<TextEditState>,
    zoom: f32,
    /// Screen-space offset of the image's top-left from the canvas top-left.
    pan: Vec2,
    view_fitted: bool,
    /// Canvas rect measured last frame, for shortcut handlers that refit the
    /// view before this frame's layout is known.
    last_canvas_size: Vec2,
    /// Where the user dragged the toolbar; `None` = auto beside the region.
    toolbar_pos: Option<Pos2>,
    /// Toolbar rect measured last frame, for the auto placement.
    toolbar_size: Vec2,
    toast: Option<Toast>,
    out_dir: PathBuf,
    /// Docs/dev hook (`SCRANNOTATE_SHOT`): save a window screenshot here
    /// once the UI settles, then quit.
    shot_path: Option<PathBuf>,
    shot_frames: u32,
    /// Demo scene active (`SCRANNOTATE_DEMO`) — keep the user's prefs out
    /// of it entirely.
    demo: bool,
    /// `--cursor`, kept for recaptures from the Change screen button.
    embed_cursor: bool,
    /// In-flight recapture (Change screen button); the thread sends the new
    /// frame once the user answers the portal chooser.
    recapture: Option<std::sync::mpsc::Receiver<anyhow::Result<RgbaImage>>>,
}

impl ScreencapApp {
    pub fn new(img: RgbaImage, out_dir: PathBuf, select_full: bool, embed_cursor: bool) -> Self {
        let demo_mode = std::env::var("SCRANNOTATE_DEMO").ok();
        let min_dim = img.width().min(img.height());
        // Default sizes scale with the screenshot so strokes stay legible on
        // HiDPI captures; deliberately-set (persisted) sizes win over that.
        let saved = if demo_mode.is_some() { prefs::Prefs::default() } else { prefs::load() };
        let width = (f64::from(min_dim) / 450.0).clamp(2.0, 8.0).round();
        let stroke_width = saved.width.unwrap_or(width as f32);
        let font_size = saved.font_size.unwrap_or(stroke_width * 8.0);
        let persist_style = saved.width.is_some() || saved.font_size.is_some();
        let mut palette = saved.palette.unwrap_or_else(|| PALETTE.to_vec());
        palette.truncate(6);
        // The head of the MRU list is the active color.
        let color = palette.first().copied().unwrap_or(PALETTE[0]);
        let selection = select_full.then(|| {
            Rect::from_min_size(Pos2::ZERO, Vec2::new(img.width() as f32, img.height() as f32))
        });
        let mut app = Self {
            base: img,
            texture: None,
            baked_pixelates: Vec::new(),
            annotations: Vec::new(),
            undo: Vec::new(),
            redo: Vec::new(),
            tool: Tool::Select,
            style: Style { color, width: stroke_width, font_size },
            palette,
            custom_picker: None,
            style_undo_pushed: false,
            persist_style,
            space_panned: false,
            discard_armed: None,
            marker_next: 1,
            drag: None,
            select_drag: None,
            item_drag: None,
            selected: None,
            selection,
            text_edit: None,
            zoom: 1.0,
            pan: Vec2::ZERO,
            view_fitted: false,
            last_canvas_size: Vec2::new(1400.0, 850.0),
            toolbar_pos: None,
            toolbar_size: Vec2::new(210.0, 620.0),
            toast: None,
            out_dir,
            shot_path: std::env::var_os("SCRANNOTATE_SHOT").map(PathBuf::from),
            shot_frames: 0,
            demo: demo_mode.is_some(),
            embed_cursor,
            recapture: None,
        };
        if let Some(mode) = demo_mode {
            app.seed_demo(&mode);
        }
        app
    }

    /// Canned scene for the README screenshots
    /// (`SCRANNOTATE_DEMO=annotate|picker`).
    fn seed_demo(&mut self, mode: &str) {
        let style = |color, width: f32, font_size: f32| Style { color, width, font_size };
        let red = PALETTE[0];
        self.style = style(red, 5.0, 36.0);
        self.selection =
            Some(Rect::from_min_max(Pos2::new(430.0, 70.0), Pos2::new(1560.0, 950.0)));
        self.annotations = vec![
            Annotation::new(
                Shape::Rect {
                    rect: Rect::from_min_max(Pos2::new(1108.0, 740.0), Pos2::new(1372.0, 850.0)),
                },
                style(red, 5.0, 36.0),
            ),
            Annotation::new(
                Shape::Arrow { a: Pos2::new(880.0, 560.0), b: Pos2::new(1090.0, 760.0) },
                style(red, 5.0, 36.0),
            ),
            Annotation::new(
                Shape::Highlight {
                    rect: Rect::from_min_max(Pos2::new(532.0, 276.0), Pos2::new(1180.0, 322.0)),
                },
                style(PALETTE[1], 5.0, 36.0),
            ),
            Annotation::new(
                Shape::Pixelate {
                    rect: Rect::from_min_max(Pos2::new(532.0, 404.0), Pos2::new(1010.0, 464.0)),
                },
                style(red, 5.0, 36.0),
            ),
            Annotation::new(
                Shape::Marker {
                    pos: Pos2::new(492.0, 232.0),
                    number: 1,
                    target: Some(Pos2::new(556.0, 292.0)),
                },
                style(PALETTE[3], 4.0, 26.0),
            ),
            Annotation::new(
                Shape::Text { pos: Pos2::new(560.0, 596.0), text: "Ship this build!".to_owned() },
                style(red, 5.0, 44.0),
            ),
        ];
        self.marker_next = 2;
        self.tool = Tool::Select;
        self.selected = Some(0);
        if mode == "picker" {
            self.custom_picker = Some(self.style.color);
        }
    }

    fn image_size(&self) -> Vec2 {
        Vec2::new(self.base.width() as f32, self.base.height() as f32)
    }

    fn image_rect(&self) -> Rect {
        Rect::from_min_size(Pos2::ZERO, self.image_size())
    }

    fn to_screen(&self, canvas: Rect, p: Pos2) -> Pos2 {
        canvas.min + self.pan + p.to_vec2() * self.zoom
    }

    fn to_image(&self, canvas: Rect, p: Pos2) -> Pos2 {
        Pos2::ZERO + (p - canvas.min - self.pan) / self.zoom
    }

    fn fit_view(&mut self, canvas_size: Vec2) {
        let img = self.image_size();
        if img.x <= 0.0 || img.y <= 0.0 || canvas_size.x <= 0.0 || canvas_size.y <= 0.0 {
            return;
        }
        self.zoom = (canvas_size.x / img.x).min(canvas_size.y / img.y).clamp(0.02, 1.0);
        self.pan = (canvas_size - img * self.zoom) * 0.5;
    }

    fn snapshot(&self) -> Snapshot {
        Snapshot {
            annotations: self.annotations.clone(),
            marker_next: self.marker_next,
        }
    }

    fn push_undo(&mut self) {
        self.undo.push(self.snapshot());
        self.redo.clear();
    }

    /// MRU bookkeeping: the color in use moves to the head of the palette,
    /// so the head is always the active color.
    fn promote_color(&mut self, color: Color32) {
        self.palette.retain(|c| *c != color);
        self.palette.insert(0, color);
        self.palette.truncate(6);
    }

    /// Pick a color: becomes the active color (and MRU head); text being
    /// edited or a selected object gets recolored too (undoably — the text
    /// case folds into its commit).
    fn apply_color(&mut self, color: Color32) {
        self.style.color = color;
        self.promote_color(color);
        if let Some(edit) = &mut self.text_edit {
            edit.style.color = color;
        } else if self.tool == Tool::Select
            && let Some(idx) = self.selected
            && idx < self.annotations.len()
            && self.annotations[idx].style.color != color
        {
            self.push_undo();
            self.annotations[idx].style.color = color;
        }
    }

    fn restore(&mut self, snapshot: Snapshot) {
        self.annotations = snapshot.annotations;
        self.marker_next = snapshot.marker_next;
        self.selected = None;
        self.item_drag = None;
    }

    fn undo(&mut self) {
        if let Some(snapshot) = self.undo.pop() {
            let now = self.snapshot();
            self.redo.push(now);
            self.restore(snapshot);
        }
    }

    fn redo(&mut self) {
        if let Some(snapshot) = self.redo.pop() {
            let now = self.snapshot();
            self.undo.push(now);
            self.restore(snapshot);
        }
    }

    fn set_toast(&mut self, ctx: &Context, message: impl Into<String>, is_error: bool) {
        self.toast = Some(Toast {
            message: message.into(),
            until: ctx.input(|i| i.time) + 5.0,
            is_error,
        });
    }

    /// The region (or the full frame when nothing is selected) with
    /// annotations rendered in.
    fn rendered(&self) -> anyhow::Result<RgbaImage> {
        export::render_to_image(&self.base, &self.annotations, self.selection)
    }

    /// Save the region instantly; the app stays open for more edits.
    fn save(&mut self, ctx: &Context) {
        let result = self.rendered().and_then(|img| {
            std::fs::create_dir_all(&self.out_dir)?;
            let name = format!("scrannotate-{}.png", chrono::Local::now().format("%Y-%m-%d_%H%M%S"));
            let path = self.out_dir.join(name);
            img.save(&path)?;
            Ok(path)
        });
        match result {
            Ok(path) => {
                println!("{}", path.display());
                self.set_toast(ctx, format!("Saved {}", path.display()), false);
            }
            Err(err) => self.set_toast(ctx, format!("Save failed: {err:#}"), true),
        }
    }

    /// Copy the region to the clipboard and quit; on failure stay open.
    fn copy(&mut self, ctx: &Context) {
        match self.rendered().and_then(|img| clipboard::copy_image(&img)) {
            Ok(()) => ctx.send_viewport_cmd(ViewportCommand::Close),
            Err(err) => self.set_toast(ctx, format!("Copy failed: {err:#}"), true),
        }
    }

    /// Rebuild the display texture when the pixelate set changed.
    fn sync_texture(&mut self, ctx: &Context) {
        let current = pixelate_rects(&self.annotations);
        if self.texture.is_some() && current == self.baked_pixelates {
            return;
        }
        let mut composited = self.base.clone();
        composite_pixelates(&mut composited, &self.annotations);
        let size = [composited.width() as usize, composited.height() as usize];
        let color_image = ColorImage::from_rgba_unmultiplied(size, composited.as_raw());
        match &mut self.texture {
            Some(tex) => tex.set(color_image, TextureOptions::LINEAR),
            None => {
                self.texture =
                    Some(ctx.load_texture("screenshot", color_image, TextureOptions::LINEAR));
            }
        }
        self.baked_pixelates = current;
    }

    fn commit_text(&mut self) {
        let Some(edit) = self.text_edit.take() else { return };
        let text = edit.buffer.trim_end().to_owned();
        match edit.index {
            Some(idx) => {
                let Some(ann) = self.annotations.get(idx) else { return };
                let Shape::Text { text: old, .. } = &ann.shape else { return };
                if text.is_empty() {
                    self.push_undo();
                    self.annotations.remove(idx);
                    self.selected = None;
                } else if text != *old || edit.style != ann.style {
                    self.push_undo();
                    let ann = &mut self.annotations[idx];
                    // Style tweaks made while editing land with the text.
                    ann.style = edit.style;
                    if let Shape::Text { text: slot, .. } = &mut ann.shape {
                        *slot = text;
                    }
                }
            }
            None => {
                if !text.is_empty() {
                    self.push_undo();
                    self.annotations
                        .push(Annotation::new(Shape::Text { pos: edit.pos, text }, edit.style));
                    self.promote_color(edit.style.color);
                }
            }
        }
    }

    /// Explicit tool invocation (key, space tap, or button): always drops
    /// the item selection, even when the tool is unchanged — re-invoking
    /// Select is how you get back to a clean slate.
    fn set_tool(&mut self, tool: Tool) {
        self.selected = None;
        self.tool = tool;
    }

    /// Back to the initial region-selection state: annotations, region, and
    /// all in-flight interaction cleared, view refitted. Undoable.
    fn reset_all(&mut self) {
        if !self.annotations.is_empty() {
            self.push_undo();
        }
        self.annotations.clear();
        self.marker_next = 1;
        self.selection = None;
        self.selected = None;
        self.drag = None;
        self.select_drag = None;
        self.item_drag = None;
        self.text_edit = None;
        self.custom_picker = None;
        self.tool = Tool::Select;
        self.view_fitted = false; // refit next frame
    }

    /// Recapture from a different monitor: reopen the portal chooser on a
    /// worker thread (the dialog can take arbitrarily long) and swap the
    /// frame in when it arrives.
    fn start_recapture(&mut self, ctx: &Context) {
        // Hide our fullscreen frozen frame first — otherwise the new capture
        // is a screenshot of the screenshot tool.
        ctx.send_viewport_cmd(ViewportCommand::Visible(false));
        let (tx, rx) = std::sync::mpsc::channel();
        let embed_cursor = self.embed_cursor;
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            // Give the compositor a beat to actually unmap the window.
            std::thread::sleep(std::time::Duration::from_millis(200));
            let _ = tx.send(capture::capture_screenshot(embed_cursor, true));
            // A hidden Wayland window gets no frame callbacks, so the UI
            // loop may be stalled — visibility must be restored from here.
            ctx.send_viewport_cmd(ViewportCommand::Visible(true));
            ctx.send_viewport_cmd(ViewportCommand::Fullscreen(true));
            ctx.send_viewport_cmd(ViewportCommand::Focus);
            ctx.request_repaint();
        });
        self.recapture = Some(rx);
    }

    /// Poll the in-flight recapture; on success everything resets onto the
    /// new frame.
    fn poll_recapture(&mut self, ctx: &Context) {
        let Some(rx) = &self.recapture else { return };
        match rx.try_recv() {
            Ok(Ok(img)) => {
                self.recapture = None;
                self.base = img;
                self.texture = None;
                self.baked_pixelates.clear();
                self.reset_all();
                // Old-screen annotations make no sense on the new frame.
                self.undo.clear();
                self.redo.clear();
                self.sync_texture(ctx);
            }
            Ok(Err(err)) => {
                self.recapture = None;
                self.set_toast(ctx, format!("Screen change failed: {err:#}"), true);
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {
                ctx.request_repaint_after(std::time::Duration::from_millis(200));
            }
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.recapture = None;
                self.set_toast(ctx, "Screen change failed", true);
            }
        }
    }

    /// Tiny top-center button on the initial select screen: recapture from
    /// a different monitor via the portal chooser.
    fn screen_button_overlay(&mut self, ctx: &Context, canvas: Rect) {
        if self.selection.is_some() {
            return;
        }
        egui::Area::new(Id::new("change-screen"))
            .fixed_pos(Pos2::new(canvas.center().x, canvas.min.y + 6.0))
            .pivot(Align2::CENTER_TOP)
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    if self.recapture.is_some() {
                        ui.label(
                            RichText::new("Waiting for the screen picker…").size(13.0).weak(),
                        );
                    } else {
                        let btn =
                            ui.add(Button::new(RichText::new("Change screen…").size(13.0)));
                        if btn.clicked() {
                            btn.surrender_focus();
                            self.start_recapture(ctx);
                        }
                    }
                });
            });
    }

    /// Text-tool click: edit the text under the pointer, or start a new one.
    fn text_tool_click(&mut self, ctx: &Context, p: Pos2) {
        if let Some(idx) = self.hit_test_items(ctx, p, true) {
            self.open_text_editor(idx);
        } else {
            self.text_edit = Some(TextEditState {
                index: None,
                pos: p,
                buffer: String::new(),
                style: self.style,
                just_created: true,
            });
        }
    }

    /// Re-open an existing text annotation for inline editing.
    fn open_text_editor(&mut self, idx: usize) {
        if let Some(ann) = self.annotations.get(idx)
            && let Shape::Text { pos, text } = &ann.shape
        {
            self.text_edit = Some(TextEditState {
                index: Some(idx),
                pos: *pos,
                buffer: text.clone(),
                style: ann.style,
                just_created: true,
            });
            self.selected = Some(idx);
        }
    }

    /// Drop a numbered marker at `pos`; a drag towards `target` pulls an
    /// arrow out of it — one object, an arrow with a fat numbered tail.
    fn drop_marker(&mut self, pos: Pos2, target: Option<Pos2>) {
        // A drag too short to clear the circle is a plain drop.
        let min_len = marker_radius(&self.style) * 1.6;
        let target = target.filter(|t| (*t - pos).length() >= min_len);
        self.push_undo();
        self.annotations.push(Annotation::new(
            Shape::Marker { pos, number: self.marker_next, target },
            self.style,
        ));
        self.marker_next += 1;
        self.promote_color(self.style.color);
    }

    fn finish_drag(&mut self) {
        let Some(drag) = self.drag.take() else { return };
        if self.tool == Tool::Marker {
            self.drop_marker(drag.start, Some(drag.current));
            return;
        }
        let min_px = 3.0 / self.zoom; // discard sub-3-screen-pixel accidents
        let rect = Rect::from_two_pos(drag.start, drag.current);
        let shape = match self.tool {
            Tool::Pen if !drag.points.is_empty() => Some(Shape::Pen { points: drag.points }),
            Tool::Line if (drag.current - drag.start).length() >= min_px => {
                Some(Shape::Line { a: drag.start, b: drag.current })
            }
            Tool::Arrow if (drag.current - drag.start).length() >= min_px => {
                Some(Shape::Arrow { a: drag.start, b: drag.current })
            }
            Tool::Rect | Tool::Ellipse | Tool::Highlight | Tool::Pixelate
                if rect.width() >= min_px && rect.height() >= min_px =>
            {
                Some(match self.tool {
                    Tool::Rect => Shape::Rect { rect },
                    Tool::Ellipse => Shape::Ellipse { rect },
                    Tool::Highlight => Shape::Highlight { rect },
                    _ => Shape::Pixelate { rect },
                })
            }
            _ => None,
        };
        if let Some(shape) = shape {
            self.push_undo();
            self.annotations.push(Annotation::new(shape, self.style));
            self.promote_color(self.style.color);
        }
    }

    fn handle_shortcuts(&mut self, ctx: &Context, canvas_size: Vec2) {
        // The inline text editor owns the keyboard completely.
        if self.text_edit.is_some() {
            return;
        }
        // Plain keys must not fire while some widget (a slider, a drag
        // value…) holds keyboard focus — but modifier shortcuts stay live;
        // stray focus must never eat Ctrl+C.
        let focused = ctx.memory(|m| m.focused().is_some());
        let cmd = |key| KeyboardShortcut::new(Modifiers::COMMAND, key);
        let cmd_shift = |key| KeyboardShortcut::new(Modifiers::COMMAND | Modifiers::SHIFT, key);
        let (save, copy, undo, redo, quit, fit, escape, reset, delete, space, tool) =
            ctx.input_mut(|i| {
                let save = i.consume_shortcut(&cmd(Key::S));
                let copy = i.consume_shortcut(&cmd(Key::C))
                    || (!focused && i.key_pressed(Key::Enter));
                let undo = i.consume_shortcut(&cmd(Key::Z));
                let redo =
                    i.consume_shortcut(&cmd_shift(Key::Z)) || i.consume_shortcut(&cmd(Key::Y));
                let quit = i.consume_shortcut(&cmd(Key::Q)) || i.consume_shortcut(&cmd(Key::W));
                let fit =
                    (!focused && i.key_pressed(Key::F)) || i.consume_shortcut(&cmd(Key::Num0));
                let escape = !focused && i.key_pressed(Key::Escape) && !i.modifiers.shift;
                let reset = i.key_pressed(Key::Escape) && i.modifiers.shift;
                let delete = !focused
                    && (i.key_pressed(Key::Delete) || i.key_pressed(Key::Backspace));
                let space = (
                    !focused && i.key_pressed(Key::Space),
                    !focused && i.key_released(Key::Space),
                );
                let mut tool = None;
                if !focused && !i.modifiers.command {
                    for (t, key) in TOOLS {
                        if i.key_pressed(key) {
                            tool = Some(t);
                        }
                    }
                }
                (save, copy, undo, redo, quit, fit, escape, reset, delete, space, tool)
            });
        if let Some(tool) = tool {
            self.set_tool(tool);
        }
        // Tapping space jumps to the Select tool; a space+drag pan doesn't.
        let (space_pressed, space_released) = space;
        if space_pressed {
            self.space_panned = false;
        }
        if space_released {
            if !self.space_panned {
                self.set_tool(Tool::Select);
            }
            self.space_panned = false;
        }
        if save {
            self.save(ctx);
        }
        if copy {
            self.copy(ctx);
        }
        if redo {
            self.redo();
        } else if undo {
            self.undo();
        }
        if fit {
            self.fit_view(canvas_size);
        }
        if reset {
            self.reset_all();
        }
        if delete
            && self.tool == Tool::Select
            && let Some(idx) = self.selected.take()
            && idx < self.annotations.len()
        {
            self.push_undo();
            self.annotations.remove(idx);
        }
        if escape {
            let now = ctx.input(|i| i.time);
            // Ladder: cancel the current op → back to the Select tool →
            // double-Esc within a second discards everything.
            if self.custom_picker.take().is_some() || self.drag.take().is_some() {
                // Op cancelled by the take() itself.
            } else if let Some(item_drag) = self.item_drag.take() {
                if let Some(slot) = self.annotations.get_mut(item_drag.index) {
                    *slot = item_drag.start;
                }
                self.undo.pop(); // the reverted drag made no net change
            } else if let Some(select_drag) = self.select_drag.take() {
                match select_drag {
                    SelectDrag::Move { start_rect, .. }
                    | SelectDrag::Resize { start_rect, .. } => {
                        self.selection = Some(start_rect);
                    }
                    SelectDrag::Draw { .. } => self.selection = None,
                }
            } else if self.selected.is_some() || self.tool != Tool::Select {
                self.selected = None;
                self.tool = Tool::Select;
            } else if self.discard_armed.is_some_and(|until| now < until) {
                ctx.send_viewport_cmd(ViewportCommand::Close);
            } else {
                self.discard_armed = Some(now + 1.0);
                self.toast = Some(Toast {
                    message: "Press Esc again to discard".to_owned(),
                    until: now + 1.0,
                    is_error: false,
                });
                ctx.request_repaint_after(std::time::Duration::from_secs(1));
            }
        }
        if quit {
            ctx.send_viewport_cmd(ViewportCommand::Close);
        }
    }

    /// Bounding box of an annotation in image coords, ignoring its rotation.
    fn annotation_bbox(&self, ctx: &Context, ann: &Annotation) -> Rect {
        match &ann.shape {
            Shape::Pen { points } => {
                let mut r = points
                    .first()
                    .map_or(Rect::ZERO, |p| Rect::from_min_max(*p, *p));
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
                let galley = ctx.fonts_mut(|f| {
                    f.layout_no_wrap(
                        text.clone(),
                        FontId::proportional(ann.style.font_size),
                        Color32::WHITE,
                    )
                });
                Rect::from_min_size(*pos, galley.size())
            }
            Shape::Marker { pos, target, .. } => {
                let mut bbox =
                    Rect::from_center_size(*pos, Vec2::splat(marker_radius(&ann.style) * 2.0));
                if let Some(target) = target {
                    bbox = bbox
                        .union(Rect::from_center_size(*target, Vec2::splat(ann.style.width)));
                }
                bbox
            }
        }
    }

    /// Whether `p` (image coords) lands on the annotation's visible geometry.
    fn hit_annotation(&self, p: Pos2, ann: &Annotation, bbox: Rect) -> bool {
        let tol = ann.style.width * 0.5 + 6.0 / self.zoom;
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
    fn hit_test_items(&self, ctx: &Context, p: Pos2, text_only: bool) -> Option<usize> {
        for (i, ann) in self.annotations.iter().enumerate().rev() {
            if text_only && !matches!(ann.shape, Shape::Text { .. }) {
                continue;
            }
            let bbox = self.annotation_bbox(ctx, ann);
            if self.hit_annotation(p, ann, bbox) {
                return Some(i);
            }
        }
        None
    }

    /// Manipulation handles for an annotation, in screen coords.
    fn item_handles(&self, ctx: &Context, canvas: Rect, ann: &Annotation) -> Vec<(Pos2, ItemDragKind)> {
        let mut out = Vec::new();
        if let Shape::Line { a, b } | Shape::Arrow { a, b } = &ann.shape {
            out.push((self.to_screen(canvas, *a), ItemDragKind::Endpoint { second: false }));
            out.push((self.to_screen(canvas, *b), ItemDragKind::Endpoint { second: true }));
            return out;
        }
        // A marker with an arrow behaves like an arrow: two draggable ends.
        if let Shape::Marker { pos, target: Some(target), .. } = &ann.shape {
            out.push((self.to_screen(canvas, *pos), ItemDragKind::Endpoint { second: false }));
            out.push((self.to_screen(canvas, *target), ItemDragKind::Endpoint { second: true }));
            return out;
        }
        let bbox = self.annotation_bbox(ctx, ann);
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
                out.push((self.to_screen(canvas, rotate_around(pos, c, ann.rotation)), kind));
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
        if rotatable {
            let c_screen = self.to_screen(canvas, c);
            let top = self.to_screen(canvas, rotate_around(bbox.center_top(), c, ann.rotation));
            let dir = top - c_screen;
            let dir = if dir.length() > 0.1 { dir.normalized() } else { Vec2::new(0.0, -1.0) };
            out.push((top + dir * 30.0, ItemDragKind::Rotate));
        }
        // Text gets an explicit move knob (below the box, opposite the
        // rotate knob) since its corners are taken by font scaling.
        if matches!(ann.shape, Shape::Text { .. }) {
            let c_screen = self.to_screen(canvas, c);
            let bottom =
                self.to_screen(canvas, rotate_around(bbox.center_bottom(), c, ann.rotation));
            let dir = bottom - c_screen;
            let dir = if dir.length() > 0.1 { dir.normalized() } else { Vec2::new(0.0, 1.0) };
            out.push((bottom + dir * 30.0, ItemDragKind::Move));
        }
        out
    }

    /// The selected item's handle under the pointer, if any.
    fn item_handle_at(&self, ctx: &Context, canvas: Rect, pointer: Pos2) -> Option<ItemDragKind> {
        let idx = self.selected?;
        let ann = self.annotations.get(idx)?;
        self.item_handles(ctx, canvas, ann)
            .into_iter()
            .find(|(pos, _)| (pointer - *pos).abs().max_elem() <= HANDLE_HIT)
            .map(|(_, kind)| kind)
    }

    /// Rebuild the dragged annotation from the drag's start state + pointer.
    fn apply_item_drag(&mut self, p: Pos2) {
        let Some(d) = &self.item_drag else { return };
        let (idx, kind, bbox0, center, grab) = (d.index, d.kind, d.bbox0, d.center, d.grab);
        let mut ann = d.start.clone();
        if idx >= self.annotations.len() {
            return;
        }
        match kind {
            ItemDragKind::Move => translate_shape(&mut ann.shape, p - grab),
            ItemDragKind::Endpoint { second } => match &mut ann.shape {
                Shape::Line { a, b } | Shape::Arrow { a, b } => {
                    if second {
                        *b = p;
                    } else {
                        *a = p;
                    }
                }
                Shape::Marker { pos, target, .. } => {
                    if second {
                        *target = Some(p);
                    } else {
                        *pos = p;
                    }
                }
                _ => {}
            },
            ItemDragKind::Resize { edges } => match &mut ann.shape {
                Shape::Rect { rect }
                | Shape::Ellipse { rect }
                | Shape::Highlight { rect }
                | Shape::Pixelate { rect } => {
                    // Resize in the shape's local (unrotated) frame, then
                    // shift so the untouched edges stay fixed on screen.
                    let c0 = bbox0.center();
                    let pl = rotate_around(p, c0, -ann.rotation);
                    let r1 = resize_rect(bbox0, edges, pl);
                    let c1w = rotate_around(r1.center(), c0, ann.rotation);
                    *rect = r1.translate(c1w - r1.center());
                }
                Shape::Pen { points } => {
                    let r1 = resize_rect(bbox0, edges, p);
                    let s0 = bbox0.size().max(Vec2::splat(1.0));
                    for q in points.iter_mut() {
                        let t = (*q - bbox0.min) / s0;
                        *q = r1.min + t * r1.size();
                    }
                }
                _ => {}
            },
            ItemDragKind::ScaleUniform => {
                let denom = (grab - center).length().max(1.0);
                let factor = ((p - center).length() / denom).clamp(0.05, 20.0);
                ann.style.font_size = (ann.style.font_size * factor).clamp(6.0, 400.0);
            }
            ItemDragKind::Rotate => {
                let delta = (p - center).angle() - (grab - center).angle();
                match &mut ann.shape {
                    // Pen/line/arrow bake rotation into their points.
                    Shape::Pen { points } => {
                        for q in points.iter_mut() {
                            *q = rotate_around(*q, center, delta);
                        }
                    }
                    Shape::Line { a, b } | Shape::Arrow { a, b } => {
                        *a = rotate_around(*a, center, delta);
                        *b = rotate_around(*b, center, delta);
                    }
                    _ => ann.rotation += delta,
                }
            }
        }
        self.annotations[idx] = ann;
    }

    /// Draw one annotation onto the canvas. Geometry mirrors the export
    /// renderer via the shared helpers in `annotate`.
    fn paint_annotation(&self, painter: &egui::Painter, canvas: Rect, ann: &Annotation) {
        let to = |p| self.to_screen(canvas, p);
        let zoom = self.zoom;
        let rot = ann.rotation;
        // Keep hairlines visible when zoomed way out; export uses true width.
        let stroke = Stroke::new((ann.style.width * zoom).max(1.0), ann.style.color);
        match &ann.shape {
            Shape::Pen { points } => {
                if points.len() == 1 {
                    painter.circle_filled(
                        to(points[0]),
                        (ann.style.width * 0.5 * zoom).max(0.75),
                        ann.style.color,
                    );
                } else {
                    let pts: Vec<Pos2> = points.iter().map(|p| to(*p)).collect();
                    painter.add(EguiShape::line(pts, stroke));
                }
            }
            Shape::Line { a, b } => {
                painter.line_segment([to(*a), to(*b)], stroke);
            }
            Shape::Arrow { a, b } => {
                self.paint_arrow(painter, canvas, *a, *b, &ann.style);
            }
            Shape::Rect { rect } => {
                if rot == 0.0 {
                    let r = Rect::from_min_max(to(rect.min), to(rect.max));
                    painter.rect_stroke(r, CornerRadius::ZERO, stroke, StrokeKind::Middle);
                } else {
                    let c = rect.center();
                    let pts: Vec<Pos2> =
                        [rect.left_top(), rect.right_top(), rect.right_bottom(), rect.left_bottom()]
                            .into_iter()
                            .map(|q| to(rotate_around(q, c, rot)))
                            .collect();
                    painter.add(EguiShape::closed_line(pts, stroke));
                }
            }
            Shape::Ellipse { rect } => {
                let r = Rect::from_min_max(to(rect.min), to(rect.max));
                painter.add(EguiShape::Ellipse(EllipseShape {
                    center: r.center(),
                    radius: r.size() * 0.5,
                    fill: Color32::TRANSPARENT,
                    stroke,
                    angle: rot,
                }));
            }
            Shape::Highlight { rect } => {
                let fill = highlight_color(ann.style.color);
                if rot == 0.0 {
                    let r = Rect::from_min_max(to(rect.min), to(rect.max));
                    painter.rect_filled(r, CornerRadius::ZERO, fill);
                } else {
                    let c = rect.center();
                    let pts: Vec<Pos2> =
                        [rect.left_top(), rect.right_top(), rect.right_bottom(), rect.left_bottom()]
                            .into_iter()
                            .map(|q| to(rotate_around(q, c, rot)))
                            .collect();
                    painter.add(EguiShape::convex_polygon(pts, fill, Stroke::NONE));
                }
            }
            // Baked into the texture; nothing to draw here.
            Shape::Pixelate { .. } => {}
            Shape::Text { pos, text } => {
                let font = FontId::proportional(ann.style.font_size * zoom);
                let galley = painter.layout_no_wrap(text.clone(), font, ann.style.color);
                if rot == 0.0 {
                    painter.galley(to(*pos), galley, ann.style.color);
                } else {
                    let c = *pos + (galley.size() / zoom) * 0.5;
                    let mut shape =
                        TextShape::new(to(rotate_around(*pos, c, rot)), galley, ann.style.color);
                    shape.angle = rot;
                    painter.add(EguiShape::Text(shape));
                }
            }
            Shape::Marker { pos, number, target } => {
                // Arrow first; the circle covers the shaft's root.
                if let Some(target) = target {
                    self.paint_arrow(painter, canvas, *pos, *target, &ann.style);
                }
                let center = to(*pos);
                painter.circle_filled(center, marker_radius(&ann.style) * zoom, ann.style.color);
                painter.text(
                    center,
                    Align2::CENTER_CENTER,
                    number.to_string(),
                    FontId::proportional(ann.style.font_size * zoom),
                    Color32::WHITE,
                );
            }
        }
    }

    /// Shaft + filled head from `a` to `b` (shared by arrows and markers).
    fn paint_arrow(&self, painter: &egui::Painter, canvas: Rect, a: Pos2, b: Pos2, style: &Style) {
        let stroke = Stroke::new((style.width * self.zoom).max(1.0), style.color);
        let geo = arrow_geometry(a, b, style.width);
        painter.line_segment(
            [self.to_screen(canvas, a), self.to_screen(canvas, geo.shaft_end)],
            stroke,
        );
        let head: Vec<Pos2> = geo.head.iter().map(|p| self.to_screen(canvas, *p)).collect();
        painter.add(EguiShape::convex_polygon(head, style.color, Stroke::NONE));
    }

    /// Live preview of the annotation being dragged.
    fn paint_drag_preview(&self, painter: &egui::Painter, canvas: Rect) {
        let Some(drag) = &self.drag else { return };
        let rect = Rect::from_two_pos(drag.start, drag.current);
        match self.tool {
            Tool::Pen => self.paint_annotation(
                painter,
                canvas,
                &Annotation::new(Shape::Pen { points: drag.points.clone() }, self.style),
            ),
            Tool::Line | Tool::Arrow => {
                let shape = if self.tool == Tool::Line {
                    Shape::Line { a: drag.start, b: drag.current }
                } else {
                    Shape::Arrow { a: drag.start, b: drag.current }
                };
                self.paint_annotation(painter, canvas, &Annotation::new(shape, self.style));
            }
            Tool::Rect | Tool::Ellipse | Tool::Highlight => {
                let shape = match self.tool {
                    Tool::Rect => Shape::Rect { rect },
                    Tool::Ellipse => Shape::Ellipse { rect },
                    _ => Shape::Highlight { rect },
                };
                self.paint_annotation(painter, canvas, &Annotation::new(shape, self.style));
            }
            Tool::Pixelate => {
                let r = Rect::from_min_max(
                    self.to_screen(canvas, rect.min),
                    self.to_screen(canvas, rect.max),
                );
                painter.rect_filled(r, CornerRadius::ZERO, Color32::from_black_alpha(90));
                painter.rect_stroke(
                    r,
                    CornerRadius::ZERO,
                    Stroke::new(1.0, Color32::WHITE),
                    StrokeKind::Middle,
                );
            }
            Tool::Marker => {
                // Marker pinned at the start, arrow following the pointer
                // once the drag clears the circle (mirrors drop_marker).
                let min_len = marker_radius(&self.style) * 1.6;
                let target =
                    Some(drag.current).filter(|t| (*t - drag.start).length() >= min_len);
                self.paint_annotation(
                    painter,
                    canvas,
                    &Annotation::new(
                        Shape::Marker { pos: drag.start, number: self.marker_next, target },
                        self.style,
                    ),
                );
            }
            Tool::Select | Tool::Text => {}
        }
    }

    /// Selection box, handles and rotate knob around the selected item.
    fn paint_item_chrome(&self, ctx: &Context, painter: &egui::Painter, canvas: Rect) {
        if self.tool != Tool::Select {
            return;
        }
        let Some(idx) = self.selected else { return };
        let Some(ann) = self.annotations.get(idx) else { return };
        if self.text_edit.as_ref().is_some_and(|e| e.index == Some(idx)) {
            return;
        }
        let accent = ACCENT;
        // Endpoint-handled shapes get no dashed bounding box.
        let endpoint_style = matches!(
            ann.shape,
            Shape::Line { .. } | Shape::Arrow { .. } | Shape::Marker { target: Some(_), .. }
        );
        if !endpoint_style {
            let bbox = self.annotation_bbox(ctx, ann);
            let c = bbox.center();
            let corners =
                [bbox.left_top(), bbox.right_top(), bbox.right_bottom(), bbox.left_bottom()]
                    .map(|q| self.to_screen(canvas, rotate_around(q, c, ann.rotation)));
            for i in 0..4 {
                painter.extend(EguiShape::dashed_line(
                    &[corners[i], corners[(i + 1) % 4]],
                    Stroke::new(1.0, accent),
                    5.0,
                    4.0,
                ));
            }
        }
        for (pos, kind) in self.item_handles(ctx, canvas, ann) {
            match kind {
                ItemDragKind::Rotate => {
                    painter.circle_filled(pos, KNOB_RADIUS, Color32::WHITE);
                    painter.circle_stroke(pos, KNOB_RADIUS, Stroke::new(1.5, accent));
                    paint_rotate_icon(painter, pos, 6.0, Color32::from_gray(45));
                }
                ItemDragKind::Move => {
                    painter.circle_filled(pos, KNOB_RADIUS, accent);
                    painter.circle_stroke(pos, KNOB_RADIUS, Stroke::new(1.5, Color32::WHITE));
                    paint_move_icon(painter, pos, 7.0, Color32::WHITE);
                }
                _ => {
                    let r = Rect::from_center_size(pos, Vec2::splat(ITEM_HANDLE_SIZE));
                    painter.rect_filled(r, 1.0, Color32::WHITE);
                    painter.rect_stroke(r, 1.0, Stroke::new(1.0, accent), StrokeKind::Middle);
                }
            }
        }
    }

    /// Vertical toolbar beside the region (left, else right, else inside),
    /// draggable by its grip once the auto spot doesn't suit.
    fn toolbar_overlay(&mut self, ctx: &Context, canvas: Rect, sel_screen: Option<Rect>) {
        let Some(ss) = sel_screen else {
            // No toolbar → no picker; don't let it pop back up later.
            self.custom_picker = None;
            return;
        };
        let margin = 12.0;
        let size = self.toolbar_size;
        let default_pos = {
            let x = if ss.min.x - size.x - margin >= canvas.min.x {
                ss.min.x - size.x - margin
            } else if ss.max.x + size.x + margin <= canvas.max.x {
                ss.max.x + margin
            } else {
                ss.min.x + margin
            };
            Pos2::new(x, ss.min.y)
        };
        let lo = canvas.min;
        let hi = (canvas.max - size).max(lo);
        let pos = self.toolbar_pos.unwrap_or(default_pos).clamp(lo, hi);
        let area = egui::Area::new(Id::new("toolbar"))
            .fixed_pos(pos)
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.set_width(190.0);
                    // Chunky, easy-to-hit controls that stand out from the
                    // popup background.
                    let spacing = ui.spacing_mut();
                    spacing.slider_width = 150.0;
                    spacing.button_padding = Vec2::new(12.0, 8.0);
                    spacing.item_spacing = Vec2::new(8.0, 7.0);
                    spacing.interact_size = Vec2::new(40.0, 34.0);
                    let visuals = ui.visuals_mut();
                    visuals.widgets.inactive.weak_bg_fill = Color32::from_gray(58);
                    visuals.widgets.inactive.fg_stroke = Stroke::new(1.0, Color32::from_gray(235));
                    visuals.widgets.hovered.weak_bg_fill = Color32::from_gray(80);
                    visuals.widgets.hovered.fg_stroke = Stroke::new(1.5, Color32::WHITE);
                    visuals.widgets.active.weak_bg_fill = Color32::from_gray(96);
                    let styles = &mut ui.style_mut().text_styles;
                    if let Some(font) = styles.get_mut(&egui::TextStyle::Button) {
                        font.size = 17.0;
                    }
                    if let Some(font) = styles.get_mut(&egui::TextStyle::Body) {
                        font.size = 16.0;
                    }
                    let (grip_rect, grip) = ui
                        .allocate_exact_size(Vec2::new(ui.available_width(), 22.0), Sense::drag());
                    ui.painter().text(
                        grip_rect.center(),
                        Align2::CENTER_CENTER,
                        "• • •",
                        FontId::proportional(14.0),
                        ui.visuals().weak_text_color(),
                    );
                    if grip.hovered() || grip.dragged() {
                        ctx.set_cursor_icon(egui::CursorIcon::Grab);
                    }
                    if grip.dragged() {
                        self.toolbar_pos = Some(pos + grip.drag_delta());
                    }
                    ui.separator();
                    ui.with_layout(
                        egui::Layout::top_down_justified(egui::Align::Min),
                        |ui| {
                            for (tool, key) in TOOLS {
                                let active = self.tool == tool;
                                // Select is the workhorse: give it more
                                // presence than the drawing tools.
                                let is_select = tool == Tool::Select;
                                let text = RichText::new(format!("{key:?}  ·  {}", tool.label()))
                                    .size(if is_select { 19.0 } else { 17.0 })
                                    .color(if active {
                                        Color32::WHITE
                                    } else {
                                        Color32::from_gray(235)
                                    });
                                let mut btn = Button::new(text)
                                    .min_size(Vec2::new(0.0, if is_select { 46.0 } else { 34.0 }));
                                if active {
                                    btn = btn.fill(ACTIVE_TOOL_FILL);
                                }
                                let resp = ui.add(btn);
                                if resp.clicked() {
                                    resp.surrender_focus();
                                    self.set_tool(tool);
                                }
                            }
                        },
                    );
                    ui.separator();
                    // Settings target: text being edited, the selected
                    // object, or the defaults new objects will use. The
                    // controls display and edit the target's actual values.
                    let item_target = (self.tool == Tool::Select)
                        .then_some(self.selected)
                        .flatten()
                        .filter(|i| *i < self.annotations.len());
                    let editing_text = self.text_edit.is_some();
                    let shown_style = if let Some(edit) = &self.text_edit {
                        edit.style
                    } else if let Some(idx) = item_target {
                        self.annotations[idx].style
                    } else {
                        self.style
                    };
                    ui.vertical_centered(|ui| {
                        if editing_text || item_target.is_some() {
                            ui.label(RichText::new("For Current Object").color(ACCENT).strong());
                        } else {
                            ui.label(RichText::new("For New Objects").weak());
                        }
                    });
                    // Current color; click to open the picker.
                    let (swatch_rect, swatch) = ui.allocate_exact_size(
                        Vec2::new(ui.available_width(), 30.0),
                        Sense::click(),
                    );
                    ui.painter().rect_filled(swatch_rect, 4.0, shown_style.color);
                    ui.painter().rect_stroke(
                        swatch_rect,
                        4.0,
                        Stroke::new(1.0, Color32::from_gray(110)),
                        StrokeKind::Middle,
                    );
                    let [r, g, b, _] = shown_style.color.to_srgba_unmultiplied();
                    let luma =
                        0.299 * f32::from(r) + 0.587 * f32::from(g) + 0.114 * f32::from(b);
                    ui.painter().text(
                        swatch_rect.center(),
                        Align2::CENTER_CENTER,
                        "Color…",
                        FontId::proportional(15.0),
                        if luma > 140.0 { Color32::BLACK } else { Color32::WHITE },
                    );
                    if swatch.clicked() {
                        self.custom_picker = Some(shown_style.color);
                    }
                    if let Some(mut working) = self.custom_picker {
                        let mut commit = false;
                        let mut cancel = false;
                        let recents = self.palette.clone();
                        egui::Area::new(Id::new("custom-color-picker"))
                            .fixed_pos(swatch_rect.right_top() + Vec2::new(16.0, -160.0))
                            .order(egui::Order::Foreground)
                            .constrain_to(canvas)
                            .show(ctx, |ui| {
                                egui::Frame::popup(ui.style()).show(ui, |ui| {
                                    // Pin the popup width and let every
                                    // section fill it edge to edge.
                                    let popup_width = 460.0;
                                    ui.set_width(popup_width);
                                    ui.spacing_mut().slider_width = popup_width;
                                    ui.spacing_mut().interact_size = Vec2::new(56.0, 26.0);
                                    egui::color_picker::color_picker_color32(
                                        ui,
                                        &mut working,
                                        egui::color_picker::Alpha::Opaque,
                                    );
                                    ui.separator();
                                    ui.label(RichText::new("Selected color").strong());
                                    let (sel_rect, _) = ui.allocate_exact_size(
                                        Vec2::new(ui.available_width(), 40.0),
                                        Sense::hover(),
                                    );
                                    ui.painter().rect_filled(sel_rect, 4.0, working);
                                    ui.painter().rect_stroke(
                                        sel_rect,
                                        4.0,
                                        Stroke::new(1.0, Color32::from_gray(110)),
                                        StrokeKind::Middle,
                                    );
                                    ui.separator();
                                    // Recents load into the picker.
                                    ui.label(RichText::new("Recent").strong());
                                    ui.horizontal(|ui| {
                                        let n = recents.len().max(1) as f32;
                                        let spacing = ui.spacing().item_spacing.x;
                                        let sw = ((ui.available_width()
                                            - spacing * (n - 1.0))
                                            / n)
                                            .max(24.0);
                                        for &recent in &recents {
                                            let (rect, resp) = ui.allocate_exact_size(
                                                Vec2::new(sw, 46.0),
                                                Sense::click(),
                                            );
                                            ui.painter().rect_filled(rect, 4.0, recent);
                                            ui.painter().rect_stroke(
                                                rect,
                                                4.0,
                                                Stroke::new(1.0, Color32::from_gray(110)),
                                                StrokeKind::Middle,
                                            );
                                            if resp.clicked() {
                                                working = recent;
                                            }
                                        }
                                    });
                                    ui.separator();
                                    // Big OK / Cancel.
                                    ui.horizontal(|ui| {
                                        let half = (ui.available_width()
                                            - ui.spacing().item_spacing.x)
                                            / 2.0;
                                        let ok = ui.add(
                                            Button::new(RichText::new("OK").size(18.0))
                                                .min_size(Vec2::new(half, 46.0)),
                                        );
                                        if ok.clicked() {
                                            ok.surrender_focus();
                                            commit = true;
                                        }
                                        let cancel_btn = ui.add(
                                            Button::new(RichText::new("Cancel").size(18.0))
                                                .min_size(Vec2::new(half, 46.0)),
                                        );
                                        if cancel_btn.clicked() {
                                            cancel_btn.surrender_focus();
                                            cancel = true;
                                        }
                                    });
                                });
                            });
                        if commit {
                            self.apply_color(working);
                            self.custom_picker = None;
                        } else if cancel {
                            self.custom_picker = None;
                        } else {
                            self.custom_picker = Some(working);
                        }
                    }
                    ui.label("Width");
                    let mut width_val = shown_style.width;
                    let mut size_val = shown_style.font_size;
                    let width_slider =
                        ui.add(Slider::new(&mut width_val, 1.0..=24.0).fixed_decimals(0));
                    ui.label("Text size");
                    let size_slider =
                        ui.add(Slider::new(&mut size_val, 8.0..=120.0).fixed_decimals(0));
                    let changed = width_slider.changed() || size_slider.changed();
                    if changed {
                        // The new values become the session defaults too.
                        self.style.width = width_val;
                        self.style.font_size = size_val;
                        if let Some(edit) = &mut self.text_edit {
                            // Restyle the text being typed; lands with its
                            // commit (one undo step).
                            edit.style.width = width_val;
                            edit.style.font_size = size_val;
                        } else if let Some(idx) = item_target {
                            // Restyle the selected item live — one undo step
                            // per continuous adjustment.
                            if !self.style_undo_pushed {
                                self.push_undo();
                                self.style_undo_pushed = true;
                            }
                            let ann = &mut self.annotations[idx];
                            ann.style.width = width_val;
                            ann.style.font_size = size_val;
                        } else {
                            // Nothing selected: a deliberate default, worth
                            // persisting as a preference.
                            self.persist_style = true;
                        }
                    } else if !width_slider.dragged() && !size_slider.dragged() {
                        self.style_undo_pushed = false;
                    }
                    // Sliders hold keyboard focus after use, which would
                    // silently eat every shortcut (S, Space, Esc, Del…).
                    if width_slider.drag_stopped() {
                        width_slider.surrender_focus();
                    }
                    if size_slider.drag_stopped() {
                        size_slider.surrender_focus();
                    }
                    ui.separator();
                    ui.with_layout(
                        egui::Layout::top_down_justified(egui::Align::Min),
                        |ui| {
                            let tall = Vec2::new(0.0, 34.0);
                            let fit_btn = ui.add(Button::new("F  ·  Reset view").min_size(tall));
                            if fit_btn.clicked() {
                                fit_btn.surrender_focus();
                                self.fit_view(canvas.size());
                            }
                            let reset_btn =
                                ui.add(Button::new("Shift+Esc  ·  Reset All").min_size(tall));
                            if reset_btn.clicked() {
                                reset_btn.surrender_focus();
                                self.reset_all();
                            }
                            let undo_btn = ui.add_enabled(
                                !self.undo.is_empty(),
                                Button::new("Ctrl+Z  ·  Undo").min_size(tall),
                            );
                            if undo_btn.clicked() {
                                undo_btn.surrender_focus();
                                self.undo();
                            }
                            let redo_btn = ui.add_enabled(
                                !self.redo.is_empty(),
                                Button::new("Ctrl+Y  ·  Redo").min_size(tall),
                            );
                            if redo_btn.clicked() {
                                redo_btn.surrender_focus();
                                self.redo();
                            }
                            ui.separator();
                            let tall = Vec2::new(0.0, 38.0);
                            if ui.add(Button::new("Ctrl+C  ·  Copy").min_size(tall)).clicked() {
                                self.copy(ctx);
                            }
                            let save_btn = ui.add(Button::new("Ctrl+S  ·  Save").min_size(tall));
                            if save_btn.clicked() {
                                save_btn.surrender_focus();
                                self.save(ctx);
                            }
                            if ui.add(Button::new("Esc  ·  Close").min_size(tall)).clicked() {
                                ctx.send_viewport_cmd(ViewportCommand::Close);
                            }
                        },
                    );
                });
            });
        self.toolbar_size = area.response.rect.size();
    }

    /// Inline text editing: a frameless TextEdit rendered at the text's spot,
    /// with the same font/color the committed annotation will have. Never
    /// soft-wraps — lines break only at typed newlines.
    fn text_editor_overlay(&mut self, ctx: &Context, canvas: Rect) {
        let Some(mut edit) = self.text_edit.take() else { return };
        let screen_pos = self.to_screen(canvas, edit.pos);
        let font = FontId::proportional((edit.style.font_size * self.zoom).max(9.0));
        // Size the editor to its content so clicks next to the text still
        // reach the canvas (and commit); the margin absorbs the one-frame
        // lag of the measurement.
        let width = ctx
            .fonts_mut(|f| f.layout_no_wrap(edit.buffer.clone(), font.clone(), Color32::WHITE))
            .size()
            .x
            + font.size * 2.0;
        let mut commit = false;
        let mut cancel = false;
        let layout_font = font.clone();
        let text_color = edit.style.color;
        let mut layouter = move |ui: &Ui, buf: &dyn egui::TextBuffer, _wrap: f32| {
            ui.fonts_mut(|f| {
                f.layout_no_wrap(buf.as_str().to_owned(), layout_font.clone(), text_color)
            })
        };
        egui::Area::new(Id::new("text-editor"))
            .fixed_pos(screen_pos)
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                let response = ui.add(
                    TextEdit::multiline(&mut edit.buffer)
                        .font(font)
                        .text_color(edit.style.color)
                        .frame(egui::Frame::NONE)
                        .margin(Margin::ZERO)
                        .desired_rows(1)
                        .desired_width(width)
                        .layouter(&mut layouter),
                );
                if edit.just_created {
                    response.request_focus();
                    edit.just_created = false;
                }
                let (enter, shift, esc) = ui.input(|i| {
                    (
                        i.key_pressed(Key::Enter),
                        i.modifiers.shift,
                        i.key_pressed(Key::Escape),
                    )
                });
                if enter && !shift {
                    commit = true;
                }
                if esc {
                    cancel = true;
                }
            });
        if cancel {
            // Drop the buffer; an existing annotation reappears unchanged.
        } else {
            self.text_edit = Some(edit);
            if commit {
                self.commit_text();
            }
        }
    }
}

/// Small triangle used as an arrowhead for the knob icons.
fn icon_arrowhead(painter: &egui::Painter, tip: Pos2, dir: Vec2, size: f32, color: Color32) {
    let normal = Vec2::new(-dir.y, dir.x) * (size * 0.6);
    let base = tip - dir * size;
    painter.add(EguiShape::convex_polygon(
        vec![tip, base + normal, base - normal],
        color,
        Stroke::NONE,
    ));
}

/// Curved two-headed arrow: the rotate glyph.
fn paint_rotate_icon(painter: &egui::Painter, center: Pos2, radius: f32, color: Color32) {
    let (start, end) = (-220_f32.to_radians(), 40_f32.to_radians());
    let steps = 16;
    let points: Vec<Pos2> = (0..=steps)
        .map(|i| {
            let a = start + (end - start) * (i as f32 / steps as f32);
            center + Vec2::new(a.cos(), a.sin()) * radius
        })
        .collect();
    painter.add(EguiShape::line(points, Stroke::new(1.8, color)));
    // Heads tangent to the arc at both ends.
    let tangent = |a: f32| Vec2::new(-a.sin(), a.cos());
    icon_arrowhead(
        painter,
        center + Vec2::new(start.cos(), start.sin()) * radius,
        -tangent(start),
        4.5,
        color,
    );
    icon_arrowhead(
        painter,
        center + Vec2::new(end.cos(), end.sin()) * radius,
        tangent(end),
        4.5,
        color,
    );
}

/// Four-way arrow cross: the move glyph.
fn paint_move_icon(painter: &egui::Painter, center: Pos2, radius: f32, color: Color32) {
    let stroke = Stroke::new(1.8, color);
    for dir in [Vec2::RIGHT, Vec2::LEFT, Vec2::DOWN, Vec2::UP] {
        painter.line_segment([center, center + dir * (radius - 2.0)], stroke);
        icon_arrowhead(painter, center + dir * radius, dir, 4.0, color);
    }
}

/// Full-screen alignment lines at the given x/y coordinates. Each is a
/// white–black–white sandwich so it stands out on any background color.
fn paint_crosshairs(painter: &egui::Painter, canvas: Rect, xs: &[f32], ys: &[f32]) {
    let white = Stroke::new(1.0, Color32::from_white_alpha(200));
    let black = Stroke::new(1.0, Color32::from_black_alpha(220));
    for &x in xs {
        for (offset, stroke) in [(-1.0, white), (0.0, black), (1.0, white)] {
            painter.line_segment(
                [Pos2::new(x + offset, canvas.min.y), Pos2::new(x + offset, canvas.max.y)],
                stroke,
            );
        }
    }
    for &y in ys {
        for (offset, stroke) in [(-1.0, white), (0.0, black), (1.0, white)] {
            painter.line_segment(
                [Pos2::new(canvas.min.x, y + offset), Pos2::new(canvas.max.x, y + offset)],
                stroke,
            );
        }
    }
}

/// The up-to-four rectangles of `outer` not covered by `inner`.
fn subtract_rect(outer: Rect, inner: Rect) -> Vec<Rect> {
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

impl eframe::App for ScreencapApp {
    fn on_exit(&mut self) {
        if self.demo {
            return;
        }
        // The invariant (head == active color) makes the palette the whole
        // color state worth keeping.
        self.promote_color(self.style.color);
        prefs::save(&self.palette, self.persist_style.then_some(&self.style));
    }

    fn ui(&mut self, root: &mut Ui, _frame: &mut eframe::Frame) {
        let ctx = root.ctx().clone();
        self.sync_texture(&ctx);
        let ctx = &ctx;
        self.poll_recapture(ctx);

        // Docs/dev hook: request a window screenshot once the UI settles,
        // save it, quit.
        if self.shot_path.is_some() {
            self.shot_frames += 1;
            if self.shot_frames == 15 {
                ctx.send_viewport_cmd(ViewportCommand::Screenshot(Default::default()));
            }
            let shot = ctx.input(|i| {
                i.events.iter().find_map(|e| match e {
                    egui::Event::Screenshot { image, .. } => Some(image.clone()),
                    _ => None,
                })
            });
            if let Some(image) = shot
                && let Some(path) = self.shot_path.take()
            {
                let (w, h) = (
                    u32::try_from(image.size[0]).unwrap_or(0),
                    u32::try_from(image.size[1]).unwrap_or(0),
                );
                let bytes: Vec<u8> = image.pixels.iter().flat_map(|p| p.to_array()).collect();
                match RgbaImage::from_raw(w, h, bytes) {
                    Some(shot) => {
                        if let Err(err) = shot.save(&path) {
                            eprintln!("screenshot save failed: {err}");
                        } else {
                            println!("{}", path.display());
                        }
                    }
                    None => eprintln!("screenshot buffer size mismatch"),
                }
                ctx.send_viewport_cmd(ViewportCommand::Close);
            }
            ctx.request_repaint();
        }

        // The discard prompt disarms after its second, or as soon as the
        // user does anything other than pressing Esc again.
        if let Some(until) = self.discard_armed {
            let (now, other_activity) = ctx.input(|i| {
                let other = i.events.iter().any(|e| match e {
                    egui::Event::Key { key, pressed: true, .. } => *key != Key::Escape,
                    egui::Event::PointerButton { pressed: true, .. } => true,
                    egui::Event::MouseWheel { .. } => true,
                    _ => false,
                });
                (i.time, other)
            });
            if other_activity {
                self.discard_armed = None;
                self.toast = None;
            } else if now >= until {
                self.discard_armed = None;
            }
        }

        self.handle_shortcuts(ctx, self.last_canvas_size);

        CentralPanel::default()
            .frame(egui::Frame::NONE.fill(Color32::BLACK))
            .show(root, |ui| {
                let (response, painter) =
                    ui.allocate_painter(ui.available_size(), Sense::click_and_drag());
                let canvas = response.rect;
                self.last_canvas_size = canvas.size();

                if !self.view_fitted {
                    self.fit_view(canvas.size());
                    self.view_fitted = true;
                }

                // Zoom around the pointer.
                if response.hovered() {
                    let scroll = ctx.input(|i| i.smooth_scroll_delta.y);
                    if scroll.abs() > 0.1
                        && let Some(pointer) = response.hover_pos()
                    {
                        let factor = (scroll * 0.003).exp();
                        let new_zoom = (self.zoom * factor).clamp(0.02, 16.0);
                        let factor = new_zoom / self.zoom;
                        let rel = pointer - canvas.min - self.pan;
                        self.pan += rel * (1.0 - factor);
                        self.zoom = new_zoom;
                    }
                }

                // Pan with middle drag (right drag redraws the region), or
                // with space+drag regardless of the active tool.
                if response.dragged_by(PointerButton::Middle) {
                    self.pan += response.drag_delta();
                }
                let space_pan =
                    self.text_edit.is_none() && ctx.input(|i| i.key_down(Key::Space));
                if space_pan
                    && response.dragged_by(PointerButton::Primary)
                    && self.item_drag.is_none()
                    && self.select_drag.is_none()
                    && self.drag.is_none()
                {
                    self.pan += response.drag_delta();
                    self.space_panned = true;
                }

                let img_rect = self.image_rect();
                // Hit-test drag starts against the box as it was when the
                // button went down (selection is untouched since last frame).
                let sel_screen = self.selection.map(|s| {
                    Rect::from_min_max(self.to_screen(canvas, s.min), self.to_screen(canvas, s.max))
                });

                let primary_started = response.drag_started_by(PointerButton::Primary);
                let secondary_started = response.drag_started_by(PointerButton::Secondary);
                if !space_pan
                    && (primary_started || secondary_started)
                    && let Some(pos) = ctx
                        .input(|i| i.pointer.press_origin())
                        .or_else(|| response.interact_pointer_pos())
                {
                    if self.text_edit.is_some() {
                        self.commit_text();
                    } else {
                        let p = self.to_image(canvas, pos);
                        let clamped = p.clamp(img_rect.min, img_rect.max);
                        if secondary_started {
                            // Right-drag always starts a fresh box.
                            if self.item_drag.is_none() {
                                self.selection = None;
                                self.select_drag = Some(SelectDrag::Draw { anchor: clamped });
                            }
                        } else if self.tool == Tool::Select
                            && let Some(kind) = self.item_handle_at(ctx, canvas, pos)
                            && let Some(idx) = self.selected
                            && let Some(ann) = self.annotations.get(idx).cloned()
                        {
                            let bbox = self.annotation_bbox(ctx, &ann);
                            self.push_undo();
                            self.item_drag = Some(ItemDrag {
                                index: idx,
                                center: bbox.center(),
                                bbox0: bbox,
                                grab: p,
                                start: ann,
                                kind,
                            });
                        } else if let (Some(sel), Some(ss)) = (self.selection, sel_screen)
                            && let Some(edges) = hit_handle(ss, pos)
                        {
                            // Region handles win over the active tool.
                            self.select_drag =
                                Some(SelectDrag::Resize { start_rect: sel, edges });
                        } else if self.tool == Tool::Select {
                            if let Some(idx) = self.hit_test_items(ctx, p, false) {
                                self.selected = Some(idx);
                                let ann = self.annotations[idx].clone();
                                let bbox = self.annotation_bbox(ctx, &ann);
                                self.push_undo();
                                self.item_drag = Some(ItemDrag {
                                    index: idx,
                                    center: bbox.center(),
                                    bbox0: bbox,
                                    grab: p,
                                    start: ann,
                                    kind: ItemDragKind::Move,
                                });
                            } else if let (Some(sel), Some(ss)) = (self.selection, sel_screen)
                                && ss.contains(pos)
                            {
                                self.select_drag =
                                    Some(SelectDrag::Move { start_rect: sel, grab: p });
                            } else {
                                self.selection = None;
                                self.select_drag = Some(SelectDrag::Draw { anchor: clamped });
                            }
                        } else {
                            self.drag =
                                Some(DragState { start: p, current: p, points: vec![p] });
                        }
                    }
                }
                if let Some(pos) = response.interact_pointer_pos() {
                    let p = self.to_image(canvas, pos);
                    if response.dragged_by(PointerButton::Primary) && self.item_drag.is_some() {
                        self.apply_item_drag(p);
                    } else if (response.dragged_by(PointerButton::Primary)
                        || response.dragged_by(PointerButton::Secondary))
                        && self.select_drag.is_some()
                    {
                        let clamped = p.clamp(img_rect.min, img_rect.max);
                        match self.select_drag {
                            Some(SelectDrag::Draw { anchor }) => {
                                self.selection = Some(Rect::from_two_pos(anchor, clamped));
                            }
                            Some(SelectDrag::Move { start_rect, grab }) => {
                                self.selection = Some(clamp_rect_within(
                                    start_rect.translate(p - grab),
                                    img_rect,
                                ));
                            }
                            Some(SelectDrag::Resize { start_rect, edges }) => {
                                self.selection = Some(resize_rect(start_rect, edges, clamped));
                            }
                            None => {}
                        }
                    } else if response.dragged_by(PointerButton::Primary) && self.drag.is_some() {
                        let threshold = (0.75 / self.zoom).max(0.25);
                        let is_pen = self.tool == Tool::Pen;
                        if let Some(drag) = &mut self.drag {
                            drag.current = p;
                            if is_pen
                                && drag
                                    .points
                                    .last()
                                    .is_none_or(|last| last.distance(p) >= threshold)
                            {
                                drag.points.push(p);
                            }
                        }
                    }
                }
                if response.drag_stopped_by(PointerButton::Primary)
                    || response.drag_stopped_by(PointerButton::Secondary)
                {
                    if let Some(item_drag) = self.item_drag.take() {
                        // A no-op drag (just a grab) shouldn't burn an undo slot.
                        if self
                            .annotations
                            .get(item_drag.index)
                            .is_some_and(|a| *a == item_drag.start)
                        {
                            self.undo.pop();
                        }
                    } else if self.select_drag.is_some() {
                        // Discard sub-4-px accidents; moves/resizes just end.
                        if let Some(SelectDrag::Draw { .. }) = self.select_drag.take()
                            && let Some(sel) = self.selection
                            && (sel.width() < 4.0 || sel.height() < 4.0)
                        {
                            self.selection = None;
                        }
                    } else if response.drag_stopped_by(PointerButton::Primary) {
                        self.finish_drag();
                    }
                }
                if response.clicked_by(PointerButton::Primary) && !space_pan {
                    if self.text_edit.is_some() {
                        self.commit_text();
                        // With the Text tool, a click elsewhere chains
                        // straight into new text there.
                        if self.tool == Tool::Text
                            && let Some(pos) = response.interact_pointer_pos()
                        {
                            let p = self.to_image(canvas, pos);
                            self.text_tool_click(ctx, p);
                        }
                    } else if let Some(pos) = response.interact_pointer_pos() {
                        let p = self.to_image(canvas, pos);
                        match self.tool {
                            Tool::Text => self.text_tool_click(ctx, p),
                            Tool::Marker => self.drop_marker(p, None),
                            Tool::Select => {
                                if let Some(idx) = self.hit_test_items(ctx, p, false) {
                                    self.selected = Some(idx);
                                } else if self.selected.take().is_some() {
                                    // Just deselect.
                                } else if sel_screen
                                    .is_none_or(|ss| !ss.expand(HANDLE_HIT).contains(pos))
                                {
                                    // A click away from the box clears it.
                                    self.selection = None;
                                }
                            }
                            _ => {}
                        }
                    }
                }
                // Double-click on text edits it, whatever the tool (Marker
                // excepted — its clicks drop markers).
                if response.double_clicked_by(PointerButton::Primary)
                    && self.tool != Tool::Marker
                    && self.text_edit.is_none()
                    && let Some(pos) = response.interact_pointer_pos()
                {
                    let p = self.to_image(canvas, pos);
                    if let Some(idx) = self.hit_test_items(ctx, p, true) {
                        self.open_text_editor(idx);
                    }
                }

                let sel_screen = self.selection.map(|s| {
                    Rect::from_min_max(self.to_screen(canvas, s.min), self.to_screen(canvas, s.max))
                });

                // Item under the pointer (Select tool, idle) — drives the
                // hover outline and the Move cursor.
                let hovered_item = if self.tool == Tool::Select
                    && self.item_drag.is_none()
                    && self.select_drag.is_none()
                    && self.drag.is_none()
                {
                    response
                        .hover_pos()
                        .and_then(|pos| self.hit_test_items(ctx, self.to_image(canvas, pos), false))
                } else {
                    None
                };

                let cursor = if let Some(item_drag) = &self.item_drag {
                    match item_drag.kind {
                        ItemDragKind::Move => egui::CursorIcon::Grabbing,
                        ItemDragKind::Resize { edges } => edges_cursor(edges),
                        ItemDragKind::ScaleUniform => egui::CursorIcon::ResizeNwSe,
                        ItemDragKind::Rotate => egui::CursorIcon::Grabbing,
                        ItemDragKind::Endpoint { .. } => egui::CursorIcon::Crosshair,
                    }
                } else if let Some(select_drag) = &self.select_drag {
                    match select_drag {
                        SelectDrag::Draw { .. } => egui::CursorIcon::Crosshair,
                        SelectDrag::Move { .. } => egui::CursorIcon::Grabbing,
                        SelectDrag::Resize { edges, .. } => edges_cursor(*edges),
                    }
                } else if self.drag.is_some() {
                    egui::CursorIcon::Crosshair
                } else if space_pan {
                    if response.dragged_by(PointerButton::Primary) {
                        egui::CursorIcon::Grabbing
                    } else {
                        egui::CursorIcon::Grab
                    }
                } else if let Some(pos) = response.hover_pos() {
                    let handle = (self.tool == Tool::Select)
                        .then(|| self.item_handle_at(ctx, canvas, pos))
                        .flatten();
                    if let Some(kind) = handle {
                        match kind {
                            ItemDragKind::Resize { edges } => edges_cursor(edges),
                            ItemDragKind::ScaleUniform => egui::CursorIcon::ResizeNwSe,
                            ItemDragKind::Rotate => egui::CursorIcon::Grab,
                            ItemDragKind::Move => egui::CursorIcon::Move,
                            _ => egui::CursorIcon::Crosshair,
                        }
                    } else if let Some(ss) = sel_screen
                        && let Some(edges) = hit_handle(ss, pos)
                    {
                        edges_cursor(edges)
                    } else if self.tool == Tool::Select {
                        if hovered_item.is_some() {
                            egui::CursorIcon::Move
                        } else if sel_screen.is_some_and(|ss| ss.contains(pos)) {
                            egui::CursorIcon::Grab
                        } else {
                            egui::CursorIcon::Crosshair
                        }
                    } else {
                        egui::CursorIcon::Crosshair
                    }
                } else {
                    egui::CursorIcon::Default
                };
                ctx.set_cursor_icon(cursor);

                // Draw: texture, then annotations (clipped to the image so
                // strokes don't spill onto the letterbox), then chrome.
                let img_screen = Rect::from_min_max(
                    self.to_screen(canvas, img_rect.min),
                    self.to_screen(canvas, img_rect.max),
                );
                if let Some(texture) = &self.texture {
                    painter.image(
                        texture.id(),
                        img_screen,
                        Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                        Color32::WHITE,
                    );
                }
                let clipped = painter.with_clip_rect(img_screen.intersect(canvas));
                let editing_idx = self.text_edit.as_ref().and_then(|e| e.index);
                for (i, ann) in self.annotations.iter().enumerate() {
                    if editing_idx == Some(i) {
                        continue;
                    }
                    self.paint_annotation(&clipped, canvas, ann);
                }
                self.paint_drag_preview(&clipped, canvas);

                // Ghost previews following the cursor while a placement tool
                // is armed.
                let tool_idle = self.drag.is_none()
                    && self.select_drag.is_none()
                    && self.item_drag.is_none()
                    && self.text_edit.is_none()
                    && !space_pan;
                if self.tool == Tool::Marker
                    && tool_idle
                    && let Some(pos) = response.hover_pos()
                {
                    let radius = marker_radius(&self.style) * self.zoom;
                    painter.circle_filled(pos, radius, self.style.color.gamma_multiply(0.45));
                    painter.text(
                        pos,
                        Align2::CENTER_CENTER,
                        self.marker_next.to_string(),
                        FontId::proportional(self.style.font_size * self.zoom),
                        Color32::WHITE.gamma_multiply(0.6),
                    );
                }

                if let (Some(sel), Some(ss)) = (self.selection, sel_screen) {
                    for outside in subtract_rect(canvas, ss) {
                        painter.rect_filled(
                            outside,
                            CornerRadius::ZERO,
                            Color32::from_black_alpha(120),
                        );
                    }
                    painter.rect_stroke(
                        ss,
                        CornerRadius::ZERO,
                        Stroke::new(1.5, Color32::WHITE),
                        StrokeKind::Middle,
                    );
                    if !matches!(self.select_drag, Some(SelectDrag::Draw { .. })) {
                        for (pos, _) in selection_handles(ss) {
                            let handle =
                                Rect::from_center_size(pos, Vec2::splat(REGION_HANDLE_SIZE));
                            painter.rect_filled(handle, 2.0, Color32::WHITE);
                            painter.rect_stroke(
                                handle,
                                2.0,
                                Stroke::new(1.0, Color32::from_black_alpha(180)),
                                StrokeKind::Middle,
                            );
                        }
                    }
                    let dims = format!("{}×{}", sel.width().round(), sel.height().round());
                    let (pos, align) = if ss.min.y - canvas.min.y > 24.0 {
                        (ss.left_top() + Vec2::new(0.0, -10.0), Align2::LEFT_BOTTOM)
                    } else {
                        (ss.left_top() + Vec2::new(10.0, 10.0), Align2::LEFT_TOP)
                    };
                    painter.text(pos, align, dims, FontId::proportional(14.0), Color32::WHITE);
                } else {
                    painter.rect_filled(canvas, CornerRadius::ZERO, Color32::from_black_alpha(70));
                    painter.text(
                        Pos2::new(canvas.center().x, canvas.min.y + 40.0),
                        Align2::CENTER_CENTER,
                        "drag to select a region · Enter: copy whole screen · Esc Esc: discard",
                        FontId::proportional(16.0),
                        Color32::WHITE,
                    );
                }

                // Soft outline around the hoverable item so it reads as
                // clickable before committing to a selection.
                if let Some(idx) = hovered_item
                    && self.selected != Some(idx)
                    && editing_idx != Some(idx)
                    && let Some(ann) = self.annotations.get(idx)
                {
                    let bbox = self.annotation_bbox(ctx, ann).expand(6.0 / self.zoom);
                    let c = bbox.center();
                    let pts: Vec<Pos2> =
                        [bbox.left_top(), bbox.right_top(), bbox.right_bottom(), bbox.left_bottom()]
                            .into_iter()
                            .map(|q| self.to_screen(canvas, rotate_around(q, c, ann.rotation)))
                            .collect();
                    painter.add(EguiShape::closed_line(
                        pts,
                        Stroke::new(2.0, ACCENT.gamma_multiply(0.6)),
                    ));
                }

                // Full-screen crosshairs: box-edge extensions while drawing a
                // region, a single pair under the cursor while none exists.
                if matches!(self.select_drag, Some(SelectDrag::Draw { .. }))
                    && let Some(ss) = sel_screen
                {
                    paint_crosshairs(
                        &painter,
                        canvas,
                        &[ss.min.x, ss.max.x],
                        &[ss.min.y, ss.max.y],
                    );
                } else if self.selection.is_none()
                    && self.select_drag.is_none()
                    && self.drag.is_none()
                    && self.item_drag.is_none()
                    && !space_pan
                    && let Some(pos) = response.hover_pos()
                {
                    paint_crosshairs(&painter, canvas, &[pos.x], &[pos.y]);
                }

                self.paint_item_chrome(ctx, &painter, canvas);

                // Bottom line: an error toast, or a key hint when idle.
                let now = ui.input(|i| i.time);
                let bottom = Pos2::new(canvas.center().x, canvas.max.y - 24.0);
                if let Some(toast) = &self.toast {
                    if toast.until > now {
                        let color = if toast.is_error {
                            ui.visuals().error_fg_color
                        } else {
                            Color32::WHITE
                        };
                        painter.text(
                            bottom,
                            Align2::CENTER_CENTER,
                            &toast.message,
                            FontId::proportional(15.0),
                            color,
                        );
                    } else {
                        self.toast = None;
                    }
                } else if self.selection.is_some()
                    && self.select_drag.is_none()
                    && self.drag.is_none()
                    && self.item_drag.is_none()
                {
                    painter.text(
                        bottom,
                        Align2::CENTER_CENTER,
                        "S / tap Space: select tool · double-click text to edit · Del: delete · right-drag: new region · Space+drag: pan · F: reset view · Enter/Ctrl+C: copy+quit · Ctrl+S: save · Esc Esc: discard",
                        FontId::proportional(13.0),
                        Color32::from_gray(190),
                    );
                }

                self.screen_button_overlay(ctx, canvas);
                self.toolbar_overlay(ctx, canvas, sel_screen);
                self.text_editor_overlay(ctx, canvas);
            });
    }
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
        let mut rect =
            Shape::Rect { rect: Rect::from_min_max(Pos2::ZERO, Pos2::new(2.0, 2.0)) };
        translate_shape(&mut rect, d);
        assert_eq!(
            rect,
            Shape::Rect {
                rect: Rect::from_min_max(Pos2::new(5.0, -3.0), Pos2::new(7.0, -1.0))
            }
        );
    }
}
