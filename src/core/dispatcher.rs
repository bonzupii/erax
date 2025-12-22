use crate::core::app::EditorApp;

/// Action requiring user input
#[derive(Debug, Clone, PartialEq)]
pub enum InputAction {
    /// Open a file (find-file)
    OpenFile,
    /// Save to a new path (write-file / save-as)
    SaveAs,
    /// Search forward for text
    SearchForward,
    /// Search backward for text
    SearchBackward,
    /// Query replace (old -> new)
    QueryReplace,
    /// Rename symbol (LSP)
    RenameSymbol,
    /// Switch to a different buffer
    SwitchToBuffer,
    /// Read file content to cursor position
    ReadFile,
    ShellCommand,
    FilterBuffer,
    GotoLine,
    Calculator,
    SedPreview,
    ExecuteNamedCommand,
}

/// Result of command dispatch
#[derive(Debug, PartialEq)]
pub enum DispatchResult {
    /// Command executed successfully
    Success,
    /// Command not found/handled
    NotHandled,
    /// File modified on disk - needs user confirmation
    FileModified,
    /// Exit requested
    Exit,
    /// Command needs user input before completing
    NeedsInput { prompt: String, action: InputAction },
    /// Informational message to display
    Info(String),
    /// Force full display redraw
    Redraw,
}

/// Maximum recursion depth to prevent stack overflow from recursive macros
const MAX_DISPATCH_DEPTH: usize = 64;

/// Command dispatcher for EditorApp (Command Pattern architecture)
///
/// All commands are looked up in the command registry and executed.
/// Supports repetition via count parameter (uEmacs universal argument).
///
/// The dispatcher is intentionally simple: it records macros if active,
/// looks up the command in the registry, and executes it. All command logic
/// is implemented in command structs in the `commands` modules.
///
/// # Parameters
/// - `app`: Mutable reference to the editor application state
/// - `command`: Command name to dispatch
/// - `count`: Repetition count (default 1)
///
/// # Returns
/// `DispatchResult` indicating success, failure, or special handling needs
pub fn dispatch(
    app: &mut EditorApp,
    command_name: Option<&str>,
    insert_char: Option<char>,
    count: usize,
) -> DispatchResult {
    // Recursion depth check to prevent stack overflow from recursive macros
    if app.dispatch_depth > MAX_DISPATCH_DEPTH {
        return DispatchResult::Info("Command recursion limit exceeded".to_string());
    }
    app.dispatch_depth += 1;

    let result = dispatch_inner(app, command_name, insert_char, count);

    app.dispatch_depth -= 1;
    result
}

/// Inner dispatch logic (separated for clean recursion tracking)
fn dispatch_inner(
    app: &mut EditorApp,
    command_name: Option<&str>,
    insert_char: Option<char>,
    count: usize,
) -> DispatchResult {
    // Handle character insertion first
    if let Some(c) = insert_char {
        if let Some(window) = app.windows.get_mut(&app.active_window) {
            if let Some(buffer) = app.buffers.get_mut(&window.buffer_id) {
                // Record character insertion to macro if recording
                if app.is_recording_macro {
                    // Use special prefix to distinguish from command names
                    app.current_macro.push((format!("__insert:{}", c), count));
                }
                for _ in 0..count {
                    // Apply count for char insertion
                    window.insert_char(buffer, c);
                }
                return DispatchResult::Success;
            }
        }
        return DispatchResult::NotHandled;
    }

    // Handle named commands
    if let Some(command_str) = command_name {
        if app.is_recording_macro && command_str != "end-macro" && command_str != "begin-macro" {
            app.current_macro.push((command_str.to_string(), count));
        }

        if let Some(command_obj) = app.command_registry.get(command_str).cloned() {
            let result = command_obj.execute(app, count);

            if result == DispatchResult::Success && !command_str.starts_with("kill-") {
                app.reset_kill_flag();
            }

            return result;
        }

        eprintln!("Command not found in registry: {}", command_str);
        return DispatchResult::NotHandled;
    }

    DispatchResult::NotHandled // No command or char to handle
}

#[cfg(test)]
mod tests {
    use super::*;

    // Helper to setup app with registered commands for tests
    fn setup_test_app() -> EditorApp {
        let mut app = EditorApp::new();
        crate::core::commands::register_all(&mut app);
        app
    }

    #[test]
    fn test_dispatch_forward_char() {
        let mut app = setup_test_app();
        let result = dispatch(&mut app, Some("forward-character"), None, 1);
        assert_eq!(result, DispatchResult::Success);
    }

    #[test]
    fn test_dispatch_split_window() {
        let mut app = setup_test_app();
        assert_eq!(app.windows.len(), 1);
        let result = dispatch(&mut app, Some("split-current-window"), None, 1);
        assert_eq!(result, DispatchResult::Success);
        assert_eq!(app.windows.len(), 2);
    }

    #[test]
    fn test_dispatch_exit() {
        let mut app = setup_test_app();
        let result = dispatch(&mut app, Some("exit-erax"), None, 1);
        assert_eq!(result, DispatchResult::Exit);
    }

    #[test]
    fn test_dispatch_unknown() {
        let mut app = setup_test_app();
        let result = dispatch(&mut app, Some("unknown-command"), None, 1);
        assert_eq!(result, DispatchResult::NotHandled);
    }
}
