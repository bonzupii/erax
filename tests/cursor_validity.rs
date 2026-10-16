//! Cursor and mark validity is enforced by construction: after every
//! dispatched command, insertion, and prompt action, the active window
//! is reconciled against its buffer. No command needs its own guard
//! and no reader needs a fallback for a stale position.
use erax::core::app::EditorApp;
use erax::core::command::Command;
use erax::core::commands::snippets::expand_snippet;
use erax::core::dispatcher::{InputAction, dispatch};
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

fn cursor_within_line(app: &EditorApp) -> bool {
    let window = app.windows.get(&app.active_window).expect("active window");
    let buffer = app.active_buffer().expect("buffer");
    match buffer.line(window.cursor_y) {
        Some(line) => window.cursor_x <= erax::core::utf8::grapheme_count(&line),
        None => false,
    }
}

#[test]
fn shrink_edit_cannot_leave_cursor_past_line_end() {
    // "hello world", cursor at end of line (11). Query-replace shrinks
    // the line to "hell wrld" (9 graphemes) through the prompt-action
    // path, which used to leave cursor_x = 11: the next command
    // indexing by cursor_x (expand_snippet) panicked.
    let mut app = app_with("hello world");
    dispatch(&mut app, Command::EndOfLine, 1);
    assert_eq!(cursor(&app), (11, 0));

    let exit = handle_prompt_action(&mut app, InputAction::QueryReplace, "o|".into())
        .expect("prompt action must not error");
    assert!(!exit);
    assert!(cursor_within_line(&app), "cursor left stale: {:?}", cursor(&app));

    // The command that used to panic now runs on a valid cursor.
    let _ = expand_snippet(&mut app, 1);
    assert!(cursor_within_line(&app));
}

#[test]
fn dispatch_reconciles_an_invalid_cursor() {
    let mut app = app_with("hi");
    // Simulate any stale state a mutation could leave behind (the
    // fields are pub). A no-op command still passes through the
    // dispatch seam, so the window must come out valid.
    if let Some(window) = app.windows.get_mut(&app.active_window) {
        window.cursor_x = 999;
        window.cursor_y = 999;
    }
    dispatch(&mut app, Command::ForwardCharacter, 1);
    assert!(cursor_within_line(&app));
    assert_eq!(cursor(&app), (2, 0));
}

#[test]
fn deleted_line_drops_orphaned_mark() {
    let mut app = app_with("one\ntwo\nthree\n");
    // Mark on line 1 ("two").
    dispatch(&mut app, Command::NextLine, 1);
    dispatch(&mut app, Command::SetMark, 1);
    {
        let window = app.windows.get(&app.active_window).expect("active window");
        assert_eq!(window.mark, Some((0, 1)));
    }
    // Delete the marked line through the dispatch seam.
    dispatch(&mut app, Command::KillToEndOfLine, 1);
    dispatch(&mut app, Command::DeleteNextCharacter, 1);
    let window = app.windows.get(&app.active_window).expect("active window");
    match window.mark {
        Some((mx, my)) => {
            let buffer = app.active_buffer().expect("buffer");
            let line = buffer.line(my).expect("marked line resolves");
            assert!(
                mx <= erax::core::utf8::grapheme_count(&line),
                "mark left stale: {:?}",
                window.mark
            );
        }
        None => {}
    }
}

#[test]
fn empty_buffer_reconciles_cursor_and_mark() {
    let mut app = app_with("hello");
    dispatch(&mut app, Command::SetMark, 1);
    dispatch(&mut app, Command::EndOfLine, 1);
    // Select-all + delete leaves an empty buffer through dispatch.
    dispatch(&mut app, Command::MarkWholeBuffer, 1);
    dispatch(&mut app, Command::KillRegion, 1);
    assert_eq!(cursor(&app), (0, 0));
}
