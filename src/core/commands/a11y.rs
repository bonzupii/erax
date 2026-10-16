//! Accessibility commands: the spoken-state description.
//!
//! `describe-state` renders the document snapshot as one concise
//! spoken-style line through the normal `Info` result path: the TUI
//! shows it in the echo area, and it is exactly the text a speech
//! sink would speak. The rendering rules live in
//! [`crate::core::semantics::render_spoken_state`]; this module is
//! only the command plumbing.

use crate::core::app::EditorApp;
use crate::core::dispatcher::DispatchResult;

/// Render the active buffer's snapshot as one spoken-style line.
///
/// Always succeeds with `Info(text)` when a buffer is active; when no
/// buffer is active (not reachable in a live editor) it reports that
/// instead of failing silently.
pub fn describe_state(app: &mut EditorApp, _count: usize) -> DispatchResult {
    match crate::core::semantics::capture(app) {
        Some(snapshot) => {
            let line = crate::core::semantics::render_spoken_state(&snapshot);
            DispatchResult::Info(line)
        }
        None => DispatchResult::Info("No active buffer".to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::command::Command;
    use crate::core::dispatcher::dispatch;

    fn info_text(app: &mut EditorApp) -> String {
        match dispatch(app, Command::DescribeState, 1) {
            DispatchResult::Info(text) => text,
            other => panic!("expected Info, got {other:?}"),
        }
    }

    #[test]
    fn empty_scratch_names_itself_and_counts_lines() {
        let mut app = EditorApp::new();
        let text = info_text(&mut app);
        assert!(text.contains("*scratch*"), "{text}");
        assert!(text.contains("line 1 of 1"), "{text}");
        assert!(text.contains("column 1"), "{text}");
    }

    #[test]
    fn modified_marker_appears_only_when_dirty() {
        let mut app = EditorApp::new();
        let clean = info_text(&mut app);
        assert!(!clean.contains("modified"), "{clean}");
        app.active_buffer_mut().unwrap().insert(0, "x");
        let dirty = info_text(&mut app);
        assert!(dirty.contains("(modified)"), "{dirty}");
    }

    #[test]
    fn selection_size_appears_only_when_selected() {
        let mut app = EditorApp::new();
        let plain = info_text(&mut app);
        assert!(!plain.contains("selected"), "{plain}");
        app.active_buffer_mut().unwrap().insert(0, "hello");
        {
            // Mark at byte 0, cursor at byte 5: the selection derives
            // from the mark. The window starts with the cursor at 0,
            // so move it to the end of "hello" first.
            let window_id = app.active_window;
            let window = app.windows.get_mut(&window_id).unwrap();
            window.cursor_x = 5;
            window.selection_manager.set_mark(0);
        }
        let selected = info_text(&mut app);
        assert!(selected.contains("5 bytes selected"), "{selected}");
    }

    #[test]
    fn diagnostics_appear_only_when_present() {
        use crate::core::diagnostics::{Diagnostic, DiagnosticSeverity};
        let mut app = EditorApp::new();
        let plain = info_text(&mut app);
        assert!(!plain.contains("error"), "{plain}");
        assert!(!plain.contains("warning"), "{plain}");
        {
            let buffer = app.active_buffer_mut().unwrap();
            buffer.insert(0, "one\ntwo\n");
            buffer.add_diagnostic(Diagnostic::new(
                DiagnosticSeverity::Error,
                "main.rs",
                1,
                "boom",
            ));
            buffer.add_diagnostic(Diagnostic::new(
                DiagnosticSeverity::Warning,
                "main.rs",
                2,
                "careful",
            ));
        }
        let text = info_text(&mut app);
        assert!(text.contains("1 error"), "{text}");
        assert!(text.contains("1 warning"), "{text}");
    }

    #[test]
    fn cursor_position_tracks_the_window() {
        let mut app = EditorApp::new();
        app.active_buffer_mut().unwrap().insert(0, "one\ntwo\nthree\n");
        {
            let window_id = app.active_window;
            let window = app.windows.get_mut(&window_id).unwrap();
            window.cursor_y = 2;
            window.cursor_x = 3;
        }
        let text = info_text(&mut app);
        assert!(text.contains("line 3 of 4"), "{text}");
        assert!(text.contains("column 4"), "{text}");
    }
}
