//! Macro recording and execution commands.
//!
//! Macros are typed edit records (N-157, N-229): each step is a
//! [`MacroStep`] — a typed [`Command`] or a literally-inserted character.
//! Replay is iterative over the typed steps; there is no string sentinel
//! encoding and no per-playback clone of the macro (N-229). Nested macro
//! execution is rejected with a typed message instead of a magic recursion
//! depth counter (N-157).

use crate::core::app::EditorApp;
use crate::core::command::Command;
use crate::core::dispatcher::{dispatch, dispatch_insert, DispatchResult, MacroStep};

/// Start recording a macro.
pub fn begin_macro(app: &mut EditorApp, _count: usize) -> DispatchResult {
    app.is_recording_macro = true;
    app.current_macro.clear();
    DispatchResult::Info("Start macro".to_string())
}

/// Stop recording a macro.
pub fn end_macro(app: &mut EditorApp, _count: usize) -> DispatchResult {
    if app.is_recording_macro {
        app.is_recording_macro = false;
        app.last_macro = core::mem::take(&mut app.current_macro);
        DispatchResult::Info("End macro".to_string())
    } else {
        DispatchResult::Info("Not recording".to_string())
    }
}

/// Execute the last recorded macro `count` times.
///
/// Replay iterates over the typed steps (N-157). A macro that invokes
/// `execute-macro` on itself is rejected instead of recursing: the
/// `executing_macro` flag makes nested execution structurally impossible
/// rather than depth-limited.
pub fn execute_macro(app: &mut EditorApp, count: usize) -> DispatchResult {
    if app.is_recording_macro {
        return DispatchResult::Info("Cannot run macro while recording".to_string());
    }
    if app.executing_macro {
        return DispatchResult::Info("Macros cannot invoke execute-macro".to_string());
    }
    if app.last_macro.is_empty() {
        return DispatchResult::Info("No macro defined".to_string());
    }

    // Move the steps out for replay; restore afterwards so the macro
    // remains defined. No clone of the whole macro per the N-229 direction.
    let steps = core::mem::take(&mut app.last_macro);
    app.executing_macro = true;
    let mut result = DispatchResult::Success;
    'replay: for _ in 0..count {
        for (step, cnt) in &steps {
            let step_result = match step {
                MacroStep::Command(Command::ExecuteMacro) => {
                    // Nested execution is rejected; the flag above guards
                    // the direct case, this guards macro-contained nesting.
                    DispatchResult::Info("Macros cannot invoke execute-macro".to_string())
                }
                MacroStep::Command(cmd) => dispatch(app, *cmd, *cnt),
                MacroStep::Insert(c) => dispatch_insert(app, *c, *cnt),
            };
            match step_result {
                // A step that only asks for a repaint folds into the
                // final Success: the replay's caller repaints anyway.
                DispatchResult::Success | DispatchResult::Redraw => {}
                // Anything else (Exit, NeedsInput, AwaitKey, Info,
                // NotHandled, FileModified) stops the replay so the
                // signal reaches the event layer instead of being
                // swallowed mid-macro.
                other => {
                    result = other;
                    break 'replay;
                }
            }
        }
    }
    app.executing_macro = false;
    app.last_macro = steps;
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::dispatcher::dispatch;

    fn test_app() -> EditorApp {
        EditorApp::new()
    }

    #[test]
    fn macro_records_and_replays_typed_steps() {
        let mut app = test_app();
        dispatch(&mut app, Command::BeginMacro, 1);
        dispatch(&mut app, Command::ForwardCharacter, 2);
        dispatch_insert(&mut app, 'q', 1);
        dispatch(&mut app, Command::EndMacro, 1);

        assert_eq!(
            app.last_macro,
            vec![
                (MacroStep::Command(Command::ForwardCharacter), 2),
                (MacroStep::Insert('q'), 1),
            ]
        );

        // Replay inserts the character in full (N-229: no first-char truncation).
        dispatch(&mut app, Command::ExecuteMacro, 1);
        let text = app.active_buffer().expect("buffer").content_string();
        assert!(text.contains('q'), "replay must insert recorded char");
    }

    #[test]
    fn multibyte_insert_replays_fully() {
        let mut app = test_app();
        dispatch(&mut app, Command::BeginMacro, 1);
        dispatch_insert(&mut app, 'é', 1);
        dispatch(&mut app, Command::EndMacro, 1);
        dispatch(&mut app, Command::ExecuteMacro, 1);
        let text = app.active_buffer().expect("buffer").content_string();
        assert!(text.contains('é'));
    }

    #[test]
    fn nested_execute_macro_is_rejected_not_recursive() {
        let mut app = test_app();
        dispatch(&mut app, Command::BeginMacro, 1);
        dispatch(&mut app, Command::ExecuteMacro, 1);
        dispatch(&mut app, Command::EndMacro, 1);
        let result = dispatch(&mut app, Command::ExecuteMacro, 1);
        assert_eq!(
            result,
            DispatchResult::Info("Macros cannot invoke execute-macro".to_string())
        );
    }

    #[test]
    fn begin_end_macro_state_transitions() {
        let mut app = test_app();
        assert_eq!(
            dispatch(&mut app, Command::EndMacro, 1),
            DispatchResult::Info("Not recording".to_string())
        );
        dispatch(&mut app, Command::BeginMacro, 1);
        assert!(app.is_recording_macro);
        dispatch(&mut app, Command::EndMacro, 1);
        assert!(!app.is_recording_macro);
    }
}

#[cfg(test)]
mod propagation_tests {
    use super::*;

    #[test]
    fn macro_stops_and_propagates_exit() {
        let mut app = EditorApp::new();
        // Exit, then an insert: the exit must stop the replay, so the
        // result propagates and the insert never runs.
        app.last_macro = vec![
            (MacroStep::Command(Command::ExitErax), 1),
            (MacroStep::Insert('x'), 1),
        ];
        let result = execute_macro(&mut app, 1);
        assert_eq!(result, DispatchResult::Exit);
        let text = app.active_buffer().expect("buffer").content_string();
        assert!(!text.contains('x'), "replay continued past Exit");
        // The macro definition survives the aborted replay.
        assert_eq!(app.last_macro.len(), 2);
        assert!(!app.executing_macro);
    }
}
