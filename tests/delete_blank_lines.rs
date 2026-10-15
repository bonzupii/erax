//! delete_blank_lines leaves the cursor on valid text.
//!
//! Deleting a run of blank lines that includes lines above the cursor
//! shifts the buffer up under a stationary cursor; the command must move
//! the cursor with the deletion (Emacs: point stays on the surviving
//! blank line) and clamp it into the buffer.

use erax::core::app::EditorApp;
use erax::core::buffer::Buffer;
use erax::core::commands::text::delete_blank_lines;
use erax::core::dispatcher::DispatchResult;

fn set_content(app: &mut EditorApp, text: &str) {
    let buffer = app.buffers.values_mut().next().expect("initial buffer");
    *buffer = Buffer::from_string(text);
}

fn set_cursor(app: &mut EditorApp, x: usize, y: usize) {
    let w = app.active_window_mut().expect("active window");
    w.cursor_x = x;
    w.cursor_y = y;
}

#[test]
fn delete_blank_lines_keeps_cursor_on_the_surviving_blank_line() {
    let mut app = EditorApp::new();
    set_content(&mut app, "\n\n\n");
    // Cursor on the middle blank line; blanks above and below it.
    set_cursor(&mut app, 0, 2);

    let r = delete_blank_lines(&mut app, 1);
    assert!(matches!(r, DispatchResult::Success));

    let buffer = app.buffers.values().next().expect("buffer");
    let w = app.windows.get(&app.active_window).expect("window");
    assert!(
        w.cursor_y < buffer.line_count(),
        "cursor_y={} escaped the {}-line buffer",
        w.cursor_y,
        buffer.line_count()
    );
    // The whole run collapsed to a single blank line; point stays on it.
    assert_eq!(buffer.line(0).as_deref(), Some(""));
    assert_eq!(w.cursor_y, 0);
}

#[test]
fn delete_blank_lines_below_point_keeps_point_still() {
    let mut app = EditorApp::new();
    set_content(&mut app, "foo\n\n\nbar\n");
    set_cursor(&mut app, 0, 0);

    let r = delete_blank_lines(&mut app, 1);
    assert!(matches!(r, DispatchResult::Success));

    let buffer = app.buffers.values().next().expect("buffer");
    let w = app.windows.get(&app.active_window).expect("window");
    assert_eq!(buffer.line(0).as_deref(), Some("foo"));
    assert_eq!(buffer.line(1).as_deref(), Some("bar"));
    assert_eq!((w.cursor_x, w.cursor_y), (0, 0));
    assert!(w.cursor_y < buffer.line_count());
}
