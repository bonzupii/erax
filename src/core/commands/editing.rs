//! Core text-editing commands (insert, delete, transpose).
use crate::config::keys::InputModel;
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
            let data = {
                let cursor_x = app.windows.get(&active_window_id).map(|w| w.cursor_x).unwrap_or_default();
                let cursor_y = app.windows.get(&active_window_id).map(|w| w.cursor_y).unwrap_or_default();

                let line_content = app.buffers.get(&buffer_id).and_then(|b| b.line(cursor_y)).unwrap_or_default();

                let graphemes: Vec<String> =
                    crate::core::utf8::GraphemeIterator::new(&line_content)
                        .map(|s| s.to_string())
                        .collect();

                // Determine positions to transpose
                let (pos1, pos2) = if cursor_x == 0 && graphemes.len() < 2 {
                    return DispatchResult::Success;
                } else if cursor_x >= graphemes.len() && graphemes.len() >= 2 {
                    (graphemes.len() - 2, graphemes.len() - 1)
                } else if cursor_x >= 1 && cursor_x < graphemes.len() {
                    (cursor_x - 1, cursor_x)
                } else {
                    return DispatchResult::Success;
                };

                let char1 = graphemes.get(pos1).cloned().unwrap_or_default();
                let char2 = graphemes.get(pos2).cloned().unwrap_or_default();

                if char1.is_empty() || char2.is_empty() {
                    return DispatchResult::Success;
                }

                (pos1, pos2, char1, char2)
            };

            let (pos1, _pos2, char1, char2) = data;

            // Now we can mutably borrow
            if let Some(buffer) = app.buffers.get_mut(&buffer_id)
                && let Some(window) = app.windows.get_mut(&active_window_id) {
                    // Delete second char first (higher index)
                    window.cursor_x = pos1 + 1;
                    window.delete_char(buffer, false);
                    // Delete first char
                    window.cursor_x = pos1;
                    window.delete_char(buffer, false);

                    // Insert in swapped order
                    for c in char2.chars() {
                        window.insert_char(buffer, c);
                    }
                    for c in char1.chars() {
                        window.insert_char(buffer, c);
                    }
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
        // This command needs special handling in the event loop
        // to capture the next keystroke literally
        DispatchResult::Info("quote-character: Press next key to insert literally".to_string())
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
