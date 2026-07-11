//! The annotated screenshot: base image, annotation list with stable ids,
//! the capture region, and snapshot-based undo/redo.
//!
//! Annotations are addressed by [`AnnotationId`], never by index — ids stay
//! valid across undo/redo/delete, so a stale reference resolves to nothing
//! instead of to the wrong object. All mutations that should be undoable
//! happen inside a transaction: `begin()` before the first change,
//! `commit()` when done (a no-op transaction is dropped, so speculative
//! begins are free), `rollback()` to revert (Esc).

use eframe::egui::{Pos2, Rect, Vec2};
use image::RgbaImage;

use crate::annotate::Annotation;

/// Stable identity of an annotation, unique within a session.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct AnnotationId(u64);

#[derive(Clone, PartialEq)]
struct Snapshot {
    annotations: Vec<(AnnotationId, Annotation)>,
    marker_next: u32,
    region: Option<Rect>,
}

pub struct Document {
    pub base: RgbaImage,
    /// In z-order: later entries draw on top.
    annotations: Vec<(AnnotationId, Annotation)>,
    next_id: u64,
    /// The capture region (image coords) — also the export crop.
    pub region: Option<Rect>,
    /// Number the next dropped marker gets.
    pub marker_next: u32,
    undo: Vec<Snapshot>,
    redo: Vec<Snapshot>,
    /// Pre-transaction state; present while a transaction is open.
    pending: Option<Snapshot>,
}

impl Document {
    pub fn new(base: RgbaImage) -> Self {
        Self {
            base,
            annotations: Vec::new(),
            next_id: 0,
            region: None,
            marker_next: 1,
            undo: Vec::new(),
            redo: Vec::new(),
            pending: None,
        }
    }

    pub fn image_size(&self) -> Vec2 {
        Vec2::new(self.base.width() as f32, self.base.height() as f32)
    }

    pub fn image_rect(&self) -> Rect {
        Rect::from_min_size(Pos2::ZERO, self.image_size())
    }

    /// All annotations in z-order (bottom first).
    pub fn annotations(&self) -> &[(AnnotationId, Annotation)] {
        &self.annotations
    }

    /// Just the shapes, for renderers that don't care about identity.
    pub fn shapes(&self) -> impl Iterator<Item = &Annotation> + Clone {
        self.annotations.iter().map(|(_, a)| a)
    }

    pub fn ids(&self) -> impl Iterator<Item = AnnotationId> + '_ {
        self.annotations.iter().map(|(id, _)| *id)
    }

    pub fn contains(&self, id: AnnotationId) -> bool {
        self.annotations.iter().any(|(i, _)| *i == id)
    }

    pub fn get(&self, id: AnnotationId) -> Option<&Annotation> {
        self.annotations.iter().find(|(i, _)| *i == id).map(|(_, a)| a)
    }

    pub fn get_mut(&mut self, id: AnnotationId) -> Option<&mut Annotation> {
        self.annotations.iter_mut().find(|(i, _)| *i == id).map(|(_, a)| a)
    }

    /// Append on top of the z-order.
    pub fn push(&mut self, ann: Annotation) -> AnnotationId {
        let id = AnnotationId(self.next_id);
        self.next_id += 1;
        self.annotations.push((id, ann));
        id
    }

    pub fn remove(&mut self, id: AnnotationId) -> bool {
        let before = self.annotations.len();
        self.annotations.retain(|(i, _)| *i != id);
        self.annotations.len() != before
    }

    pub fn clear_annotations(&mut self) {
        self.annotations.clear();
    }

    fn snapshot(&self) -> Snapshot {
        Snapshot {
            annotations: self.annotations.clone(),
            marker_next: self.marker_next,
            region: self.region,
        }
    }

    fn restore(&mut self, snapshot: Snapshot) {
        self.annotations = snapshot.annotations;
        self.marker_next = snapshot.marker_next;
        self.region = snapshot.region;
    }

    /// Open a transaction. An already-open one is committed first (should
    /// not happen when driven by the editor FSM, but never lose history).
    pub fn begin(&mut self) {
        if self.pending.is_some() {
            self.commit();
        }
        self.pending = Some(self.snapshot());
    }

    /// Close the transaction; it becomes one undo step unless nothing
    /// actually changed. The no-op check compares in place — no snapshot is
    /// built just to be thrown away.
    pub fn commit(&mut self) {
        if let Some(before) = self.pending.take()
            && (before.annotations != self.annotations
                || before.marker_next != self.marker_next
                || before.region != self.region)
        {
            self.undo.push(before);
            self.redo.clear();
        }
    }

    /// Revert everything since `begin()`.
    pub fn rollback(&mut self) {
        if let Some(before) = self.pending.take() {
            self.restore(before);
        }
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    pub fn undo(&mut self) {
        // Fold any in-flight adjustment into history first so it is what
        // gets undone, rather than silently discarded.
        self.commit();
        if let Some(snapshot) = self.undo.pop() {
            let now = self.snapshot();
            self.redo.push(now);
            self.restore(snapshot);
        }
    }

    pub fn redo(&mut self) {
        self.commit();
        if let Some(snapshot) = self.redo.pop() {
            let now = self.snapshot();
            self.undo.push(now);
            self.restore(snapshot);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::annotate::{Shape, Style};
    use eframe::egui::Color32;

    fn doc() -> Document {
        Document::new(RgbaImage::new(100, 80))
    }

    fn line(x: f32) -> Annotation {
        Annotation::new(
            Shape::Line { a: Pos2::new(x, 0.0), b: Pos2::new(x, 10.0) },
            Style { color: Color32::RED, width: 2.0, font_size: 16.0 },
        )
    }

    #[test]
    fn ids_survive_undo_redo() {
        let mut d = doc();
        d.begin();
        let a = d.push(line(1.0));
        d.commit();
        d.begin();
        let b = d.push(line(2.0));
        d.commit();
        assert!(d.contains(a) && d.contains(b));
        d.undo();
        assert!(d.contains(a) && !d.contains(b));
        d.redo();
        assert!(d.contains(a) && d.contains(b));
        // New pushes never reuse an id that exists in history.
        d.undo();
        d.begin();
        let c = d.push(line(3.0));
        d.commit();
        assert_ne!(c, b);
    }

    #[test]
    fn noop_transaction_is_dropped() {
        let mut d = doc();
        d.begin();
        d.commit();
        assert!(!d.can_undo());
        // A rollback never creates history either.
        d.begin();
        d.push(line(1.0));
        d.rollback();
        assert!(!d.can_undo());
        assert_eq!(d.annotations().len(), 0);
    }

    #[test]
    fn region_is_part_of_history() {
        let mut d = doc();
        d.begin();
        d.region = Some(Rect::from_min_max(Pos2::ZERO, Pos2::new(50.0, 50.0)));
        d.commit();
        assert!(d.can_undo());
        d.undo();
        assert_eq!(d.region, None);
        d.redo();
        assert!(d.region.is_some());
    }

    #[test]
    fn undo_folds_open_transaction() {
        let mut d = doc();
        d.begin();
        d.push(line(1.0));
        // Undo with the transaction still open: the pending change becomes
        // the step being undone.
        d.undo();
        assert_eq!(d.annotations().len(), 0);
        d.redo();
        assert_eq!(d.annotations().len(), 1);
    }
}
