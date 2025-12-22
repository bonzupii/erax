use crate::core::app::EditorApp;
/// Window management and navigation commands
use crate::core::command::Command;
use crate::core::dispatcher::DispatchResult;

/// Split active window vertically
#[derive(Clone)]
pub struct SplitWindowVertically;

impl Command for SplitWindowVertically {
    fn execute(&self, app: &mut EditorApp, _count: usize) -> DispatchResult {
        app.split_window_vertically();
        DispatchResult::Success
    }
}

/// Split active window horizontally
#[derive(Clone)]
pub struct SplitWindowHorizontally;

impl Command for SplitWindowHorizontally {
    fn execute(&self, app: &mut EditorApp, _count: usize) -> DispatchResult {
        app.split_window_horizontally();
        DispatchResult::Success
    }
}

/// Close other windows, keeping only active window
#[derive(Clone)]
pub struct DeleteOtherWindows;

impl Command for DeleteOtherWindows {
    fn execute(&self, app: &mut EditorApp, _count: usize) -> DispatchResult {
        app.delete_other_windows();
        DispatchResult::Success
    }
}

/// Close active window
#[derive(Clone)]
pub struct DeleteWindow;

impl Command for DeleteWindow {
    fn execute(&self, app: &mut EditorApp, _count: usize) -> DispatchResult {
        app.delete_window();
        DispatchResult::Success
    }
}

/// Switch to next window
#[derive(Clone)]
pub struct OtherWindow;

impl Command for OtherWindow {
    fn execute(&self, app: &mut EditorApp, _count: usize) -> DispatchResult {
        app.next_window();
        DispatchResult::Success
    }
}

/// Minimize current window (hide it, don't delete)
#[derive(Clone)]
pub struct MinimizeWindow;

impl Command for MinimizeWindow {
    fn execute(&self, app: &mut EditorApp, _count: usize) -> DispatchResult {
        app.message = Some("minimize-window: Not yet implemented".to_string());
        DispatchResult::Success
    }
}

/// Show list of windows, including minimized ones
#[derive(Clone)]
pub struct WindowPicker;

impl Command for WindowPicker {
    fn execute(&self, app: &mut EditorApp, _count: usize) -> DispatchResult {
        app.message = Some("window-picker: Not yet implemented".to_string());
        DispatchResult::Success
    }
}

/// Increase window size
#[derive(Clone)]
pub struct GrowWindow;

impl Command for GrowWindow {
    fn execute(&self, app: &mut EditorApp, _count: usize) -> DispatchResult {
        app.message = Some("grow-window: Not yet implemented".to_string());
        DispatchResult::Success
    }
}

/// Decrease window size
#[derive(Clone)]
pub struct ShrinkWindow;

impl Command for ShrinkWindow {
    fn execute(&self, app: &mut EditorApp, _count: usize) -> DispatchResult {
        app.message = Some("shrink-window: Not yet implemented".to_string());
        DispatchResult::Success
    }
}
