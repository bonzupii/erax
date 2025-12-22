use crate::core::app::EditorApp;
use crate::core::buffer::Buffer;
/// Buffer info and introspection commands
use crate::core::command::Command;
use crate::core::dispatcher::DispatchResult;
use crate::core::id::BufferId;

/// Create a new empty buffer
#[derive(Clone)]
pub struct NewBuffer;

impl Command for NewBuffer {
    fn execute(&self, app: &mut EditorApp, _count: usize) -> DispatchResult {
        let buffer = Buffer::new();
        // Manually allocate buffer ID since alloc_buffer_id is private
        let buffer_id = BufferId(app.next_buffer_id);
        app.next_buffer_id += 1;

        app.buffers.insert(buffer_id, buffer);

        // Switch the active window to the new buffer
        if let Some(window) = app.windows.get_mut(&app.active_window) {
            window.buffer_id = buffer_id;
        }

        DispatchResult::Success
    }
}

/// Switch to the next buffer in the list
#[derive(Clone)]
pub struct NextBuffer;

impl Command for NextBuffer {
    fn execute(&self, app: &mut EditorApp, count: usize) -> DispatchResult {
        if app.buffers.len() <= 1 {
            return DispatchResult::Success;
        }

        // Get current buffer id
        let current_buffer_id = if let Some(window) = app.windows.get(&app.active_window) {
            window.buffer_id
        } else {
            return DispatchResult::Success;
        };

        // Get ordered list of buffer ids
        let mut buffer_ids: Vec<BufferId> = app.buffers.keys().cloned().collect();
        buffer_ids.sort_by_key(|id| id.0); // Ensure deterministic order

        // Find current position and move forward
        if let Some(current_pos) = buffer_ids.iter().position(|id| *id == current_buffer_id) {
            let next_pos = (current_pos + count) % buffer_ids.len();
            let next_buffer_id = buffer_ids[next_pos];

            if let Some(window) = app.windows.get_mut(&app.active_window) {
                window.buffer_id = next_buffer_id;
                window.cursor_y = 0; // Reset to top
                window.cursor_x = 0;
                if let Some(buffer) = app.buffers.get(&next_buffer_id) {
                    window.update_visual_cursor(buffer);
                    window.ensure_cursor_visible(buffer);
                }
            }
        }

        DispatchResult::Success
    }
}

/// Switch to the previous buffer in the list
#[derive(Clone)]
pub struct PreviousBuffer;

impl Command for PreviousBuffer {
    fn execute(&self, app: &mut EditorApp, count: usize) -> DispatchResult {
        if app.buffers.len() <= 1 {
            return DispatchResult::Success;
        }

        // Get current buffer id
        let current_buffer_id = if let Some(window) = app.windows.get(&app.active_window) {
            window.buffer_id
        } else {
            return DispatchResult::Success;
        };

        // Get ordered list of buffer ids
        let mut buffer_ids: Vec<BufferId> = app.buffers.keys().cloned().collect();
        buffer_ids.sort_by_key(|id| id.0); // Ensure deterministic order

        // Find current position and move backward
        if let Some(current_pos) = buffer_ids.iter().position(|id| *id == current_buffer_id) {
            let prev_pos =
                (current_pos + buffer_ids.len() - (count % buffer_ids.len())) % buffer_ids.len();
            let prev_buffer_id = buffer_ids[prev_pos];

            if let Some(window) = app.windows.get_mut(&app.active_window) {
                window.buffer_id = prev_buffer_id;
                window.cursor_y = 0; // Reset to top
                window.cursor_x = 0;
                if let Some(buffer) = app.buffers.get(&prev_buffer_id) {
                    window.update_visual_cursor(buffer);
                    window.ensure_cursor_visible(buffer);
                }
            }
        }

        DispatchResult::Success
    }
}

/// Display buffer statistics
#[derive(Clone)]
pub struct BufferInfo;

impl Command for BufferInfo {
    fn execute(&self, app: &mut EditorApp, _count: usize) -> DispatchResult {
        if let Some(buffer) = app.active_buffer() {
            let is_empty = buffer.is_empty();
            let line_count = buffer.line_count();
            let total_bytes = buffer.len();
            eprintln!(
                "Buffer: {} lines, {} bytes, empty: {}",
                line_count, total_bytes, is_empty
            );
        }
        DispatchResult::Success
    }
}

/// Display cursor position info
#[derive(Clone)]
pub struct WhatCursorPosition;

impl Command for WhatCursorPosition {
    fn execute(&self, app: &mut EditorApp, _count: usize) -> DispatchResult {
        let active_window_id = app.active_window;
        if let Some(window) = app.windows.get(&active_window_id) {
            if let Some(buffer) = app.buffers.get(&window.buffer_id) {
                let win_id = window.id.0;
                let win_width = window.width;

                if let Some(byte_off) = window.get_byte_offset(buffer) {
                    let line = buffer.byte_to_line(byte_off);
                    let char_at_cursor = buffer.char_at(byte_off);
                    eprintln!(
                        "Window {}, width {}. Byte {}, Line {}, Char {:?}",
                        win_id, win_width, byte_off, line, char_at_cursor
                    );
                }
            }
        }
        DispatchResult::Success
    }
}

/// Count words in entire buffer
#[derive(Clone)]
pub struct CountWords;

impl Command for CountWords {
    fn execute(&self, app: &mut EditorApp, _count: usize) -> DispatchResult {
        if let Some(buffer) = app.active_buffer() {
            let text = buffer.to_string();
            let word_count = text.split_whitespace().count();
            let line_count = buffer.line_count();
            let char_count = text.len();
            DispatchResult::Info(format!(
                "Words: {}, Lines: {}, Chars: {}",
                word_count, line_count, char_count
            ))
        } else {
            DispatchResult::Success
        }
    }
}

/// Go to byte offset (prompts for byte number)
#[derive(Clone)]
pub struct GotoByte;

impl Command for GotoByte {
    fn execute(&self, _app: &mut EditorApp, _count: usize) -> DispatchResult {
        // Return NeedsInput to prompt user for byte offset
        DispatchResult::NeedsInput {
            prompt: "Go to byte: ".to_string(),
            action: crate::core::dispatcher::InputAction::GotoLine, // Reuse GotoLine action handler
        }
    }
}

/// List all buffers (uEmacs compatible)
#[derive(Clone)]
pub struct ListBuffers;

impl Command for ListBuffers {
    fn execute(&self, app: &mut EditorApp, _count: usize) -> DispatchResult {
        let mut buffer_list = String::new();
        buffer_list.push_str("Buffer List:\n");
        buffer_list.push_str("------------\n");

        for (buffer_id, buffer) in &app.buffers {
            let active_marker = if let Some(active_window) = app.windows.get(&app.active_window) {
                if active_window.buffer_id == *buffer_id {
                    "*"
                } else {
                    " "
                }
            } else {
                " "
            };

            let modified_marker = if buffer.modified { "*" } else { " " };

            let filename_str = buffer
                .filename
                .as_ref()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_else(|| "untitled".to_string());

            buffer_list.push_str(&format!(
                "{} {} {}\n",
                active_marker, modified_marker, filename_str
            ));
        }

        // In a real implementation, this would create a new buffer with the list
        // For now, we'll just return the info
        DispatchResult::Info(buffer_list)
    }
}

/// Display line, column and byte position (uEmacs show-position)
#[derive(Clone)]
pub struct ShowPosition;

impl Command for ShowPosition {
    fn execute(&self, app: &mut EditorApp, _count: usize) -> DispatchResult {
        let active_window_id = app.active_window;
        if let Some(window) = app.windows.get(&active_window_id) {
            if let Some(buffer) = app.buffers.get(&window.buffer_id) {
                if let Some(byte_off) = window.get_byte_offset(buffer) {
                    let line = buffer.byte_to_line(byte_off);
                    let total_bytes = buffer.len();
                    let percent = if total_bytes > 0 {
                        (byte_off * 100) / total_bytes
                    } else {
                        100
                    };
                    return DispatchResult::Info(format!(
                        "Line {}, Col {}, Byte {}/{} ({}%)",
                        line + 1,
                        window.cursor_x + 1,
                        byte_off,
                        total_bytes,
                        percent
                    ));
                }
            }
        }
        DispatchResult::Success
    }
}
