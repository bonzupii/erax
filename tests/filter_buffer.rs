//! `filter-buffer` pipes the buffer through a shell command: the old
//! prompt handler only echoed the typed command back as a message and
//! never executed anything. These tests drive the real prompt action.
use erax::core::app::EditorApp;
use erax::core::dispatcher::InputAction;
use erax::core::prompt::handle_prompt_action;

fn app_with(text: &str) -> EditorApp {
    let mut app = EditorApp::new();
    if let Some(buffer) = app.active_buffer_mut() {
        buffer.insert(0, text);
    }
    app
}

fn buffer_text(app: &EditorApp) -> String {
    app.active_buffer()
        .map(|b| b.content_string())
        .unwrap_or_default()
}

#[test]
fn filter_buffer_replaces_content_with_command_stdout() {
    let mut app = app_with("hello\nworld\n");
    let exit = handle_prompt_action(&mut app, InputAction::FilterBuffer, "tr a-z A-Z".into())
        .expect("prompt action must not error");
    assert!(!exit);
    assert_eq!(buffer_text(&app), "HELLO\nWORLD\n");
    assert!(
        app.active_buffer().is_some_and(|b| b.modified),
        "filtering marks the buffer modified"
    );
}

#[test]
fn filter_buffer_leaves_buffer_untouched_on_failure() {
    let mut app = app_with("precious\n");
    let exit = handle_prompt_action(&mut app, InputAction::FilterBuffer, "exit 3".into())
        .expect("prompt action must not error");
    assert!(!exit);
    assert_eq!(buffer_text(&app), "precious\n");
    assert!(
        app.message.as_deref().is_some_and(|m| m.contains("Filter failed")),
        "the failure must be reported, got: {:?}",
        app.message
    );
}

#[test]
fn filter_buffer_handles_content_larger_than_the_pipe() {
    // A sequential stdin-write-then-wait deadlocks here: `cat` fills
    // the stdout pipe while the editor blocks on the stdin pipe.
    let big = "x".repeat(512 * 1024);
    let mut app = app_with(&big);
    let exit = handle_prompt_action(&mut app, InputAction::FilterBuffer, "cat".into())
        .expect("prompt action must not error");
    assert!(!exit);
    assert_eq!(buffer_text(&app), big);
}

#[test]
fn filter_buffer_empty_input_is_a_noop() {
    let mut app = app_with("untouched\n");
    let exit = handle_prompt_action(&mut app, InputAction::FilterBuffer, String::new())
        .expect("prompt action must not error");
    assert!(!exit);
    assert_eq!(buffer_text(&app), "untouched\n");
}
