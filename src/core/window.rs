use crate::core::buffer::Buffer;
use crate::core::id::{BufferId, WindowId};
use crate::core::selection::SelectionManager;

/// `Window` represents a visual viewport into a `Buffer`.
///
/// Each window maintains its own cursor position, scroll offset, and display dimensions,
/// allowing multiple views into the same or different buffers. It handles cursor movement
/// within its viewport and interaction with the underlying buffer's content.
#[derive(Debug, Clone)]
pub struct Window {
    /// Window ID
    pub id: WindowId,
    /// Which buffer this window displays
    pub buffer_id: BufferId,
    /// Cursor column position (grapheme clusters, not bytes)
    pub cursor_x: usize,
    /// Cursor line position
    pub cursor_y: usize,
    /// Visual cursor column (for rendering, accounts for tab width and emoji width)
    pub visual_cursor_x: usize,
    /// Vertical scroll offset (top visible line)
    pub scroll_offset: usize,
    /// Horizontal scroll offset (leftmost visible column)
    pub scroll_x: usize,
    /// Viewport width (columns)
    pub width: usize,
    /// Viewport height (rows)
    pub height: usize,
    /// Mark (x, y) for region operations
    pub mark: Option<(usize, usize)>,
    /// Tab width for this window
    pub tab_width: usize,
    /// Selection manager for this window
    pub selection_manager: SelectionManager,
}

impl Window {
    /// Create a new window viewing a buffer
    pub fn new(id: WindowId, buffer_id: BufferId, tab_width: usize) -> Self {
        Self {
            id,
            buffer_id,
            cursor_x: 0,
            cursor_y: 0,
            visual_cursor_x: 0,
            scroll_offset: 0,
            scroll_x: 0,
            width: 80,
            height: 24,
            mark: None,
            tab_width,
            selection_manager: SelectionManager::new(),
        }
    }

    /// Set window dimensions
    pub fn set_dimensions(&mut self, width: usize, height: usize) {
        self.width = width;
        self.height = height;
    }

    /// Move cursor forward one grapheme cluster
    pub fn move_forward(&mut self, buffer: &Buffer) {
        let line_count = buffer.line_count();

        if let Some(line_text) = buffer.line(self.cursor_y) {
            let graphemes: Vec<&str> =
                crate::core::utf8::GraphemeIterator::new(&line_text).collect();

            if self.cursor_x < graphemes.len() {
                self.cursor_x += 1;
            } else if self.cursor_y < line_count.saturating_sub(1) {
                self.cursor_y += 1;
                self.cursor_x = 0;
            }
        }

        self.ensure_cursor_valid(buffer);
        self.update_visual_cursor(buffer);
        self.ensure_cursor_visible(buffer);
    }

    /// Move cursor backward one grapheme cluster
    /// Returns true if moved, false if already at start
    pub fn move_backward(&mut self, buffer: &Buffer) -> bool {
        let moved = if self.cursor_x > 0 {
            self.cursor_x -= 1;
            true
        } else if self.cursor_y > 0 {
            self.cursor_y -= 1;
            if let Some(line_text) = buffer.line(self.cursor_y) {
                self.cursor_x = crate::core::utf8::grapheme_count(&line_text);
            }
            true
        } else {
            false
        };

        if moved {
            self.ensure_cursor_valid(buffer);
            self.update_visual_cursor(buffer);
            self.ensure_cursor_visible(buffer);
        }
        moved
    }

    /// Move cursor down one line
    pub fn move_down(&mut self, buffer: &Buffer) {
        if self.cursor_y < buffer.line_count().saturating_sub(1) {
            self.cursor_y += 1;
        }
        self.ensure_cursor_valid(buffer);
        self.update_visual_cursor(buffer);
        self.ensure_cursor_visible(buffer);
    }

    /// Move cursor up one line
    pub fn move_up(&mut self, buffer: &Buffer) {
        if self.cursor_y > 0 {
            self.cursor_y -= 1;
        }
        self.ensure_cursor_valid(buffer);
        self.update_visual_cursor(buffer);
        self.ensure_cursor_visible(buffer);
    }

    /// Move cursor to beginning of line
    pub fn beginning_of_line(&mut self, buffer: &Buffer) {
        self.cursor_x = 0;
        self.update_visual_cursor(buffer);
        self.ensure_cursor_visible(buffer);
    }

    /// Move cursor to end of line
    pub fn end_of_line(&mut self, buffer: &Buffer) {
        if let Some(line_text) = buffer.line(self.cursor_y) {
            self.cursor_x = crate::core::utf8::grapheme_count(&line_text);
        }
        self.update_visual_cursor(buffer);
        self.ensure_cursor_visible(buffer);
    }

    /// Move cursor to beginning of buffer
    pub fn beginning_of_buffer(&mut self, buffer: &Buffer) {
        self.cursor_x = 0;
        self.cursor_y = 0;
        self.scroll_offset = 0;
        self.update_visual_cursor(buffer);
    }

    /// Move cursor to end of buffer
    pub fn end_of_buffer(&mut self, buffer: &Buffer) {
        let line_count = buffer.line_count();
        self.cursor_y = line_count.saturating_sub(1);
        if let Some(line_text) = buffer.line(self.cursor_y) {
            self.cursor_x = crate::core::utf8::grapheme_count(&line_text);
        }
        self.update_visual_cursor(buffer);
        self.ensure_cursor_visible(buffer);
    }

    /// Move cursor forward by word
    pub fn forward_word(&mut self, buffer: &Buffer) {
        let line_count = buffer.line_count();

        if let Some(line_text) = buffer.line(self.cursor_y) {
            let graphemes: Vec<&str> =
                crate::core::utf8::GraphemeIterator::new(&line_text).collect();
            let grapheme_count = graphemes.len();

            // Skip current word/whitespace
            let mut in_word = self.cursor_x < grapheme_count
                && !graphemes
                    .get(self.cursor_x)
                    .map_or(true, |g| g.chars().all(|c| c.is_whitespace()));

            while self.cursor_x < grapheme_count {
                let grapheme = graphemes[self.cursor_x];
                let is_space = grapheme.chars().all(|c| c.is_whitespace());

                if in_word && is_space {
                    break;
                } else if !in_word && !is_space {
                    in_word = true;
                }

                self.cursor_x += 1;
            }

            // If we reached end of line and there's a next line, move to it
            if self.cursor_x >= grapheme_count && self.cursor_y < line_count.saturating_sub(1) {
                self.cursor_y += 1;
                self.cursor_x = 0;
                // Skip leading whitespace on new line
                if let Some(new_line_text) = buffer.line(self.cursor_y) {
                    let new_graphemes: Vec<&str> =
                        crate::core::utf8::GraphemeIterator::new(&new_line_text).collect();
                    while self.cursor_x < new_graphemes.len()
                        && new_graphemes[self.cursor_x]
                            .chars()
                            .all(|c| c.is_whitespace())
                    {
                        self.cursor_x += 1;
                    }
                }
            }
        }

        self.update_visual_cursor(buffer);
        self.ensure_cursor_visible(buffer);
    }

    /// Move cursor backward by word
    pub fn backward_word(&mut self, buffer: &Buffer) {
        // If at beginning of line, move to end of previous line
        if self.cursor_x == 0 && self.cursor_y > 0 {
            self.cursor_y -= 1;
            if let Some(line_text) = buffer.line(self.cursor_y) {
                self.cursor_x = crate::core::utf8::grapheme_count(&line_text);
            }
        }

        if let Some(line_text) = buffer.line(self.cursor_y) {
            let graphemes: Vec<&str> =
                crate::core::utf8::GraphemeIterator::new(&line_text).collect();

            // Skip trailing whitespace
            while self.cursor_x > 0
                && graphemes
                    .get(self.cursor_x.saturating_sub(1))
                    .map_or(false, |g| g.chars().all(|c| c.is_whitespace()))
            {
                self.cursor_x -= 1;
            }

            // Skip word characters
            while self.cursor_x > 0
                && !graphemes
                    .get(self.cursor_x.saturating_sub(1))
                    .map_or(true, |g| g.chars().all(|c| c.is_whitespace()))
            {
                self.cursor_x -= 1;
            }
        }

        self.update_visual_cursor(buffer);
        self.ensure_cursor_visible(buffer);
    }

    /// Update the visual cursor position based on content width (tabs, wide chars)
    pub fn update_visual_cursor(&mut self, buffer: &Buffer) {
        let line_text = buffer.line(self.cursor_y).unwrap_or_default();
        self.visual_cursor_x =
            crate::core::utf8::visual_width_up_to(&line_text, self.cursor_x, self.tab_width);
    }

    /// Ensure cursor is within valid bounds
    pub fn ensure_cursor_valid(&mut self, buffer: &Buffer) {
        let line_count = buffer.line_count();
        if line_count == 0 {
            self.cursor_x = 0;
            self.cursor_y = 0;
            return;
        }

        self.cursor_y = self.cursor_y.min(line_count.saturating_sub(1));

        if let Some(line_text) = buffer.line(self.cursor_y) {
            let grapheme_count = crate::core::utf8::grapheme_count(&line_text);
            self.cursor_x = self.cursor_x.min(grapheme_count);
        } else {
            self.cursor_x = 0;
        }
    }

    /// Ensure cursor is visible in viewport (scroll only at edges)
    pub fn ensure_cursor_visible(&mut self, buffer: &Buffer) {
        let _ = buffer; // Used for context if needed

        // Vertical scrolling - only when cursor hits edge
        if self.cursor_y < self.scroll_offset {
            // Cursor above viewport - scroll up so cursor is at top
            self.scroll_offset = self.cursor_y;
        } else if self.cursor_y >= self.scroll_offset + self.height {
            // Cursor below viewport - scroll down so cursor is at bottom
            self.scroll_offset = self.cursor_y.saturating_sub(self.height) + 1;
        }

        // Horizontal scrolling - only when cursor hits edge (when wrap_lines=false)
        if self.visual_cursor_x < self.scroll_x {
            // Cursor left of viewport - scroll left so cursor is at left edge
            self.scroll_x = self.visual_cursor_x;
        } else if self.visual_cursor_x >= self.scroll_x + self.width {
            // Cursor right of viewport - scroll right so cursor is at right edge
            self.scroll_x = self.visual_cursor_x.saturating_sub(self.width) + 1;
        }
    }

    // =========================================================================
    // Scroll API - All scroll_offset manipulation should go through these
    // =========================================================================

    /// Scroll by a number of lines (positive = down, negative = up)
    /// Cursor is clamped to stay within visible viewport
    pub fn scroll_by(&mut self, lines: isize, buffer: &Buffer) {
        let line_count = buffer.line_count();
        let max_scroll = line_count.saturating_sub(self.height);

        if lines > 0 {
            self.scroll_offset = self.scroll_offset.saturating_add(lines as usize).min(max_scroll);
        } else {
            self.scroll_offset = self.scroll_offset.saturating_sub((-lines) as usize);
        }

        // Clamp cursor to visible range
        self.clamp_cursor_to_viewport(buffer);
    }

    /// Scroll to an absolute line position
    /// Cursor is clamped to stay within visible viewport
    pub fn scroll_to(&mut self, line: usize, buffer: &Buffer) {
        let line_count = buffer.line_count();
        let max_scroll = line_count.saturating_sub(self.height);
        self.scroll_offset = line.min(max_scroll);
        self.clamp_cursor_to_viewport(buffer);
    }

    /// Scroll down one page
    pub fn page_down(&mut self, buffer: &Buffer) {
        self.scroll_by(self.height.saturating_sub(2) as isize, buffer);
    }

    /// Scroll up one page
    pub fn page_up(&mut self, buffer: &Buffer) {
        self.scroll_by(-(self.height.saturating_sub(2) as isize), buffer);
    }

    /// Get the visible line range (start, end exclusive)
    pub fn visible_range(&self, buffer: &Buffer) -> (usize, usize) {
        let start = self.scroll_offset;
        let end = (self.scroll_offset + self.height).min(buffer.line_count());
        (start, end)
    }

    /// Clamp cursor to stay within visible viewport (helper for scroll methods)
    fn clamp_cursor_to_viewport(&mut self, buffer: &Buffer) {
        if self.cursor_y < self.scroll_offset {
            self.cursor_y = self.scroll_offset;
        } else if self.cursor_y >= self.scroll_offset + self.height {
            self.cursor_y = self.scroll_offset + self.height.saturating_sub(1);
        }
        // Ensure cursor_x is valid for the new line
        self.ensure_cursor_valid(buffer);
        self.update_visual_cursor(buffer);
    }

    /// Insert a character at the cursor position
    pub fn insert_char(&mut self, buffer: &mut Buffer, c: char) {
        let absolute_pos = self.get_byte_offset(buffer).unwrap_or(0);

        // Use stack buffer to avoid heap allocation
        let mut char_buf = [0u8; 4];
        let char_str = c.encode_utf8(&mut char_buf);

        // Use Buffer's insert method which handles undo recording internally
        buffer.insert(absolute_pos, char_str);

        // Update cursor position
        if c == '\n' {
            self.cursor_y += 1;
            self.cursor_x = 0;
        } else {
            self.cursor_x += 1;
        }

        self.update_visual_cursor(buffer);
    }

    /// Delete a character (backward if true, forward if false)
    pub fn delete_char(&mut self, buffer: &mut Buffer, backward: bool) {
        let line_text = buffer.line(self.cursor_y);

        if backward {
            // Backspace
            if self.cursor_x == 0 {
                // At beginning of line - delete newline from previous line
                if self.cursor_y > 0 {
                    // Move cursor to end of previous line first
                    self.cursor_y -= 1;
                    if let Some(prev_line) = buffer.line(self.cursor_y) {
                        self.cursor_x = crate::core::utf8::grapheme_count(&prev_line);
                    } else {
                        self.cursor_x = 0;
                    }
                    // Now delete the newline character at cursor position
                    let absolute_pos = self.get_byte_offset(buffer).unwrap_or(0);
                    buffer.delete(absolute_pos, 1);
                }
                // Else: at beginning of buffer, nothing to delete
            } else {
                // Delete character before cursor
                self.cursor_x -= 1;
                let absolute_pos = self.get_byte_offset(buffer).unwrap_or(0);

                // Get the byte length of the grapheme at cursor position
                if let Some(ref text) = line_text {
                    let graphemes: Vec<&str> =
                        crate::core::utf8::GraphemeIterator::new(text).collect();
                    if self.cursor_x < graphemes.len() {
                        let char_len = graphemes[self.cursor_x].len();
                        buffer.delete(absolute_pos, char_len);
                    }
                }
            }
        } else {
            // Delete forward
            let absolute_pos = self.get_byte_offset(buffer).unwrap_or(0);

            if let Some(ref text) = line_text {
                let graphemes: Vec<&str> = crate::core::utf8::GraphemeIterator::new(text).collect();

                if self.cursor_x < graphemes.len() {
                    // Delete character at cursor
                    let char_len = graphemes[self.cursor_x].len();
                    buffer.delete(absolute_pos, char_len);
                } else if self.cursor_y < buffer.line_count().saturating_sub(1) {
                    // At end of line, delete the newline
                    buffer.delete(absolute_pos, 1);
                }
            } else if self.cursor_y < buffer.line_count().saturating_sub(1) {
                // Empty line, delete newline
                buffer.delete(absolute_pos, 1);
            }
            // Cursor position doesn't change for forward delete
        }

        self.update_visual_cursor(buffer);
    }

    /// Get the absolute byte offset of the cursor
    pub fn get_byte_offset(&self, buffer: &Buffer) -> Option<usize> {
        let line_start = buffer.line_to_byte(self.cursor_y)?;

        if self.cursor_x == 0 {
            return Some(line_start);
        }

        if let Some(line_text) = buffer.line(self.cursor_y) {
            // Fast path: if content is ASCII, cursor_x == byte offset
            if line_text.is_ascii() {
                return Some(line_start + self.cursor_x.min(line_text.len()));
            }

            // Slow path: use grapheme_byte_index for proper Unicode handling
            if let Some(byte_pos) =
                crate::core::utf8::grapheme_byte_index(&line_text, self.cursor_x)
            {
                Some(line_start + byte_pos)
            } else {
                // cursor_x is past end of line, return end of line
                Some(line_start + line_text.len())
            }
        } else {
            Some(line_start)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_window_new() {
        let window = Window::new(WindowId(1), BufferId(1), 4);
        assert_eq!(window.cursor_x, 0);
        assert_eq!(window.cursor_y, 0);
        assert_eq!(window.scroll_offset, 0);
        assert_eq!(window.width, 80);
        assert_eq!(window.height, 24);
        assert_eq!(window.tab_width, 4);
    }

    #[test]
    fn test_window_set_dimensions() {
        let mut window = Window::new(WindowId(1), BufferId(1), 4);
        window.set_dimensions(120, 40);
        assert_eq!(window.width, 120);
        assert_eq!(window.height, 40);
    }

    #[test]
    fn test_window_beginning_of_line() {
        let mut window = Window::new(WindowId(1), BufferId(1), 4);
        let buffer = Buffer::new();
        window.cursor_x = 5;
        window.beginning_of_line(&buffer);
        assert_eq!(window.cursor_x, 0);
    }
}
