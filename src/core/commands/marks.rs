use crate::core::app::EditorApp;
/// Mark and region manipulation commands
use crate::core::command::Command;
use crate::core::dispatcher::DispatchResult;

/// Set mark at current cursor position
#[derive(Clone)]
pub struct SetMark;

impl Command for SetMark {
    fn execute(&self, app: &mut EditorApp, _count: usize) -> DispatchResult {
        if let Some(window) = app.active_window_mut() {
            window.mark = Some((window.cursor_x, window.cursor_y));
            DispatchResult::Info("Mark set".to_string())
        } else {
            DispatchResult::Success
        }
    }
}

/// Swap cursor and mark positions
#[derive(Clone)]
pub struct ExchangePointAndMark;

impl Command for ExchangePointAndMark {
    fn execute(&self, app: &mut EditorApp, _count: usize) -> DispatchResult {
        let active_window_id = app.active_window;

        if let Some(window) = app.windows.get_mut(&active_window_id) {
            if let Some(mark) = window.mark {
                let current = (window.cursor_x, window.cursor_y);

                // Swap
                window.cursor_x = mark.0;
                window.cursor_y = mark.1;
                window.mark = Some(current);

                let buffer_id = window.buffer_id;

                if let Some(buffer) = app.buffers.get(&buffer_id) {
                    window.update_visual_cursor(buffer);
                    window.ensure_cursor_visible(buffer);
                }
                DispatchResult::Success
            } else {
                DispatchResult::Info("No mark set".to_string())
            }
        } else {
            DispatchResult::Info("No active window".to_string())
        }
    }
}

/// Kill text from cursor to mark
#[derive(Clone)]
pub struct KillRegion;

impl Command for KillRegion {
    fn execute(&self, app: &mut EditorApp, _count: usize) -> DispatchResult {
        let (start_byte, end_byte, text_to_kill, buffer_id) = {
            let active_window_id = app.active_window;
            if let Some(window) = app.windows.get(&active_window_id) {
                if let Some(buffer) = app.buffers.get(&window.buffer_id) {
                    if let Some((mx, my)) = window.mark {
                        let cursor_byte = match window.get_byte_offset(buffer) {
                            Some(b) => b,
                            None => 0,
                        };

                        // Calculate mark byte offset using temp window
                        let mut temp_win = window.clone();
                        temp_win.cursor_x = mx;
                        temp_win.cursor_y = my;
                        let mark_byte = match temp_win.get_byte_offset(buffer) {
                            Some(b) => b,
                            None => 0,
                        };

                        let (s, e) = if cursor_byte < mark_byte {
                            (cursor_byte, mark_byte)
                        } else {
                            (mark_byte, cursor_byte)
                        };

                        // Use Buffer's get_range_as_string method
                        let text = buffer.get_range_as_string(s, e - s);
                        (s, e, Some(text), Some(window.buffer_id))
                    } else {
                        return DispatchResult::Info("No mark set".to_string());
                    }
                } else {
                    return DispatchResult::Info("No active buffer".to_string());
                }
            } else {
                return DispatchResult::Info("No active window".to_string());
            }
        };

        if let (Some(text), Some(buf_id)) = (text_to_kill, buffer_id) {
            app.kill_ring.push(&text, false);
            app.reset_kill_flag();

            // Delete from buffer using Buffer's delete method
            if let Some(buffer) = app.buffers.get_mut(&buf_id) {
                buffer.delete(start_byte, end_byte - start_byte);
            }

            // Update window cursor to start of deleted region
            // Calculate line/col from byte position
            if let Some(buffer) = app.buffers.get(&buf_id) {
                let line = buffer.byte_to_line(start_byte);
                let active_window_id = app.active_window;

                if let Some(window) = app.windows.get_mut(&active_window_id) {
                    if window.buffer_id == buf_id {
                        window.cursor_y = line;
                        // Calculate cursor_x
                        if let Some(line_start) = buffer.line_to_byte(line) {
                            if let Some(line_text) = buffer.line(line) {
                                let byte_offset = start_byte.saturating_sub(line_start);
                                window.cursor_x = crate::core::utf8::byte_to_grapheme_col(
                                    &line_text,
                                    byte_offset,
                                );
                            }
                        }
                        window.update_visual_cursor(buffer);
                        window.ensure_cursor_visible(buffer);
                    }
                }
            }
            DispatchResult::Info("Region killed".to_string())
        } else {
            DispatchResult::Success
        }
    }
}

/// Copy region to kill ring without deleting
#[derive(Clone)]
pub struct CopyRegion;

impl Command for CopyRegion {
    fn execute(&self, app: &mut EditorApp, _count: usize) -> DispatchResult {
        let text_to_copy: Option<String> = {
            let window_opt = app.active_window_ref();
            let buffer_opt = app.active_buffer();

            if let (Some(window_ref), Some(buffer_ref)) = (window_opt, buffer_opt) {
                if let Some((mx, my)) = window_ref.mark {
                    let cursor_byte = match window_ref.get_byte_offset(buffer_ref) {
                        Some(b) => b,
                        None => 0,
                    };
                    let mut temp_win = window_ref.clone();
                    temp_win.cursor_x = mx;
                    temp_win.cursor_y = my;
                    let mark_byte = match temp_win.get_byte_offset(buffer_ref) {
                        Some(b) => b,
                        None => 0,
                    };

                    let (start_b, end_b) = if cursor_byte < mark_byte {
                        (cursor_byte, mark_byte)
                    } else {
                        (mark_byte, cursor_byte)
                    };

                    // Use Buffer's get_range_as_string method
                    Some(buffer_ref.get_range_as_string(start_b, end_b - start_b))
                } else {
                    return DispatchResult::Info("No mark set".to_string());
                }
            } else {
                return DispatchResult::Info("No active buffer or window".to_string());
            }
        };

        if let Some(text) = text_to_copy {
            app.kill_ring.push(&text, false);
            app.reset_kill_flag();
            DispatchResult::Info("Region copied".to_string())
        } else {
            DispatchResult::Success
        }
    }
}
