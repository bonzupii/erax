//! Editor control commands (quit, suspend, redraw).
use crate::core::app::EditorApp;
/// Application control commands
use crate::core::dispatcher::{DispatchResult, InputAction};

pub fn exit_eras(app: &mut EditorApp, _count: usize) -> DispatchResult {
    // L-008: never silently discard unsaved work. The pristine docs promised
    // `^X^C` prompts on unsaved work; the old default binding pointed at
    // `exit-without-save` instead, quitting with data loss. Now `^X^C`
    // targets this command, which refuses to exit while modified buffers
    // exist and says exactly what to do instead. Explicit discard remains
    // available as `exit-without-save` (e.g. vim's `:q!`).
    let modified = app.buffers.values().filter(|b| b.modified).count();
    if modified == 0 {
        return DispatchResult::Exit;
    }
    app.message = Some(format!(
        "{modified} modified buffer{} not saved; exit refused. \
         Save them (save-buffer) or exit-and-save, \
         or exit-without-save to discard.",
        if modified == 1 { "" } else { "s" }
    ));
    DispatchResult::Success
}

pub fn exit_without_save(_app: &mut EditorApp, _count: usize) -> DispatchResult {
        DispatchResult::Exit
    }

pub fn universal_argument(app: &mut EditorApp, _count: usize) -> DispatchResult {
        // If already in universal argument mode, multiply by 4
        if app.universal_argument.is_some() {
            let current = app.universal_argument.unwrap_or(1);
            app.universal_argument = Some(current * 4);
        } else {
            // First C-u sets to 4
            app.universal_argument = Some(4);
        }
        DispatchResult::Success
    }

pub fn keyboard_quit(app: &mut EditorApp, _count: usize) -> DispatchResult {
        // Clear universal argument
        app.universal_argument = None;
        // Clear mark if active
        if let Some(window) = app.windows.get_mut(&app.active_window) {
            window.mark = None;
        }
        // Clear any in-progress state
        app.message = Some("Quit".to_string());
        DispatchResult::Success
    }
pub fn exit_and_save(app: &mut EditorApp, _count: usize) -> DispatchResult {
        // Save all modified buffers
        let mut save_errors = Vec::new();

        // Collect buffer IDs to avoid borrow issues
        let buffer_ids: Vec<crate::core::id::BufferId> = app.buffers.keys().cloned().collect();

        for id in buffer_ids {
            if let Some(buffer) = app.buffers.get_mut(&id)
                && buffer.modified && buffer.filename.is_some()
                    && let Err(e) = buffer.save() {
                        save_errors.push(format!("Error saving buffer {:?}: {}", id, e));
                    }
        }

        if save_errors.is_empty() {
            DispatchResult::Exit
        } else {
            DispatchResult::Info(format!("Failed to save: {}", save_errors.join(", ")))
        }
    }

pub fn describe_key(_app: &mut EditorApp, _count: usize) -> DispatchResult {
        DispatchResult::AwaitKey(InputAction::DescribeKey)
    }

pub fn execute_named_command(_app: &mut EditorApp, _count: usize) -> DispatchResult {
        DispatchResult::NeedsInput {
            prompt: "M-x ".to_string(),
            action: InputAction::ExecuteNamedCommand,
        }
    }

#[cfg(test)]
mod tests {
    use super::*;

    /// L-008: `exit-erax` with no modified buffers exits.
    #[test]
    fn exit_eras_exits_when_clean() {
        let mut app = EditorApp::new();
        let result = exit_eras(&mut app, 1);
        assert_eq!(result, DispatchResult::Exit);
    }

    /// L-008: `exit-erax` must NOT silently discard unsaved work.
    #[test]
    fn exit_eras_refuses_with_modified_buffers() {
        let mut app = EditorApp::new();
        if let Some(buffer) = app.active_buffer_mut() {
            buffer.insert(0, "unsaved work");
        } else {
            panic!("test app has no active buffer");
        }
        let result = exit_eras(&mut app, 1);
        assert_eq!(result, DispatchResult::Success);
        let message = app.message.as_deref().unwrap_or("");
        assert!(
            message.contains("1 modified buffer"),
            "message names the problem, got: {message}"
        );
        assert!(
            message.contains("exit-without-save"),
            "message names the explicit-discard alternative, got: {message}"
        );
    }

    /// Explicit discard still exits even with unsaved work.
    #[test]
    fn exit_without_save_still_exits() {
        let mut app = EditorApp::new();
        if let Some(buffer) = app.active_buffer_mut() {
            buffer.insert(0, "unsaved work");
        }
        assert_eq!(exit_without_save(&mut app, 1), DispatchResult::Exit);
    }
}
