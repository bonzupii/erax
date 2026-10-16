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
use crate::core::dispatcher::{
    dispatch, dispatch_insert, sequence_halts_after, DispatchResult, MacroStep,
};

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
                    // Nested execution is rejected, not recursive: the
                    // flag above guards the direct case, this guards
                    // macro-contained nesting. The rejection is a
                    // refusal, so the shared sequence stop rule below
                    // halts the replay on it.
                    DispatchResult::Refused("Macros cannot invoke execute-macro".to_string())
                }
                MacroStep::Command(cmd) => dispatch(app, *cmd, *cnt),
                MacroStep::Insert(c) => dispatch_insert(app, *c, *cnt),
            };
            // An informational message ("Mark set") is shown and the
            // replay continues: treating Info as a stop aborted every
            // macro containing a mark or kill command. Whether the
            // replay continues past this step is the one shared
            // sequence rule: a refusal (a state-changing command that
            // did not perform its action, like an exit refused) halts
            // the replay so later steps never run against the
            // uncompleted intent.
            if let DispatchResult::Info(msg) | DispatchResult::Refused(msg) = &step_result {
                app.message = Some(msg.clone());
            }
            if sequence_halts_after(&step_result) {
                result = step_result;
                break 'replay;
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
        // Rejected, not recursive: the replay halts on the refusal and
        // the message names it.
        assert!(matches!(result, DispatchResult::Refused(_)));
        assert!(app
            .message
            .as_deref()
            .is_some_and(|m| m.contains("Macros cannot invoke execute-macro")));
    }

    #[test]
    fn replay_continues_past_info_returning_steps() {
        // set-mark and kill-region report Info ("Mark set", "Region
        // killed"): a macro containing them must run to completion,
        // not abort at the first Info result.
        let mut app = test_app();
        if let Some(buffer) = app.active_buffer_mut() {
            buffer.insert(0, "hello world");
        }
        dispatch(&mut app, Command::BeginMacro, 1);
        dispatch(&mut app, Command::SetMark, 1);
        dispatch(&mut app, Command::ForwardCharacter, 3);
        dispatch(&mut app, Command::KillRegion, 1);
        dispatch(&mut app, Command::EndMacro, 1);

        let result = dispatch(&mut app, Command::ExecuteMacro, 1);
        assert_eq!(result, DispatchResult::Success);

        // Every step ran. Recording executes the commands live, so the
        // recording-time kill left "lo world"; the replay-time kill must
        // run too. If the replay had aborted at set-mark's Info, the
        // text would still be "lo world".
        let text = app.active_buffer().expect("buffer").content_string();
        assert_eq!(text, "world");
        // ...and the kill's message reached the message line.
        assert!(app
            .message
            .as_deref()
            .is_some_and(|m| m.contains("Region killed")));
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

    #[test]
    fn macro_halts_at_refused_exit_and_save() {
        let mut app = EditorApp::new();
        // A refused ExitAndSave followed by an observable step: the
        // refusal must stop the replay, so the insert never runs.
        if let Some(buffer) = app.active_buffer_mut() {
            buffer.insert(0, "unsaved work");
        }
        // The default scratch buffer has no filename: exiting would
        // destroy its content, so exit_and_save refuses.
        app.last_macro = vec![
            (MacroStep::Command(Command::ExitAndSave), 1),
            (MacroStep::Insert('z'), 1),
        ];
        let result = execute_macro(&mut app, 1);
        match result {
            DispatchResult::Refused(msg) => assert!(msg.contains("exit refused")),
            other => panic!("expected refusal, got {other:?}"),
        }
        let text = app.active_buffer().expect("buffer").content_string();
        assert!(!text.contains('z'), "replay continued past the refusal");
        assert!(app
            .message
            .is_some_and(|m| m.contains("exit refused")));
    }

    #[test]
    fn macro_continues_past_informational_info() {
        let mut app = EditorApp::new();
        // set-mark reports Info("Mark set"): informational success, so
        // the replay continues past it to the insert.
        app.last_macro = vec![
            (MacroStep::Command(Command::SetMark), 1),
            (MacroStep::Insert('q'), 1),
        ];
        let result = execute_macro(&mut app, 1);
        assert_eq!(result, DispatchResult::Success);
        let text = app.active_buffer().expect("buffer").content_string();
        assert!(text.contains('q'), "replay stopped at informational Info");
    }

    #[test]
    fn macro_rejects_nested_execute_macro_as_refusal() {
        let mut app = EditorApp::new();
        // Macro-contained execute-macro is a refusal, so the shared
        // stop rule halts the replay and the insert never runs.
        app.last_macro = vec![
            (MacroStep::Command(Command::ExecuteMacro), 1),
            (MacroStep::Insert('z'), 1),
        ];
        let result = execute_macro(&mut app, 1);
        match result {
            DispatchResult::Refused(msg) => assert!(msg.contains("cannot invoke")),
            other => panic!("expected refusal, got {other:?}"),
        }
        let text = app.active_buffer().expect("buffer").content_string();
        assert!(
            !text.contains('z'),
            "replay continued past the nested rejection"
        );
    }
}
