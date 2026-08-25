//! The interaction controller: owns the [`Document`], the current
//! [`EditorState`], the tool, and the (multi-)selection, and turns pointer
//! and key input into state transitions and document transactions.
//!
//! Everything here is egui-free apart from math types — text extent comes in
//! through a [`Measure`] closure — so transitions are unit-testable.

pub mod geometry;
pub mod hit;
pub mod state;

use std::collections::BTreeSet;

use eframe::egui::{Color32, Modifiers, Pos2, Rect, Vec2};

use crate::annotate::{Annotation, Shape, Style, Tool, marker_target};
use crate::document::{AnnotationId, Document};
use crate::editor::geometry::{
    HANDLE_HIT, clamp_rect_within, resize_rect, rotate_around, translate_shape,
};
use crate::editor::hit::{Measure, annotation_bbox, handle_at, item_handles, topmost_hit};
use crate::editor::state::{EditorState, ItemDragKind, RegionMode, TextEditState};
use crate::view::View;

/// What Esc did; `Quit` means the second Esc of the discard confirmation
/// landed — the app should close.
#[derive(PartialEq, Eq, Debug)]
pub enum EscapeOutcome {
    Consumed,
    Quit,
}

/// How the status line should be presented.
#[derive(PartialEq, Eq, Debug, Clone, Copy)]
pub enum StatusKind {
    /// Contextual explanation of what the current state affords.
    Hint,
    /// Needs attention now (e.g. the discard confirmation).
    Alert,
}

pub struct Editor {
    pub doc: Document,
    pub state: EditorState,
    pub tool: Tool,
    /// The multi-selection (Select tool). Ids, not indices: stale entries
    /// after undo/delete resolve to nothing instead of the wrong object.
    pub selected: BTreeSet<AnnotationId>,
    /// Style new annotations get.
    pub style: Style,
    /// Most-recently-used colors, newest first; the head is the active color.
    pub palette: Vec<Color32>,
    /// Working color of the open custom-color picker.
    pub color_picker: Option<Color32>,
    pub view: View,
    /// Width/size were deliberately set (this or a prior session), so they
    /// are a preference worth persisting — not just resolution defaults.
    pub persist_style: bool,
    /// One undo step per continuous slider adjustment of the selection.
    style_adjusting: bool,
    /// A pan happened while space was held, so releasing space is the end
    /// of a pan gesture, not a tap-to-Select.
    space_panned: bool,
}

impl Editor {
    pub fn new(doc: Document, style: Style, palette: Vec<Color32>, persist_style: bool) -> Self {
        Self {
            doc,
            state: EditorState::Idle,
            tool: Tool::Select,
            selected: BTreeSet::new(),
            style,
            palette,
            color_picker: None,
            view: View::new(),
            persist_style,
            style_adjusting: false,
            space_panned: false,
        }
    }

    /// The selected annotation when exactly one is selected (handles and
    /// endpoint drags only exist for single selection).
    pub fn single_selected(&self) -> Option<AnnotationId> {
        if self.selected.len() == 1 { self.selected.iter().next().copied() } else { None }
    }

    /// The annotation hidden behind the inline text editor, if any — it is
    /// painted by the editor overlay instead of the canvas.
    pub fn editing_target(&self) -> Option<AnnotationId> {
        match &self.state {
            EditorState::TextEditing(edit) => edit.target,
            _ => None,
        }
    }

    /// Advance time-driven state: the discard confirmation expires on its
    /// own — it is a state, not a painted-over toast.
    pub fn tick(&mut self, now: f64) {
        if let EditorState::ConfirmDiscard { until } = self.state
            && now >= until
        {
            self.state = EditorState::Idle;
        }
    }

    /// Any user action other than Esc leaves the discard confirmation.
    pub fn dismiss_transient(&mut self) {
        if self.state.is_confirm_discard() {
            self.state = EditorState::Idle;
        }
    }

    /// The status line: what the current state means / affords right now.
    pub fn status(&self) -> (StatusKind, String) {
        let hint = |s: &str| (StatusKind::Hint, s.to_owned());
        match &self.state {
            EditorState::ConfirmDiscard { .. } => {
                (StatusKind::Alert, "Press Esc again to discard and close".to_owned())
            }
            EditorState::TextEditing(_) => {
                hint("Type your text.  Enter to finish · Shift+Enter for a new line · Esc to cancel")
            }
            EditorState::DrawingShape { .. } => hint("Release to place it · Esc to cancel"),
            EditorState::RegionDraw { .. } => hint("Release to set the area · Esc to cancel"),
            EditorState::RegionAdjust { .. } => hint("Release to keep · Esc to undo"),
            EditorState::ItemsDrag { .. } => hint("Release to keep · Esc to undo"),
            EditorState::RubberBand { .. } => hint("Release to select what's inside"),
            EditorState::Panning => hint("Panning… release to stop"),
            EditorState::Idle => {
                if self.doc.region.is_none() {
                    return hint("Drag to select an area · Enter to copy the whole screen");
                }
                match self.tool {
                    Tool::Select if !self.selected.is_empty() => (
                        StatusKind::Hint,
                        format!(
                            "{} selected · Drag to move · Del to delete · Esc to deselect",
                            self.selected.len()
                        ),
                    ),
                    Tool::Select => {
                        hint("Click an item to select it · Drag to select several · Right-drag for a new area")
                    }
                    Tool::Text => hint("Click to add text · Click existing text to edit it"),
                    Tool::Marker => (
                        StatusKind::Hint,
                        format!(
                            "Click to drop marker {} · Drag it to pull out an arrow",
                            self.doc.marker_next
                        ),
                    ),
                    tool => (
                        StatusKind::Hint,
                        format!("Drag to draw a {} · Press Space to select", tool.label()),
                    ),
                }
            }
        }
    }

    /// MRU bookkeeping: the color in use moves to the head of the palette,
    /// so the head is always the active color.
    pub fn promote_color(&mut self, color: Color32) {
        self.palette.retain(|c| *c != color);
        self.palette.insert(0, color);
        self.palette.truncate(6);
    }

    /// Pick a color for exactly one scope: the text being edited, the
    /// selected objects (undoably), or — only when neither exists — the
    /// default for new objects. Restyling an existing object never changes
    /// what the next one will look like.
    pub fn apply_color(&mut self, color: Color32) {
        self.dismiss_transient();
        self.promote_color(color);
        if let EditorState::TextEditing(edit) = &mut self.state {
            edit.style.color = color;
            return;
        }
        if self.tool == Tool::Select && !self.selected.is_empty() {
            let changes = self
                .selected
                .iter()
                .any(|id| self.doc.get(*id).is_some_and(|a| a.style.color != color));
            if changes {
                self.doc.begin();
                for id in self.selected.iter() {
                    if let Some(ann) = self.doc.get_mut(*id) {
                        ann.style.color = color;
                    }
                }
                self.doc.commit();
            }
            return;
        }
        self.style.color = color;
    }

    /// The style the settings panel shows: the text being edited, the first
    /// selected object (z-order), or the defaults for new objects.
    pub fn shown_style(&self) -> Style {
        if let EditorState::TextEditing(edit) = &self.state {
            return edit.style;
        }
        if self.tool == Tool::Select
            && let Some((_, ann)) =
                self.doc.annotations().iter().find(|(id, _)| self.selected.contains(id))
        {
            return ann.style;
        }
        self.style
    }

    /// Slider adjustment — only the property the user actually moved is
    /// passed as `Some`, so nudging Width can't homogenize every selected
    /// object's font size (and vice versa). Targets exactly one scope: the
    /// text being edited, the selection (one undo step per continuous
    /// gesture), or — only when neither exists — the defaults for new
    /// objects. Restyling never bleeds into what the next object gets.
    pub fn adjust_style(&mut self, width: Option<f32>, font_size: Option<f32>) {
        // A stray slider event mid-drag (arrow key on a focused slider) must
        // not open a transaction inside the drag's transaction.
        if self.state.is_pointer_op() {
            return;
        }
        self.dismiss_transient();
        let apply = |style: &mut Style| {
            if let Some(width) = width {
                style.width = width;
            }
            if let Some(font_size) = font_size {
                style.font_size = font_size;
            }
        };
        if let EditorState::TextEditing(edit) = &mut self.state {
            // Lands with the text's commit (one undo step).
            apply(&mut edit.style);
            return;
        }
        if self.tool == Tool::Select && !self.selected.is_empty() {
            if !self.style_adjusting {
                self.doc.begin();
                self.style_adjusting = true;
            }
            for id in self.selected.iter() {
                if let Some(ann) = self.doc.get_mut(*id) {
                    apply(&mut ann.style);
                }
            }
            return;
        }
        // Nothing selected: a deliberate default, worth persisting.
        apply(&mut self.style);
        self.persist_style = true;
    }

    /// Ends the current continuous slider gesture (commits its undo step).
    pub fn end_style_adjust(&mut self) {
        if self.style_adjusting {
            self.doc.commit();
            self.style_adjusting = false;
        }
    }

    /// Explicit tool invocation (key, space tap, or button): commits any
    /// text edit and always drops the selection, even when the tool is
    /// unchanged — re-invoking Select is how you get back to a clean slate.
    /// Ignored mid-drag so a stray key can't corrupt an interaction.
    pub fn set_tool(&mut self, tool: Tool) {
        if self.state.is_pointer_op() {
            return;
        }
        self.dismiss_transient();
        if self.state.is_text_editing() {
            self.commit_text();
        }
        self.selected.clear();
        self.tool = tool;
    }

    /// Back to the initial region-selection state: annotations, region, and
    /// all in-flight interaction cleared, view refitted. One undo step.
    pub fn reset_all(&mut self) {
        self.cancel_pointer_op();
        self.dismiss_transient();
        if self.state.is_text_editing() {
            self.state = EditorState::Idle; // cancel, don't commit
        }
        self.doc.begin();
        self.doc.clear_annotations();
        self.doc.marker_next = 1;
        self.doc.region = None;
        self.doc.commit();
        self.selected.clear();
        self.color_picker = None;
        self.tool = Tool::Select;
        self.view.fitted = false; // refit next frame
    }

    // ------------------------------------------------------------------
    // History

    pub fn undo(&mut self) {
        // Mid-drag undo cancels the drag itself — like the old push-at-drag-
        // start behavior, it must not reach back and undo the previous edit.
        if self.state.is_pointer_op() {
            self.cancel_pointer_op();
            return;
        }
        // A pending text edit lands first (to its own target), so nothing
        // can write through changed history afterwards.
        if self.state.is_text_editing() {
            self.commit_text();
        }
        self.dismiss_transient();
        // An in-flight slider gesture ends here so it is what gets undone;
        // clearing the flag lets the continuing gesture open a fresh
        // transaction instead of mutating outside history.
        self.end_style_adjust();
        self.doc.undo();
        self.prune_selection();
    }

    pub fn redo(&mut self) {
        if self.state.is_pointer_op() {
            self.cancel_pointer_op();
            return;
        }
        if self.state.is_text_editing() {
            self.commit_text();
        }
        self.dismiss_transient();
        self.end_style_adjust();
        self.doc.redo();
        self.prune_selection();
    }

    /// Cancel any pointer interaction, reverting document changes. Also the
    /// safety net for drags egui force-ends (Esc, other-button release)
    /// without a release our drag-stopped check would see.
    pub fn cancel_pointer_op(&mut self) {
        if !self.state.is_pointer_op() {
            return;
        }
        match std::mem::replace(&mut self.state, EditorState::Idle) {
            EditorState::RegionDraw { .. }
            | EditorState::RegionAdjust { .. }
            | EditorState::ItemsDrag { .. } => self.doc.rollback(),
            _ => {}
        }
    }

    fn prune_selection(&mut self) {
        self.selected.retain(|id| self.doc.contains(*id));
    }

    // ------------------------------------------------------------------
    // Escape / cancel

    /// The Esc ladder. Each state defines its own cancel; at the root, the
    /// first Esc enters [`EditorState::ConfirmDiscard`] and a second one
    /// inside its window quits.
    pub fn escape(&mut self, now: f64) -> EscapeOutcome {
        // egui unconditionally aborts any in-flight pointer drag on Esc, so
        // our state must follow suit before anything else is considered —
        // otherwise a rung above (the color picker) would consume the Esc
        // and leave a ghost drag with an open transaction.
        if self.state.is_pointer_op() {
            let was_pan = matches!(self.state, EditorState::Panning);
            self.cancel_pointer_op();
            // A pan is a view gesture, not a cancellable edit: fall through
            // so Esc still does what it would have done without the pan.
            if !was_pan {
                return EscapeOutcome::Consumed;
            }
        }
        if self.color_picker.take().is_some() {
            return EscapeOutcome::Consumed;
        }
        if self.state.is_text_editing() {
            self.cancel_text();
            return EscapeOutcome::Consumed;
        }
        // One rung: back to a clean Select slate.
        if !self.selected.is_empty() || self.tool != Tool::Select {
            self.selected.clear();
            self.tool = Tool::Select;
            return EscapeOutcome::Consumed;
        }
        // Root: arm the discard confirmation, or honor it.
        match self.state {
            EditorState::ConfirmDiscard { until } if now < until => EscapeOutcome::Quit,
            _ => {
                self.state = EditorState::ConfirmDiscard { until: now + 1.0 };
                EscapeOutcome::Consumed
            }
        }
    }

    // ------------------------------------------------------------------
    // Selection

    pub fn select_all(&mut self) {
        if self.state.is_pointer_op() {
            return;
        }
        if self.state.is_text_editing() {
            self.commit_text();
        }
        self.dismiss_transient();
        self.tool = Tool::Select;
        self.selected = self.doc.ids().collect();
    }

    /// Delete the whole selection as one undo step.
    pub fn delete_selected(&mut self) {
        self.dismiss_transient();
        if self.tool != Tool::Select || !self.state.is_idle() || self.selected.is_empty() {
            return;
        }
        self.doc.begin();
        for id in std::mem::take(&mut self.selected) {
            self.doc.remove(id);
        }
        self.doc.commit();
    }

    /// Item under the pointer while idle with the Select tool — drives the
    /// hover outline and the Move cursor.
    pub fn hovered_item(&self, p: Pos2, measure: Measure) -> Option<AnnotationId> {
        if !self.state.is_idle() || self.tool != Tool::Select {
            return None;
        }
        topmost_hit(&self.doc, p, false, self.view.zoom, measure)
    }

    /// The single selected item's handle under the pointer, if any. Handles
    /// only exist for the Select tool — other tools paint none.
    pub fn handle_under(
        &self,
        screen: Pos2,
        canvas: Rect,
        measure: Measure,
    ) -> Option<ItemDragKind> {
        if self.tool != Tool::Select {
            return None;
        }
        let id = self.single_selected()?;
        let ann = self.doc.get(id)?;
        handle_at(&item_handles(ann, &self.view, canvas, measure), screen)
    }

    // ------------------------------------------------------------------
    // Pointer input

    /// Primary-button drag start. `p` is image coords, `screen` the raw
    /// pointer position (handles have screen-space hit areas).
    pub fn primary_drag_start(
        &mut self,
        p: Pos2,
        screen: Pos2,
        canvas: Rect,
        mods: Modifiers,
        measure: Measure,
    ) {
        if self.state.is_text_editing() {
            self.commit_text();
            return;
        }
        self.dismiss_transient();
        if !self.state.is_idle() {
            return;
        }
        let img_rect = self.doc.image_rect();
        let clamped = p.clamp(img_rect.min, img_rect.max);

        // Nothing to annotate yet: every tool's first drag draws the
        // region instead (the toolbar itself only appears once one exists,
        // so any other tool's gesture here would be invisible).
        if self.doc.region.is_none() {
            self.begin_region_draw(clamped);
            return;
        }

        if self.tool == Tool::Select {
            // 1. Handles of the single selected item.
            if let Some(kind) = self.handle_under(screen, canvas, measure)
                && let Some(id) = self.single_selected()
                && let Some(ann) = self.doc.get(id).cloned()
            {
                let bbox = annotation_bbox(&ann, measure);
                self.doc.begin();
                self.state = EditorState::ItemsDrag {
                    items: vec![(id, ann)],
                    kind,
                    bbox0: bbox,
                    center: bbox.center(),
                    grab: p,
                };
                return;
            }
            // 2. Region move grip, then resize handles.
            if let Some(region) = self.doc.region {
                let ss = self.view.rect_to_screen(canvas, region);
                if geometry::region_grip_at(ss, screen) {
                    self.doc.begin();
                    self.state = EditorState::RegionAdjust {
                        start_rect: region,
                        mode: RegionMode::Move { grab: p },
                    };
                    return;
                }
                if let Some(edges) = geometry::region_handle_at(ss, screen) {
                    self.doc.begin();
                    self.state = EditorState::RegionAdjust {
                        start_rect: region,
                        mode: RegionMode::Resize { edges },
                    };
                    return;
                }
            }
            // 3. Shift+drag adds to the selection; the plain drag in rung 5
            //    replaces it. Both rubber-band.
            if mods.shift {
                self.state = EditorState::RubberBand { anchor: p, current: p, additive: true };
                return;
            }
            // 4. Drag an item: the whole selection moves together.
            if let Some(id) = topmost_hit(&self.doc, p, false, self.view.zoom, measure) {
                if mods.command {
                    self.selected.insert(id);
                } else if !self.selected.contains(&id) {
                    self.selected.clear();
                    self.selected.insert(id);
                }
                let items: Vec<(AnnotationId, Annotation)> = self
                    .doc
                    .annotations()
                    .iter()
                    .filter(|(i, _)| self.selected.contains(i))
                    .map(|(i, a)| (*i, a.clone()))
                    .collect();
                let bbox = items
                    .iter()
                    .map(|(_, a)| annotation_bbox(a, measure))
                    .reduce(|a, b| a.union(b))
                    .unwrap_or(Rect::ZERO);
                self.doc.begin();
                self.state = EditorState::ItemsDrag {
                    items,
                    kind: ItemDragKind::Move,
                    bbox0: bbox,
                    center: bbox.center(),
                    grab: p,
                };
                return;
            }
            // 5. Empty space: a plain drag rubber-band-selects, replacing
            //    the selection. Move the region with its grip (rung 2),
            //    redraw it with a right-drag.
            self.state = EditorState::RubberBand { anchor: p, current: p, additive: false };
            return;
        }

        // Drawing tools. Text places on click, not drag.
        if self.tool != Tool::Text {
            self.state = EditorState::DrawingShape { start: p, current: p, points: vec![p] };
        }
    }

    /// Right-drag always rubber-bands a fresh region, whatever the tool.
    pub fn secondary_drag_start(&mut self, p: Pos2) {
        if self.state.is_text_editing() {
            self.commit_text();
            return;
        }
        self.dismiss_transient();
        if !self.state.is_idle() {
            return;
        }
        let img_rect = self.doc.image_rect();
        self.begin_region_draw(p.clamp(img_rect.min, img_rect.max));
    }

    fn begin_region_draw(&mut self, anchor: Pos2) {
        // Transactional: Esc rolls the old region back, and the change is
        // one undo step.
        self.doc.begin();
        self.doc.region = None;
        self.state = EditorState::RegionDraw { anchor };
    }

    pub fn pointer_moved(&mut self, p: Pos2) {
        let img_rect = self.doc.image_rect();
        let clamped = p.clamp(img_rect.min, img_rect.max);
        match &mut self.state {
            EditorState::DrawingShape { current, points, .. } => {
                *current = p;
                let threshold = (0.75 / self.view.zoom).max(0.25);
                if self.tool == Tool::Pen
                    && points.last().is_none_or(|last| last.distance(p) >= threshold)
                {
                    points.push(p);
                }
            }
            EditorState::RegionDraw { anchor } => {
                let anchor = *anchor;
                self.doc.region = Some(Rect::from_two_pos(anchor, clamped));
            }
            EditorState::RegionAdjust { start_rect, mode } => {
                let (start_rect, mode) = (*start_rect, *mode);
                self.doc.region = Some(match mode {
                    RegionMode::Move { grab } => {
                        clamp_rect_within(start_rect.translate(p - grab), img_rect)
                    }
                    RegionMode::Resize { edges } => resize_rect(start_rect, edges, clamped),
                });
            }
            EditorState::ItemsDrag { items, kind, bbox0, center, grab } => {
                let (kind, bbox0, center, grab) = (*kind, *bbox0, *center, *grab);
                for (id, start) in items.iter() {
                    let ann = dragged_item(start, kind, bbox0, center, grab, p);
                    if let Some(slot) = self.doc.get_mut(*id) {
                        *slot = ann;
                    }
                }
            }
            EditorState::RubberBand { current, .. } => *current = p,
            EditorState::Idle
            | EditorState::Panning
            | EditorState::TextEditing(_)
            | EditorState::ConfirmDiscard { .. } => {}
        }
    }

    pub fn pointer_up(&mut self, measure: Measure) {
        match std::mem::replace(&mut self.state, EditorState::Idle) {
            EditorState::DrawingShape { start, current, points } => {
                self.finish_drawing(start, current, points);
            }
            EditorState::RegionDraw { .. } => {
                // A sub-4-px accident is discarded — by rolling back, so the
                // region it would have replaced comes back too.
                if let Some(r) = self.doc.region
                    && (r.width() < 4.0 || r.height() < 4.0)
                {
                    self.doc.rollback();
                } else {
                    self.doc.commit();
                }
            }
            // No-op drags are dropped by commit itself — a grab that never
            // moved doesn't burn an undo slot.
            EditorState::RegionAdjust { .. } | EditorState::ItemsDrag { .. } => self.doc.commit(),
            EditorState::RubberBand { anchor, current, additive } => {
                let band = Rect::from_two_pos(anchor, current);
                if !additive {
                    self.selected.clear();
                }
                let hits: Vec<AnnotationId> = self
                    .doc
                    .annotations()
                    .iter()
                    .filter(|(_, ann)| band.intersects(annotation_bbox(ann, measure)))
                    .map(|(id, _)| *id)
                    .collect();
                self.selected.extend(hits);
            }
            EditorState::Panning => {}
            // Not pointer-driven: put back untouched.
            other @ (EditorState::Idle
            | EditorState::TextEditing(_)
            | EditorState::ConfirmDiscard { .. }) => self.state = other,
        }
    }

    /// Primary click (press+release without a drag).
    pub fn click(&mut self, p: Pos2, screen: Pos2, canvas: Rect, mods: Modifiers, measure: Measure) {
        if self.state.is_text_editing() {
            self.commit_text();
            // With the Text tool, a click elsewhere chains straight into
            // new text there.
            if self.tool == Tool::Text {
                self.text_tool_click(p, measure);
            }
            return;
        }
        self.dismiss_transient();
        match self.tool {
            // Nothing to place on yet — a region doesn't exist to hold it
            // (and the toolbar that would show it doesn't either).
            Tool::Text if self.doc.region.is_some() => self.text_tool_click(p, measure),
            Tool::Marker if self.doc.region.is_some() => self.drop_marker(p, None),
            Tool::Text | Tool::Marker => {}
            Tool::Select => {
                if let Some(id) = topmost_hit(&self.doc, p, false, self.view.zoom, measure) {
                    if mods.command || mods.shift {
                        // Toggle membership.
                        if !self.selected.remove(&id) {
                            self.selected.insert(id);
                        }
                    } else {
                        self.selected.clear();
                        self.selected.insert(id);
                    }
                } else if mods.command || mods.shift {
                    // A missed additive click keeps the selection intact.
                } else if !self.selected.is_empty() {
                    // First click away just deselects.
                    self.selected.clear();
                } else if self.doc.region.is_some_and(|r| {
                    !self.view.rect_to_screen(canvas, r).expand(HANDLE_HIT).contains(screen)
                }) {
                    // A click away from the box clears it (one undo step).
                    self.doc.begin();
                    self.doc.region = None;
                    self.doc.commit();
                }
            }
            _ => {}
        }
    }

    /// Double-click on text edits it, whatever the tool (Marker excepted —
    /// its clicks drop markers).
    pub fn double_click(&mut self, p: Pos2, measure: Measure) {
        self.dismiss_transient();
        if self.tool == Tool::Marker || self.state.is_text_editing() {
            return;
        }
        if let Some(id) = topmost_hit(&self.doc, p, true, self.view.zoom, measure) {
            self.open_text_editor(id);
        }
    }

    // ------------------------------------------------------------------
    // Space / pan

    /// A primary drag began while space was held: it's a pan.
    pub fn begin_space_pan(&mut self) {
        self.dismiss_transient();
        if self.state.is_idle() {
            self.state = EditorState::Panning;
            self.space_panned = true;
        }
    }

    /// Tapping space jumps to the Select tool; a space+drag pan doesn't.
    pub fn space_released(&mut self) {
        if matches!(self.state, EditorState::Panning) {
            self.state = EditorState::Idle;
        }
        if !self.space_panned {
            self.set_tool(Tool::Select);
        }
        self.space_panned = false;
    }

    // ------------------------------------------------------------------
    // Creating annotations

    fn finish_drawing(&mut self, start: Pos2, current: Pos2, points: Vec<Pos2>) {
        if self.tool == Tool::Marker {
            self.drop_marker(start, Some(current));
            return;
        }
        let min_px = 3.0 / self.view.zoom; // discard sub-3-screen-pixel accidents
        let rect = Rect::from_two_pos(start, current);
        let shape = match self.tool {
            Tool::Pen if !points.is_empty() => Some(Shape::Pen { points }),
            Tool::Line if (current - start).length() >= min_px => {
                Some(Shape::Line { a: start, b: current })
            }
            Tool::Arrow if (current - start).length() >= min_px => {
                Some(Shape::Arrow { a: start, b: current })
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
            self.doc.begin();
            self.doc.push(Annotation::new(shape, self.style));
            self.doc.commit();
            self.promote_color(self.style.color);
        }
    }

    /// Drop a numbered marker at `pos`; a drag towards `target` pulls an
    /// arrow out of it — one object, an arrow with a fat numbered tail.
    pub fn drop_marker(&mut self, pos: Pos2, target: Option<Pos2>) {
        // Clamp into the image: a click in the letterbox must not create an
        // annotation that's clipped invisible.
        let img = self.doc.image_rect();
        let pos = pos.clamp(img.min, img.max);
        let target = marker_target(pos, target, &self.style);
        self.doc.begin();
        let number = self.doc.marker_next;
        self.doc.push(Annotation::new(Shape::Marker { pos, number, target }, self.style));
        self.doc.marker_next += 1;
        self.doc.commit();
        self.promote_color(self.style.color);
    }

    // ------------------------------------------------------------------
    // Text editing

    /// Text-tool click: edit the text under the pointer, or start a new one.
    pub fn text_tool_click(&mut self, p: Pos2, measure: Measure) {
        if let Some(id) = topmost_hit(&self.doc, p, true, self.view.zoom, measure) {
            self.open_text_editor(id);
        } else {
            let img = self.doc.image_rect();
            self.state = EditorState::TextEditing(TextEditState {
                target: None,
                pos: p.clamp(img.min, img.max),
                buffer: String::new(),
                style: self.style,
                just_created: true,
            });
        }
    }

    /// Re-open an existing text annotation for inline editing.
    pub fn open_text_editor(&mut self, id: AnnotationId) {
        if let Some(ann) = self.doc.get(id)
            && let Shape::Text { pos, text } = &ann.shape
        {
            self.state = EditorState::TextEditing(TextEditState {
                target: Some(id),
                pos: *pos,
                buffer: text.clone(),
                style: ann.style,
                just_created: true,
            });
            self.selected.clear();
            self.selected.insert(id);
        }
    }

    pub fn commit_text(&mut self) {
        if !self.state.is_text_editing() {
            return;
        }
        let EditorState::TextEditing(edit) = std::mem::replace(&mut self.state, EditorState::Idle)
        else {
            return;
        };
        let text = edit.buffer.trim_end().to_owned();
        match edit.target {
            Some(id) => {
                let Some(ann) = self.doc.get(id) else { return };
                let Shape::Text { text: old, .. } = &ann.shape else { return };
                if text.is_empty() {
                    self.doc.begin();
                    self.doc.remove(id);
                    self.doc.commit();
                    self.selected.remove(&id);
                } else if text != *old || edit.style != ann.style {
                    self.doc.begin();
                    if let Some(ann) = self.doc.get_mut(id) {
                        // Style tweaks made while editing land with the text.
                        ann.style = edit.style;
                        if let Shape::Text { text: slot, .. } = &mut ann.shape {
                            *slot = text;
                        }
                    }
                    self.doc.commit();
                }
            }
            None => {
                if !text.is_empty() {
                    self.doc.begin();
                    self.doc
                        .push(Annotation::new(Shape::Text { pos: edit.pos, text }, edit.style));
                    self.doc.commit();
                    self.promote_color(edit.style.color);
                }
            }
        }
    }

    /// Drop the buffer; an existing annotation reappears unchanged.
    pub fn cancel_text(&mut self) {
        if self.state.is_text_editing() {
            self.state = EditorState::Idle;
        }
    }
}

/// Rebuild one dragged annotation from its pre-drag state and the pointer.
/// `bbox0`/`center` are the drag-start bounding box and pivot.
fn dragged_item(
    start: &Annotation,
    kind: ItemDragKind,
    bbox0: Rect,
    center: Pos2,
    grab: Pos2,
    p: Pos2,
) -> Annotation {
    let mut ann = start.clone();
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
                // Resize in the shape's local (unrotated) frame, then shift
                // so the untouched edges stay fixed on screen.
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
    ann
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::RgbaImage;

    fn measure(_: &str, size: f32) -> Vec2 {
        Vec2::new(size * 5.0, size)
    }

    fn canvas() -> Rect {
        Rect::from_min_max(Pos2::ZERO, Pos2::new(800.0, 600.0))
    }

    fn editor() -> Editor {
        let doc = Document::new(RgbaImage::new(800, 600));
        let style = Style { color: Color32::RED, width: 4.0, font_size: 24.0 };
        let mut ed = Editor::new(doc, style, vec![Color32::RED], false);
        ed.view.fitted = true; // zoom 1, pan 0: image coords == screen coords
        ed
    }

    fn add_highlight(ed: &mut Editor, r: Rect) -> AnnotationId {
        ed.doc.begin();
        let id = ed.doc.push(Annotation::new(Shape::Highlight { rect: r }, ed.style));
        ed.doc.commit();
        id
    }

    #[test]
    fn draw_rect_lifecycle_creates_one_undo_step() {
        let mut ed = editor();
        ed.doc.region = Some(ed.doc.image_rect());
        ed.set_tool(Tool::Rect);
        ed.primary_drag_start(
            Pos2::new(10.0, 10.0),
            Pos2::new(10.0, 10.0),
            canvas(),
            Modifiers::NONE,
            &measure,
        );
        assert!(matches!(ed.state, EditorState::DrawingShape { .. }));
        ed.pointer_moved(Pos2::new(60.0, 40.0));
        ed.pointer_up(&measure);
        assert!(ed.state.is_idle());
        assert_eq!(ed.doc.annotations().len(), 1);
        assert!(ed.doc.can_undo());
        ed.undo();
        assert_eq!(ed.doc.annotations().len(), 0);
    }

    #[test]
    fn escape_reverts_item_drag_without_history() {
        let mut ed = editor();
        ed.doc.region = Some(ed.doc.image_rect());
        let r = Rect::from_min_max(Pos2::new(100.0, 100.0), Pos2::new(200.0, 150.0));
        let id = add_highlight(&mut ed, r);
        let undo_before = ed.doc.can_undo();
        ed.set_tool(Tool::Select);
        // Drag from inside the highlight (Highlight hits by containment).
        ed.primary_drag_start(
            Pos2::new(150.0, 120.0),
            Pos2::new(150.0, 120.0),
            canvas(),
            Modifiers::NONE,
            &measure,
        );
        assert!(matches!(ed.state, EditorState::ItemsDrag { .. }));
        assert_eq!(ed.selected.len(), 1);
        ed.pointer_moved(Pos2::new(300.0, 300.0));
        ed.escape(0.0);
        assert!(ed.state.is_idle());
        let Shape::Highlight { rect } = ed.doc.get(id).expect("still there").shape else {
            panic!("shape changed")
        };
        assert_eq!(rect, r);
        assert_eq!(ed.doc.can_undo(), undo_before); // reverted drag left no step
    }

    #[test]
    fn noop_item_drag_burns_no_undo_slot() {
        let mut ed = editor();
        let r = Rect::from_min_max(Pos2::new(100.0, 100.0), Pos2::new(200.0, 150.0));
        add_highlight(&mut ed, r);
        ed.set_tool(Tool::Select);
        let undo_len_matters = ed.doc.can_undo();
        ed.primary_drag_start(
            Pos2::new(150.0, 120.0),
            Pos2::new(150.0, 120.0),
            canvas(),
            Modifiers::NONE,
            &measure,
        );
        ed.pointer_up(&measure); // grabbed, never moved
        assert_eq!(ed.doc.can_undo(), undo_len_matters);
    }

    #[test]
    fn rubber_band_multi_selects_and_group_deletes() {
        let mut ed = editor();
        let a = add_highlight(&mut ed, Rect::from_min_max(Pos2::new(10.0, 10.0), Pos2::new(30.0, 30.0)));
        let b = add_highlight(&mut ed, Rect::from_min_max(Pos2::new(50.0, 10.0), Pos2::new(70.0, 30.0)));
        let far = add_highlight(
            &mut ed,
            Rect::from_min_max(Pos2::new(400.0, 400.0), Pos2::new(420.0, 420.0)),
        );
        // Away from the drag points below, so its edges/grip don't intercept them.
        ed.doc.region = Some(Rect::from_min_max(Pos2::new(-1000.0, -1000.0), Pos2::new(1000.0, 1000.0)));
        ed.set_tool(Tool::Select);
        ed.primary_drag_start(
            Pos2::new(0.0, 0.0),
            Pos2::new(0.0, 0.0),
            canvas(),
            Modifiers::SHIFT,
            &measure,
        );
        assert!(matches!(ed.state, EditorState::RubberBand { .. }));
        ed.pointer_moved(Pos2::new(100.0, 100.0));
        ed.pointer_up(&measure);
        assert!(ed.selected.contains(&a) && ed.selected.contains(&b));
        assert!(!ed.selected.contains(&far));
        // One undo step deletes the whole selection.
        ed.delete_selected();
        assert_eq!(ed.doc.annotations().len(), 1);
        ed.undo();
        assert_eq!(ed.doc.annotations().len(), 3);
        assert!(ed.selected.is_empty()); // pruned, not dangling
    }

    #[test]
    fn plain_drag_on_empty_rubber_bands_and_replaces() {
        let mut ed = editor();
        let a = add_highlight(&mut ed, Rect::from_min_max(Pos2::new(100.0, 100.0), Pos2::new(130.0, 130.0)));
        let b = add_highlight(&mut ed, Rect::from_min_max(Pos2::new(150.0, 100.0), Pos2::new(180.0, 130.0)));
        let far = add_highlight(
            &mut ed,
            Rect::from_min_max(Pos2::new(400.0, 400.0), Pos2::new(420.0, 420.0)),
        );
        // A region already exists, so a plain drag selects rather than
        // drawing a new region.
        ed.doc.region = Some(ed.doc.image_rect());
        ed.set_tool(Tool::Select);
        ed.selected.insert(far); // must be cleared by the non-additive band
        // Empty-space start: no item, no region handle, no grip.
        ed.primary_drag_start(
            Pos2::new(90.0, 90.0),
            Pos2::new(90.0, 90.0),
            canvas(),
            Modifiers::NONE,
            &measure,
        );
        assert!(matches!(ed.state, EditorState::RubberBand { additive: false, .. }));
        ed.pointer_moved(Pos2::new(200.0, 200.0));
        ed.pointer_up(&measure);
        assert!(ed.selected.contains(&a) && ed.selected.contains(&b));
        assert!(!ed.selected.contains(&far), "a plain band replaces the selection");
    }

    #[test]
    fn plain_drag_draws_region_when_none_exists() {
        let mut ed = editor();
        assert!(ed.doc.region.is_none());
        ed.primary_drag_start(
            Pos2::new(50.0, 50.0),
            Pos2::new(50.0, 50.0),
            canvas(),
            Modifiers::NONE,
            &measure,
        );
        // With no region yet, the first drag is still the capture-setup flow.
        assert!(matches!(ed.state, EditorState::RegionDraw { .. }));
    }

    #[test]
    fn drawing_tool_still_draws_the_region_first() {
        // A drawing tool picked before any region exists (e.g. its keyboard
        // shortcut, hit before ever dragging one out) must not start an
        // invisible annotation — the toolbar that would show it doesn't
        // appear until a region exists either.
        let mut ed = editor();
        ed.set_tool(Tool::Line);
        assert!(ed.doc.region.is_none());
        ed.primary_drag_start(
            Pos2::new(50.0, 50.0),
            Pos2::new(50.0, 50.0),
            canvas(),
            Modifiers::NONE,
            &measure,
        );
        assert!(matches!(ed.state, EditorState::RegionDraw { .. }));
        ed.pointer_moved(Pos2::new(150.0, 150.0));
        ed.pointer_up(&measure);
        assert!(ed.doc.region.is_some());
        assert_eq!(ed.doc.annotations().len(), 0);
    }

    #[test]
    fn text_and_marker_clicks_are_ignored_without_a_region() {
        let mut ed = editor();
        for tool in [Tool::Text, Tool::Marker] {
            ed.set_tool(tool);
            ed.click(
                Pos2::new(50.0, 50.0),
                Pos2::new(50.0, 50.0),
                canvas(),
                Modifiers::NONE,
                &measure,
            );
        }
        assert!(ed.doc.region.is_none());
        assert_eq!(ed.doc.annotations().len(), 0);
        assert!(!ed.state.is_text_editing());
    }

    #[test]
    fn shift_drag_adds_to_the_selection() {
        let mut ed = editor();
        let a = add_highlight(&mut ed, Rect::from_min_max(Pos2::new(100.0, 100.0), Pos2::new(130.0, 130.0)));
        let b = add_highlight(&mut ed, Rect::from_min_max(Pos2::new(150.0, 100.0), Pos2::new(180.0, 130.0)));
        ed.doc.region = Some(ed.doc.image_rect());
        ed.set_tool(Tool::Select);
        ed.selected.insert(a);
        // Shift+drag a band over b only (well clear of a's handles).
        ed.primary_drag_start(
            Pos2::new(200.0, 200.0),
            Pos2::new(200.0, 200.0),
            canvas(),
            Modifiers::SHIFT,
            &measure,
        );
        assert!(matches!(ed.state, EditorState::RubberBand { additive: true, .. }));
        ed.pointer_moved(Pos2::new(155.0, 105.0));
        ed.pointer_up(&measure);
        assert!(ed.selected.contains(&a), "shift keeps the existing selection");
        assert!(ed.selected.contains(&b), "shift adds the band's contents");
    }

    #[test]
    fn region_move_grip_moves_the_region() {
        let mut ed = editor();
        let region = Rect::from_min_max(Pos2::new(100.0, 100.0), Pos2::new(300.0, 200.0));
        ed.doc.region = Some(region);
        ed.set_tool(Tool::Select);
        // View is 1:1 with no pan in tests, so screen coords equal image
        // coords: the grip sits at the same point on both.
        let grip = geometry::region_move_grip(region);
        ed.primary_drag_start(grip, grip, canvas(), Modifiers::NONE, &measure);
        assert!(matches!(
            ed.state,
            EditorState::RegionAdjust { mode: RegionMode::Move { .. }, .. }
        ));
        ed.pointer_moved(grip + Vec2::new(20.0, 20.0));
        ed.pointer_up(&measure);
        assert_eq!(ed.doc.region, Some(region.translate(Vec2::new(20.0, 20.0))));
    }

    #[test]
    fn ctrl_click_toggles_membership() {
        let mut ed = editor();
        let a = add_highlight(&mut ed, Rect::from_min_max(Pos2::new(10.0, 10.0), Pos2::new(30.0, 30.0)));
        let b = add_highlight(&mut ed, Rect::from_min_max(Pos2::new(50.0, 10.0), Pos2::new(70.0, 30.0)));
        ed.set_tool(Tool::Select);
        ed.click(Pos2::new(20.0, 20.0), Pos2::new(20.0, 20.0), canvas(), Modifiers::NONE, &measure);
        assert_eq!(ed.selected.iter().copied().collect::<Vec<_>>(), vec![a]);
        ed.click(Pos2::new(60.0, 20.0), Pos2::new(60.0, 20.0), canvas(), Modifiers::COMMAND, &measure);
        assert!(ed.selected.contains(&a) && ed.selected.contains(&b));
        ed.click(Pos2::new(20.0, 20.0), Pos2::new(20.0, 20.0), canvas(), Modifiers::COMMAND, &measure);
        assert!(!ed.selected.contains(&a) && ed.selected.contains(&b));
    }

    #[test]
    fn escape_restores_region_lost_to_accidental_redraw() {
        let mut ed = editor();
        let region = Rect::from_min_max(Pos2::new(100.0, 100.0), Pos2::new(300.0, 200.0));
        ed.doc.begin();
        ed.doc.region = Some(region);
        ed.doc.commit();
        // Start redrawing (right-drag) — the old box vanishes...
        ed.secondary_drag_start(Pos2::new(400.0, 400.0));
        ed.pointer_moved(Pos2::new(500.0, 500.0));
        assert_ne!(ed.doc.region, Some(region));
        // ...but Esc brings it back.
        assert_eq!(ed.escape(0.0), EscapeOutcome::Consumed);
        assert_eq!(ed.doc.region, Some(region));
    }

    #[test]
    fn undo_with_open_text_edit_cannot_corrupt_other_annotations() {
        let mut ed = editor();
        ed.doc.begin();
        let t1 = ed.doc.push(Annotation::new(
            Shape::Text { pos: Pos2::new(10.0, 10.0), text: "one".into() },
            ed.style,
        ));
        ed.doc.commit();
        ed.doc.begin();
        let t2 = ed.doc.push(Annotation::new(
            Shape::Text { pos: Pos2::new(10.0, 60.0), text: "two".into() },
            ed.style,
        ));
        ed.doc.commit();
        ed.open_text_editor(t2);
        if let EditorState::TextEditing(edit) = &mut ed.state {
            edit.buffer = "edited".into();
        }
        // Undo while editing: the edit commits first (to its own target),
        // then history moves — nothing writes through a stale reference.
        ed.undo();
        let Shape::Text { text, .. } = &ed.doc.get(t1).expect("t1 alive").shape else {
            panic!("t1 not text")
        };
        assert_eq!(text, "one");
        assert!(ed.state.is_idle());
        // The undo undid the just-committed edit, leaving t2's original text.
        let Shape::Text { text, .. } = &ed.doc.get(t2).expect("t2 alive").shape else {
            panic!("t2 not text")
        };
        assert_eq!(text, "two");
    }

    #[test]
    fn escape_ladder_reaches_the_discard_confirmation() {
        let mut ed = editor();
        add_highlight(&mut ed, Rect::from_min_max(Pos2::new(10.0, 10.0), Pos2::new(30.0, 30.0)));
        ed.set_tool(Tool::Pen);
        assert_eq!(ed.escape(0.0), EscapeOutcome::Consumed); // back to Select
        assert_eq!(ed.tool, Tool::Select);
        ed.click(Pos2::new(20.0, 20.0), Pos2::new(20.0, 20.0), canvas(), Modifiers::NONE, &measure);
        assert!(!ed.selected.is_empty());
        assert_eq!(ed.escape(0.0), EscapeOutcome::Consumed); // clear selection
        assert!(ed.selected.is_empty());
        // Root: first Esc arms the confirmation state...
        assert_eq!(ed.escape(0.0), EscapeOutcome::Consumed);
        assert!(ed.state.is_confirm_discard());
        // ...and a second Esc inside the window quits.
        assert_eq!(ed.escape(0.5), EscapeOutcome::Quit);
    }

    #[test]
    fn confirm_discard_expires_and_dismisses() {
        let mut ed = editor();
        // Arm, then let it expire: back to Idle, and the next Esc re-arms
        // instead of quitting.
        assert_eq!(ed.escape(0.0), EscapeOutcome::Consumed);
        assert!(ed.state.is_confirm_discard());
        ed.tick(0.5);
        assert!(ed.state.is_confirm_discard()); // still inside the window
        ed.tick(1.5);
        assert!(ed.state.is_idle()); // expired on its own
        assert_eq!(ed.escape(2.0), EscapeOutcome::Consumed); // re-arm, not quit
        // Any other action leaves the state too.
        ed.set_tool(Tool::Pen);
        assert!(ed.state.is_idle());
        assert_eq!(ed.tool, Tool::Pen);
    }

    #[test]
    fn undo_mid_drag_cancels_only_the_drag() {
        let mut ed = editor();
        let r = Rect::from_min_max(Pos2::new(100.0, 100.0), Pos2::new(200.0, 150.0));
        let id = add_highlight(&mut ed, r);
        ed.set_tool(Tool::Select);
        ed.primary_drag_start(
            Pos2::new(150.0, 120.0),
            Pos2::new(150.0, 120.0),
            canvas(),
            Modifiers::NONE,
            &measure,
        );
        ed.pointer_moved(Pos2::new(300.0, 300.0));
        // Ctrl+Z mid-drag: the drag reverts, the annotation's creation stays.
        ed.undo();
        assert!(ed.state.is_idle());
        let Shape::Highlight { rect } = ed.doc.get(id).expect("annotation survives").shape else {
            panic!("shape changed")
        };
        assert_eq!(rect, r);
    }

    #[test]
    fn missed_additive_click_keeps_selection() {
        let mut ed = editor();
        let a = add_highlight(&mut ed, Rect::from_min_max(Pos2::new(10.0, 10.0), Pos2::new(30.0, 30.0)));
        ed.set_tool(Tool::Select);
        ed.selected.insert(a);
        let miss = Pos2::new(300.0, 300.0);
        ed.click(miss, miss, canvas(), Modifiers::COMMAND, &measure);
        assert!(ed.selected.contains(&a), "ctrl-click miss must not wipe the selection");
        ed.click(miss, miss, canvas(), Modifiers::NONE, &measure);
        assert!(ed.selected.is_empty());
    }

    #[test]
    fn degenerate_region_redraw_restores_previous_region() {
        let mut ed = editor();
        let region = Rect::from_min_max(Pos2::new(100.0, 100.0), Pos2::new(300.0, 200.0));
        ed.doc.begin();
        ed.doc.region = Some(region);
        ed.doc.commit();
        ed.secondary_drag_start(Pos2::new(400.0, 400.0));
        ed.pointer_moved(Pos2::new(430.0, 401.0)); // a 30×1 px slip
        ed.pointer_up(&measure);
        assert_eq!(ed.doc.region, Some(region));
    }

    #[test]
    fn escape_mid_pan_still_reaches_the_next_rung() {
        let mut ed = editor();
        ed.set_tool(Tool::Pen);
        ed.begin_space_pan();
        assert_eq!(ed.escape(0.0), EscapeOutcome::Consumed);
        assert!(ed.state.is_idle());
        assert_eq!(ed.tool, Tool::Select); // fell through to the clean-slate rung
    }

    #[test]
    fn adjust_style_touches_only_the_moved_property() {
        let mut ed = editor();
        let a = add_highlight(&mut ed, Rect::from_min_max(Pos2::new(10.0, 10.0), Pos2::new(30.0, 30.0)));
        ed.doc.begin();
        let b = ed.doc.push(Annotation::new(
            Shape::Highlight {
                rect: Rect::from_min_max(Pos2::new(50.0, 10.0), Pos2::new(70.0, 30.0)),
            },
            Style { color: Color32::RED, width: 2.0, font_size: 40.0 },
        ));
        ed.doc.commit();
        ed.set_tool(Tool::Select);
        ed.selected.insert(a);
        ed.selected.insert(b);
        ed.adjust_style(Some(9.0), None);
        ed.end_style_adjust();
        assert_eq!(ed.doc.get(a).expect("a").style.width, 9.0);
        assert_eq!(ed.doc.get(b).expect("b").style.width, 9.0);
        // The untouched property keeps each object's own value.
        assert_eq!(ed.doc.get(a).expect("a").style.font_size, 24.0);
        assert_eq!(ed.doc.get(b).expect("b").style.font_size, 40.0);
        // Restyling a selection never bleeds into the new-object defaults.
        assert_eq!(ed.style.width, 4.0);
    }

    #[test]
    fn styling_while_typing_never_touches_new_object_defaults() {
        let mut ed = editor();
        let defaults = ed.style;
        ed.set_tool(Tool::Text);
        ed.text_tool_click(Pos2::new(50.0, 50.0), &measure);
        assert!(ed.state.is_text_editing());
        // Restyle mid-typing: the text being typed changes...
        ed.adjust_style(Some(11.0), Some(60.0));
        ed.apply_color(Color32::BLUE);
        let EditorState::TextEditing(edit) = &mut ed.state else { panic!("still editing") };
        assert_eq!(edit.style.width, 11.0);
        assert_eq!(edit.style.font_size, 60.0);
        assert_eq!(edit.style.color, Color32::BLUE);
        edit.buffer = "styled".into();
        ed.commit_text();
        // ...but the next text area starts from the untouched defaults.
        assert_eq!(ed.style, defaults);
        ed.text_tool_click(Pos2::new(600.0, 400.0), &measure);
        let EditorState::TextEditing(edit) = &ed.state else { panic!("editing again") };
        assert_eq!(edit.style, defaults);
    }

    /// What a right click on the canvas does: keep the typing, drop the
    /// tool. (Esc is the discarding way out.)
    #[test]
    fn returning_to_select_keeps_the_text_being_typed() {
        let mut ed = editor();
        ed.set_tool(Tool::Text);
        ed.text_tool_click(Pos2::new(50.0, 50.0), &measure);
        let EditorState::TextEditing(edit) = &mut ed.state else { panic!("editing") };
        edit.buffer = "kept".into();
        ed.set_tool(Tool::Select);
        assert_eq!(ed.tool, Tool::Select);
        assert!(ed.state.is_idle());
        assert!(
            ed.doc
                .annotations()
                .iter()
                .any(|(_, a)| matches!(&a.shape, Shape::Text { text, .. } if text == "kept"))
        );
    }

    #[test]
    fn group_move_translates_all_selected() {
        let mut ed = editor();
        let a = add_highlight(&mut ed, Rect::from_min_max(Pos2::new(10.0, 10.0), Pos2::new(30.0, 30.0)));
        let b = add_highlight(&mut ed, Rect::from_min_max(Pos2::new(50.0, 10.0), Pos2::new(70.0, 30.0)));
        ed.doc.region = Some(ed.doc.image_rect());
        ed.set_tool(Tool::Select);
        ed.selected.insert(a);
        ed.selected.insert(b);
        ed.primary_drag_start(
            Pos2::new(20.0, 20.0),
            Pos2::new(20.0, 20.0),
            canvas(),
            Modifiers::NONE,
            &measure,
        );
        ed.pointer_moved(Pos2::new(120.0, 20.0)); // +100 in x
        ed.pointer_up(&measure);
        let Shape::Highlight { rect: ra } = ed.doc.get(a).expect("a").shape else { panic!() };
        let Shape::Highlight { rect: rb } = ed.doc.get(b).expect("b").shape else { panic!() };
        assert_eq!(ra.min, Pos2::new(110.0, 10.0));
        assert_eq!(rb.min, Pos2::new(150.0, 10.0));
        // Selection survived the drag.
        assert_eq!(ed.selected.len(), 2);
    }
}
