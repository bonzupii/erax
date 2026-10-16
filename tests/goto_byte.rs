//! `go-to-byte` navigates by byte offset: the command used to return
//! `InputAction::GotoLine`, so the entered byte offset was silently
//! treated as a line number and `EditorApp::goto_byte` never ran. These
//! tests pin the typed action, the byte-accurate navigation, and the
//! invalid-input messages.
use erax::core::app::EditorApp;
use erax::core::command::Command;
use erax::core::dispatcher::{DispatchResult, InputAction, dispatch};
use erax::core::prompt::handle_prompt_action;

fn app_with(text: &str) -> EditorApp {
    let mut app = EditorApp::new();
    if let Some(buffer) = app.active_buffer_mut() {
        buffer.insert(0, text);
    }
    app
}

fn cursor(app: &EditorApp) -> (usize, usize) {
    let window = app.windows.get(&app.active_window).expect("active window");
    (window.cursor_x, window.cursor_y)
}

#[test]
fn goto_byte_command_prompts_with_goto_byte_action() {
    let mut app = app_with("abc\n");
    let result = dispatch(&mut app, Command::GotoByte, 1);
    match result {
        DispatchResult::NeedsInput { prompt, action } => {
            assert_eq!(prompt, "Go to byte: ");
            assert_eq!(action, InputAction::GotoByte);
        }
        other => panic!("expected NeedsInput, got {:?}", other),
    }
}

#[test]
fn goto_byte_prompt_moves_to_byte_offset() {
    // "abc\ndef\n": byte 0='a', 3='\n', 4='d'.
    let mut app = app_with("abc\ndef\n");
    let exit = handle_prompt_action(&mut app, InputAction::GotoByte, "4".into())
        .expect("prompt action must not error");
    assert!(!exit);
    assert_eq!(cursor(&app), (0, 1));

    let exit = handle_prompt_action(&mut app, InputAction::GotoByte, "2".into())
        .expect("prompt action must not error");
    assert!(!exit);
    assert_eq!(cursor(&app), (2, 0));
}

#[test]
fn goto_byte_prompt_clamps_out_of_range_offset() {
    let mut app = app_with("abc\n");
    let exit = handle_prompt_action(&mut app, InputAction::GotoByte, "9999".into())
        .expect("prompt action must not error");
    assert!(!exit);
    // End of buffer for a trailing-newline buffer is the start of the
    // final empty line.
    assert_eq!(cursor(&app), (0, 1));
}

#[test]
fn goto_byte_prompt_clamps_to_end_of_last_line() {
    let mut app = app_with("abc");
    let exit = handle_prompt_action(&mut app, InputAction::GotoByte, "9999".into())
        .expect("prompt action must not error");
    assert!(!exit);
    assert_eq!(cursor(&app), (3, 0));
}

#[test]
fn goto_byte_prompt_rejects_non_numeric_input() {
    let mut app = app_with("abc\n");
    let before = cursor(&app);
    let exit = handle_prompt_action(&mut app, InputAction::GotoByte, "abc".into())
        .expect("prompt action must not error");
    assert!(!exit);
    assert_eq!(cursor(&app), before);
    assert!(app
        .message
        .as_deref()
        .is_some_and(|m| m.contains("Invalid byte offset")));
}

#[test]
fn goto_line_prompt_rejects_invalid_input_with_message() {
    let mut app = app_with("abc\n");
    let before = cursor(&app);
    for bad in ["abc", "0", "-3"] {
        let exit = handle_prompt_action(&mut app, InputAction::GotoLine, bad.into())
            .expect("prompt action must not error");
        assert!(!exit);
        assert_eq!(cursor(&app), before, "cursor moved on {bad:?}");
        assert!(
            app.message
                .as_deref()
                .is_some_and(|m| m.contains("Invalid line number")),
            "no message for {bad:?}"
        );
    }
}
