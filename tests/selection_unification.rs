//! The mark/selection single-authority invariant.
//!
//! The selection manager's mark (a byte offset) is the only copy of
//! mark/selection state: the selection is always mark<->cursor, derived
//! at read time. These tests pin that the keyboard path, the mouse path,
//! the semantic snapshot, and the mode-line readout all agree, because
//! they read the same field.

use erax::core::app::EditorApp;
use erax::core::command::Command;
use erax::core::dispatcher::{dispatch, DispatchResult};
use erax::core::mouse::{MouseButton, MouseEvent, MouseHandler};

fn app_with(text: &str) -> EditorApp {
    let mut app = EditorApp::new();
    app.active_buffer_mut().unwrap().insert(0, text);
    app
}

fn cursor_byte(app: &EditorApp) -> usize {
    let w = app.active_window_ref().unwrap();
    let b = app.active_buffer().unwrap();
    w.get_byte_offset(b).unwrap()
}

#[test]
fn keyboard_mark_and_mouse_selection_read_the_same_state() {
    let mut app = app_with("hello world\n");
    // Keyboard: set the mark at byte 0, move the cursor to byte 5.
    dispatch(&mut app, Command::SetMark, 1);
    {
        let w = app.active_window_mut().unwrap();
        w.cursor_x = 5;
    }
    let mark_after_keyboard = app
        .active_window_ref()
        .unwrap()
        .selection_manager
        .mark;
    assert_eq!(mark_after_keyboard, Some(0));

    // The semantic snapshot derives the selection from the same mark.
    let snap = erax::core::semantics::capture(&app).unwrap();
    assert_eq!(
        snap.selection,
        Some(erax::core::semantics::ByteRange { start: 0, end: 5 })
    );
    assert_eq!(snap.mark, Some(0));
}

#[test]
fn mouse_click_sets_the_mark_no_sync_step() {
    let mut app = app_with("hello world\n");
    let window_id = app.active_window;
    let handler = MouseHandler::new();
    {
        let buffer_id = app.windows.get(&window_id).unwrap().buffer_id;
        let window = app.windows.get_mut(&window_id).unwrap();
        let buffer = app.buffers.get(&buffer_id).unwrap();
        // Click at grapheme column 3: the mark anchor is byte 3.
        handler.handle_event(
            &MouseEvent::Click(3, 0, MouseButton::Left),
            window,
            buffer,
        );
    }
    let window = app.windows.get(&window_id).unwrap();
    assert_eq!(window.selection_manager.mark, Some(3));
    // Click is a point: no selection yet, and the snapshot agrees.
    assert!(!window.selection_manager.has_selection(cursor_byte(&app)));
    let snap = erax::core::semantics::capture(&app).unwrap();
    assert!(snap.selection.is_none());
    assert_eq!(snap.mark, Some(3));
}

#[test]
fn mouse_drag_selection_derives_from_mark_and_cursor() {
    let mut app = app_with("hello world\n");
    let window_id = app.active_window;
    let handler = MouseHandler::new();
    {
        let buffer_id = app.windows.get(&window_id).unwrap().buffer_id;
        let window = app.windows.get_mut(&window_id).unwrap();
        let buffer = app.buffers.get(&buffer_id).unwrap();
        handler.handle_event(
            &MouseEvent::Click(0, 0, MouseButton::Left),
            window,
            buffer,
        );
        handler.handle_event(
            &MouseEvent::Drag(0, 0, 5, 0, MouseButton::Left),
            window,
            buffer,
        );
    }
    let window = app.windows.get(&window_id).unwrap();
    let cb = cursor_byte(&app);
    assert_eq!(cb, 5);
    let sel = window.selection_manager.selection(cb).expect("selection");
    assert_eq!((sel.start(), sel.end()), (0, 5));
    // The mode-line char count reads the same mark.
    let buffer = app.active_buffer().unwrap();
    let count =
        erax::terminal::display::selection_char_count(window, buffer).expect("count");
    assert_eq!(count, 5);
}

#[test]
fn mark_cleared_everywhere_at_once() {
    let mut app = app_with("hello\n");
    dispatch(&mut app, Command::SetMark, 1);
    {
        let w = app.active_window_mut().unwrap();
        w.cursor_x = 3;
    }
    // There is exactly one copy to clear: no stale second copy can
    // survive and keep a highlight or snapshot alive.
    dispatch(&mut app, Command::KeyboardQuit, 1);
    let window = app.active_window_ref().unwrap();
    assert_eq!(window.selection_manager.mark, None);
    assert!(!window.selection_manager.has_selection(cursor_byte(&app)));
    let snap = erax::core::semantics::capture(&app).unwrap();
    assert!(snap.selection.is_none());
    assert_eq!(snap.mark, None);
}

#[test]
fn exchange_point_and_mark_swaps_bytes() {
    let mut app = app_with("hello world\n");
    dispatch(&mut app, Command::SetMark, 1);
    {
        let w = app.active_window_mut().unwrap();
        w.cursor_x = 5;
    }
    let result = dispatch(&mut app, Command::ExchangePointAndMark, 1);
    assert_eq!(result, DispatchResult::Success);
    let window = app.active_window_ref().unwrap();
    // Mark is now where the cursor was; the cursor is at the old mark.
    assert_eq!(window.selection_manager.mark, Some(5));
    assert_eq!((window.cursor_x, window.cursor_y), (0, 0));
}
