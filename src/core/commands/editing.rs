//! Core text-editing commands (insert, delete, transpose).
use crate::core::bindings::InputModel;
use crate::core::app::EditorApp;
/// Basic editing commands (insert, delete, newline)
use crate::core::dispatcher::DispatchResult;

pub fn open_line(app: &mut EditorApp, _count: usize) -> DispatchResult {
        let active_window_id = app.active_window;
        if let Some(window) = app.windows.get_mut(&active_window_id) {
            let buffer_id = window.buffer_id;
            if let Some(buffer) = app.buffers.get_mut(&buffer_id) {
                window.insert_char(buffer, '\n');
                window.move_up(buffer);
                window.end_of_line(buffer);
            }
        }
        DispatchResult::Success
    }

pub fn insert_newline(app: &mut EditorApp, _count: usize) -> DispatchResult {
        let active_window_id = app.active_window;
        let buffer_id = app.windows.get(&active_window_id).map(|w| w.buffer_id);

        if let Some(buffer_id) = buffer_id
            && let Some(window) = app.windows.get_mut(&active_window_id)
                && let Some(buffer) = app.buffers.get_mut(&buffer_id) {
                    window.insert_char(buffer, '\n');
                }
        DispatchResult::Success
    }

pub fn insert_tab(app: &mut EditorApp, _count: usize) -> DispatchResult {
        let active_window_id = app.active_window;
        let buffer_id = app.windows.get(&active_window_id).map(|w| w.buffer_id);

        if let Some(buffer_id) = buffer_id
            && let Some(window) = app.windows.get_mut(&active_window_id)
                && let Some(buffer) = app.buffers.get_mut(&buffer_id) {
                    window.insert_char(buffer, '\t');
                }
        DispatchResult::Success
    }

/// Delete the previous character (Backspace), in the given input model.
///
/// Standard mode deletes an active selection first (CUA); other models
/// keep traditional emacs/vim behavior. The runtime model is
/// [`crate::config::keys::INPUT_MODEL`].
pub fn delete_previous_character(app: &mut EditorApp, count: usize) -> DispatchResult {
    delete_previous_character_in_model(app, count, crate::config::keys::INPUT_MODEL)
}

/// [`delete_previous_character`] with an explicit input model.
///
/// Passing the compile-time [`crate::config::keys::INPUT_MODEL`] const
/// monomorphizes away the branch, so this is zero-cost; tests may pass any
/// [`InputModel`] to exercise model-specific paths.
pub fn delete_previous_character_in_model(
        app: &mut EditorApp,
        _count: usize,
        model: InputModel,
) -> DispatchResult {
        // Standard mode: Backspace with an active selection deletes it first.
        if model == InputModel::Standard
            && crate::core::commands::marks::delete_active_selection(app) {
                return DispatchResult::Success;
            }
        let active_window_id = app.active_window;
        let buffer_id = app.windows.get(&active_window_id).map(|w| w.buffer_id);

        if let Some(buffer_id) = buffer_id
            && let Some(window) = app.windows.get_mut(&active_window_id)
                && let Some(buffer) = app.buffers.get_mut(&buffer_id) {
                    window.delete_char(buffer, true);
                }
        DispatchResult::Success
    }

/// Delete the next character (Delete), in the given input model.
///
/// Standard mode deletes an active selection first (CUA); other models
/// keep traditional emacs/vim behavior. The runtime model is
/// [`crate::config::keys::INPUT_MODEL`].
pub fn delete_next_character(app: &mut EditorApp, count: usize) -> DispatchResult {
    delete_next_character_in_model(app, count, crate::config::keys::INPUT_MODEL)
}

/// [`delete_next_character`] with an explicit input model.
///
/// Passing the compile-time [`crate::config::keys::INPUT_MODEL`] const
/// monomorphizes away the branch, so this is zero-cost; tests may pass any
/// [`InputModel`] to exercise model-specific paths.
pub fn delete_next_character_in_model(
        app: &mut EditorApp,
        _count: usize,
        model: InputModel,
) -> DispatchResult {
        // Standard mode: Delete with an active selection deletes it first.
        if model == InputModel::Standard
            && crate::core::commands::marks::delete_active_selection(app) {
                return DispatchResult::Success;
            }
        let active_window_id = app.active_window;
        let buffer_id = app.windows.get(&active_window_id).map(|w| w.buffer_id);

        if let Some(buffer_id) = buffer_id
            && let Some(window) = app.windows.get_mut(&active_window_id)
                && let Some(buffer) = app.buffers.get_mut(&buffer_id) {
                    window.delete_char(buffer, false);
                }
        DispatchResult::Success
    }

pub fn transpose_characters(app: &mut EditorApp, _count: usize) -> DispatchResult {
        let active_window_id = app.active_window;
        let buffer_id = app.windows.get(&active_window_id).map(|w| w.buffer_id);

        if let Some(buffer_id) = buffer_id {
            // Compute the edit from the pre-edit state; borrows end
            // before the buffer is mutated. The two graphemes are always
            // adjacent, so the edit is one byte span and its swap.
            let edit: Option<(usize, usize, String, usize)> = (|| {
                let window = app.windows.get(&active_window_id)?;
                let buffer = app.buffers.get(&buffer_id)?;
                let cursor_x = window.cursor_x;
                let line = buffer.line(window.cursor_y)?;
                let graphemes: Vec<&str> =
                    crate::core::utf8::GraphemeIterator::new(&line).collect();

                let (pos1, pos2) = if cursor_x == 0 && graphemes.len() < 2 {
                    return None;
                } else if cursor_x >= graphemes.len() && graphemes.len() >= 2 {
                    (graphemes.len() - 2, graphemes.len() - 1)
                } else if cursor_x >= 1 && cursor_x < graphemes.len() {
                    (cursor_x - 1, cursor_x)
                } else {
                    return None;
                };

                let g1 = graphemes.get(pos1)?;
                let g2 = graphemes.get(pos2)?;
                if g1.is_empty() || g2.is_empty() {
                    return None;
                }
                let line_start = buffer.line_to_byte(window.cursor_y).unwrap_or(0);
                let byte_index = |n: usize| {
                    crate::core::utf8::grapheme_byte_index(&line, n)
                };
                let start = line_start + byte_index(pos1)?;
                let mid = line_start + byte_index(pos2)?;
                let end = mid + g2.len();
                Some((start, end, format!("{g2}{g1}"), pos1))
            })();

            if let Some((start, end, swapped, pos1)) = edit
                && let Some(buffer) = app.buffers.get_mut(&buffer_id)
                && let Some(window) = app.windows.get_mut(&active_window_id)
            {
                // One delete plus one insert moves both clusters whole: a
                // multi-scalar cluster is never split, overwrite mode
                // cannot eat following text, and the cursor lands past the
                // pair in grapheme units (Emacs "move forward one").
                buffer.delete(start, end - start);
                buffer.insert(start, &swapped);
                window.cursor_x = pos1 + 2;
                window.update_visual_cursor(buffer);
            }
        }
        DispatchResult::Success
    }

pub fn toggle_overwrite_mode(app: &mut EditorApp, _count: usize) -> DispatchResult {
        if let Some(window) = app.windows.get_mut(&app.active_window) {
            window.overwrite_mode = !window.overwrite_mode;
            let mode = if window.overwrite_mode {
                "Overwrite"
            } else {
                "Insert"
            };
            DispatchResult::Info(format!("{} mode", mode))
        } else {
            DispatchResult::NotHandled
        }
    }

pub fn insert_space(app: &mut EditorApp, count: usize) -> DispatchResult {
        let active_window_id = app.active_window;
        let buffer_id = app.windows.get(&active_window_id).map(|w| w.buffer_id);

        if let Some(buffer_id) = buffer_id
            && let Some(window) = app.windows.get_mut(&active_window_id)
                && let Some(buffer) = app.buffers.get_mut(&buffer_id) {
                    for _ in 0..count {
                        window.insert_char(buffer, ' ');
                    }
                }
        DispatchResult::Success
    }

pub fn redraw_display(_app: &mut EditorApp, _count: usize) -> DispatchResult {
        // Signal that display needs full refresh
        // The actual redraw is handled by the event loop
        DispatchResult::Redraw
    }

pub fn quote_character(_app: &mut EditorApp, _count: usize) -> DispatchResult {
        // The event loop (and the modal engine) route AwaitKey through the
        // focus manager: the next keypress is consumed by the QuoteCharacter
        // focus target, which inserts it literally.
        DispatchResult::AwaitKey(crate::core::dispatcher::InputAction::QuoteCharacter)
    }

pub fn newline_and_indent(app: &mut EditorApp, _count: usize) -> DispatchResult {
        let active_window_id = app.active_window;

        // Get current line's leading whitespace
        let indent = {
            if let Some(window) = app.windows.get(&active_window_id) {
                if let Some(buffer) = app.buffers.get(&window.buffer_id) {
                    buffer.line(window.cursor_y).map(|line| {
                        line.chars()
                            .take_while(|c| c.is_whitespace() && *c != '\n')
                            .collect::<String>()
                    }).unwrap_or_default()
                } else {
                    String::new()
                }
            } else {
                String::new()
            }
        };

        // Insert newline then indent
        if let Some(window) = app.windows.get_mut(&active_window_id) {
            let buffer_id = window.buffer_id;
            if let Some(buffer) = app.buffers.get_mut(&buffer_id) {
                window.insert_char(buffer, '\n');
                for c in indent.chars() {
                    window.insert_char(buffer, c);
                }
            }
        }

        DispatchResult::Success
    }
