use crate::core::app::EditorApp;
use crate::core::command::Command;
use crate::core::dispatcher::DispatchResult;

/// Undo the last operation
#[derive(Clone)]
pub struct Undo;

impl Command for Undo {
    fn execute(&self, app: &mut EditorApp, _count: usize) -> DispatchResult {
        let window_id = app.active_window;
        if let Some(window) = app.windows.get_mut(&window_id) {
            let buffer_id = window.buffer_id;
            if let Some(buffer) = app.buffers.get_mut(&buffer_id) {
                if buffer.undo() {
                    // Ensure cursor is valid after undo
                    window.ensure_cursor_valid(buffer);
                    return DispatchResult::Success;
                }
            }
        }
        DispatchResult::Info("Nothing to undo".to_string())
    }
}

/// Redo the last undone operation
#[derive(Clone)]
pub struct Redo;

impl Command for Redo {
    fn execute(&self, app: &mut EditorApp, _count: usize) -> DispatchResult {
        let window_id = app.active_window;
        if let Some(window) = app.windows.get_mut(&window_id) {
            let buffer_id = window.buffer_id;
            if let Some(buffer) = app.buffers.get_mut(&buffer_id) {
                if buffer.redo() {
                    // Ensure cursor is valid after redo
                    window.ensure_cursor_valid(buffer);
                    return DispatchResult::Success;
                }
            }
        }
        DispatchResult::Info("Nothing to redo".to_string())
    }
}
