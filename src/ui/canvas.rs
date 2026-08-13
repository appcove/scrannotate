//! The canvas: translates egui pointer input into editor calls, decides the
//! cursor, and paints the frame (texture → annotations → previews → region →
//! chrome).

use eframe::egui::{
    self, Align2, Color32, FontId, Key, PointerButton, Pos2, Rect, Sense, TextureHandle, Ui,
    Vec2,
};

use crate::annotate::Tool;
use crate::editor::Editor;
use crate::editor::geometry::{edges_cursor, region_grip_at, region_handle_at};
use crate::editor::state::{EditorState, ItemDragKind, RegionMode};
use crate::ui::paint;

/// Show the canvas for one frame and return its rect.
pub fn show(ui: &mut Ui, editor: &mut Editor, texture: Option<&TextureHandle>) -> Rect {
    let ctx = ui.ctx().clone();
    let (response, painter) = ui.allocate_painter(ui.available_size(), Sense::click_and_drag());
    let canvas = response.rect;

    // Text extent for hit-testing comes from egui's font system.
    let measure = |text: &str, size: f32| -> Vec2 {
        ctx.fonts_mut(|f| {
            f.layout_no_wrap(text.to_owned(), FontId::proportional(size), Color32::WHITE).size()
        })
    };

    // Also refits when the canvas changes size under an untouched view:
    // fullscreen arrives asynchronously on X11 (the WM resizes the window
    // frames after the first paint), so the initial fit can be to the small
    // pre-fullscreen window and would otherwise stick.
    if editor.view.needs_fit(canvas.size()) {
        editor.view.fit(editor.doc.image_size(), canvas.size());
    }

    // Zoom around the pointer: plain scroll, plus Ctrl+scroll / trackpad
    // pinch (which egui routes into zoom_delta, not smooth_scroll_delta).
    if response.hovered() {
        let (scroll, pinch) = ctx.input(|i| (i.smooth_scroll_delta.y, i.zoom_delta()));
        let mut factor = pinch;
        if scroll.abs() > 0.1 {
            factor *= (scroll * 0.003).exp();
        }
        if factor != 1.0
            && let Some(pointer) = response.hover_pos()
        {
            editor.dismiss_transient();
            editor.view.zoom_about(canvas, pointer, factor);
        }
    }

    // Pan with middle drag — a pure view scroll, but egui's dragged_by only
    // checks that the button is down, so gate it off while a primary-button
    // interaction runs (holding middle mid-drag must not pan under it)...
    if response.dragged_by(PointerButton::Middle) && !editor.state.is_pointer_op() {
        editor.view.pan_by(response.drag_delta());
    }
    // ...or with space+drag, which does occupy the state machine.
    let space_down = !editor.state.is_text_editing() && ctx.input(|i| i.key_down(Key::Space));
    if space_down
        && response.drag_started_by(PointerButton::Primary)
        && editor.state.is_idle()
    {
        editor.begin_space_pan();
    }
    if matches!(editor.state, EditorState::Panning)
        && response.dragged_by(PointerButton::Primary)
    {
        editor.view.pan_by(response.drag_delta());
    }

    // Drag starts are hit-tested where the button went down, not where the
    // pointer had travelled to by the time the drag threshold was passed.
    let primary_started = response.drag_started_by(PointerButton::Primary);
    let secondary_started = response.drag_started_by(PointerButton::Secondary);
    if !space_down
        && (primary_started || secondary_started)
        && let Some(pos) = ctx
            .input(|i| i.pointer.press_origin())
            .or_else(|| response.interact_pointer_pos())
    {
        let p = editor.view.to_image(canvas, pos);
        let mods = ctx.input(|i| i.modifiers);
        if secondary_started {
            editor.secondary_drag_start(p);
        } else {
            editor.primary_drag_start(p, pos, canvas, mods, &measure);
        }
    }

    if (response.dragged_by(PointerButton::Primary)
        || response.dragged_by(PointerButton::Secondary))
        && !matches!(editor.state, EditorState::Panning)
        && let Some(pos) = response.interact_pointer_pos()
    {
        editor.pointer_moved(editor.view.to_image(canvas, pos));
    }

    // Any button's release ends the egui drag (drag_stopped_by(Primary)
    // would miss e.g. a middle-button release killing a primary drag).
    if response.drag_stopped() {
        editor.pointer_up(&measure);
    }
    // Safety net: egui also force-ends drags without any release we can see
    // (Esc aborts them unconditionally). A pointer state must never outlive
    // the actual drag, or the next gesture resumes a ghost interaction.
    if editor.state.is_pointer_op() && !response.dragged() && !ctx.input(|i| i.pointer.any_down())
    {
        editor.cancel_pointer_op();
    }

    if response.clicked_by(PointerButton::Primary)
        && !space_down
        && let Some(pos) = response.interact_pointer_pos()
    {
        let p = editor.view.to_image(canvas, pos);
        let mods = ctx.input(|i| i.modifiers);
        editor.click(p, pos, canvas, mods, &measure);
    }

    // Right *click* parks you back on Select, keeping whatever was in
    // flight (set_tool commits an open text edit and ignores mid-drag
    // calls). Right *drag* still rubber-bands a new region.
    if response.clicked_by(PointerButton::Secondary) {
        editor.set_tool(Tool::Select);
    }

    if response.double_clicked_by(PointerButton::Primary)
        && let Some(pos) = response.interact_pointer_pos()
    {
        editor.double_click(editor.view.to_image(canvas, pos), &measure);
    }

    // Item under the pointer (Select tool, idle) — drives the hover outline
    // and the Move cursor.
    let hovered_item = response
        .hover_pos()
        .and_then(|pos| editor.hovered_item(editor.view.to_image(canvas, pos), &measure));

    let sel_screen = editor.doc.region.map(|r| editor.view.rect_to_screen(canvas, r));

    // Cursor: one exhaustive read of the state, then hover fallbacks.
    let cursor = match &editor.state {
        EditorState::ItemsDrag { kind, .. } => match kind {
            ItemDragKind::Move | ItemDragKind::Rotate => egui::CursorIcon::Grabbing,
            ItemDragKind::Resize { edges } => edges_cursor(*edges),
            ItemDragKind::ScaleUniform => egui::CursorIcon::ResizeNwSe,
            ItemDragKind::Endpoint { .. } => egui::CursorIcon::Crosshair,
        },
        EditorState::RegionDraw { .. } => egui::CursorIcon::Crosshair,
        EditorState::RegionAdjust { mode, .. } => match mode {
            RegionMode::Move { .. } => egui::CursorIcon::Grabbing,
            RegionMode::Resize { edges } => edges_cursor(*edges),
        },
        EditorState::DrawingShape { .. } | EditorState::RubberBand { .. } => {
            egui::CursorIcon::Crosshair
        }
        EditorState::Panning => egui::CursorIcon::Grabbing,
        EditorState::Idle | EditorState::TextEditing(_) | EditorState::ConfirmDiscard { .. } => {
            if space_down {
                egui::CursorIcon::Grab
            } else if let Some(pos) = response.hover_pos() {
                if let Some(kind) = editor.handle_under(pos, canvas, &measure) {
                    match kind {
                        ItemDragKind::Resize { edges } => edges_cursor(edges),
                        ItemDragKind::ScaleUniform => egui::CursorIcon::ResizeNwSe,
                        ItemDragKind::Rotate => egui::CursorIcon::Grab,
                        ItemDragKind::Move => egui::CursorIcon::Move,
                        ItemDragKind::Endpoint { .. } => egui::CursorIcon::Crosshair,
                    }
                } else if editor.tool == Tool::Select
                    && sel_screen.is_some_and(|ss| region_grip_at(ss, pos))
                {
                    egui::CursorIcon::Move
                } else if editor.tool == Tool::Select
                    && let Some(edges) = sel_screen.and_then(|ss| region_handle_at(ss, pos))
                {
                    edges_cursor(edges)
                } else if editor.tool == Tool::Select {
                    // A plain drag rubber-band-selects (crosshair); only an
                    // item under the pointer offers a move.
                    if hovered_item.is_some() {
                        egui::CursorIcon::Move
                    } else {
                        egui::CursorIcon::Crosshair
                    }
                } else {
                    egui::CursorIcon::Crosshair
                }
            } else {
                egui::CursorIcon::Default
            }
        }
    };
    ctx.set_cursor_icon(cursor);

    // ---- Painting ----
    let img_screen = editor.view.rect_to_screen(canvas, editor.doc.image_rect());
    if let Some(texture) = texture {
        painter.image(
            texture.id(),
            img_screen,
            Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
            Color32::WHITE,
        );
    }

    // Annotations clipped to the image so strokes don't spill onto the
    // letterbox; the one being text-edited is hidden (the editor shows it).
    let clipped = painter.with_clip_rect(img_screen.intersect(canvas));
    let editing_id = editor.editing_target();
    for (id, ann) in editor.doc.annotations() {
        if editing_id == Some(*id) {
            continue;
        }
        paint::paint_annotation(&clipped, &editor.view, canvas, ann);
    }
    paint::paint_drag_preview(&clipped, editor, canvas);

    // Ghost preview following the cursor while the marker tool is armed.
    if editor.tool == Tool::Marker
        && editor.state.is_idle()
        && !space_down
        && let Some(pos) = response.hover_pos()
    {
        let radius = crate::annotate::marker_radius(&editor.style) * editor.view.zoom;
        painter.circle_filled(pos, radius, editor.style.color.gamma_multiply(0.45));
        painter.text(
            pos,
            Align2::CENTER_CENTER,
            editor.doc.marker_next.to_string(),
            FontId::proportional(editor.style.font_size * editor.view.zoom),
            Color32::WHITE.gamma_multiply(0.6),
        );
    }

    paint::paint_region(&painter, editor, canvas);

    // (hovered_item is None in every non-Idle state, so it can never be the
    // annotation behind the text editor.)
    if let Some(id) = hovered_item
        && !editor.selected.contains(&id)
        && let Some(ann) = editor.doc.get(id)
    {
        paint::paint_hover_outline(&painter, editor, canvas, ann, &measure);
    }

    // Full-screen crosshairs: box-edge extensions while drawing a region, a
    // single pair under the cursor while none exists.
    if matches!(editor.state, EditorState::RegionDraw { .. })
        && let Some(ss) = sel_screen
    {
        paint::paint_crosshairs(&painter, canvas, &[ss.min.x, ss.max.x], &[ss.min.y, ss.max.y]);
    } else if editor.doc.region.is_none()
        && editor.state.is_idle()
        && !space_down
        && let Some(pos) = response.hover_pos()
    {
        paint::paint_crosshairs(&painter, canvas, &[pos.x], &[pos.y]);
    }

    paint::paint_selection_chrome(&painter, editor, canvas, &measure);
    paint::paint_rubber_band(&painter, editor, canvas);

    canvas
}
