//! Accessibility semantic layer tests.
//!
//! Annotation derivation, snapshot rules, spoken-state rendering,
//! announcement diffs, and cue wiring.
//!
//! The contract under test: annotations are derived from authoritative
//! state at snapshot time (diagnostics from the diagnostic list,
//! selection and marks from the window), so they track edits with no
//! incremental bookkeeping and can never go stale.

use erax::core::bindings::{ModalMode, ModalOperator};
use erax::core::app::EditorApp;
use erax::core::cue::{Cue, CueBus};
use erax::core::diagnostics::{Diagnostic, DiagnosticSeverity};
use erax::core::id::BufferId;
use erax::core::semantics::{
    AnnotationKind, ByteRange, DocumentSnapshot, ModeView, SnapshotSummary, TokenAnnotation,
    TokenClass, announce_diff, capture, capture_summary, render_spoken_state,
};

fn app_with(text: &str) -> EditorApp {
    let mut app = EditorApp::new();
    app.cue_bus = CueBus::new(true);
    app.active_buffer_mut().unwrap().insert(0, text);
    app
}

fn error_on(app: &mut EditorApp, line: usize, message: &str) {
    app.active_buffer_mut().unwrap().add_diagnostic(Diagnostic::new(
        DiagnosticSeverity::Error,
        "main.rs",
        line,
        message,
    ));
}

fn diagnostic_ranges(app: &EditorApp) -> Vec<ByteRange> {
    capture(app)
        .unwrap()
        .annotations
        .iter()
        .filter(|a| matches!(a.kind, AnnotationKind::Diagnostic(_)))
        .map(|a| a.range)
        .collect()
}

#[test]
fn diagnostic_annotations_cover_their_lines() {
    let mut app = app_with("one\ntwo\nthree\n");
    error_on(&mut app, 1, "first");
    error_on(&mut app, 3, "third");
    let ranges = diagnostic_ranges(&app);
    // Line 1 is bytes 0..4 ("one\n"), line 3 is bytes 8..14 ("three\n").
    assert_eq!(
        ranges,
        vec![
            ByteRange { start: 0, end: 4 },
            ByteRange { start: 8, end: 14 },
        ]
    );
}

#[test]
fn diagnostic_detail_carries_the_message() {
    let mut app = app_with("one\n");
    error_on(&mut app, 1, "expected semicolon");
    let annotations = capture(&app).unwrap().annotations;
    let detail = annotations
        .iter()
        .find(|a| matches!(a.kind, AnnotationKind::Diagnostic(_)))
        .and_then(|a| a.detail.clone());
    assert_eq!(detail.as_deref(), Some("expected semicolon"));
}

#[test]
fn annotations_track_insertions_before_their_line() {
    let mut app = app_with("one\ntwo\nthree\n");
    error_on(&mut app, 3, "third");
    // Insert a line above: the diagnostic's line moves from 3 to 4,
    // and the derived range follows it ("zero\n" is 5 bytes, so the
    // line that was 8..14 is now 13..19).
    app.active_buffer_mut().unwrap().insert(0, "zero\n");
    assert_eq!(
        diagnostic_ranges(&app),
        vec![ByteRange { start: 13, end: 19 }]
    );
}

#[test]
fn annotations_track_edits_inside_their_line() {
    let mut app = app_with("one\ntwo\nthree\n");
    error_on(&mut app, 2, "second");
    // Lengthen line 2: the range still covers exactly the line.
    app.active_buffer_mut().unwrap().insert(4, "XXX");
    assert_eq!(
        diagnostic_ranges(&app),
        vec![ByteRange { start: 4, end: 11 }]
    );
}

#[test]
fn annotations_track_deletions_above_their_line() {
    let mut app = app_with("one\ntwo\nthree\n");
    error_on(&mut app, 3, "third");
    app.active_buffer_mut().unwrap().delete(0, 8);
    assert_eq!(
        diagnostic_ranges(&app),
        vec![ByteRange { start: 0, end: 6 }]
    );
}

#[test]
fn annotations_vanish_with_their_line() {
    let mut app = app_with("one\ntwo\nthree\n");
    error_on(&mut app, 2, "second");
    // Delete line 2 wholesale: no line, no annotation. A stale range
    // pointing at another line's bytes would be worse than none.
    app.active_buffer_mut().unwrap().delete(4, 4);
    assert!(diagnostic_ranges(&app).is_empty());
}

#[test]
fn selection_annotation_reflects_the_selection_manager() {
    let mut app = app_with("hello world");
    let snap = capture(&app).unwrap();
    assert!(snap.selection.is_none());
    assert!(!snap
        .annotations
        .iter()
        .any(|a| matches!(a.kind, AnnotationKind::Selection)));

    let window_id = app.active_window;
    app.windows.get_mut(&window_id).unwrap().selection_manager.primary =
        Some(erax::core::selection::Selection::range(0, 5));

    let snap = capture(&app).unwrap();
    assert_eq!(
        snap.selection,
        Some(ByteRange { start: 0, end: 5 })
    );
    let sel: Vec<_> = snap
        .annotations
        .iter()
        .filter(|a| matches!(a.kind, AnnotationKind::Selection))
        .collect();
    assert_eq!(sel.len(), 1);
    assert_eq!(sel[0].range, ByteRange { start: 0, end: 5 });

    // Clearing the selection removes the annotation: the snapshot
    // mirrors the manager, never a cached copy.
    app.windows
        .get_mut(&window_id)
        .unwrap()
        .selection_manager
        .primary = None;
    let snap = capture(&app).unwrap();
    assert!(snap.selection.is_none());
    assert!(!snap
        .annotations
        .iter()
        .any(|a| matches!(a.kind, AnnotationKind::Selection)));
}

#[test]
fn mark_annotation_is_a_point_at_the_mark_byte() {
    let mut app = app_with("ab\ncd\n");
    let window_id = app.active_window;
    // Mark at grapheme (1, 1): line 2 (0-based 1), column 1 -> byte 4.
    app.windows.get_mut(&window_id).unwrap().mark = Some((1, 1));
    let snap = capture(&app).unwrap();
    assert_eq!(snap.mark, Some(4));
    let marks: Vec<_> = snap
        .annotations
        .iter()
        .filter(|a| matches!(a.kind, AnnotationKind::Mark))
        .collect();
    assert_eq!(marks.len(), 1);
    assert_eq!(marks[0].range, ByteRange { start: 4, end: 4 });
}

#[test]
fn token_store_maps_into_the_snapshot() {
    // The plug-in seam for the future highlighter: whatever the store
    // holds, the snapshot maps, without knowing who filled it.
    let mut app = app_with("fn main() {}");
    app.active_buffer_mut().unwrap().set_token_annotations(vec![
        TokenAnnotation {
            range: ByteRange { start: 0, end: 2 },
            class: TokenClass::Keyword,
        },
        TokenAnnotation {
            range: ByteRange { start: 3, end: 7 },
            class: TokenClass::Function,
        },
    ]);
    let snap = capture(&app).unwrap();
    let tokens: Vec<_> = snap
        .annotations
        .iter()
        .filter(|a| matches!(a.kind, AnnotationKind::Token(_)))
        .collect();
    assert_eq!(tokens.len(), 2);
    assert_eq!(tokens[0].kind, AnnotationKind::Token(TokenClass::Keyword));
    assert_eq!(tokens[0].range, ByteRange { start: 0, end: 2 });
    assert_eq!(tokens[1].kind, AnnotationKind::Token(TokenClass::Function));
}

#[test]
fn snapshot_carries_window_and_buffer_lists() {
    let app = app_with("x");
    let snap = capture(&app).unwrap();
    assert_eq!(snap.windows.len(), 1);
    assert_eq!(snap.buffers.len(), 1);
    assert_eq!(snap.buffer_name, "*scratch*");
    // app_with inserted text, so the buffer is dirty: the snapshot
    // reports the real flag, not a constant.
    assert!(snap.modified);
    assert_eq!(snap.line_count, 1);
    // Test builds compile with the KeyTrie input model.
    assert_eq!(snap.mode, ModeView::Emacs);
}

#[test]
fn snapshot_text_access_goes_through_the_buffer() {
    let app = app_with("hello world");
    let snap = capture(&app).unwrap();
    assert_eq!(snap.buffer.get_range_as_string(0, 5), "hello");
    // The snapshot's buffer id is the live window's buffer id:
    // identity, not a literal.
    let window_id = app.active_window;
    let window = app.windows.get(&window_id).unwrap();
    assert_eq!(snap.buffer_id, window.buffer_id);
}

/// Build a synthetic snapshot to test rendering rules for mode
/// states the test build's compile-time input model cannot produce.
fn synthetic_snapshot<'a>(
    buffer: &'a erax::core::buffer::Buffer,
    mode: ModeView,
) -> DocumentSnapshot<'a> {
    DocumentSnapshot {
        buffer,
        buffer_id: BufferId(0),
        buffer_name: "*scratch*".to_string(),
        modified: false,
        line_count: 1,
        cursor: erax::core::semantics::CursorView {
            line: 0,
            column: 0,
            byte: 0,
        },
        selection: None,
        mark: None,
        annotations: Vec::new(),
        mode,
        message: None,
        diagnostics: Default::default(),
        windows: Vec::new(),
        buffers: Vec::new(),
    }
}

#[test]
fn spoken_state_names_vim_substate() {
    let app = app_with("x");
    let buffer = app.active_buffer().unwrap();

    let snap = synthetic_snapshot(
        buffer,
        ModeView::Vim {
            mode: ModalMode::Insert,
            count: 0,
            operator: None,
        },
    );
    let text = render_spoken_state(&snap);
    assert!(text.contains("Vim insert"), "{text}");
    assert!(!text.contains("pending"), "{text}");

    let snap = synthetic_snapshot(
        buffer,
        ModeView::Vim {
            mode: ModalMode::Normal,
            count: 3,
            operator: Some(ModalOperator::Delete),
        },
    );
    let text = render_spoken_state(&snap);
    assert!(text.contains("Vim normal"), "{text}");
    assert!(text.contains("delete pending"), "{text}");
    assert!(text.contains("count 3"), "{text}");
}

#[test]
fn spoken_state_reports_standard_and_emacs() {
    let app = app_with("x");
    let buffer = app.active_buffer().unwrap();
    let text = render_spoken_state(&synthetic_snapshot(buffer, ModeView::Standard));
    assert!(text.contains("Standard"), "{text}");
    let text = render_spoken_state(&synthetic_snapshot(buffer, ModeView::Emacs));
    assert!(text.contains("Emacs"), "{text}");
}

fn summary() -> SnapshotSummary {
    SnapshotSummary {
        buffer_id: BufferId(0),
        buffer_name: "*scratch*".to_string(),
        mode: ModeView::Emacs,
        message: None,
        cursor_line: 0,
        selection_bytes: 0,
        errors: 0,
        warnings: 0,
    }
}

#[test]
fn announce_diff_silent_when_nothing_changed() {
    let s = summary();
    assert_eq!(announce_diff(&s, &s, true), None);
}

#[test]
fn announce_diff_names_each_change_once() {
    let old = summary();
    let mut new = summary();
    new.cursor_line = 4;
    new.errors = 2;
    new.warnings = 1;
    new.selection_bytes = 10;
    new.message = Some("Saved".to_string());
    let text = announce_diff(&old, &new, true).expect("must announce");
    assert!(text.contains("line 5"), "{text}");
    assert!(text.contains("2 errors"), "{text}");
    assert!(text.contains("1 warning"), "{text}");
    assert!(text.contains("10 bytes selected"), "{text}");
    assert!(text.contains("Saved"), "{text}");
}

#[test]
fn announce_diff_column_moves_are_never_announced() {
    // Summaries have no column: only the line is comparable, so a
    // same-line move is silence by construction.
    let old = summary();
    let new = summary();
    assert_eq!(announce_diff(&old, &new, true), None);
}

#[test]
fn announce_diff_cursor_line_respects_the_flag() {
    let old = summary();
    let mut new = summary();
    new.cursor_line = 2;
    assert!(announce_diff(&old, &new, true).is_some());
    assert_eq!(announce_diff(&old, &new, false), None);
}

#[test]
fn announce_diff_mode_change() {
    let old = summary();
    let mut new = summary();
    new.mode = ModeView::Vim {
        mode: ModalMode::Insert,
        count: 0,
        operator: None,
    };
    let text = announce_diff(&old, &new, true).expect("must announce");
    assert!(text.contains("Insert mode"), "{text}");
}

#[test]
fn announce_diff_buffer_switch() {
    let old = summary();
    let mut new = summary();
    new.buffer_id = BufferId(7);
    new.buffer_name = "main.rs".to_string();
    let text = announce_diff(&old, &new, true).expect("must announce");
    assert!(text.contains("Buffer main.rs"), "{text}");
}

#[test]
fn announce_diff_diagnostics_clearing() {
    let mut old = summary();
    old.errors = 1;
    let new = summary();
    let text = announce_diff(&old, &new, true).expect("must announce");
    assert!(text.contains("no errors or warnings"), "{text}");
}

#[test]
fn announce_diff_selection_cleared() {
    let mut old = summary();
    old.selection_bytes = 12;
    let new = summary();
    let text = announce_diff(&old, &new, true).expect("must announce");
    assert!(text.contains("selection cleared"), "{text}");
}

#[test]
fn save_success_emits_save_completed_cue() {
    let dir = std::env::temp_dir().join("erax-a11y-save-ok");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("note.txt");
    let mut app = app_with("hello");
    app.active_buffer_mut().unwrap().filename = Some(path.clone());
    let result = erax::core::dispatcher::dispatch(
        &mut app,
        erax::core::command::Command::SaveBuffer,
        1,
    );
    assert_eq!(result, erax::core::dispatcher::DispatchResult::Success);
    let cues: Vec<Cue> = app.cue_bus.drain().collect();
    assert_eq!(cues, vec![Cue::SaveCompleted]);
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "hello");
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn save_failure_emits_error_cue_and_typed_message() {
    let mut app = app_with("hello");
    // No such directory: the atomic write cannot create its temp file.
    app.active_buffer_mut().unwrap().filename =
        Some(std::path::PathBuf::from("/erax-no-such-dir-xyz/note.txt"));
    let result = erax::core::dispatcher::dispatch(
        &mut app,
        erax::core::command::Command::SaveBuffer,
        1,
    );
    match result {
        erax::core::dispatcher::DispatchResult::Info(message) => {
            assert!(message.contains("Save failed"), "{message}");
        }
        other => panic!("expected Info, got {other:?}"),
    }
    let cues: Vec<Cue> = app.cue_bus.drain().collect();
    assert_eq!(cues, vec![Cue::Error]);
}

#[test]
fn reload_question_emits_its_cue_once() {
    let mut app = app_with("hello");
    erax::core::focus::push_reload_question(&mut app, BufferId(0));
    let cues: Vec<Cue> = app.cue_bus.drain().collect();
    assert_eq!(cues, vec![Cue::ReloadQuestionAsked]);
    // Asking again for the same buffer is a no-op: no second cue.
    erax::core::focus::push_reload_question(&mut app, BufferId(0));
    assert_eq!(app.cue_bus.pending(), 0);
}

#[test]
fn summary_agrees_with_full_capture() {
    let mut app = app_with("one\ntwo\n");
    error_on(&mut app, 2, "bad");
    let full = capture(&app).unwrap();
    let sum = capture_summary(&app).unwrap();
    assert_eq!(sum.buffer_id, full.buffer_id);
    assert_eq!(sum.buffer_name, full.buffer_name);
    assert_eq!(sum.mode, full.mode);
    assert_eq!(sum.message, full.message);
    assert_eq!(sum.cursor_line, full.cursor.line);
    assert_eq!(sum.errors, full.diagnostics.errors);
    assert_eq!(sum.warnings, full.diagnostics.warnings);
}

#[test]
fn unresolvable_anchors_are_left_untouched_by_edits() {
    // A diagnostic pointing past the end of the buffer (line 50 of 3
    // lines) or at line 0 can never resolve. Edits must not delete
    // it and must not rewrite it: the producer owns that data.
    let mut app = app_with("one\ntwo\nthree\n");
    error_on(&mut app, 50, "far away");
    {
        let buffer = app.active_buffer_mut().unwrap();
        buffer.add_diagnostic(Diagnostic::new(
            DiagnosticSeverity::Warning,
            "main.rs",
            0,
            "line zero",
        ));
    }
    app.active_buffer_mut().unwrap().insert(0, "x");
    app.active_buffer_mut().unwrap().delete(0, 2);
    let buffer = app.active_buffer().unwrap();
    assert_eq!(buffer.diagnostics.len(), 2);
    assert_eq!(buffer.diagnostics[0].line, 50);
    assert_eq!(buffer.diagnostics[0].column, None);
    assert_eq!(buffer.diagnostics[1].line, 0);
    // Unresolvable diagnostics produce no annotations, but they are
    // still counted in the summary.
    let snap = capture(&app).unwrap();
    assert!(snap
        .annotations
        .iter()
        .all(|a| !matches!(a.kind, AnnotationKind::Diagnostic(_))));
    assert_eq!(snap.diagnostics.errors, 1);
    assert_eq!(snap.diagnostics.warnings, 1);
}

#[test]
fn undo_restores_diagnostics_dropped_by_delete() {
    // Deleting a diagnostic's whole line drops it; undo brings the
    // text back, so the diagnostic comes back too. Redo re-drops it.
    let mut app = app_with("one\ntwo\nthree\n");
    error_on(&mut app, 2, "bad");
    assert_eq!(diagnostic_ranges(&app).len(), 1);
    // Delete the second line ("two\n", bytes 4..8).
    app.active_buffer_mut().unwrap().delete(4, 4);
    assert!(diagnostic_ranges(&app).is_empty());
    assert!(app.active_buffer().unwrap().diagnostics.is_empty());
    app.active_buffer_mut().unwrap().undo();
    assert_eq!(diagnostic_ranges(&app).len(), 1);
    assert_eq!(diagnostic_ranges(&app)[0], ByteRange { start: 4, end: 8 });
    app.active_buffer_mut().unwrap().redo();
    assert!(diagnostic_ranges(&app).is_empty());
    // And undo again: the diagnostic returns a second time.
    app.active_buffer_mut().unwrap().undo();
    assert_eq!(diagnostic_ranges(&app).len(), 1);
}

#[test]
fn column_none_stays_none_through_shifts() {
    // A diagnostic added without a column keeps `column: None` while
    // its line tracks edits: the column was never specified.
    let mut app = app_with("one\ntwo\nthree\n");
    error_on(&mut app, 3, "bad");
    app.active_buffer_mut().unwrap().insert(0, "zero\n");
    let buffer = app.active_buffer().unwrap();
    assert_eq!(buffer.diagnostics.len(), 1);
    assert_eq!(buffer.diagnostics[0].line, 4);
    assert_eq!(buffer.diagnostics[0].column, None);
}
