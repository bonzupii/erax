//! Mark commands (set, jump, exchange).
use crate::config::keys::{INPUT_MODEL, InputModel};
use crate::core::app::EditorApp;
/// Mark and region manipulation commands
use crate::core::dispatcher::DispatchResult;

pub fn set_mark(app: &mut EditorApp, _count: usize) -> DispatchResult {
        if let Some(window) = app.active_window_mut() {
            window.mark = Some((window.cursor_x, window.cursor_y));
            DispatchResult::Info("Mark set".to_string())
        } else {
            DispatchResult::Success
        }
    }

pub fn exchange_point_and_mark(app: &mut EditorApp, _count: usize) -> DispatchResult {
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

/// Kill the active region (cut), in the given input model.
///
/// Standard mode stores the killed text in the clipboard register;
/// Emacs/Vim store it in the kill-ring. The runtime model is
/// [`crate::config::keys::INPUT_MODEL`].
pub fn kill_region(app: &mut EditorApp, count: usize) -> DispatchResult {
    kill_region_in_model(app, count, INPUT_MODEL)
}

/// [`kill_region`] with an explicit input model.
///
/// Passing the compile-time [`crate::config::keys::INPUT_MODEL`] const
/// monomorphizes away the branch, so this is zero-cost; tests may pass any
/// [`InputModel`] to exercise model-specific paths.
pub fn kill_region_in_model(
        app: &mut EditorApp,
        _count: usize,
        model: InputModel,
) -> DispatchResult {
        let (start_byte, end_byte, text_to_kill, buffer_id) = {
            let active_window_id = app.active_window;
            if let Some(window) = app.windows.get(&active_window_id) {
                if let Some(buffer) = app.buffers.get(&window.buffer_id) {
                    if let Some((mx, my)) = window.mark {
                        let cursor_byte = window.get_byte_offset(buffer).unwrap_or_default();

                        let mark_byte =
                            window.get_byte_offset_at(buffer, mx, my).unwrap_or_default();

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
            // Standard mode uses the clipboard register; Emacs/Vim use the kill-ring.
            if model == InputModel::Standard {
                app.clipboard = Some(text);
            } else {
                app.kill_ring.push(&text, false);
                app.reset_kill_flag();
            }

            // Delete from buffer using Buffer's delete method
            if let Some(buffer) = app.buffers.get_mut(&buf_id) {
                buffer.delete(start_byte, end_byte - start_byte);
            }

            // Update window cursor to start of deleted region
            // Calculate line/col from byte position
            if let Some(buffer) = app.buffers.get(&buf_id) {
                let line = buffer.byte_to_line(start_byte);
                let active_window_id = app.active_window;

                if let Some(window) = app.windows.get_mut(&active_window_id)
                    && window.buffer_id == buf_id {
                        window.cursor_y = line;
                        // Calculate cursor_x
                        if let Some(line_start) = buffer.line_to_byte(line)
                            && let Some(line_text) = buffer.line(line) {
                                let byte_offset = start_byte.saturating_sub(line_start);
                                window.cursor_x = crate::core::utf8::byte_to_grapheme_col(
                                    &line_text,
                                    byte_offset,
                                );
                            }
                        window.update_visual_cursor(buffer);
                        window.ensure_cursor_visible(buffer);
                    }
            }
            DispatchResult::Info("Region killed".to_string())
        } else {
            DispatchResult::Success
        }
    }

/// Copy the active region, in the given input model.
///
/// Standard mode stores the copied text in the clipboard register;
/// Emacs/Vim store it in the kill-ring. The runtime model is
/// [`crate::config::keys::INPUT_MODEL`].
pub fn copy_region(app: &mut EditorApp, count: usize) -> DispatchResult {
    copy_region_in_model(app, count, INPUT_MODEL)
}

/// [`copy_region`] with an explicit input model.
///
/// Passing the compile-time [`crate::config::keys::INPUT_MODEL`] const
/// monomorphizes away the branch, so this is zero-cost; tests may pass any
/// [`InputModel`] to exercise model-specific paths.
pub fn copy_region_in_model(
        app: &mut EditorApp,
        _count: usize,
        model: InputModel,
) -> DispatchResult {
        let text_to_copy: Option<String> = {
            let window_opt = app.active_window_ref();
            let buffer_opt = app.active_buffer();

            if let (Some(window_ref), Some(buffer_ref)) = (window_opt, buffer_opt) {
                if let Some((mx, my)) = window_ref.mark {
                    let cursor_byte = window_ref.get_byte_offset(buffer_ref).unwrap_or_default();
                    let mark_byte = window_ref
                        .get_byte_offset_at(buffer_ref, mx, my)
                        .unwrap_or_default();

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
            // Standard mode uses the clipboard register; Emacs/Vim use the kill-ring.
            if model == InputModel::Standard {
                app.clipboard = Some(text);
            } else {
                app.kill_ring.push(&text, false);
                app.reset_kill_flag();
            }
            DispatchResult::Info("Region copied".to_string())
        } else {
            DispatchResult::Success
        }
    }

/// Ensure a selection anchor exists at the current cursor position.
/// Returns the window ID for the caller to move.
fn ensure_selection_anchor(app: &mut EditorApp) {
    let active_window_id = app.active_window;
    if let Some(window) = app.windows.get_mut(&active_window_id)
        && window.mark.is_none() {
            window.mark = Some((window.cursor_x, window.cursor_y));
        }
}

/// Extend the selection right by one character (Shift+Right).
pub fn extend_selection_right(app: &mut EditorApp, count: usize) -> DispatchResult {
    ensure_selection_anchor(app);
    crate::core::commands::movement::forward_character(app, count)
}

/// Extend the selection left by one character (Shift+Left).
pub fn extend_selection_left(app: &mut EditorApp, count: usize) -> DispatchResult {
    ensure_selection_anchor(app);
    crate::core::commands::movement::backward_character(app, count)
}

/// Extend the selection down by one line (Shift+Down).
pub fn extend_selection_down(app: &mut EditorApp, count: usize) -> DispatchResult {
    ensure_selection_anchor(app);
    crate::core::commands::movement::next_line(app, count)
}

/// Extend the selection up by one line (Shift+Up).
pub fn extend_selection_up(app: &mut EditorApp, count: usize) -> DispatchResult {
    ensure_selection_anchor(app);
    crate::core::commands::movement::previous_line(app, count)
}

/// Extend the selection to the smart-home position (Shift+Home).
pub fn extend_selection_home(app: &mut EditorApp, count: usize) -> DispatchResult {
    ensure_selection_anchor(app);
    crate::core::commands::movement::smart_home(app, count)
}

/// Extend the selection to end of line (Shift+End).
pub fn extend_selection_end(app: &mut EditorApp, count: usize) -> DispatchResult {
    ensure_selection_anchor(app);
    crate::core::commands::movement::end_of_line(app, count)
}

/// Delete the active selection (mark to cursor) if one exists.
///
/// Returns true if a non-empty selection was deleted. This is the shared
/// primitive for "replace selection on type" and "delete selection first"
/// used by Standard mode (and available to all models). The mark is cleared.
pub fn delete_active_selection(app: &mut EditorApp) -> bool {
    let (start_byte, end_byte, buffer_id) = {
        let active_window_id = app.active_window;
        let (window, buffer) = match (
            app.windows.get(&active_window_id),
            app.windows.get(&active_window_id).and_then(|w| app.buffers.get(&w.buffer_id)),
        ) {
            (Some(w), Some(b)) => (w, b),
            _ => return false,
        };
        let (mx, my) = match window.mark {
            Some(m) => m,
            None => return false,
        };
        // Empty selection (mark == cursor): just clear the mark.
        if (mx, my) == (window.cursor_x, window.cursor_y) {
            return false;
        }
        let cursor_byte = window.get_byte_offset(buffer).unwrap_or(0);
        let mark_byte = window.get_byte_offset_at(buffer, mx, my).unwrap_or(0);
        let (s, e) = if cursor_byte < mark_byte {
            (cursor_byte, mark_byte)
        } else {
            (mark_byte, cursor_byte)
        };
        (s, e, window.buffer_id)
    };

    if start_byte == end_byte {
        // Clear the mark even for empty selections.
        if let Some(window) = app.windows.get_mut(&app.active_window) {
            window.mark = None;
        }
        return false;
    }

    if let Some(buffer) = app.buffers.get_mut(&buffer_id) {
        buffer.delete(start_byte, end_byte - start_byte);
    }
    // Move cursor to start of deleted region and clear mark.
    if let Some(buffer) = app.buffers.get(&buffer_id) {
        let line = buffer.byte_to_line(start_byte);
        let active_window_id = app.active_window;
        if let Some(window) = app.windows.get_mut(&active_window_id) {
            window.cursor_y = line;
            if let Some(line_start) = buffer.line_to_byte(line)
                && let Some(line_text) = buffer.line(line) {
                    let byte_offset = start_byte.saturating_sub(line_start);
                    window.cursor_x =
                        crate::core::utf8::byte_to_grapheme_col(&line_text, byte_offset);
                }
            window.mark = None;
            window.update_visual_cursor(buffer);
            window.ensure_cursor_visible(buffer);
        }
    }
    true
}
