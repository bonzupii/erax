//! wrap_word never moves the cursor without wrapping.
//!
//! The word search walks the cursor backward through the buffer. When no
//! wrap happens the buffer is byte-identical, and the cursor must be
//! exactly where it started: a no-op command must not move point.

use erax::core::app::EditorApp;
use erax::core::buffer::Buffer;
use erax::core::commands::text::wrap_word;
use erax::core::dispatcher::DispatchResult;

fn cursor_xy(app: &EditorApp) -> (usize, usize) {
    let w = app.windows.get(&app.active_window).expect("window");
    (w.cursor_x, w.cursor_y)
}

#[test]
fn wrap_word_no_wrap_leaves_cursor_still() {
    let texts = [
        "hello world foo bar",
        "helloworld",
        "  leading spaces here",
        "one\ntwo three\nfour",
        "",
        "a",
        "word ",
        "\ttab\tseparated\twords",
    ];
    for text in texts {
        let mut template = EditorApp::new();
        *template.buffers.values_mut().next().expect("buffer") =
            Buffer::from_string(text);
        let line_count = template.buffers.values().next().expect("buffer").line_count();
        for y in 0..line_count {
            let line_len = template.buffers.values().next().expect("buffer")
                .line(y)
                .map(|l| erax::core::utf8::grapheme_count(&l))
                .unwrap_or(0);
            for x in 0..=line_len {
                let mut app = EditorApp::new();
                *app.buffers.values_mut().next().expect("buffer") =
                    Buffer::from_string(text);
                {
                    let w = app.active_window_mut().expect("window");
                    w.cursor_x = x.min(line_len);
                    w.cursor_y = y;
                }
                let before_text = app.buffers.values().next().expect("buffer").content_string();
                let before_xy = cursor_xy(&app);
                let r = wrap_word(&mut app, 1);
                assert!(matches!(r, DispatchResult::Success));
                let after_text = app.buffers.values().next().expect("buffer").content_string();
                if after_text == before_text {
                    assert_eq!(
                        cursor_xy(&app),
                        before_xy,
                        "no-op wrap moved the cursor on {text:?} at {before_xy:?}"
                    );
                }
            }
        }
    }
}
