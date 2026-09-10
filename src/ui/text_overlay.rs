//! Inline text editing: a frameless TextEdit rendered at the text's spot,
//! with the same font/color the committed annotation will have. Never
//! soft-wraps — lines break only at typed newlines. A grip pinned to the
//! box drags it, so text can be repositioned without committing first.

use eframe::egui::text_selection::CCursorRange;
use eframe::egui::widgets::text_edit::TextEditState;
use eframe::egui::{
    self, Align2, Color32, Context, FontId, Id, Key, Margin, Modifiers, Rect, Sense, TextEdit, Ui,
    Vec2,
};

use crate::editor::Editor;
use crate::editor::state::EditorState;

/// Explicit id for the inline TextEdit, so its cursor can be read back out
/// of egui memory *before* the widget runs this frame.
const INPUT_ID: &str = "text-editor-input";
/// Height of the drag grip, in screen px.
const GRIP_H: f32 = 20.0;
/// Floor for the grip's width: an empty buffer measures near zero, and the
/// grip is the only way to move a box before anything is typed into it.
const GRIP_MIN_W: f32 = 54.0;

/// Something the text editor deliberately declined to handle.
pub enum TextEditAction {
    /// Ctrl+C that the editor has no use for: run the app's copy-and-close,
    /// exactly as if no text edit were open.
    CopyAndClose,
}

pub fn show(ctx: &Context, editor: &mut Editor, canvas: Rect) -> Option<TextEditAction> {
    let view = editor.view;
    // Read before the state borrow: the grip clamps the box to the image.
    let img = editor.doc.image_rect();
    let mut commit = false;
    let mut cancel = false;
    let EditorState::TextEditing(edit) = &mut editor.state else { return None };

    // Ctrl+C reaches us as a synthetic Copy event (see handle_shortcuts).
    // egui's TextEdit silently drops it when there is nothing selected, so
    // that dead case becomes the app's copy-and-close instead. Decided
    // before the widget is added: afterwards the cursor has already moved
    // and the event is spent. Nothing consumes the event here because the
    // early return skips the TextEdit entirely, and events do not survive
    // the frame.
    let caret = TextEditState::load(ctx, Id::new(INPUT_ID)).and_then(|s| s.cursor.char_range());
    if copy_falls_through(&edit.buffer, caret)
        && ctx.input(|i| i.events.iter().any(|e| matches!(e, egui::Event::Copy)))
    {
        return Some(TextEditAction::CopyAndClose);
    }

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
    let grip_w = width.max(GRIP_MIN_W);
    // The grip rides above the box, unless the box sits close enough to the
    // top of the canvas that there is no room for it up there.
    let grip_above = screen_pos.y - GRIP_H >= canvas.min.y;
    let area_pos = if grip_above { screen_pos - Vec2::new(0.0, GRIP_H) } else { screen_pos };
    let mut grip = None;
    egui::Area::new(Id::new("text-editor"))
        .fixed_pos(area_pos)
        .order(egui::Order::Foreground)
        .show(ctx, |ui| {
            // Nothing between grip and box: the TextEdit has to land exactly
            // on screen_pos or it stops lining up with the text it stands in
            // for.
            ui.spacing_mut().item_spacing.y = 0.0;
            if grip_above {
                grip = Some(drag_grip(ui, grip_w));
            }
            // Plain Enter commits — consumed *before* the TextEdit runs, or
            // the widget first inserts a newline at the cursor and that
            // newline lands in the committed text (trim_end only strips
            // trailing ones). Shift+Enter must fall through to the TextEdit
            // as a line break — but consume_key matches modifiers logically,
            // so a bare Modifiers::NONE pattern swallows Shift+Enter too
            // (extra Shift is ignored). Guard on shift so only unmodified
            // Enter commits.
            if ui.input_mut(|i| !i.modifiers.shift && i.consume_key(Modifiers::NONE, Key::Enter)) {
                commit = true;
            }
            let response = ui.add(
                TextEdit::multiline(&mut edit.buffer)
                    .id(Id::new(INPUT_ID))
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
            if !grip_above {
                grip = Some(drag_grip(ui, grip_w));
            }
            if ui.input(|i| i.key_pressed(Key::Escape)) {
                cancel = true;
            }
        });
    // Outside the closure: a drag reports screen px, but `pos` is in image
    // coords, so the delta has to come back through the zoom.
    if let Some(grip) = grip {
        if grip.dragged() {
            edit.pos = (edit.pos + grip.drag_delta() / view.zoom).clamp(img.min, img.max);
        }
        // Pressing anywhere outside the TextEdit clears egui's focus, which
        // would leave the keyboard nowhere. Hand it back when the drag ends.
        if grip.drag_stopped() {
            ctx.memory_mut(|m| m.request_focus(Id::new(INPUT_ID)));
        }
    }
    if cancel {
        editor.cancel_text();
    } else if commit {
        editor.commit_text();
    }
    None
}

/// The box's drag handle: a dotted bar the width of the editor. Drawn with
/// its own fill rather than the egui widget visuals because it sits over
/// captured pixels, which can be any color at all.
fn drag_grip(ui: &mut Ui, width: f32) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(width, GRIP_H), Sense::drag());
    let hot = resp.hovered() || resp.dragged();
    ui.painter().rect_filled(
        rect,
        4.0,
        if hot { Color32::from_black_alpha(200) } else { Color32::from_black_alpha(140) },
    );
    ui.painter().text(
        rect.center(),
        Align2::CENTER_CENTER,
        "• • •",
        FontId::proportional(13.0),
        if hot { Color32::WHITE } else { Color32::from_gray(205) },
    );
    if resp.dragged() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
    } else if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
    }
    resp
}

/// Does Ctrl+C belong to the app rather than to the text editor? Only when
/// the editor would do nothing with it: nothing selected, caret at the end.
/// Anywhere else it stays a text operation — mid-buffer it is the caret the
/// user would grow a selection from, so stealing it would surprise.
///
/// A just-opened editor has no stored cursor yet; its buffer is empty, so
/// the caret is trivially at the end.
fn copy_falls_through(buffer: &str, caret: Option<CCursorRange>) -> bool {
    caret.is_none_or(|r| r.is_empty() && usize::from(r.primary.index) >= buffer.chars().count())
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::text::CCursor;

    fn caret_at(index: usize) -> Option<CCursorRange> {
        Some(CCursorRange::one(CCursor::new(index)))
    }

    #[test]
    fn copy_falls_through_only_at_the_end_without_a_selection() {
        assert!(copy_falls_through("hello", caret_at(5)));
        assert!(copy_falls_through("", caret_at(0)));
        // Mid-buffer: the editor's own caret, leave it alone.
        assert!(!copy_falls_through("hello", caret_at(4)));
        assert!(!copy_falls_through("hello", caret_at(0)));
        // A selection is always the editor's, even one ending at the end.
        assert!(!copy_falls_through(
            "hello",
            Some(CCursorRange::two(CCursor::new(0), CCursor::new(5)))
        ));
    }

    #[test]
    fn a_fresh_editor_has_no_cursor_yet() {
        assert!(copy_falls_through("", None));
    }

    #[test]
    fn the_end_is_counted_in_chars_not_bytes() {
        // 5 chars, 10 bytes: a byte-indexed comparison would never match.
        assert!(copy_falls_through("héllö", caret_at(5)));
        assert!(!copy_falls_through("héllö", caret_at(4)));
    }
}
