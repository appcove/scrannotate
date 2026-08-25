//! The always-visible toolbar: tools two per row, the style controls, the
//! action buttons, and — at its foot — the status line explaining what the
//! current editor state affords (or demanding attention, e.g. the discard
//! confirmation). Auto-placed beside the region, draggable by its grip.

use eframe::egui::{
    self, Align2, Button, Color32, Context, CornerRadius, FontId, Id, Pos2, Rect, RichText, Sense,
    Slider, Stroke, StrokeKind, Vec2,
};

use crate::annotate::Tool;
use crate::editor::{Editor, StatusKind};
use crate::ui::{ACCENT, ACTIVE_TOOL_FILL, TOOLS, color_picker};

/// The chord key as the user sees it: ⌘ on macOS, Ctrl elsewhere. (The actual
/// binding accepts the platform key either way — this is only the label.)
#[cfg(target_os = "macos")]
const MOD: &str = "Cmd";
#[cfg(not(target_os = "macos"))]
const MOD: &str = "Ctrl";

/// Fill for the destructive Close button (native-macOS-style red).
const CLOSE_RED: Color32 = Color32::from_rgb(0xd9, 0x3f, 0x3a);

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
}

impl Toolbar {
    pub fn new() -> Self {
        Self { pos: None, size: Vec2::new(300.0, 700.0) }
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
        let Some(ss) = editor.doc.region.map(|r| editor.view.rect_to_screen(canvas, r)) else {
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
        let area = egui::Area::new(Id::new("toolbar"))
            .fixed_pos(pos)
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.set_width(270.0);
                    // Chunky, easy-to-hit controls that stand out from the
                    // popup background.
                    let spacing = ui.spacing_mut();
                    spacing.slider_width = 230.0;
                    spacing.button_padding = Vec2::new(10.0, 8.0);
                    spacing.item_spacing = Vec2::new(8.0, 7.0);
                    spacing.interact_size = Vec2::new(40.0, 34.0);
                    let visuals = ui.visuals_mut();
                    visuals.widgets.inactive.weak_bg_fill = Color32::from_gray(58);
                    visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, Color32::from_gray(75));
                    visuals.widgets.inactive.fg_stroke = Stroke::new(1.0, Color32::from_gray(235));
                    visuals.widgets.hovered.weak_bg_fill = Color32::from_gray(80);
                    visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, ACCENT);
                    visuals.widgets.hovered.fg_stroke = Stroke::new(1.5, Color32::WHITE);
                    visuals.widgets.active.weak_bg_fill = Color32::from_gray(96);
                    // Rounded, modern-looking buttons instead of egui's default sharp corners.
                    for style in [
                        &mut visuals.widgets.inactive,
                        &mut visuals.widgets.hovered,
                        &mut visuals.widgets.active,
                        &mut visuals.widgets.open,
                    ] {
                        style.corner_radius = CornerRadius::same(7);
                    }
                    let styles = &mut ui.style_mut().text_styles;
                    if let Some(font) = styles.get_mut(&egui::TextStyle::Button) {
                        font.size = 15.0;
                    }
                    if let Some(font) = styles.get_mut(&egui::TextStyle::Body) {
                        font.size = 15.0;
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
                        self.pos = Some(pos + grip.drag_delta());
                    }
                    ui.separator();

                    // Tools, two per row.
                    let gap = ui.spacing().item_spacing.x;
                    let half = (ui.available_width() - gap) / 2.0;
                    for pair in TOOLS.chunks(2) {
                        ui.horizontal(|ui| {
                            for (tool, key) in pair {
                                let active = editor.tool == *tool;
                                let text =
                                    RichText::new(format!("{key:?} · {}", tool.label()))
                                        .size(15.0)
                                        .color(if active {
                                            Color32::WHITE
                                        } else {
                                            Color32::from_gray(235)
                                        });
                                let mut btn =
                                    Button::new(text).min_size(Vec2::new(half, 36.0));
                                if active {
                                    btn = btn.fill(ACTIVE_TOOL_FILL);
                                }
                                let resp = ui.add(btn);
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
                    let (swatch_rect, swatch) = ui
                        .allocate_exact_size(Vec2::new(ui.available_width(), 30.0), Sense::click());
                    ui.painter().rect_filled(swatch_rect, 4.0, shown_style.color);
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
                        FontId::proportional(15.0),
                        if luma > 140.0 { Color32::BLACK } else { Color32::WHITE },
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

                    // Paired action buttons. `accent` fills the button so
                    // the two primary "ship" actions (Copy+Close/Save+Close)
                    // read as the default action rather than equal-weight
                    // siblings of Copy/Save.
                    // Each button is forced to exactly `half` wide (via
                    // add_sized) so the two columns line up no matter how long
                    // the labels are. `accent` fills the primary "ship" action.
                    let pair = |ui: &mut egui::Ui,
                                    a: (&str, bool, bool),
                                    b: (&str, bool, bool)|
                     -> (bool, bool) {
                        let mut clicked = (false, false);
                        ui.horizontal(|ui| {
                            for (i, (label, enabled, accent)) in [a, b].into_iter().enumerate() {
                                let mut btn = Button::new(label);
                                if accent {
                                    btn = btn.fill(ACCENT);
                                }
                                let resp = ui
                                    .add_enabled_ui(enabled, |ui| {
                                        ui.add_sized(Vec2::new(half, 34.0), btn)
                                    })
                                    .inner;
                                if resp.clicked() {
                                    resp.surrender_focus();
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
                        (&format!("Undo  {MOD}+Z"), editor.doc.can_undo(), false),
                        (&format!("Redo  {MOD}+Y"), editor.doc.can_redo(), false),
                    );
                    if undo {
                        editor.undo();
                    }
                    if redo {
                        editor.redo();
                    }
                    let (fit, reset) =
                        pair(ui, ("Reset view  F", true, false), ("Reset all", true, false));
                    if fit {
                        editor.view.fit(editor.doc.image_size(), canvas.size());
                    }
                    if reset {
                        editor.reset_all();
                    }
                    ui.separator();
                    let (copy, copy_close) = pair(
                        ui,
                        ("Copy", true, false),
                        (&format!("Copy+Close  {MOD}+C"), true, true),
                    );
                    if copy {
                        action = Some(ToolbarAction::Copy { close: false });
                    }
                    if copy_close {
                        action = Some(ToolbarAction::Copy { close: true });
                    }
                    let (save, save_close) = pair(
                        ui,
                        ("Save", true, false),
                        (&format!("Save+Close  {MOD}+S"), true, true),
                    );
                    if save {
                        action = Some(ToolbarAction::Save { close: false });
                    }
                    if save_close {
                        action = Some(ToolbarAction::Save { close: true });
                    }
                    // Full-width, destructive-red Close.
                    let close = Button::new(
                        RichText::new("Close  Esc")
                            .color(Color32::WHITE)
                            .strong()
                    )
                    .fill(CLOSE_RED)
                    .min_size(Vec2::new(ui.available_width(), 36.0))
                    ;

                    if ui.add_sized(
                        [ui.available_width(), 36.0],
                        close
                    ).clicked() {
                        action = Some(ToolbarAction::Close);
                    }
                    ui.separator();

                    // The status line: app-level results/errors win over the
                    // editor's own state message.
                    let (bg, fg, message) = match &status_override {
                        Some(over) if over.is_error => (
                            Color32::from_rgb(0x7a, 0x1d, 0x1d),
                            Color32::WHITE,
                            over.message.clone(),
                        ),
                        Some(over) => {
                            (Color32::from_gray(60), Color32::WHITE, over.message.clone())
                        }
                        None => match editor.status() {
                            (StatusKind::Alert, message) => {
                                (Color32::from_rgb(0xf2, 0xd0, 0x2e), Color32::BLACK, message)
                            }
                            (StatusKind::Hint, message) => {
                                (Color32::from_gray(45), Color32::from_gray(200), message)
                            }
                        },
                    };
                    egui::Frame::default().fill(bg).corner_radius(4.0).inner_margin(8.0).show(
                        ui,
                        |ui| {
                            ui.set_width(ui.available_width());
                            ui.label(RichText::new(message).color(fg).size(13.0));
                        },
                    );
                });
            });
        self.size = area.response.rect.size();
        action
    }
}
