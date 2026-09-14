//! The always-visible toolbar: tools two per row, the style controls, the
//! action buttons, and — at its foot — the status line explaining what the
//! current editor state affords (or demanding attention, e.g. the discard
//! confirmation). Auto-placed beside the region, draggable by its grip.

use eframe::egui::{
    self, Align2, Button, Color32, Context, FontId, Id, Pos2, Rect, RichText, Sense, Slider,
    Stroke, StrokeKind, Vec2,
};

use crate::annotate::Tool;
use crate::editor::{Editor, StatusKind};
use crate::ui::{ACCENT, ACTIVE_TOOL_FILL, BTN_H, TOOLBAR_W, TOOLS, UiScale, color_picker};

/// Actions the toolbar can't perform itself (they need export/clipboard/
/// viewport access); the app layer executes them.
pub enum ToolbarAction {
    Copy { close: bool },
    Save { close: bool },
    Close,
}

/// An app-level message that overrides the editor's status line (save/copy
/// results and failures).
pub struct StatusOverride {
    pub message: String,
    pub is_error: bool,
}

pub struct Toolbar {
    /// Where the user dragged the toolbar; `None` = auto beside the region.
    pos: Option<Pos2>,
    /// Rect measured last frame, for the auto placement.
    size: Vec2,
    /// User-chosen toolbar density; persisted via `prefs`.
    pub ui_scale: UiScale,
    /// Tallest the status line has needed to be so far this session, so it
    /// can only grow — never shrink back down and make the panel jump.
    status_min_h: f32,
}

impl Toolbar {
    pub fn new(ui_scale: UiScale) -> Self {
        // Only the first frame's auto-placement reads this; the measured
        // rect replaces it after that.
        let w = TOOLBAR_W * ui_scale.factor();
        Self { pos: None, size: Vec2::new(w + 30.0, 700.0), ui_scale, status_min_h: 0.0 }
    }

    pub fn show(
        &mut self,
        ctx: &Context,
        editor: &mut Editor,
        canvas: Rect,
        status_override: Option<StatusOverride>,
    ) -> Option<ToolbarAction> {
        // Initial region selection shows nothing but the frozen frame and
        // the crosshairs — no toolbar until a region exists (and no picker
        // left behind to pop back up later).
        let Some(ss) = editor
            .doc
            .region
            .map(|r| editor.view.rect_to_screen(canvas, r))
        else {
            editor.color_picker = None;
            return None;
        };
        let mut action = None;
        let margin = 12.0;
        let size = self.size;
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
        let pos = self.pos.unwrap_or(default_pos).clamp(lo, hi);
        // Every size in the panel derives from this one factor, so Small,
        // Medium, and Large stay proportional instead of drifting apart.
        let scale = self.ui_scale.factor();
        let toolbar_w = (TOOLBAR_W * scale).round();
        let btn_h = (BTN_H * scale).round();
        let btn_font = (15.0 * scale).round().max(10.0);
        let area = egui::Area::new(Id::new("toolbar"))
            .fixed_pos(pos)
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.set_width(toolbar_w);
                    // Chunky, easy-to-hit controls that stand out from the
                    // popup background.
                    let spacing = ui.spacing_mut();
                    // Rail, less the drag-value box egui puts beside it.
                    spacing.slider_width = toolbar_w - 40.0 * scale;
                    spacing.button_padding = Vec2::new(8.0, 6.0) * scale;
                    spacing.item_spacing = Vec2::new(6.0, 6.0) * scale;
                    spacing.interact_size = Vec2::new(36.0 * scale, btn_h);
                    let visuals = ui.visuals_mut();
                    visuals.widgets.inactive.weak_bg_fill = Color32::from_gray(58);
                    visuals.widgets.inactive.fg_stroke = Stroke::new(1.0, Color32::from_gray(235));
                    visuals.widgets.hovered.weak_bg_fill = Color32::from_gray(80);
                    visuals.widgets.hovered.fg_stroke = Stroke::new(1.5, Color32::WHITE);
                    visuals.widgets.active.weak_bg_fill = Color32::from_gray(96);
                    let styles = &mut ui.style_mut().text_styles;
                    if let Some(font) = styles.get_mut(&egui::TextStyle::Button) {
                        font.size = btn_font;
                    }
                    if let Some(font) = styles.get_mut(&egui::TextStyle::Body) {
                        font.size = btn_font;
                    }
                    let gap = ui.spacing().item_spacing.x;
                    let (grip_rect, grip) = ui.allocate_exact_size(
                        Vec2::new(ui.available_width(), 22.0 * scale),
                        Sense::drag(),
                    );
                    ui.painter().text(
                        grip_rect.center(),
                        Align2::CENTER_CENTER,
                        "• • •",
                        FontId::proportional((14.0 * scale).round().max(10.0)),
                        ui.visuals().weak_text_color(),
                    );
                    if grip.hovered() || grip.dragged() {
                        ctx.set_cursor_icon(egui::CursorIcon::Grab);
                    }
                    if grip.dragged() {
                        self.pos = Some(pos + grip.drag_delta());
                    }
                    ui.separator();

                    // Toolbar density: a tiny, always-visible setting near
                    // the top rather than buried in a menu, so it's easy to
                    // find the first time the default doesn't fit. The label
                    // sits on its own centered line — same pattern as "For
                    // New Objects" below — so the three buttons start flush
                    // with the panel's left edge instead of wherever the
                    // label happened to end, lining their span up with
                    // every other full-width row (tool pairs, action pairs).
                    ui.vertical_centered(|ui| {
                        ui.label(
                            RichText::new("UI size")
                                .size((11.0 * scale).round().max(9.0))
                                .color(ui.visuals().weak_text_color()),
                        );
                    });
                    ui.horizontal(|ui| {
                        let third = (ui.available_width() - 2.0 * gap) / 3.0;
                        for opt in UiScale::ALL {
                            let active = self.ui_scale == opt;
                            let text = RichText::new(opt.letter())
                                .size((13.0 * scale).round().max(10.0))
                                .color(if active { Color32::WHITE } else { Color32::from_gray(235) });
                            let mut btn = Button::new(text);
                            if active {
                                btn = btn.fill(ACTIVE_TOOL_FILL);
                            }
                            let resp = ui
                                .add_sized(Vec2::new(third, (22.0 * scale).round()), btn)
                                .on_hover_text(opt.name());
                            if resp.clicked() {
                                resp.surrender_focus();
                                self.ui_scale = opt;
                            }
                        }
                    });
                    ui.separator();

                    // Tools, two per row.
                    let half = (ui.available_width() - gap) / 2.0;
                    for pair in TOOLS.chunks(2) {
                        ui.horizontal(|ui| {
                            for (tool, key) in pair {
                                let active = editor.tool == *tool;
                                let text = RichText::new(format!("{key:?} · {}", tool.label()))
                                    .size(btn_font)
                                    .color(if active {
                                        Color32::WHITE
                                    } else {
                                        Color32::from_gray(235)
                                    });
                                let mut btn = Button::new(text);
                                if active {
                                    btn = btn.fill(ACTIVE_TOOL_FILL);
                                }
                                let resp = ui.add_sized(Vec2::new(half, btn_h), btn);
                                if resp.clicked() {
                                    resp.surrender_focus();
                                    editor.set_tool(*tool);
                                }
                            }
                        });
                    }
                    ui.separator();

                    // Settings target: text being edited, the selection, or
                    // the defaults new objects will use. The controls display
                    // and edit the target's actual values.
                    let editing_text = editor.state.is_text_editing();
                    let selection_target =
                        editor.tool == Tool::Select && !editor.selected.is_empty();
                    let shown_style = editor.shown_style();
                    ui.vertical_centered(|ui| {
                        if editing_text || (selection_target && editor.selected.len() == 1) {
                            ui.label(RichText::new("For Current Object").color(ACCENT).strong());
                        } else if selection_target {
                            ui.label(
                                RichText::new(format!("For {} Selected", editor.selected.len()))
                                    .color(ACCENT)
                                    .strong(),
                            );
                        } else {
                            ui.label(RichText::new("For New Objects").weak());
                        }
                    });
                    // Current color; click to open the picker.
                    let (swatch_rect, swatch) = ui.allocate_exact_size(
                        Vec2::new(ui.available_width(), 30.0 * scale),
                        Sense::click(),
                    );
                    ui.painter()
                        .rect_filled(swatch_rect, 4.0, shown_style.color);
                    ui.painter().rect_stroke(
                        swatch_rect,
                        4.0,
                        Stroke::new(1.0, Color32::from_gray(110)),
                        StrokeKind::Middle,
                    );
                    let [r, g, b, _] = shown_style.color.to_srgba_unmultiplied();
                    let luma = 0.299 * f32::from(r) + 0.587 * f32::from(g) + 0.114 * f32::from(b);
                    ui.painter().text(
                        swatch_rect.center(),
                        Align2::CENTER_CENTER,
                        "Color…",
                        FontId::proportional(btn_font),
                        if luma > 140.0 {
                            Color32::BLACK
                        } else {
                            Color32::WHITE
                        },
                    );
                    if swatch.clicked() {
                        editor.color_picker = Some(shown_style.color);
                    }
                    color_picker::show(ctx, editor, canvas, swatch_rect);
                    ui.label("Width");
                    let mut width_val = shown_style.width;
                    let mut size_val = shown_style.font_size;
                    let width_slider =
                        ui.add(Slider::new(&mut width_val, 1.0..=24.0).fixed_decimals(0));
                    ui.label("Text size");
                    let size_slider =
                        ui.add(Slider::new(&mut size_val, 8.0..=120.0).fixed_decimals(0));
                    // Only the property the user moved is applied, so one
                    // slider can't homogenize the other across a selection.
                    if width_slider.changed() || size_slider.changed() {
                        editor.adjust_style(
                            width_slider.changed().then_some(width_val),
                            size_slider.changed().then_some(size_val),
                        );
                    } else if !width_slider.dragged() && !size_slider.dragged() {
                        editor.end_style_adjust();
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

                    // Paired action buttons. `add_sized` allocates the rect
                    // before laying the label out inside it, so a long label
                    // can no longer push its button past its half of the row
                    // the way `min_size` (a floor, not a width) allowed.
                    // `accent` marks the row's default action (the one that
                    // closes the window) with the same blue as an active
                    // tool, so it reads as the recommended button at a
                    // glance instead of blending into Undo/Redo/Reset.
                    let pair = |ui: &mut egui::Ui,
                                a: (&str, bool, bool),
                                b: (&str, bool, bool)|
                     -> (bool, bool) {
                        let mut clicked = (false, false);
                        ui.horizontal(|ui| {
                            for (i, (label, enabled, accent)) in [a, b].into_iter().enumerate() {
                                let mut btn = Button::new(label);
                                if accent {
                                    btn = btn.fill(ACTIVE_TOOL_FILL);
                                }
                                let btn = ui
                                    .add_enabled_ui(enabled, |ui| {
                                        ui.add_sized(Vec2::new(half, btn_h), btn)
                                    })
                                    .inner;
                                if btn.clicked() {
                                    btn.surrender_focus();
                                    if i == 0 {
                                        clicked.0 = true;
                                    } else {
                                        clicked.1 = true;
                                    }
                                }
                            }
                        });
                        clicked
                    };
                    let (undo, redo) = pair(
                        ui,
                        ("Undo Ctrl+Z", editor.doc.can_undo(), false),
                        ("Redo Ctrl+Y", editor.doc.can_redo(), false),
                    );
                    if undo {
                        editor.undo();
                    }
                    if redo {
                        editor.redo();
                    }
                    let (fit, reset) =
                        pair(ui, ("Reset view F", true, false), ("Reset all", true, false));
                    if fit {
                        editor.view.fit(editor.doc.image_size(), canvas.size());
                    }
                    if reset {
                        editor.reset_all();
                    }
                    ui.separator();
                    let (copy, copy_close) =
                        pair(ui, ("Copy", true, false), ("Copy+Close Ctrl+C", true, true));
                    if copy {
                        action = Some(ToolbarAction::Copy { close: false });
                    }
                    if copy_close {
                        action = Some(ToolbarAction::Copy { close: true });
                    }
                    let (save, save_close) =
                        pair(ui, ("Save", true, false), ("Save+Close Ctrl+S", true, true));
                    if save {
                        action = Some(ToolbarAction::Save { close: false });
                    }
                    if save_close {
                        action = Some(ToolbarAction::Save { close: true });
                    }
                    // Spans the popup instead of shrink-wrapping its label.
                    if ui
                        .add_sized(
                            Vec2::new(ui.available_width(), btn_h),
                            Button::new("Close Esc"),
                        )
                        .clicked()
                    {
                        action = Some(ToolbarAction::Close);
                    }
                    ui.separator();

                    // The status line: app-level results/errors win over the
                    // editor's own state message. Only a genuine alert (the
                    // discard confirmation) or an error gets a filled,
                    // colored callout — a plain hint or success message
                    // renders as bare text so it can't be mistaken for a
                    // (non-clickable) button.
                    let status_font = (14.0 * scale).round().max(11.0);
                    let (fill, fg, message) = match &status_override {
                        Some(over) if over.is_error => (
                            Some(Color32::from_rgb(0x7a, 0x1d, 0x1d)),
                            Color32::WHITE,
                            over.message.clone(),
                        ),
                        Some(over) => (None, Color32::from_gray(220), over.message.clone()),
                        None => match editor.status() {
                            (StatusKind::Alert, message) => {
                                (Some(Color32::from_rgb(0xf2, 0xd0, 0x2e)), Color32::BLACK, message)
                            }
                            (StatusKind::Hint, message) => {
                                (None, Color32::from_gray(210), message)
                            }
                        },
                    };
                    // The status line's height only ever grows to fit the
                    // longest message shown so far this session — never
                    // shrinks — so switching between a one-line hint and a
                    // wrapped one can't make the whole panel jump.
                    let pad = 8.0 * scale;
                    let wrap_width = (ui.available_width() - 2.0 * pad).max(10.0);
                    let galley = ctx.fonts_mut(|f| {
                        f.layout(
                            message.clone(),
                            FontId::proportional(status_font),
                            fg,
                            wrap_width,
                        )
                    });
                    self.status_min_h = self.status_min_h.max(galley.size().y + pad * 2.0);
                    egui::Frame::default()
                        .fill(fill.unwrap_or(Color32::TRANSPARENT))
                        .corner_radius(if fill.is_some() { 4.0 } else { 0.0 })
                        .inner_margin(pad)
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            let inner_h = (self.status_min_h - pad * 2.0).max(0.0);
                            ui.set_min_height(inner_h);
                            // Centered vertically in the reserved space, so
                            // a short message doesn't sit pinned to the top
                            // of a box sized for a longer one.
                            let extra = (inner_h - galley.size().y).max(0.0);
                            if extra > 0.0 {
                                ui.add_space(extra / 2.0);
                            }
                            ui.label(RichText::new(message).color(fg).size(status_font));
                        });
                });
            });
        self.size = area.response.rect.size();
        action
    }
}
