//! Buffer-management commands (list, switch, kill).
use crate::core::app::EditorApp;
use crate::core::buffer::Buffer;
/// Buffer info and introspection commands
use crate::core::dispatcher::DispatchResult;
use crate::core::id::BufferId;

pub fn new_buffer(app: &mut EditorApp, _count: usize) -> DispatchResult {
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

pub fn next_buffer(app: &mut EditorApp, count: usize) -> DispatchResult {
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

pub fn previous_buffer(app: &mut EditorApp, count: usize) -> DispatchResult {
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

pub fn buffer_info(app: &mut EditorApp, _count: usize) -> DispatchResult {
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

pub fn what_cursor_position(app: &mut EditorApp, _count: usize) -> DispatchResult {
        let active_window_id = app.active_window;
        if let Some(window) = app.windows.get(&active_window_id)
            && let Some(buffer) = app.buffers.get(&window.buffer_id)
            && let Some(byte_off) = window.get_byte_offset(buffer) {
                let line = buffer.byte_to_line(byte_off);
                let char_at_cursor = buffer.char_at(byte_off);
                return DispatchResult::Info(format!(
                    "Byte {byte_off}, Line {line}, Char {char_at_cursor:?}"
                ));
            }
        DispatchResult::Success
    }

pub fn count_words(app: &mut EditorApp, _count: usize) -> DispatchResult {
        if let Some(buffer) = app.active_buffer() {
            let text = buffer.content_string();
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

pub fn goto_byte(_app: &mut EditorApp, _count: usize) -> DispatchResult {
        // Return NeedsInput to prompt user for byte offset
        DispatchResult::NeedsInput {
            prompt: "Go to byte: ".to_string(),
            action: crate::core::dispatcher::InputAction::GotoLine, // Reuse GotoLine action handler
        }
    }

pub fn list_buffers(app: &mut EditorApp, _count: usize) -> DispatchResult {
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

            let filename_str = match buffer
                .filename
                .as_ref()
                .map(|p| p.to_string_lossy().to_string())
            {
                Some(s) => s,
                None => "untitled".to_string(),
            };

            buffer_list.push_str(&format!(
                "{} {} {}\n",
                active_marker, modified_marker, filename_str
            ));
        }

        // In a real implementation, this would create a new buffer with the list
        // For now, we'll just return the info
        DispatchResult::Info(buffer_list)
    }

pub fn show_position(app: &mut EditorApp, _count: usize) -> DispatchResult {
        let active_window_id = app.active_window;
        if let Some(window) = app.windows.get(&active_window_id)
            && let Some(buffer) = app.buffers.get(&window.buffer_id)
                && let Some(byte_off) = window.get_byte_offset(buffer) {
                    let line = buffer.byte_to_line(byte_off);
                    let total_bytes = buffer.len();
                    let percent = (byte_off * 100).checked_div(total_bytes).unwrap_or(0);
                    return DispatchResult::Info(format!(
                        "Line {}, Col {}, Byte {}/{} ({}%)",
                        line + 1,
                        window.cursor_x + 1,
                        byte_off,
                        total_bytes,
                        percent
                    ));
                }
        DispatchResult::Success
    }

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::command::Command;
    use crate::core::dispatcher::dispatch;

    /// `what-cursor-position` reports through the editor, not stderr: the
    /// old body `eprintln!`d the answer where no user could see it.
    #[test]
    fn what_cursor_position_surfaces_info_in_editor() {
        let mut app = EditorApp::new();
        if let Some(buffer) = app.active_buffer_mut() {
            buffer.insert(0, "hello\n");
        }
        let result = dispatch(&mut app, Command::WhatCursorPosition, 1);
        match result {
            DispatchResult::Info(message) => {
                assert!(
                    message.contains("Byte 0") && message.contains("Line 0"),
                    "unexpected position report: {message:?}"
                );
            }
            other => panic!("expected Info, got {other:?}"),
        }
    }
}
