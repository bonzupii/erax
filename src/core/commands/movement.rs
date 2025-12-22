use crate::core::app::EditorApp;
/// Cursor movement commands
use crate::core::command::Command;
use crate::core::dispatcher::DispatchResult;
use crate::core::window::Window;
use crate::core::buffer::Buffer;

/// Helper to update selection after cursor movement
fn update_selection(window: &mut Window, buffer: &Buffer) {
    // If a selection is active (even if empty/point), extend it to the new cursor position
    if window.selection_manager.primary.is_some() {
        if let Some(byte_pos) = window.get_byte_offset(buffer) {
            window.selection_manager.extend_selection(byte_pos, buffer);
        }
    }
}

/// Move cursor forward by character(s)
#[derive(Clone)]
pub struct ForwardChar;

impl Command for ForwardChar {
    fn execute(&self, app: &mut EditorApp, count: usize) -> DispatchResult {
        let active_window_id = app.active_window;
        if let Some(window) = app.windows.get_mut(&active_window_id) {
            if let Some(buffer) = app.buffers.get(&window.buffer_id) {
                for _ in 0..count {
                    window.move_forward(buffer);
                    update_selection(window, buffer);
                }
                window.ensure_cursor_visible(buffer);
            }
        }
        DispatchResult::Success
    }
}

/// Move cursor backward by character(s)
#[derive(Clone)]
pub struct BackwardChar;

impl Command for BackwardChar {
    fn execute(&self, app: &mut EditorApp, count: usize) -> DispatchResult {
        let active_window_id = app.active_window;
        if let Some(window) = app.windows.get_mut(&active_window_id) {
            if let Some(buffer) = app.buffers.get(&window.buffer_id) {
                for _ in 0..count {
                    window.move_backward(buffer);
                    update_selection(window, buffer);
                }
                window.ensure_cursor_visible(buffer);
            }
        }
        DispatchResult::Success
    }
}

/// Move cursor down by line(s)
#[derive(Clone)]
pub struct NextLine;

impl Command for NextLine {
    fn execute(&self, app: &mut EditorApp, count: usize) -> DispatchResult {
        let active_window_id = app.active_window;
        let buffer_id = app.windows.get(&active_window_id).map(|w| w.buffer_id);

        if let Some(buffer_id) = buffer_id {
            for _ in 0..count {
                if let Some(window) = app.windows.get_mut(&active_window_id) {
                    if let Some(buffer) = app.buffers.get(&buffer_id) {
                        window.move_down(buffer);
                        update_selection(window, buffer);
                        window.ensure_cursor_visible(buffer);
                    }
                }
            }
        }
        DispatchResult::Success
    }
}

/// Move cursor up by line(s)
#[derive(Clone)]
pub struct PreviousLine;

impl Command for PreviousLine {
    fn execute(&self, app: &mut EditorApp, count: usize) -> DispatchResult {
        let active_window_id = app.active_window;
        let buffer_id = app.windows.get(&active_window_id).map(|w| w.buffer_id);

        if let Some(buffer_id) = buffer_id {
            for _ in 0..count {
                if let Some(window) = app.windows.get_mut(&active_window_id) {
                    if let Some(buffer) = app.buffers.get(&buffer_id) {
                        window.move_up(buffer);
                        update_selection(window, buffer);
                        window.ensure_cursor_visible(buffer);
                    }
                }
            }
        }
        DispatchResult::Success
    }
}

/// Move cursor to beginning of line
#[derive(Clone)]
pub struct BeginningOfLine;

impl Command for BeginningOfLine {
    fn execute(&self, app: &mut EditorApp, _count: usize) -> DispatchResult {
        let active_window_id = app.active_window;
        let buffer_id = app.windows.get(&active_window_id).map(|w| w.buffer_id);

        if let Some(buffer_id) = buffer_id {
            if let Some(window) = app.windows.get_mut(&active_window_id) {
                if let Some(buffer) = app.buffers.get(&buffer_id) {
                    window.beginning_of_line(buffer);
                    update_selection(window, buffer);
                    window.ensure_cursor_visible(buffer);
                }
            }
        }
        DispatchResult::Success
    }
}

/// Move cursor to end of line
#[derive(Clone)]
pub struct EndOfLine;

impl Command for EndOfLine {
    fn execute(&self, app: &mut EditorApp, _count: usize) -> DispatchResult {
        let active_window_id = app.active_window;
        let buffer_id = app.windows.get(&active_window_id).map(|w| w.buffer_id);

        if let Some(buffer_id) = buffer_id {
            if let Some(window) = app.windows.get_mut(&active_window_id) {
                if let Some(buffer) = app.buffers.get(&buffer_id) {
                    window.end_of_line(buffer);
                    update_selection(window, buffer);
                    window.ensure_cursor_visible(buffer);
                }
            }
        }
        DispatchResult::Success
    }
}

/// Move cursor to beginning of buffer
#[derive(Clone)]
pub struct BeginningOfBuffer;

impl Command for BeginningOfBuffer {
    fn execute(&self, app: &mut EditorApp, _count: usize) -> DispatchResult {
        let active_window_id = app.active_window;
        let buffer_id = app.windows.get(&active_window_id).map(|w| w.buffer_id);

        if let Some(buffer_id) = buffer_id {
            if let Some(window) = app.windows.get_mut(&active_window_id) {
                if let Some(buffer) = app.buffers.get(&buffer_id) {
                    window.beginning_of_buffer(buffer);
                    update_selection(window, buffer);
                    window.ensure_cursor_visible(buffer);
                }
            }
        }
        DispatchResult::Success
    }
}

/// Move cursor to end of buffer
#[derive(Clone)]
pub struct EndOfBuffer;

impl Command for EndOfBuffer {
    fn execute(&self, app: &mut EditorApp, _count: usize) -> DispatchResult {
        let active_window_id = app.active_window;
        let buffer_id = app.windows.get(&active_window_id).map(|w| w.buffer_id);

        if let Some(buffer_id) = buffer_id {
            if let Some(window) = app.windows.get_mut(&active_window_id) {
                if let Some(buffer) = app.buffers.get(&buffer_id) {
                    window.end_of_buffer(buffer);
                    update_selection(window, buffer);
                    window.ensure_cursor_visible(buffer);
                }
            }
        }
        DispatchResult::Success
    }
}

/// Move cursor forward by word(s)
#[derive(Clone)]
pub struct ForwardWord;

impl Command for ForwardWord {
    fn execute(&self, app: &mut EditorApp, count: usize) -> DispatchResult {
        let active_window_id = app.active_window;
        let buffer_id = app.windows.get(&active_window_id).map(|w| w.buffer_id);

        if let Some(buffer_id) = buffer_id {
            for _ in 0..count {
                if let Some(window) = app.windows.get_mut(&active_window_id) {
                    if let Some(buffer) = app.buffers.get(&buffer_id) {
                        window.forward_word(buffer);
                        update_selection(window, buffer);
                        window.ensure_cursor_visible(buffer);
                    }
                }
            }
        }
        DispatchResult::Success
    }
}

/// Move cursor backward by word(s)
#[derive(Clone)]
pub struct BackwardWord;

impl Command for BackwardWord {
    fn execute(&self, app: &mut EditorApp, count: usize) -> DispatchResult {
        let active_window_id = app.active_window;
        let buffer_id = app.windows.get(&active_window_id).map(|w| w.buffer_id);

        if let Some(buffer_id) = buffer_id {
            for _ in 0..count {
                if let Some(window) = app.windows.get_mut(&active_window_id) {
                    if let Some(buffer) = app.buffers.get(&buffer_id) {
                        window.backward_word(buffer);
                        update_selection(window, buffer);
                        window.ensure_cursor_visible(buffer);
                    }
                }
            }
        }
        DispatchResult::Success
    }
}

/// Scroll down by page
#[derive(Clone)]
pub struct ForwardPage;

impl Command for ForwardPage {
    fn execute(&self, app: &mut EditorApp, _count: usize) -> DispatchResult {
        let active_window_id = app.active_window;
        let buffer_id = app.windows.get(&active_window_id).map(|w| w.buffer_id);

        if let Some(buffer_id) = buffer_id {
            if let Some(buffer) = app.buffers.get(&buffer_id) {
                if let Some(window) = app.windows.get_mut(&active_window_id) {
                    // Move cursor down by page_size lines
                    let page_size = window.height.saturating_sub(2).max(1);
                    for _ in 0..page_size {
                        window.move_down(buffer);
                    }
                    update_selection(window, buffer);
                    // Edge-triggered scroll will handle viewport adjustment
                    window.ensure_cursor_visible(buffer);
                }
            }
        }
        DispatchResult::Success
    }
}

/// Scroll up by page
#[derive(Clone)]
pub struct BackwardPage;

impl Command for BackwardPage {
    fn execute(&self, app: &mut EditorApp, _count: usize) -> DispatchResult {
        let active_window_id = app.active_window;
        let buffer_id = app.windows.get(&active_window_id).map(|w| w.buffer_id);

        if let Some(buffer_id) = buffer_id {
            if let Some(buffer) = app.buffers.get(&buffer_id) {
                if let Some(window) = app.windows.get_mut(&active_window_id) {
                    // Move cursor up by page_size lines
                    let page_size = window.height.saturating_sub(2).max(1);
                    for _ in 0..page_size {
                        window.move_up(buffer);
                    }
                    update_selection(window, buffer);
                    // Edge-triggered scroll will handle viewport adjustment
                    window.ensure_cursor_visible(buffer);
                }
            }
        }
        DispatchResult::Success
    }
}

/// Move cursor forward by paragraph(s)
#[derive(Clone)]
pub struct ForwardParagraph;

impl Command for ForwardParagraph {
    fn execute(&self, app: &mut EditorApp, count: usize) -> DispatchResult {
        let active_window_id = app.active_window;
        let buffer_id = app.windows.get(&active_window_id).map(|w| w.buffer_id);

        if let Some(buffer_id) = buffer_id {
            for _ in 0..count {
                if let Some(window) = app.windows.get_mut(&active_window_id) {
                    if let Some(buffer) = app.buffers.get(&buffer_id) {
                        // uEmacs paragraph movement: move to next paragraph boundary
                        // A paragraph is defined as text separated by blank lines
                        let current_line = window.cursor_y;
                        let total_lines = buffer.line_count();

                        // Find next paragraph boundary
                        let mut target_line = current_line + 1;
                        while target_line < total_lines {
                            if let Some(line) = buffer.line(target_line) {
                                if line.trim().is_empty() {
                                    // Found blank line, move to next non-blank line
                                    let mut next_paragraph = target_line + 1;
                                    while next_paragraph < total_lines {
                                        if let Some(next_line) = buffer.line(next_paragraph) {
                                            if !next_line.trim().is_empty() {
                                                break;
                                            }
                                        }
                                        next_paragraph += 1;
                                    }

                                    if next_paragraph < total_lines {
                                        window.cursor_y = next_paragraph;
                                        window.cursor_x = 0;
                                        window.update_visual_cursor(buffer);
                                        break;
                                    } else {
                                        // End of buffer
                                        window.cursor_y = total_lines.saturating_sub(1);
                                        window.cursor_x = 0;
                                        window.update_visual_cursor(buffer);
                                        break;
                                    }
                                }
                            }
                            target_line += 1;
                        }

                        // If we didn't find a blank line, go to end of buffer
                        if target_line >= total_lines {
                            window.cursor_y = total_lines.saturating_sub(1);
                            window.cursor_x = 0;
                            window.update_visual_cursor(buffer);
                        }
                        update_selection(window, buffer);
                    }
                }
            }
        }
        DispatchResult::Success
    }
}

/// Move cursor backward by paragraph(s)
#[derive(Clone)]
pub struct BackwardParagraph;

impl Command for BackwardParagraph {
    fn execute(&self, app: &mut EditorApp, count: usize) -> DispatchResult {
        let active_window_id = app.active_window;
        let buffer_id = app.windows.get(&active_window_id).map(|w| w.buffer_id);

        if let Some(buffer_id) = buffer_id {
            for _ in 0..count {
                if let Some(window) = app.windows.get_mut(&active_window_id) {
                    if let Some(buffer) = app.buffers.get(&buffer_id) {
                        let mut y = window.cursor_y;

                        // 1. If at start of paragraph (or in blank lines), move back to end of previous paragraph
                        // Check if we need to jump back because we are at the top of a paragraph
                        let mut at_start_of_para = false;
                        if y > 0 {
                            if let Some(line) = buffer.line(y) {
                                if !line.trim().is_empty() {
                                    if let Some(prev) = buffer.line(y - 1) {
                                        if prev.trim().is_empty() {
                                            at_start_of_para = true;
                                        }
                                    }
                                }
                            }
                        }

                        if at_start_of_para {
                            y = y.saturating_sub(1);
                        }

                        // 2. Skip over any blank lines (separator) backwards
                        while y > 0 {
                            if let Some(line) = buffer.line(y) {
                                if !line.trim().is_empty() {
                                    break;
                                }
                            }
                            y -= 1;
                        }

                        // 3. Scan backward until we find a blank line or SOB (Start of Buffer)
                        // The line AFTER the blank line is the start of the paragraph
                        while y > 0 {
                            if let Some(prev) = buffer.line(y - 1) {
                                if prev.trim().is_empty() {
                                    break;
                                }
                            }
                            y -= 1;
                        }

                        window.cursor_y = y;
                        window.cursor_x = 0;
                        window.update_visual_cursor(buffer);
                        update_selection(window, buffer);
                        window.ensure_cursor_visible(buffer);
                    }
                }
            }
        }
        DispatchResult::Success
    }
}
