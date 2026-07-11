//! The custom color picker popup, anchored to the toolbar's swatch. Commits
//! through `Editor::apply_color`, so the text being edited or the selection
//! is recolored undoably.

use eframe::egui::{
    self, Button, Color32, Context, Id, Rect, RichText, Sense, Stroke, StrokeKind, Vec2,
};

use crate::editor::Editor;

pub fn show(ctx: &Context, editor: &mut Editor, canvas: Rect, anchor: Rect) {
    let Some(mut working) = editor.color_picker else { return };
    let mut commit = false;
    let mut cancel = false;
    let recents = editor.palette.clone();
    egui::Area::new(Id::new("custom-color-picker"))
        .fixed_pos(anchor.right_top() + Vec2::new(16.0, -160.0))
        .order(egui::Order::Foreground)
        .constrain_to(canvas)
        .show(ctx, |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                // Pin the popup width and let every section fill it edge to
                // edge.
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
                let (sel_rect, _) =
                    ui.allocate_exact_size(Vec2::new(ui.available_width(), 40.0), Sense::hover());
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
                    let sw = ((ui.available_width() - spacing * (n - 1.0)) / n).max(24.0);
                    for &recent in &recents {
                        let (rect, resp) =
                            ui.allocate_exact_size(Vec2::new(sw, 46.0), Sense::click());
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
                    let half = (ui.available_width() - ui.spacing().item_spacing.x) / 2.0;
                    let ok = ui.add(
                        Button::new(RichText::new("OK").size(18.0)).min_size(Vec2::new(half, 46.0)),
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
        editor.apply_color(working);
        editor.color_picker = None;
    } else if cancel {
        editor.color_picker = None;
    } else {
        editor.color_picker = Some(working);
    }
}
