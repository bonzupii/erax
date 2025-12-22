//! Dirty Region Tracker for Incremental Rendering
//!
//! Tracks which regions of the screen need redrawing to minimize
//! unnecessary full redraws and improve performance.

use crate::core::layout::Rect;

/// Tracks dirty regions of the screen for incremental rendering
#[derive(Debug, Clone)]
pub struct DirtyTracker {
    /// Full screen dimensions
    width: u16,
    height: u16,
    /// Per-row dirty flags (true = needs redraw)
    dirty_rows: Vec<bool>,
    /// Whether entire screen needs redraw
    full_redraw: bool,
}

impl DirtyTracker {
    /// Create a new dirty tracker for the given dimensions
    pub fn new(width: u16, height: u16) -> Self {
        Self {
            width,
            height,
            dirty_rows: vec![true; height as usize],
            full_redraw: true,
        }
    }

    /// Resize the tracker (marks everything dirty)
    pub fn resize(&mut self, width: u16, height: u16) {
        self.width = width;
        self.height = height;
        self.dirty_rows = vec![true; height as usize];
        self.full_redraw = true;
    }

    /// Mark entire screen as needing redraw
    pub fn mark_full_redraw(&mut self) {
        self.full_redraw = true;
        for row in &mut self.dirty_rows {
            *row = true;
        }
    }

    /// Mark a specific row as dirty
    pub fn mark_row(&mut self, row: usize) {
        if row < self.dirty_rows.len() {
            self.dirty_rows[row] = true;
        }
    }

    /// Mark a rectangular region as dirty
    pub fn mark_rect(&mut self, rect: &Rect) {
        for y in rect.y..(rect.y + rect.height) {
            self.mark_row(y);
        }
    }

    /// Check if a row needs redrawing
    pub fn is_row_dirty(&self, row: usize) -> bool {
        self.full_redraw
            || match self.dirty_rows.get(row).copied() {
                Some(v) => v,
                None => false,
            }
    }

    /// Check if full redraw is needed
    pub fn needs_full_redraw(&self) -> bool {
        self.full_redraw
    }

    /// Clear all dirty flags after rendering
    pub fn clear(&mut self) {
        self.full_redraw = false;
        for row in &mut self.dirty_rows {
            *row = false;
        }
    }

    /// Get dimensions
    #[cfg(test)]
    pub fn dimensions(&self) -> (u16, u16) {
        (self.width, self.height)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_tracker_is_dirty() {
        let tracker = DirtyTracker::new(80, 24);
        assert!(tracker.needs_full_redraw());
        assert!(tracker.is_row_dirty(0));
        assert!(tracker.is_row_dirty(23));
    }

    #[test]
    fn test_clear_resets_dirty() {
        let mut tracker = DirtyTracker::new(80, 24);
        tracker.clear();
        assert!(!tracker.needs_full_redraw());
        assert!(!tracker.is_row_dirty(0));
    }

    #[test]
    fn test_mark_row() {
        let mut tracker = DirtyTracker::new(80, 24);
        tracker.clear();
        tracker.mark_row(5);
        assert!(!tracker.needs_full_redraw());
        assert!(tracker.is_row_dirty(5));
        assert!(!tracker.is_row_dirty(6));
    }

    #[test]
    fn test_mark_rect() {
        let mut tracker = DirtyTracker::new(80, 24);
        tracker.clear();
        let rect = Rect::new(0, 5, 40, 3);
        tracker.mark_rect(&rect);
        assert!(tracker.is_row_dirty(5));
        assert!(tracker.is_row_dirty(6));
        assert!(tracker.is_row_dirty(7));
        assert!(!tracker.is_row_dirty(8));
    }

    #[test]
    fn test_resize_marks_full_redraw() {
        let mut tracker = DirtyTracker::new(80, 24);
        tracker.clear();
        assert!(!tracker.needs_full_redraw());
        tracker.resize(100, 30);
        assert!(tracker.needs_full_redraw());
        assert_eq!(tracker.dimensions(), (100, 30));
    }
}
