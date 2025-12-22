use crate::core::app::EditorApp;
/// Application control commands
use crate::core::command::Command;
use crate::core::dispatcher::{DispatchResult, InputAction};

/// Exit application
#[derive(Clone)]
pub struct Exit;

impl Command for Exit {
    fn execute(&self, _app: &mut EditorApp, _count: usize) -> DispatchResult {
        DispatchResult::Exit
    }
}

/// Exit without saving
#[derive(Clone)]
pub struct ExitWithoutSave;

impl Command for ExitWithoutSave {
    fn execute(&self, _app: &mut EditorApp, _count: usize) -> DispatchResult {
        DispatchResult::Exit
    }
}

/// Universal argument (C-u) - sets numeric prefix for next command
/// Default is 4, successive C-u multiplies by 4
#[derive(Clone)]
pub struct UniversalArgument;

impl Command for UniversalArgument {
    fn execute(&self, app: &mut EditorApp, _count: usize) -> DispatchResult {
        // If already in universal argument mode, multiply by 4
        if app.universal_argument.is_some() {
            let current = match app.universal_argument {
                Some(v) => v,
                None => 1,
            };
            app.universal_argument = Some(current * 4);
        } else {
            // First C-u sets to 4
            app.universal_argument = Some(4);
        }
        DispatchResult::Success
    }
}

/// Keyboard Quit (C-g) - cancel current operation
#[derive(Clone)]
pub struct KeyboardQuit;

impl Command for KeyboardQuit {
    fn execute(&self, app: &mut EditorApp, _count: usize) -> DispatchResult {
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
}
/// Exit application after saving all buffers
#[derive(Clone)]
pub struct ExitAndSave;

impl Command for ExitAndSave {
    fn execute(&self, app: &mut EditorApp, _count: usize) -> DispatchResult {
        // Save all modified buffers
        let mut save_errors = Vec::new();

        // Collect buffer IDs to avoid borrow issues
        let buffer_ids: Vec<crate::core::id::BufferId> = app.buffers.keys().cloned().collect();

        for id in buffer_ids {
            if let Some(buffer) = app.buffers.get_mut(&id) {
                if buffer.modified && buffer.filename.is_some() {
                    if let Err(e) = buffer.save() {
                        save_errors.push(format!("Error saving buffer {:?}: {}", id, e));
                    }
                }
            }
        }

        if save_errors.is_empty() {
            DispatchResult::Exit
        } else {
            DispatchResult::Info(format!("Failed to save: {}", save_errors.join(", ")))
        }
    }
}

/// Describe a key binding
#[derive(Clone)]
pub struct DescribeKey;

impl Command for DescribeKey {
    fn execute(&self, _app: &mut EditorApp, _count: usize) -> DispatchResult {
        DispatchResult::Info("describe-key: Not implemented".to_string())
    }
}

/// Execute a named command (M-x)
#[derive(Clone)]
pub struct NamedCommand;

impl Command for NamedCommand {
    fn execute(&self, _app: &mut EditorApp, _count: usize) -> DispatchResult {
        DispatchResult::NeedsInput {
            prompt: "M-x ".to_string(),
            action: InputAction::ExecuteNamedCommand,
        }
    }
}
