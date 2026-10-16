//! kill-to-end-of-line at end of line kills the newline (uemacs:
//! `chunk = 1` when the remainder is empty), joining the lines. It used
//! to kill nothing there, so lines could never be joined with it.
use erax::core::app::EditorApp;
use erax::core::command::Command;
use erax::core::dispatcher::dispatch;

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

fn kill_ring_top(app: &EditorApp) -> String {
    app.kill_ring.yank().cloned().unwrap_or_default()
}

#[test]
fn kill_at_end_of_line_kills_the_newline() {
    let mut app = app_with("hello\nworld\n");
    dispatch(&mut app, Command::EndOfLine, 1);
    dispatch(&mut app, Command::KillToEndOfLine, 1);
    assert_eq!(buffer_text(&app), "helloworld\n");
    assert_eq!(kill_ring_top(&app), "\n");
}

#[test]
fn kill_at_end_of_last_line_is_a_noop() {
    // Nowhere to join to: uemacs delete_bytes fails at end of buffer,
    // so the command does nothing and kills nothing.
    let mut app = app_with("hello");
    dispatch(&mut app, Command::EndOfLine, 1);
    dispatch(&mut app, Command::KillToEndOfLine, 1);
    assert_eq!(buffer_text(&app), "hello");
}

#[test]
fn kill_mid_line_still_kills_to_end_of_line() {
    let mut app = app_with("hello\nworld\n");
    dispatch(&mut app, Command::ForwardCharacter, 2);
    dispatch(&mut app, Command::KillToEndOfLine, 1);
    assert_eq!(buffer_text(&app), "he\nworld\n");
    assert_eq!(kill_ring_top(&app), "llo");
}

#[test]
fn consecutive_kills_at_eol_append() {
    // "a\n\nb\n": two newlines in a row, so the cursor is at end of
    // line for both kills with no motion between them.
    let mut app = app_with("a\n\nb\n");
    dispatch(&mut app, Command::EndOfLine, 1);
    dispatch(&mut app, Command::KillToEndOfLine, 1);
    dispatch(&mut app, Command::KillToEndOfLine, 1);
    assert_eq!(buffer_text(&app), "ab\n");
    assert_eq!(kill_ring_top(&app), "\n\n");
}
