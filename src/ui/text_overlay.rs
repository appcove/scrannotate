//! Inline text editing: a frameless TextEdit rendered at the text's spot,
//! with the same font/color the committed annotation will have. Never
//! soft-wraps — lines break only at typed newlines.

use eframe::egui::{
    self, Color32, Context, FontId, Id, Key, Margin, Modifiers, Rect, TextEdit, Ui,
};

use crate::editor::Editor;
use crate::editor::state::EditorState;

pub fn show(ctx: &Context, editor: &mut Editor, canvas: Rect) {
    let view = editor.view;
    let mut commit = false;
    let mut cancel = false;
    let EditorState::TextEditing(edit) = &mut editor.state else { return };
    let screen_pos = view.to_screen(canvas, edit.pos);
    let font = FontId::proportional((edit.style.font_size * view.zoom).max(9.0));
    // Size the editor to its content so clicks next to the text still reach
    // the canvas (and commit); the margin absorbs the one-frame lag of the
    // measurement.
    let width = ctx
        .fonts_mut(|f| f.layout_no_wrap(edit.buffer.clone(), font.clone(), Color32::WHITE))
        .size()
        .x
        + font.size * 2.0;
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
            // Plain Enter commits — consumed *before* the TextEdit runs, or
            // the widget first inserts a newline at the cursor and that
            // newline lands in the committed text (trim_end only strips
            // trailing ones). Shift+Enter passes through as a line break.
            if ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Enter)) {
                commit = true;
            }
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
            if ui.input(|i| i.key_pressed(Key::Escape)) {
                cancel = true;
            }
        });
    if cancel {
        editor.cancel_text();
    } else if commit {
        editor.commit_text();
    }
}
