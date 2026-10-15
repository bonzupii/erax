//! Window-management commands (split, delete, navigate).
use crate::core::app::EditorApp;
/// Window management and navigation commands
use crate::core::dispatcher::DispatchResult;

pub fn split_current_window(app: &mut EditorApp, _count: usize) -> DispatchResult {
        app.split_window_vertically();
        DispatchResult::Success
    }

pub fn split_window_horizontally(app: &mut EditorApp, _count: usize) -> DispatchResult {
        app.split_window_horizontally();
        DispatchResult::Success
    }

pub fn delete_other_windows(app: &mut EditorApp, _count: usize) -> DispatchResult {
        app.delete_other_windows();
        DispatchResult::Success
    }

pub fn delete_window(app: &mut EditorApp, _count: usize) -> DispatchResult {
        app.delete_window();
        DispatchResult::Success
    }

pub fn next_window(app: &mut EditorApp, _count: usize) -> DispatchResult {
        app.next_window();
        DispatchResult::Success
    }

pub fn minimize_window(app: &mut EditorApp, _count: usize) -> DispatchResult {
        app.message = Some("minimize-window: Not yet implemented".to_string());
        DispatchResult::Success
    }

pub fn window_picker(app: &mut EditorApp, _count: usize) -> DispatchResult {
        app.message = Some("window-picker: Not yet implemented".to_string());
        DispatchResult::Success
    }

pub fn grow_window(app: &mut EditorApp, _count: usize) -> DispatchResult {
        app.message = Some("grow-window: Not yet implemented".to_string());
        DispatchResult::Success
    }

pub fn shrink_window(app: &mut EditorApp, _count: usize) -> DispatchResult {
        app.message = Some("shrink-window: Not yet implemented".to_string());
        DispatchResult::Success
    }
