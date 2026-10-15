//! Accepting diff hunks reproduces the modified text exactly, and quitting
//! a diff removes the two scratch buffers it created.
//!
//! Hunk spans are 1-based with an exclusive end and carry context lines;
//! the replacement text is the hunk's added lines interleaved with that
//! context. Accepting every hunk top to bottom must therefore yield the
//! modified document byte-for-byte, whatever the line-count deltas.

use erax::core::app::EditorApp;
use erax::core::buffer::Buffer;
use erax::core::commands::diff::{diff_accept_hunk, diff_quit, start_sed_diff};
use erax::core::dispatcher::DispatchResult;

fn setup(content: &str) -> EditorApp {
    let mut app = EditorApp::new();
    *app.buffers.values_mut().next().expect("initial buffer") =
        Buffer::from_string(content);
    app
}

fn real_content(app: &EditorApp) -> String {
    let id = app
        .diff_state
        .as_ref()
        .expect("diff state")
        .original_buffer_id;
    app.buffers
        .get(&id)
        .expect("original buffer")
        .content_string()
}

fn accept_all_top_to_bottom(app: &mut EditorApp) -> usize {
    let n = app
        .diff_state
        .as_ref()
        .expect("diff state")
        .hunks
        .len();
    for _ in 0..n {
        let r = diff_accept_hunk(app, 1);
        assert!(matches!(r, DispatchResult::Success));
    }
    n
}

#[test]
fn accept_hunk_keeps_context_and_newlines() {
    let mut app = setup("AAA\nk1\nk2\nk3\n");
    start_sed_diff(&mut app, "s/A/X/").expect("diff starts");
    accept_all_top_to_bottom(&mut app);
    assert_eq!(real_content(&app), "XAA\nk1\nk2\nk3\n");
}

#[test]
fn accept_hunks_top_to_bottom_tracks_line_shifts() {
    // The substitution adds a line per changed line, so the second
    // hunk's recorded span is stale after the first accept.
    let mut app = setup("AAA\nk1\nk2\nk3\nk4\nk5\nk6\nk7\nABA\n");
    start_sed_diff(&mut app, "s/A/X\nY/").expect("diff starts");
    let n = accept_all_top_to_bottom(&mut app);
    assert!(n >= 2, "offset test needs at least two hunks, got {n}");
    assert_eq!(
        real_content(&app),
        "X\nYAA\nk1\nk2\nk3\nk4\nk5\nk6\nk7\nX\nYBA\n"
    );
}

#[test]
fn diff_quit_removes_its_two_buffers() {
    let mut app = setup("AAA\nk1\n");
    start_sed_diff(&mut app, "s/A/X/").expect("diff starts");
    assert_eq!(app.buffers.len(), 3, "real + two diff scratch buffers");

    let r = diff_quit(&mut app, 1);
    assert!(matches!(r, DispatchResult::Success));

    assert_eq!(app.buffers.len(), 1, "scratch buffers must be gone");
    let real_id = {
        let st = app.diff_state.take();
        st.map(|s| s.original_buffer_id)
    };
    // diff_quit clears the state; recover it for the assertions.
    assert!(real_id.is_none(), "quit clears diff state");
    assert_eq!(app.windows.len(), 1);
    let window = app.windows.get(&app.active_window).expect("window");
    assert!(
        app.buffers.contains_key(&window.buffer_id),
        "active window shows a live buffer"
    );
    assert_eq!(app.layout.window_ids(), vec![app.active_window]);
}
