use crate::core::app::EditorApp;
/// Basic editing commands (insert, delete, newline)
use crate::core::command::Command;
use crate::core::dispatcher::DispatchResult;

/// Insert newline and move cursor up (uEmacs open-line behavior)
#[derive(Clone)]
pub struct OpenLine;

impl Command for OpenLine {
    fn execute(&self, app: &mut EditorApp, _count: usize) -> DispatchResult {
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
}

/// Insert newline at cursor position
#[derive(Clone)]
pub struct InsertNewline;

impl Command for InsertNewline {
    fn execute(&self, app: &mut EditorApp, _count: usize) -> DispatchResult {
        let active_window_id = app.active_window;
        let buffer_id = app.windows.get(&active_window_id).map(|w| w.buffer_id);

        if let Some(buffer_id) = buffer_id {
            if let Some(window) = app.windows.get_mut(&active_window_id) {
                if let Some(buffer) = app.buffers.get_mut(&buffer_id) {
                    window.insert_char(buffer, '\n');
                }
            }
        }
        DispatchResult::Success
    }
}

/// Insert tab character at cursor position
#[derive(Clone)]
pub struct InsertTab;

impl Command for InsertTab {
    fn execute(&self, app: &mut EditorApp, _count: usize) -> DispatchResult {
        let active_window_id = app.active_window;
        let buffer_id = app.windows.get(&active_window_id).map(|w| w.buffer_id);

        if let Some(buffer_id) = buffer_id {
            if let Some(window) = app.windows.get_mut(&active_window_id) {
                if let Some(buffer) = app.buffers.get_mut(&buffer_id) {
                    window.insert_char(buffer, '\t');
                }
            }
        }
        DispatchResult::Success
    }
}

/// Delete character before cursor
#[derive(Clone)]
pub struct DeleteBackwardChar;

impl Command for DeleteBackwardChar {
    fn execute(&self, app: &mut EditorApp, _count: usize) -> DispatchResult {
        let active_window_id = app.active_window;
        let buffer_id = app.windows.get(&active_window_id).map(|w| w.buffer_id);

        if let Some(buffer_id) = buffer_id {
            if let Some(window) = app.windows.get_mut(&active_window_id) {
                if let Some(buffer) = app.buffers.get_mut(&buffer_id) {
                    window.delete_char(buffer, true);
                }
            }
        }
        DispatchResult::Success
    }
}

/// Delete character at cursor position
#[derive(Clone)]
pub struct DeleteForwardChar;

impl Command for DeleteForwardChar {
    fn execute(&self, app: &mut EditorApp, _count: usize) -> DispatchResult {
        let active_window_id = app.active_window;
        let buffer_id = app.windows.get(&active_window_id).map(|w| w.buffer_id);

        if let Some(buffer_id) = buffer_id {
            if let Some(window) = app.windows.get_mut(&active_window_id) {
                if let Some(buffer) = app.buffers.get_mut(&buffer_id) {
                    window.delete_char(buffer, false);
                }
            }
        }
        DispatchResult::Success
    }
}

/// Swap characters at and before cursor
#[derive(Clone)]
pub struct TransposeChars;

impl Command for TransposeChars {
    fn execute(&self, app: &mut EditorApp, _count: usize) -> DispatchResult {
        let active_window_id = app.active_window;
        let buffer_id = app.windows.get(&active_window_id).map(|w| w.buffer_id);

        if let Some(buffer_id) = buffer_id {
            // Collect all needed data as owned values before mutable borrow
            let data = {
                let cursor_x = app
                    .windows
                    .get(&active_window_id)
                    .map(|w| w.cursor_x)
                    .unwrap_or(0);
                let cursor_y = app
                    .windows
                    .get(&active_window_id)
                    .map(|w| w.cursor_y)
                    .unwrap_or(0);

                let line_content = app
                    .buffers
                    .get(&buffer_id)
                    .and_then(|b| b.line(cursor_y))
                    .unwrap_or_default();

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
            if let Some(buffer) = app.buffers.get_mut(&buffer_id) {
                if let Some(window) = app.windows.get_mut(&active_window_id) {
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
        }
        DispatchResult::Success
    }
}

/// Toggle between insert and overwrite mode
#[derive(Clone)]
pub struct ToggleOverwriteMode;

impl Command for ToggleOverwriteMode {
    fn execute(&self, _app: &mut EditorApp, _count: usize) -> DispatchResult {
        DispatchResult::Info("toggle-overwrite-mode: Not implemented".to_string())
    }
}

/// Insert space at cursor (^C in uEmacs)
#[derive(Clone)]
pub struct InsertSpace;

impl Command for InsertSpace {
    fn execute(&self, app: &mut EditorApp, count: usize) -> DispatchResult {
        let active_window_id = app.active_window;
        let buffer_id = app.windows.get(&active_window_id).map(|w| w.buffer_id);

        if let Some(buffer_id) = buffer_id {
            if let Some(window) = app.windows.get_mut(&active_window_id) {
                if let Some(buffer) = app.buffers.get_mut(&buffer_id) {
                    for _ in 0..count {
                        window.insert_char(buffer, ' ');
                    }
                }
            }
        }
        DispatchResult::Success
    }
}

/// Redraw screen (^L in uEmacs)
#[derive(Clone)]
pub struct RedrawDisplay;

impl Command for RedrawDisplay {
    fn execute(&self, _app: &mut EditorApp, _count: usize) -> DispatchResult {
        // Signal that display needs full refresh
        // The actual redraw is handled by the event loop
        DispatchResult::Redraw
    }
}

/// Quote next character - insert literal (^Q in uEmacs)
#[derive(Clone)]
pub struct QuoteCharacter;

impl Command for QuoteCharacter {
    fn execute(&self, _app: &mut EditorApp, _count: usize) -> DispatchResult {
        // This command needs special handling in the event loop
        // to capture the next keystroke literally
        DispatchResult::Info("quote-character: Press next key to insert literally".to_string())
    }
}

/// Insert newline and indent to match previous line (^J in uEmacs)
#[derive(Clone)]
pub struct NewlineAndIndent;

impl Command for NewlineAndIndent {
    fn execute(&self, app: &mut EditorApp, _count: usize) -> DispatchResult {
        let active_window_id = app.active_window;

        // Get current line's leading whitespace
        let indent = {
            if let Some(window) = app.windows.get(&active_window_id) {
                if let Some(buffer) = app.buffers.get(&window.buffer_id) {
                    buffer
                        .line(window.cursor_y)
                        .map(|line| {
                            line.chars()
                                .take_while(|c| c.is_whitespace() && *c != '\n')
                                .collect::<String>()
                        })
                        .unwrap_or_default()
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
}
