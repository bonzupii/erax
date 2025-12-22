use crate::core::app::EditorApp;
/// Macro recording and execution commands
use crate::core::command::Command;
use crate::core::dispatcher::DispatchResult;

/// Start recording macro
#[derive(Clone)]
pub struct BeginMacro;

impl Command for BeginMacro {
    fn execute(&self, app: &mut EditorApp, _count: usize) -> DispatchResult {
        app.is_recording_macro = true;
        app.current_macro.clear();
        DispatchResult::Info("Start macro".to_string())
    }
}

/// Stop recording macro
#[derive(Clone)]
pub struct EndMacro;

impl Command for EndMacro {
    fn execute(&self, app: &mut EditorApp, _count: usize) -> DispatchResult {
        if app.is_recording_macro {
            app.is_recording_macro = false;
            app.last_macro = app.current_macro.clone();
            DispatchResult::Info("End macro".to_string())
        } else {
            DispatchResult::Info("Not recording".to_string())
        }
    }
}

/// Execute last recorded macro (count times)
#[derive(Clone)]
pub struct ExecuteMacro;

impl Command for ExecuteMacro {
    fn execute(&self, app: &mut EditorApp, count: usize) -> DispatchResult {
        if app.is_recording_macro {
            return DispatchResult::Info("Cannot run macro while recording".to_string());
        }
        if app.last_macro.is_empty() {
            return DispatchResult::Info("No macro defined".to_string());
        }

        let macro_cmds = app.last_macro.clone();
        for _ in 0..count {
            for (cmd, cnt) in &macro_cmds {
                // Check for special character insertion entry
                if let Some(char_str) = cmd.strip_prefix("__insert:") {
                    if let Some(c) = char_str.chars().next() {
                        // Dispatch character insertion
                        crate::core::dispatcher::dispatch(app, None, Some(c), *cnt);
                    }
                } else {
                    // Regular command
                    crate::core::dispatcher::dispatch(app, Some(cmd), None, *cnt);
                }
            }
        }
        DispatchResult::Success
    }
}
