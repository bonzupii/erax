//! Selection Model
//!
//! Represents text selections in the editor with support for:
//! - Point selections (cursor position)
//! - Range selections (start to end)
//! - Rectangular/column selections (for future multi-cursor)

// SelectionManager is used by Window and MouseHandler

use std::cmp::{max, min};

// =============================================================================
// SELECTION STRUCT
// =============================================================================

/// A text selection in the buffer
///
/// Selections are represented as a range from anchor to cursor.
/// The anchor is where the selection started, cursor is where it ends.
/// They can be in any order (anchor before or after cursor).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Selection {
    /// Anchor position (where selection started)
    pub anchor: usize,
    /// Cursor position (where selection ends)  
    pub cursor: usize,
}

impl Selection {
    /// Create a new selection at a single point (no selection)
    pub fn point(pos: usize) -> Self {
        Self {
            anchor: pos,
            cursor: pos,
        }
    }

    /// Create a selection from anchor to cursor
    pub fn new(anchor: usize, cursor: usize) -> Self {
        Self { anchor, cursor }
    }

    /// Create a selection from start to end (normalized order)
    pub fn range(start: usize, end: usize) -> Self {
        Self {
            anchor: start,
            cursor: end,
        }
    }

    /// Check if this is a point selection (no range)
    pub fn is_empty(&self) -> bool {
        self.anchor == self.cursor
    }

    /// Get the start of the selection (smaller position)
    pub fn start(&self) -> usize {
        min(self.anchor, self.cursor)
    }

    /// Get the end of the selection (larger position)
    pub fn end(&self) -> usize {
        max(self.anchor, self.cursor)
    }

    /// Get the length of the selection
    pub fn len(&self) -> usize {
        self.end() - self.start()
    }

    /// Check if a position is within the selection
    pub fn contains(&self, pos: usize) -> bool {
        pos >= self.start() && pos < self.end()
    }

    /// Extend the selection to a new cursor position
    pub fn extend_to(&mut self, new_cursor: usize) {
        self.cursor = new_cursor;
    }

    /// Move the selection to a new position (collapses to point)
    pub fn move_to(&mut self, pos: usize) {
        self.anchor = pos;
        self.cursor = pos;
    }

    /// Set the anchor (start of selection)
    pub fn set_anchor(&mut self, pos: usize) {
        self.anchor = pos;
    }

    /// Normalize the selection so anchor <= cursor
    pub fn normalize(&self) -> Self {
        Self {
            anchor: self.start(),
            cursor: self.end(),
        }
    }

    /// Check if the selection direction is forward (anchor <= cursor)
    pub fn is_forward(&self) -> bool {
        self.anchor <= self.cursor
    }

    /// Swap anchor and cursor
    pub fn swap(&mut self) {
        std::mem::swap(&mut self.anchor, &mut self.cursor);
    }

    /// Merge with another selection (union)
    pub fn merge(&self, other: &Selection) -> Self {
        Self {
            anchor: min(self.start(), other.start()),
            cursor: max(self.end(), other.end()),
        }
    }

    /// Check if two selections overlap
    pub fn overlaps(&self, other: &Selection) -> bool {
        self.start() < other.end() && other.start() < self.end()
    }

    /// Adjust positions after text insertion
    ///
    /// If text is inserted before the selection, both anchor and cursor shift.
    /// If text is inserted within the selection, cursor shifts.
    pub fn adjust_for_insert(&mut self, insert_pos: usize, insert_len: usize) {
        if insert_pos <= self.anchor {
            self.anchor += insert_len;
        }
        if insert_pos <= self.cursor {
            self.cursor += insert_len;
        }
    }

    /// Adjust positions after text deletion
    ///
    /// If text is deleted before the selection, both shift back.
    /// If deletion overlaps selection, selection shrinks.
    pub fn adjust_for_delete(&mut self, delete_start: usize, delete_len: usize) {
        let delete_end = delete_start + delete_len;

        // Adjust anchor
        if delete_end <= self.anchor {
            self.anchor -= delete_len;
        } else if delete_start < self.anchor {
            self.anchor = delete_start;
        }

        // Adjust cursor
        if delete_end <= self.cursor {
            self.cursor -= delete_len;
        } else if delete_start < self.cursor {
            self.cursor = delete_start;
        }
    }
}

// =============================================================================
// SELECTION MODE
// =============================================================================

/// The type of selection operation
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SelectionMode {
    /// No active selection
    #[default]
    None,
    /// Character-by-character selection
    Character,
    /// Word-by-word selection
    Word,
    /// Line-by-line selection
    Line,
    /// Rectangular/column selection
    Rectangle,
}

// =============================================================================
// SELECTION MANAGER
// =============================================================================

/// Manages selections for a window
#[derive(Debug, Default, Clone)]
pub struct SelectionManager {
    /// Primary selection
    pub primary: Option<Selection>,
    /// Selection mode
    pub mode: SelectionMode,
    /// Mark position
    pub mark: Option<usize>,
}

impl SelectionManager {
    /// Create a new selection manager
    pub fn new() -> Self {
        Self::default()
    }

    /// Check if there's an active selection
    pub fn has_selection(&self) -> bool {
        match self.primary {
            Some(s) => !s.is_empty(),
            None => false,
        }
    }

    /// Get the current selection
    pub fn get_selection(&self) -> Option<&Selection> {
        self.primary.as_ref().filter(|s| !s.is_empty())
    }

    /// Start a selection at the given position
    pub fn start_selection(&mut self, pos: usize, mode: SelectionMode) {
        self.primary = Some(Selection::point(pos));
        self.mode = mode;
    }

    /// Extend the current selection to a new position (respecting selection mode)
    pub fn extend_selection(&mut self, pos: usize, buffer: &crate::core::buffer::Buffer) {
        let raw_cursor = pos;

        // If no selection exists, start one (point)
        if self.primary.is_none() {
            self.primary = Some(Selection::point(pos));
            return;
        }

        // Calculate final cursor position based on mode
        let final_cursor = {
            let sel = match self.primary.as_ref() {
                Some(s) => s,
                None => return,
            };
            let anchor = sel.anchor;

            match self.mode {
                SelectionMode::Character | SelectionMode::None | SelectionMode::Rectangle => {
                    raw_cursor
                }
                SelectionMode::Word => {
                    // Snap to word boundaries
                    let (w_start, w_end) = Self::find_word_boundaries(buffer, raw_cursor);

                    if raw_cursor >= anchor {
                        w_end // Dragging forward: end of word
                    } else {
                        w_start // Dragging backward: start of word
                    }
                }
                SelectionMode::Line => {
                    let line_idx = buffer.byte_to_line(raw_cursor);

                    if raw_cursor >= anchor {
                        // Dragging forward: include full line (end including newline)
                        let start_pos = match buffer.line_to_byte(line_idx) {
                            Some(b) => b,
                            None => 0,
                        };
                        let len = match buffer.line_len(line_idx) {
                            Some(l) => l,
                            None => 0,
                        };
                        start_pos + len
                    } else {
                        // Dragging backward: include start of line
                        match buffer.line_to_byte(line_idx) {
                            Some(b) => b,
                            None => 0,
                        }
                    }
                }
            }
        };

        if let Some(ref mut sel) = self.primary {
            sel.extend_to(final_cursor);
        }
    }

    /// Clear the selection
    pub fn clear_selection(&mut self) {
        self.primary = None;
        self.mode = SelectionMode::None;
    }

    /// Set the mark
    pub fn set_mark(&mut self, pos: usize) {
        self.mark = Some(pos);
    }

    /// Clear the mark
    pub fn clear_mark(&mut self) {
        self.mark = None;
    }

    /// Get the region between mark and cursor
    pub fn get_region(&self, cursor: usize) -> Option<Selection> {
        self.mark.map(|mark| Selection::new(mark, cursor))
    }

    /// Exchange point and mark
    pub fn exchange_point_and_mark(&mut self, cursor: usize) -> Option<usize> {
        if let Some(mark) = self.mark {
            self.mark = Some(cursor);
            Some(mark)
        } else {
            None
        }
    }

    /// Adjust selection after text insertion
    pub fn adjust_for_insert(&mut self, insert_pos: usize, insert_len: usize) {
        if let Some(ref mut sel) = self.primary {
            sel.adjust_for_insert(insert_pos, insert_len);
        }
        if let Some(ref mut mark) = self.mark {
            if insert_pos <= *mark {
                *mark += insert_len;
            }
        }
    }

    /// Adjust selection after text deletion
    pub fn adjust_for_delete(&mut self, delete_start: usize, delete_len: usize) {
        if let Some(ref mut sel) = self.primary {
            sel.adjust_for_delete(delete_start, delete_len);
        }
        if let Some(ref mut mark) = self.mark {
            let delete_end = delete_start + delete_len;
            if delete_end <= *mark {
                *mark -= delete_len;
            } else if delete_start < *mark {
                *mark = delete_start;
            }
        }
    }

    /// Helper to find start and end bytes of the word at the given byte position
    fn find_word_boundaries(buffer: &crate::core::buffer::Buffer, pos: usize) -> (usize, usize) {
        let line_idx = buffer.byte_to_line(pos);
        if let Some(line) = buffer.line(line_idx) {
            let line_start_byte = match buffer.line_to_byte(line_idx) {
                Some(b) => b,
                None => 0,
            };
            // pos relative to line
            let rel_pos = pos.saturating_sub(line_start_byte);

            use crate::core::utf8::GraphemeIterator;
            let graphemes: Vec<&str> = GraphemeIterator::new(&line).collect();

            // Find which grapheme index we are at
            let mut current_byte = 0;
            let mut col = 0;
            // Iterate to find col
            for (i, g) in graphemes.iter().enumerate() {
                if current_byte + g.len() > rel_pos {
                    col = i;
                    break;
                }
                current_byte += g.len();
                if i == graphemes.len() - 1 {
                    col = graphemes.len(); // At end
                }
            }
            if col >= graphemes.len() {
                let end_pos = line_start_byte + graphemes.iter().map(|g| g.len()).sum::<usize>();
                return (end_pos, end_pos);
            }

            // Define word boundary: alphanumeric + underscore
            let is_word_char = |s: &str| s.chars().all(|c| c.is_alphanumeric() || c == '_');

            let clicked_grapheme = graphemes[col];
            let clicked_is_word = is_word_char(clicked_grapheme);

            // Expand start
            let mut start = col;
            while start > 0 {
                if is_word_char(graphemes[start - 1]) != clicked_is_word {
                    break;
                }
                start -= 1;
            }

            // Expand end
            let mut end = col + 1;
            while end < graphemes.len() {
                if is_word_char(graphemes[end]) != clicked_is_word {
                    break;
                }
                end += 1;
            }

            let start_byte_offset = graphemes[0..start].iter().map(|g| g.len()).sum::<usize>();
            let len_byte_offset = graphemes[start..end].iter().map(|g| g.len()).sum::<usize>();

            let start_pos = line_start_byte + start_byte_offset;
            let end_pos = start_pos + len_byte_offset;
            return (start_pos, end_pos);
        }
        (pos, pos)
    }
}

// =============================================================================
// TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_selection_point() {
        let sel = Selection::point(10);
        assert!(sel.is_empty());
        assert_eq!(sel.start(), 10);
        assert_eq!(sel.end(), 10);
        assert_eq!(sel.len(), 0);
    }

    #[test]
    fn test_selection_range() {
        let sel = Selection::new(5, 15);
        assert!(!sel.is_empty());
        assert_eq!(sel.start(), 5);
        assert_eq!(sel.end(), 15);
        assert_eq!(sel.len(), 10);
        assert!(sel.contains(5));
        assert!(sel.contains(10));
        assert!(!sel.contains(15)); // End is exclusive
    }

    #[test]
    fn test_selection_backward() {
        let sel = Selection::new(15, 5); // Backward selection
        assert!(!sel.is_forward());
        assert_eq!(sel.start(), 5);
        assert_eq!(sel.end(), 15);
        assert_eq!(sel.len(), 10);
    }

    #[test]
    fn test_selection_extend() {
        let mut sel = Selection::point(10);
        sel.extend_to(20);
        assert!(!sel.is_empty());
        assert_eq!(sel.anchor, 10);
        assert_eq!(sel.cursor, 20);
    }

    #[test]
    fn test_selection_adjust_insert() {
        let mut sel = Selection::new(10, 20);

        // Insert before selection
        sel.adjust_for_insert(5, 3);
        assert_eq!(sel.anchor, 13);
        assert_eq!(sel.cursor, 23);
    }

    #[test]
    fn test_selection_adjust_delete() {
        let mut sel = Selection::new(10, 20);

        // Delete before selection
        sel.adjust_for_delete(5, 3);
        assert_eq!(sel.anchor, 7);
        assert_eq!(sel.cursor, 17);
    }

    #[test]
    fn test_selection_overlap() {
        let sel1 = Selection::new(5, 15);
        let sel2 = Selection::new(10, 20);
        let sel3 = Selection::new(20, 30);

        assert!(sel1.overlaps(&sel2));
        assert!(sel2.overlaps(&sel1));
        assert!(!sel1.overlaps(&sel3));
    }

    #[test]
    fn test_selection_merge() {
        let sel1 = Selection::new(5, 15);
        let sel2 = Selection::new(10, 25);
        let merged = sel1.merge(&sel2);

        assert_eq!(merged.start(), 5);
        assert_eq!(merged.end(), 25);
    }

    #[test]
    fn test_selection_manager() {
        let mut mgr = SelectionManager::new();
        let buffer = crate::core::buffer::Buffer::new(); // Empty buffer is fine for Character mode
        assert!(!mgr.has_selection());

        mgr.start_selection(10, SelectionMode::Character);
        mgr.extend_selection(20, &buffer);

        assert!(mgr.has_selection());
        if let Some(sel) = mgr.get_selection() {
            assert_eq!(sel.start(), 10);
            assert_eq!(sel.end(), 20);
        } else {
            panic!("Expected selection");
        }
    }

    #[test]
    fn test_mark_and_region() {
        let mut mgr = SelectionManager::new();
        mgr.set_mark(10);

        let region = mgr.get_region(25);
        assert!(region.is_some());
        if let Some(region) = region {
            assert_eq!(region.start(), 10);
            assert_eq!(region.end(), 25);
        } else {
            panic!("Expected region");
        }
    }
}
