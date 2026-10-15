//! delete_buffer keeps the window, layout, and buffer collections consistent.
//!
//! The buffer shown in the active window is removed; every window viewing
//! it is closed through the layout (not just dropped from the map); the
//! active window always ends up live; the last buffer cannot be deleted.

use erax::core::app::EditorApp;
use erax::core::buffer::Buffer;
use erax::core::commands::file::delete_buffer;
use erax::core::dispatcher::DispatchResult;

/// The three collections must agree after every delete.
fn assert_consistent(app: &EditorApp) {
    assert!(!app.windows.is_empty(), "no windows left");
    assert!(!app.buffers.is_empty(), "no buffers left");
    let layout_ids = app.layout.window_ids();
    for id in &layout_ids {
        assert!(
            app.windows.contains_key(id),
            "layout references window {id:?} missing from the map"
        );
    }
    for id in app.windows.keys() {
        assert!(
            layout_ids.contains(id),
            "window {id:?} present in the map but missing from the layout"
        );
    }
    assert!(
        app.windows.contains_key(&app.active_window),
        "active window {:?} dangles",
        app.active_window
    );
    for (id, w) in &app.windows {
        assert!(
            app.buffers.contains_key(&w.buffer_id),
            "window {id:?} shows deleted buffer {:?}",
            w.buffer_id
        );
    }
}

#[test]
fn delete_buffer_closes_only_viewing_windows() {
    let mut app = EditorApp::new();
    // W1 (active) shows the new buffer; W0 keeps the initial buffer.
    app.split_window_vertically();
    let w1 = app.active_window;
    let b1 = app.add_buffer(Buffer::from_string("second"));
    app.active_window_mut().expect("active window").buffer_id = b1;
    assert_eq!(app.windows.len(), 2);

    let r = delete_buffer(&mut app, 1);
    assert!(matches!(r, DispatchResult::Success));

    assert_eq!(app.buffers.len(), 1, "deleted buffer must be gone");
    assert_eq!(app.windows.len(), 1, "only the viewing window closes");
    assert!(!app.windows.contains_key(&w1));
    assert_consistent(&app);
}

#[test]
fn delete_buffer_refuses_the_last_buffer() {
    let mut app = EditorApp::new();
    let r = delete_buffer(&mut app, 1);
    assert!(
        matches!(r, DispatchResult::Info(_)),
        "deleting the last buffer must refuse with a message, got {r:?}"
    );
    assert_eq!(app.buffers.len(), 1);
    assert_eq!(app.windows.len(), 1);
    assert_consistent(&app);
}

#[test]
fn delete_buffer_repoints_the_last_window() {
    let mut app = EditorApp::new();
    // Both windows show the initial buffer; a second buffer exists.
    app.split_window_vertically();
    let b1 = app.add_buffer(Buffer::from_string("spare"));
    let active = app.active_window;

    let r = delete_buffer(&mut app, 1);
    assert!(matches!(r, DispatchResult::Success));

    // One window survives, repointed at the spare buffer with a reset view.
    assert_eq!(app.windows.len(), 1);
    let w = app.windows.get(&active).expect("kept window");
    assert_eq!(w.buffer_id, b1);
    assert_eq!((w.cursor_x, w.cursor_y, w.scroll_offset), (0, 0, 0));
    assert_consistent(&app);
}
