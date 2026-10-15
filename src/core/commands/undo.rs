//! Undo and redo commands.
use crate::core::app::EditorApp;
use crate::core::dispatcher::DispatchResult;

pub fn undo(app: &mut EditorApp, count: usize) -> DispatchResult {
        let window_id = app.active_window;
        let mut undone = false;
        for _ in 0..count.max(1) {
            let did = if let Some(window) = app.windows.get_mut(&window_id) {
                let buffer_id = window.buffer_id;
                if let Some(buffer) = app.buffers.get_mut(&buffer_id)
                    && buffer.undo()
                {
                    // Ensure cursor is valid after undo
                    window.ensure_cursor_valid(buffer);
                    true
                } else {
                    false
                }
            } else {
                false
            };
            if !did {
                break;
            }
            undone = true;
        }
        if undone {
            DispatchResult::Success
        } else {
            DispatchResult::Info("Nothing to undo".to_string())
        }
    }

pub fn redo(app: &mut EditorApp, count: usize) -> DispatchResult {
        let window_id = app.active_window;
        let mut redone = false;
        for _ in 0..count.max(1) {
            let did = if let Some(window) = app.windows.get_mut(&window_id) {
                let buffer_id = window.buffer_id;
                if let Some(buffer) = app.buffers.get_mut(&buffer_id)
                    && buffer.redo()
                {
                    // Ensure cursor is valid after redo
                    window.ensure_cursor_valid(buffer);
                    true
                } else {
                    false
                }
            } else {
                false
            };
            if !did {
                break;
            }
            redone = true;
        }
        if redone {
            DispatchResult::Success
        } else {
            DispatchResult::Info("Nothing to redo".to_string())
        }
    }
