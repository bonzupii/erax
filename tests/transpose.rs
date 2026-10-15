//! transpose_characters moves whole grapheme clusters and leaves the
//! cursor in grapheme units.
//!
//! A multi-scalar cluster (base character plus combining marks) is one
//! cursor position. The transpose must move the cluster whole — the same
//! unit the display layer's ClusterText holds — and the cursor must count
//! graphemes afterward, not the scalars the cluster was built from.

use erax::core::app::EditorApp;
use erax::core::buffer::Buffer;
use erax::core::commands::editing::transpose_characters;
use erax::core::dispatcher::DispatchResult;
use erax::terminal::display::ClusterText;

fn setup(text: &str, x: usize) -> EditorApp {
    let mut app = EditorApp::new();
    *app.buffers.values_mut().next().expect("initial buffer") =
        Buffer::from_string(text);
    app.active_window_mut().expect("active window").cursor_x = x;
    app
}

fn content(app: &EditorApp) -> String {
    app.buffers
        .values()
        .next()
        .expect("buffer")
        .content_string()
}

fn cursor_x(app: &EditorApp) -> usize {
    app.windows
        .get(&app.active_window)
        .expect("window")
        .cursor_x
}

#[test]
fn transpose_ascii_moves_forward_one() {
    let mut app = setup("abc", 2);
    let r = transpose_characters(&mut app, 1);
    assert!(matches!(r, DispatchResult::Success));
    assert_eq!(content(&app), "acb");
    assert_eq!(cursor_x(&app), 3);
}

#[test]
fn transpose_multiscalar_cluster_stays_whole() {
    // "a", then e + combining acute (one grapheme, two scalars), then "b".
    let mut app = setup("ae\u{301}b", 3);
    let r = transpose_characters(&mut app, 1);
    assert!(matches!(r, DispatchResult::Success));
    assert_eq!(content(&app), "abe\u{301}");
    // The moved cluster is whole, exactly as ClusterText holds it.
    let cluster = ClusterText::new("e\u{301}");
    assert_eq!(cluster.as_str(), "e\u{301}");
    assert!(content(&app).ends_with(cluster.as_str()));
    // The cursor counts graphemes (3), not scalars (4).
    assert_eq!(cursor_x(&app), 3);
}

#[test]
fn transpose_in_overwrite_mode_keeps_following_text() {
    let mut app = setup("abcd", 2);
    app.active_window_mut()
        .expect("window")
        .overwrite_mode = true;
    let r = transpose_characters(&mut app, 1);
    assert!(matches!(r, DispatchResult::Success));
    assert_eq!(content(&app), "acbd");
    assert_eq!(cursor_x(&app), 3);
}
