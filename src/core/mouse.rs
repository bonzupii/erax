//! Mouse Event Handling
//! Handles mouse interactions including clicks, drags, double/triple clicks, and scrolling.
//! Translates screen coordinates to buffer positions and manages selection state.

use crate::core::buffer::Buffer;
use crate::core::selection::SelectionMode;
use crate::core::window::Window;

/// Mouse button identifier
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MouseButton {
    Left,
    Right,
    Middle,
}

/// Scroll direction
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScrollDirection {
    Up,
    Down,
    Left,
    Right,
}

/// Mouse event type
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MouseEvent {
    /// Single click (col, row, button)
    Click(usize, usize, MouseButton),
    /// Mouse drag (start_col, start_row, end_col, end_row, button)
    Drag(usize, usize, usize, usize, MouseButton),
    /// Double click (col, row)
    DoubleClick(usize, usize),
    /// Triple click (col, row)
    TripleClick(usize, usize),
    /// Scroll (amount, direction)
    Scroll(usize, ScrollDirection),
}

/// Handles mouse logic and state translation
#[derive(Debug, Default)]
pub struct MouseHandler;

impl MouseHandler {
    /// Create a new mouse handler
    pub fn new() -> Self {
        Self
    }

    /// Process a mouse event
    ///
    /// Returns true if the event resulted in a state change (requiring redraw).
    pub fn handle_event(&self, event: &MouseEvent, window: &mut Window, buffer: &Buffer) -> bool {
        match event {
            MouseEvent::Click(x, y, button) => self.handle_click(*x, *y, *button, window, buffer),
            MouseEvent::Drag(start_x, start_y, end_x, end_y, _button) => {
                self.handle_drag(*start_x, *start_y, *end_x, *end_y, window, buffer)
            }
            MouseEvent::DoubleClick(x, y) => self.handle_double_click(*x, *y, window, buffer),
            MouseEvent::TripleClick(x, y) => self.handle_triple_click(*x, *y, window, buffer),
            MouseEvent::Scroll(amount, direction) => {
                self.handle_scroll(*amount, *direction, window, buffer)
            }
        }
    }

    /// Convert screen coordinates to buffer position (col, row)
    ///
    /// Takes into account scrolling and visual width (tabs, wide chars).
    pub fn screen_to_buffer_pos(
        &self,
        screen_x: usize,
        screen_y: usize,
        window: &Window,
        buffer: &Buffer,
    ) -> (usize, usize) {
        // Calculate absolute line index
        let buffer_y = window.scroll_offset + screen_y;
        let line_count = buffer.line_count();

        // If clicking below the last line, snap to the last line
        if buffer_y >= line_count {
            if line_count == 0 {
                return (0, 0);
            }
            let last_line_idx = line_count - 1;
            let last_col = match buffer.line(last_line_idx) {
                Some(text) => crate::core::utf8::grapheme_count(&text),
                None => 0,
            };
            return (last_col, last_line_idx);
        }

        // Calculate column index
        let visual_x = window.scroll_x + screen_x;
        let line_text = match buffer.line(buffer_y) {
            Some(text) => text,
            None => return (0, buffer_y),
        };

        // Use utf8 helper to convert visual X (accounting for tabs) to grapheme index
        let buffer_x =
            crate::core::utf8::grapheme_index_from_visual_x(&line_text, visual_x, window.tab_width);

        (buffer_x, buffer_y)
    }

    /// Handle single click
    fn handle_click(
        &self,
        x: usize,
        y: usize,
        button: MouseButton,
        window: &mut Window,
        buffer: &Buffer,
    ) -> bool {
        match button {
            MouseButton::Left => {
                let (col, row) = self.screen_to_buffer_pos(x, y, window, buffer);

                // Move cursor
                window.cursor_x = col;
                window.cursor_y = row;
                window.update_visual_cursor(buffer);
                window.ensure_cursor_visible(buffer);

                // Reset selection to point
                if let Some(byte_pos) = window.get_byte_offset(buffer) {
                    window
                        .selection_manager
                        .start_selection(byte_pos, SelectionMode::None);
                }
                true
            }
            _ => false, // Right/Middle click handling can be added here
        }
    }

    /// Handle mouse drag (selection)
    fn handle_drag(
        &self,
        _start_x: usize,
        _start_y: usize,
        end_x: usize,
        end_y: usize,
        window: &mut Window,
        buffer: &Buffer,
    ) -> bool {
        let (col, row) = self.screen_to_buffer_pos(end_x, end_y, window, buffer);

        // Update cursor to current drag position
        window.cursor_x = col;
        window.cursor_y = row;
        window.update_visual_cursor(buffer);
        window.ensure_cursor_visible(buffer);

        // Extend selection
        if let Some(byte_pos) = window.get_byte_offset(buffer) {
            // Upgrade to character selection if we were in None mode (just started dragging)
            if window.selection_manager.mode == SelectionMode::None {
                window.selection_manager.mode = SelectionMode::Character;
            }
            window.selection_manager.extend_selection(byte_pos, buffer);
        }

        true
    }

    /// Handle double click (select word)
    fn handle_double_click(
        &self,
        x: usize,
        y: usize,
        window: &mut Window,
        buffer: &Buffer,
    ) -> bool {
        // Move cursor to click position
        let (col, row) = self.screen_to_buffer_pos(x, y, window, buffer);
        window.cursor_x = col;
        window.cursor_y = row;

        if let Some(line) = buffer.line(row) {
            use crate::core::utf8::GraphemeIterator;

            let graphemes: Vec<&str> = GraphemeIterator::new(&line).collect();
            if col >= graphemes.len() {
                // Clicked past end of line, maybe select newline or nothing
                return true;
            }

            // Define word boundary: alphanumeric + underscore vs everything else
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

            // Calculate byte offsets
            let line_start_byte = match buffer.line_to_byte(row) {
                Some(b) => b,
                None => 0,
            };

            let start_byte_offset = graphemes[0..start].iter().map(|g| g.len()).sum::<usize>();
            let len_byte_offset = graphemes[start..end].iter().map(|g| g.len()).sum::<usize>();

            let start_pos = line_start_byte + start_byte_offset;
            let end_pos = start_pos + len_byte_offset;

            // Set selection
            window
                .selection_manager
                .start_selection(start_pos, SelectionMode::Word);
            window.selection_manager.extend_selection(end_pos, buffer);

            // Move cursor to end of word
            window.cursor_x = end;
            window.update_visual_cursor(buffer);
        }

        true
    }

    /// Handle triple click (select line)
    fn handle_triple_click(
        &self,
        x: usize,
        y: usize,
        window: &mut Window,
        buffer: &Buffer,
    ) -> bool {
        let (_, row) = self.screen_to_buffer_pos(x, y, window, buffer);

        if let Some(start_pos) = buffer.line_to_byte(row) {
            window
                .selection_manager
                .start_selection(start_pos, SelectionMode::Line);
            window.selection_manager.extend_selection(start_pos, buffer);

            // Move cursor to end of line
            window.cursor_y = row;
            window.end_of_line(buffer);
        }

        true
    }

    /// Handle scrolling
    fn handle_scroll(
        &self,
        amount: usize,
        direction: ScrollDirection,
        window: &mut Window,
        buffer: &Buffer,
    ) -> bool {
        match direction {
            ScrollDirection::Up => {
                window.scroll_by(-(amount as isize), buffer);
            }
            ScrollDirection::Down => {
                window.scroll_by(amount as isize, buffer);
            }
            ScrollDirection::Left => {
                window.scroll_x = window.scroll_x.saturating_sub(amount);
            }
            ScrollDirection::Right => {
                window.scroll_x = window.scroll_x + amount;
            }
        }

        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::id::{BufferId, WindowId};

    fn setup_env() -> (Window, Buffer, MouseHandler) {
        let mut buffer = Buffer::new();
        // Insert some content:
        // Line 0: "Hello" (5 chars)
        // Line 1: "World" (5 chars)
        buffer.insert(0, "Hello\nWorld");

        let window = Window::new(WindowId(1), BufferId(1), 4);
        let handler = MouseHandler::new();

        (window, buffer, handler)
    }

    #[test]
    fn test_mouse_click() {
        let (mut window, buffer, handler) = setup_env();

        // Click on 'W' in "World" (Line 1, Col 0)
        let event = MouseEvent::Click(0, 1, MouseButton::Left);
        handler.handle_event(&event, &mut window, &buffer);

        assert_eq!(window.cursor_y, 1);
        assert_eq!(window.cursor_x, 0);
        assert!(!window.selection_manager.has_selection()); // Click clears selection
    }

    #[test]
    fn test_mouse_drag_selection() {
        let (mut window, buffer, handler) = setup_env();

        // Click at (0,0) first
        handler.handle_event(
            &MouseEvent::Click(0, 0, MouseButton::Left),
            &mut window,
            &buffer,
        );

        // Drag to (3,0) -> "Hel"
        let event = MouseEvent::Drag(0, 0, 3, 0, MouseButton::Left);
        handler.handle_event(&event, &mut window, &buffer);

        assert_eq!(window.cursor_x, 3);
        assert!(window.selection_manager.has_selection());
        if let Some(sel) = window.selection_manager.get_selection() {
            assert_eq!(sel.start(), 0);
            assert_eq!(sel.end(), 3);
        } else {
            panic!("Expected selection");
        }
    }

    #[test]
    fn test_double_click_word() {
        let (mut window, buffer, handler) = setup_env();

        // Double click on 'e' in "Hello" (0, 0 is H, 1, 0 is e)
        let event = MouseEvent::DoubleClick(1, 0);
        handler.handle_event(&event, &mut window, &buffer);

        assert!(window.selection_manager.has_selection());
        if let Some(sel) = window.selection_manager.get_selection() {
            // "Hello" is 5 bytes. Start 0, End 5.
            assert_eq!(sel.start(), 0);
            assert_eq!(sel.end(), 5);
            assert_eq!(window.selection_manager.mode, SelectionMode::Word);
        } else {
            panic!("Expected selection");
        }
    }

    #[test]
    fn test_triple_click_line() {
        let (mut window, buffer, handler) = setup_env();

        // Triple click on line 0
        let event = MouseEvent::TripleClick(2, 0);
        handler.handle_event(&event, &mut window, &buffer);

        assert!(window.selection_manager.has_selection());
        if let Some(sel) = window.selection_manager.get_selection() {
            // "Hello\n" is 6 bytes
            assert_eq!(sel.start(), 0);
            assert_eq!(sel.end(), 6);
            assert_eq!(window.selection_manager.mode, SelectionMode::Line);
        } else {
            panic!("Expected selection");
        }
    }

    #[test]
    fn test_scroll() {
        let (mut window, buffer, handler) = setup_env();

        // Setup window height 1 to force scrolling
        window.set_dimensions(80, 1);

        // Scroll down
        let event = MouseEvent::Scroll(1, ScrollDirection::Down);
        handler.handle_event(&event, &mut window, &buffer);

        assert_eq!(window.scroll_offset, 1);
        // Cursor should move if it was at 0
        assert_eq!(window.cursor_y, 1);
    }
}
