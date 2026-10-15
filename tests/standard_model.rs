//! Standard input-model tests (N-268) plus shared editing-core tests.
//!
//! The Standard-only branches in the editing commands are compile-time gated
//! on `INPUT_MODEL`, which is `KeyTrie` in test builds, so these tests drive
//! the model-parameterized `*_in_model` entry points with
//! `InputModel::Standard` explicitly. That is the whole point of the
//! refactor: the Standard paths are now reachable in tests while the
//! production call sites still pass the const (zero-cost).
//!
//! Where the three models intentionally differ (selection replacement on
//! type; clipboard routing), the difference is asserted explicitly. The
//! shared-core tests at the bottom prove everything else is byte-identical
//! across models: one editing core, three input models.

use erax::config::keys::{INPUT_MODEL, InputModel, STANDARD_BINDINGS};
use erax::core::app::EditorApp;
use erax::core::command::Command;
use erax::core::commands::editing::{
    delete_next_character_in_model, delete_previous_character_in_model,
};
use erax::core::commands::kill_ring::yank_in_model;
use erax::core::commands::marks::{copy_region_in_model, kill_region_in_model};
use erax::core::dispatcher::{dispatch, dispatch_insert, dispatch_insert_in_model, DispatchResult};
use erax::core::input::{Key, KeyPress};
use erax::terminal::keybinds::{KeyBindingManager, KeyOutcome};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// App with `text` in the active buffer, cursor at (0,0), engine ready.
fn app_with(text: &str) -> EditorApp {
    let mut app = EditorApp::new();
    app.active_buffer_mut().unwrap().insert(0, text);
    // Fresh undo history so tests observe only engine-driven edits.
    app.active_buffer_mut().unwrap().undo_stack.clear();
    app.active_buffer_mut().unwrap().redo_stack.clear();
    app
}

fn text(app: &EditorApp) -> String {
    let b = app.active_buffer().unwrap();
    b.get_range_as_string(0, b.len())
}

fn cursor(app: &EditorApp) -> (usize, usize) {
    let w = app.active_window_ref().unwrap();
    (w.cursor_x, w.cursor_y)
}

fn set_cursor(app: &mut EditorApp, x: usize, y: usize) {
    let w = app.active_window_mut().unwrap();
    w.cursor_x = x;
    w.cursor_y = y;
}

/// Set the mark (selection anchor) directly; cursor stays where it is.
fn set_mark(app: &mut EditorApp, x: usize, y: usize) {
    app.active_window_mut().unwrap().mark = Some((x, y));
}

fn mark(app: &EditorApp) -> Option<(usize, usize)> {
    app.active_window_ref().unwrap().mark
}

/// KeyBindingManager loaded with the Standard (CUA) binding table.
fn standard_manager() -> KeyBindingManager {
    let mut m = KeyBindingManager::new();
    m.load(STANDARD_BINDINGS);
    m
}

fn ctrl(c: char) -> KeyPress {
    KeyPress::ctrl(Key::Char(c))
}

fn shift_ctrl_z() -> KeyPress {
    KeyPress::mods(Key::Char('z'), true, false, true)
}

/// Process one keypress through the Standard table; panic unless it resolves
/// to exactly the expected command.
fn resolve(mgr: &mut KeyBindingManager, press: KeyPress, expected: Command) {
    assert_eq!(mgr.process_key(press), KeyOutcome::Command(expected));
}

/// Resolve a keypress to a command and dispatch it (count 1).
fn press_cmd(app: &mut EditorApp, mgr: &mut KeyBindingManager, press: KeyPress) -> DispatchResult {
    let cmd = match mgr.process_key(press) {
        KeyOutcome::Command(c) => c,
        other => panic!("expected Command, got {other:?}"),
    };
    dispatch(app, cmd, 1)
}

// ---------------------------------------------------------------------------
// Binding table
// ---------------------------------------------------------------------------

#[test]
fn test_build_uses_keytrie_runtime_model() {
    // Documents WHY the *_in_model entry points exist: in test builds the
    // runtime INPUT_MODEL is KeyTrie, so Standard branches are unreachable
    // through the plain public functions. The in_model fns take the model
    // explicitly so tests can exercise the Standard paths.
    assert_eq!(INPUT_MODEL, InputModel::KeyTrie);
}

#[test]
fn standard_clipboard_bindings() {
    let mut m = standard_manager();
    resolve(&mut m, ctrl('c'), Command::CopyRegion);
    resolve(&mut m, ctrl('x'), Command::KillRegion);
    resolve(&mut m, ctrl('v'), Command::Yank);
}

#[test]
fn standard_undo_redo_bindings() {
    let mut m = standard_manager();
    resolve(&mut m, ctrl('z'), Command::Undo);
    // Dual redo: BOTH Ctrl+Y and Ctrl+Shift+Z resolve to Redo.
    resolve(&mut m, ctrl('y'), Command::Redo);
    resolve(&mut m, shift_ctrl_z(), Command::Redo);
}

#[test]
fn standard_file_bindings() {
    let mut m = standard_manager();
    resolve(&mut m, ctrl('s'), Command::SaveBuffer);
    resolve(&mut m, ctrl('o'), Command::FindFile);
    resolve(&mut m, ctrl('n'), Command::NewBuffer);
    resolve(&mut m, ctrl('w'), Command::DeleteBuffer);
}

#[test]
fn standard_select_all_and_search_bindings() {
    let mut m = standard_manager();
    resolve(&mut m, ctrl('a'), Command::MarkWholeBuffer);
    resolve(&mut m, ctrl('f'), Command::SearchForward);
    resolve(&mut m, ctrl('h'), Command::QueryReplace);
}

#[test]
fn standard_movement_bindings() {
    let mut m = standard_manager();
    resolve(&mut m, KeyPress::plain(Key::Right), Command::ForwardCharacter);
    resolve(&mut m, KeyPress::plain(Key::Left), Command::BackwardCharacter);
    resolve(&mut m, KeyPress::plain(Key::Down), Command::NextLine);
    resolve(&mut m, KeyPress::plain(Key::Up), Command::PreviousLine);
    resolve(&mut m, KeyPress::plain(Key::Home), Command::SmartHome);
    resolve(&mut m, KeyPress::plain(Key::End), Command::EndOfLine);
    resolve(&mut m, KeyPress::plain(Key::PageUp), Command::BackwardPage);
    resolve(&mut m, KeyPress::plain(Key::PageDown), Command::ForwardPage);
    // Ctrl+arrows: word-wise movement.
    resolve(&mut m, KeyPress::ctrl(Key::Left), Command::BackwardWord);
    resolve(&mut m, KeyPress::ctrl(Key::Right), Command::ForwardWord);
    // Ctrl+Home/End: document start/end.
    resolve(&mut m, KeyPress::ctrl(Key::Home), Command::BeginningOfFile);
    resolve(&mut m, KeyPress::ctrl(Key::End), Command::EndOfFile);
    // Buffer cycling.
    resolve(&mut m, KeyPress::ctrl(Key::Tab), Command::NextBuffer);
    resolve(
        &mut m,
        KeyPress::mods(Key::Tab, true, false, true),
        Command::PreviousBuffer,
    );
}

#[test]
fn standard_shift_selection_bindings() {
    let mut m = standard_manager();
    resolve(
        &mut m,
        KeyPress::shift(Key::Right),
        Command::ExtendSelectionRight,
    );
    resolve(&mut m, KeyPress::shift(Key::Left), Command::ExtendSelectionLeft);
    resolve(&mut m, KeyPress::shift(Key::Down), Command::ExtendSelectionDown);
    resolve(&mut m, KeyPress::shift(Key::Up), Command::ExtendSelectionUp);
    resolve(
        &mut m,
        KeyPress::shift(Key::Home),
        Command::ExtendSelectionHome,
    );
    resolve(&mut m, KeyPress::shift(Key::End), Command::ExtendSelectionEnd);
}

#[test]
fn standard_deletion_bindings() {
    let mut m = standard_manager();
    resolve(
        &mut m,
        KeyPress::plain(Key::Backspace),
        Command::DeletePreviousCharacter,
    );
    resolve(
        &mut m,
        KeyPress::plain(Key::Delete),
        Command::DeleteNextCharacter,
    );
    resolve(&mut m, KeyPress::plain(Key::Enter), Command::InsertNewline);
    resolve(&mut m, KeyPress::plain(Key::Tab), Command::InsertTab);
    resolve(
        &mut m,
        KeyPress::plain(Key::Insert),
        Command::ToggleOverwriteMode,
    );
}

#[test]
fn standard_ctrl_c_is_immediate_not_prefix() {
    // In the KeyTrie table ^C waits for a longer sequence; in the Standard
    // table it must resolve immediately to CopyRegion (no AwaitingLonger).
    let mut m = standard_manager();
    assert_eq!(
        m.process_key(ctrl('c')),
        KeyOutcome::Command(Command::CopyRegion)
    );
    assert!(!m.is_awaiting());
}

#[test]
fn standard_typing_produces_insert_char() {
    // Plain printable keys are not commands in the Standard table: they come
    // back as InsertChar, which the event loop routes to dispatch_insert.
    let mut m = standard_manager();
    assert_eq!(
        m.process_key(KeyPress::plain(Key::Char('x'))),
        KeyOutcome::InsertChar('x')
    );
}

// ---------------------------------------------------------------------------
// SmartHome
// ---------------------------------------------------------------------------

#[test]
fn smart_home_two_stage() {
    let mut app = app_with("    foo\n");
    assert_eq!(cursor(&app), (0, 0));
    // First press: first non-whitespace column.
    dispatch(&mut app, Command::SmartHome, 1);
    assert_eq!(cursor(&app), (4, 0));
    // Second press: column 0.
    dispatch(&mut app, Command::SmartHome, 1);
    assert_eq!(cursor(&app), (0, 0));
}

#[test]
fn smart_home_from_end_of_indented_line() {
    let mut app = app_with("    foo\n");
    set_cursor(&mut app, 7, 0);
    dispatch(&mut app, Command::SmartHome, 1);
    assert_eq!(cursor(&app), (4, 0));
    dispatch(&mut app, Command::SmartHome, 1);
    assert_eq!(cursor(&app), (0, 0));
}

#[test]
fn smart_home_via_home_key_binding() {
    // Home is bound to SmartHome (not BeginningOfLine) in the Standard table.
    let mut app = app_with("    foo\n");
    let mut mgr = standard_manager();
    set_cursor(&mut app, 7, 0);
    press_cmd(&mut app, &mut mgr, KeyPress::plain(Key::Home));
    assert_eq!(cursor(&app), (4, 0));
}

// ---------------------------------------------------------------------------
// MarkWholeBuffer + ExtendSelection
// ---------------------------------------------------------------------------

#[test]
fn mark_whole_buffer_selects_everything() {
    let mut app = app_with("hello\nworld\n");
    dispatch(&mut app, Command::MarkWholeBuffer, 1);
    // Mark anchored at buffer start, cursor at buffer end.
    assert_eq!(mark(&app), Some((0, 0)));
    assert_eq!(cursor(&app), (0, 2));
    // The whole buffer is now the selection: Standard copy grabs it all.
    copy_region_in_model(&mut app, 1, InputModel::Standard);
    assert_eq!(app.clipboard, Some("hello\nworld\n".to_string()));
}

#[test]
fn extend_selection_all_directions() {
    // Right.
    let mut app = app_with("hello\n");
    press_cmd(
        &mut app,
        &mut standard_manager(),
        KeyPress::shift(Key::Right),
    );
    assert_eq!(mark(&app), Some((0, 0)));
    assert_eq!(cursor(&app), (1, 0));

    // Left (anchor stays where the cursor was when the shift press started).
    let mut app = app_with("hello\n");
    set_cursor(&mut app, 3, 0);
    press_cmd(&mut app, &mut standard_manager(), KeyPress::shift(Key::Left));
    assert_eq!(mark(&app), Some((3, 0)));
    assert_eq!(cursor(&app), (2, 0));

    // Down.
    let mut app = app_with("ab\ncd\n");
    set_cursor(&mut app, 1, 0);
    press_cmd(&mut app, &mut standard_manager(), KeyPress::shift(Key::Down));
    assert_eq!(mark(&app), Some((1, 0)));
    assert_eq!(cursor(&app), (1, 1));

    // Up.
    let mut app = app_with("ab\ncd\n");
    set_cursor(&mut app, 1, 1);
    press_cmd(&mut app, &mut standard_manager(), KeyPress::shift(Key::Up));
    assert_eq!(mark(&app), Some((1, 1)));
    assert_eq!(cursor(&app), (1, 0));

    // Home: Shift+Home extends to the smart-home position.
    let mut app = app_with("    foo\n");
    set_cursor(&mut app, 4, 0);
    press_cmd(&mut app, &mut standard_manager(), KeyPress::shift(Key::Home));
    assert_eq!(mark(&app), Some((4, 0)));
    assert_eq!(cursor(&app), (0, 0));

    // End.
    let mut app = app_with("hello\n");
    set_cursor(&mut app, 2, 0);
    press_cmd(&mut app, &mut standard_manager(), KeyPress::shift(Key::End));
    assert_eq!(mark(&app), Some((2, 0)));
    assert_eq!(cursor(&app), (5, 0));
}

#[test]
fn shift_arrow_extension_selects_real_text() {
    // End-to-end: Shift+Right twice from (0,0), then Standard copy/cut.
    // NOTE: plain `dispatch` always uses the compiled INPUT_MODEL (KeyTrie in
    // test builds), so clipboard routing is driven through the in_model entry
    // points; the binding table resolution and the selection machinery above
    // them are the parts exercised end-to-end here.
    let mut app = app_with("hello\n");
    let mut mgr = standard_manager();
    press_cmd(&mut app, &mut mgr, KeyPress::shift(Key::Right));
    press_cmd(&mut app, &mut mgr, KeyPress::shift(Key::Right));
    assert_eq!(cursor(&app), (2, 0));
    copy_region_in_model(&mut app, 1, InputModel::Standard);
    assert_eq!(app.clipboard, Some("he".to_string()));
    assert_eq!(text(&app), "hello\n"); // copy leaves the buffer intact
    // And cut removes it.
    set_mark(&mut app, 0, 0);
    kill_region_in_model(&mut app, 1, InputModel::Standard);
    assert_eq!(app.clipboard, Some("he".to_string()));
    assert_eq!(text(&app), "llo\n");
}

// ---------------------------------------------------------------------------
// Typing replaces selection (Standard) vs preserves mark (KeyTrie/Modal)
// ---------------------------------------------------------------------------

#[test]
fn standard_typing_replaces_selection() {
    let mut app = app_with("hello");
    set_mark(&mut app, 0, 0);
    set_cursor(&mut app, 3, 0); // selection covers "hel"
    dispatch_insert_in_model(&mut app, 'X', 1, InputModel::Standard);
    assert_eq!(text(&app), "Xlo");
    // The selection is consumed by the replacement.
    assert_eq!(mark(&app), None);
}

#[test]
fn keytrie_typing_preserves_mark() {
    // Documented intentional difference: in the emacs/trie model the mark is
    // NOT consumed by typing; the character inserts at the cursor.
    let mut app = app_with("hello");
    set_mark(&mut app, 0, 0);
    set_cursor(&mut app, 3, 0);
    dispatch_insert_in_model(&mut app, 'X', 1, InputModel::KeyTrie);
    assert_eq!(text(&app), "helXlo");
    assert_eq!(mark(&app), Some((0, 0)));
}

#[test]
fn modal_typing_preserves_mark() {
    let mut app = app_with("hello");
    set_mark(&mut app, 0, 0);
    set_cursor(&mut app, 3, 0);
    dispatch_insert_in_model(&mut app, 'X', 1, InputModel::Modal);
    assert_eq!(text(&app), "helXlo");
    assert_eq!(mark(&app), Some((0, 0)));
}

#[test]
fn standard_backspace_deletes_selection_first() {
    let mut app = app_with("hello");
    set_mark(&mut app, 0, 0);
    set_cursor(&mut app, 3, 0);
    delete_previous_character_in_model(&mut app, 1, InputModel::Standard);
    assert_eq!(text(&app), "lo");
    assert_eq!(mark(&app), None);
    // Deleting a selection is not a kill: the kill ring is untouched.
    assert!(app.kill_ring.peek().is_none());
}

#[test]
fn keytrie_backspace_preserves_mark_behavior() {
    // Documented intentional difference: without the Standard branch,
    // Backspace deletes the char before point and leaves the mark alone.
    let mut app = app_with("hello");
    set_mark(&mut app, 0, 0);
    set_cursor(&mut app, 3, 0);
    delete_previous_character_in_model(&mut app, 1, InputModel::KeyTrie);
    assert_eq!(text(&app), "helo");
    assert_eq!(mark(&app), Some((0, 0)));
}

#[test]
fn standard_delete_deletes_selection_first() {
    let mut app = app_with("hello");
    set_mark(&mut app, 0, 0);
    set_cursor(&mut app, 3, 0);
    delete_next_character_in_model(&mut app, 1, InputModel::Standard);
    assert_eq!(text(&app), "lo");
    assert_eq!(mark(&app), None);
}

#[test]
fn standard_backspace_without_selection_deletes_char() {
    // No selection: Standard Backspace behaves exactly like the shared core.
    let mut app = app_with("hello");
    set_cursor(&mut app, 3, 0);
    delete_previous_character_in_model(&mut app, 1, InputModel::Standard);
    assert_eq!(text(&app), "helo");
}

// ---------------------------------------------------------------------------
// Clipboard vs kill ring
// ---------------------------------------------------------------------------

#[test]
fn standard_copy_uses_clipboard_not_kill_ring() {
    let mut app = app_with("hello world");
    set_mark(&mut app, 0, 0);
    set_cursor(&mut app, 5, 0); // selects "hello"
    copy_region_in_model(&mut app, 1, InputModel::Standard);
    assert_eq!(app.clipboard, Some("hello".to_string()));
    assert!(app.kill_ring.peek().is_none());
    // Copy leaves the buffer intact.
    assert_eq!(text(&app), "hello world");
}

#[test]
fn standard_cut_uses_clipboard_not_kill_ring() {
    let mut app = app_with("hello world");
    set_mark(&mut app, 0, 0);
    set_cursor(&mut app, 5, 0);
    kill_region_in_model(&mut app, 1, InputModel::Standard);
    assert_eq!(app.clipboard, Some("hello".to_string()));
    assert!(app.kill_ring.peek().is_none());
    assert_eq!(text(&app), " world");
}

#[test]
fn keytrie_copy_and_cut_use_kill_ring_not_clipboard() {
    // Documented intentional difference: the emacs/trie model routes
    // copy/cut through the kill ring; the Standard clipboard is untouched.
    let mut app = app_with("hello world");
    set_mark(&mut app, 0, 0);
    set_cursor(&mut app, 5, 0);
    copy_region_in_model(&mut app, 1, InputModel::KeyTrie);
    assert_eq!(app.clipboard, None);
    assert_eq!(app.kill_ring.peek().map(String::as_str), Some("hello"));

    let mut app = app_with("hello world");
    set_mark(&mut app, 0, 0);
    set_cursor(&mut app, 5, 0);
    kill_region_in_model(&mut app, 1, InputModel::KeyTrie);
    assert_eq!(app.clipboard, None);
    assert_eq!(app.kill_ring.peek().map(String::as_str), Some("hello"));
    assert_eq!(text(&app), " world");
}

#[test]
fn standard_paste_reads_clipboard_ignoring_kill_ring() {
    let mut app = app_with("");
    app.clipboard = Some("CB".to_string());
    app.kill_ring.push("KR", false);
    yank_in_model(&mut app, 1, InputModel::Standard);
    assert_eq!(text(&app), "CB");
}

#[test]
fn keytrie_paste_reads_kill_ring_ignoring_clipboard() {
    let mut app = app_with("");
    app.clipboard = Some("CB".to_string());
    app.kill_ring.push("KR", false);
    yank_in_model(&mut app, 1, InputModel::KeyTrie);
    assert_eq!(text(&app), "KR");
}

#[test]
fn yank_pop_stays_emacs_only() {
    // After a Standard (clipboard) yank, YankPop must NOT rotate clipboard
    // entries: the kill ring is empty, so yank-pop reports that and leaves
    // the pasted clipboard text exactly as it is.
    let mut app = app_with("");
    app.clipboard = Some("CB".to_string());
    yank_in_model(&mut app, 1, InputModel::Standard);
    assert_eq!(text(&app), "CB");
    assert!(app.last_command_was_yank);
    let result = dispatch(&mut app, Command::YankPop, 1);
    assert!(matches!(result, DispatchResult::Info(_)));
    assert_eq!(text(&app), "CB");
}

#[test]
fn yank_pop_works_in_keytrie_model() {
    // Contrast: in the emacs/trie model yank-pop rotates kill-ring entries.
    let mut app = app_with("");
    app.kill_ring.push("first", false);
    app.kill_ring.push("second", false);
    yank_in_model(&mut app, 1, InputModel::KeyTrie);
    assert_eq!(text(&app), "second");
    dispatch(&mut app, Command::YankPop, 1);
    assert_eq!(text(&app), "first");
}

#[test]
fn standard_cut_paste_round_trip_via_bindings() {
    // End-to-end: Shift+Left x5 selects "hello"; the Standard cut/paste
    // routing is then driven through the in_model entry points (plain
    // `dispatch` uses the compiled INPUT_MODEL, KeyTrie in test builds).
    let mut app = app_with("hello world");
    let mut mgr = standard_manager();
    set_cursor(&mut app, 5, 0);
    for _ in 0..5 {
        press_cmd(&mut app, &mut mgr, KeyPress::shift(Key::Left));
    }
    assert_eq!(mark(&app), Some((5, 0)));
    assert_eq!(cursor(&app), (0, 0));
    kill_region_in_model(&mut app, 1, InputModel::Standard); // Ctrl+X
    assert_eq!(text(&app), " world");
    assert_eq!(app.clipboard, Some("hello".to_string()));
    set_cursor(&mut app, 0, 0);
    yank_in_model(&mut app, 1, InputModel::Standard); // Ctrl+V
    assert_eq!(text(&app), "hello world");
    assert!(app.kill_ring.peek().is_none());
}

// ---------------------------------------------------------------------------
// Dual redo, end to end
// ---------------------------------------------------------------------------

#[test]
fn dual_redo_end_to_end() {
    let mut app = app_with("");
    let mut mgr = standard_manager();
    for c in ['a', 'b'] {
        dispatch_insert_in_model(&mut app, c, 1, InputModel::Standard);
    }
    assert_eq!(text(&app), "ab");

    // Undo groups the consecutively typed word into one unit.
    press_cmd(&mut app, &mut mgr, ctrl('z')); // undo -> ""
    assert_eq!(text(&app), "");
    press_cmd(&mut app, &mut mgr, ctrl('y')); // Ctrl+Y redo -> "ab"
    assert_eq!(text(&app), "ab");

    press_cmd(&mut app, &mut mgr, ctrl('z')); // undo -> ""
    assert_eq!(text(&app), "");
    press_cmd(&mut app, &mut mgr, shift_ctrl_z()); // Ctrl+Shift+Z redo -> "ab"
    assert_eq!(text(&app), "ab");
}

// ---------------------------------------------------------------------------
// Shared editing core: identical Command-level sequences are byte-identical
// across models where semantics coincide.
// ---------------------------------------------------------------------------

/// The models share ONE editing core. They intentionally differ ONLY in:
///   (a) typing with an active selection (Standard replaces it; the others
///       preserve the mark) -- asserted above;
///   (b) copy/cut/paste routing (Standard uses `app.clipboard`; the others
///       use the kill ring) -- asserted above.
/// This sequence uses no selection and no clipboard, so all three models
/// must produce byte-identical buffers.
fn drive_shared_sequence(model: InputModel) -> String {
    let mut app = app_with("");
    for c in "hello".chars() {
        dispatch_insert_in_model(&mut app, c, 1, model);
    }
    delete_previous_character_in_model(&mut app, 1, model); // "hell"
    dispatch(&mut app, Command::Undo, 1); // "hello"
    dispatch(&mut app, Command::Redo, 1); // "hell"
    dispatch(&mut app, Command::InsertNewline, 1); // "hell\n"
    for c in "world".chars() {
        dispatch_insert_in_model(&mut app, c, 1, model);
    }
    delete_next_character_in_model(&mut app, 1, model); // no-op at end of buffer
    text(&app)
}

#[test]
fn shared_core_insert_delete_undo_redo_identical_across_models() {
    let models = [InputModel::KeyTrie, InputModel::Modal, InputModel::Standard];
    let results: Vec<String> = models.iter().map(|&m| drive_shared_sequence(m)).collect();
    assert_eq!(results[0], "hell\nworld");
    for (model, result) in models.iter().zip(results.iter()) {
        assert_eq!(
            result, &results[0],
            "model {model:?} diverged from the shared editing core"
        );
    }
}

/// A longer mixed sequence (overwrite-mode toggle, tab, transpose) run
/// through the plain `dispatch` path must also be model-independent.
#[test]
fn shared_core_mixed_editing_identical_across_models() {
    fn run(model: InputModel) -> String {
        let mut app = app_with("ab");
        set_cursor(&mut app, 1, 0);
        dispatch_insert_in_model(&mut app, 'X', 1, model);
        dispatch(&mut app, Command::TransposeCharacters, 1);
        dispatch(&mut app, Command::InsertTab, 1);
        for c in "yz".chars() {
            dispatch_insert_in_model(&mut app, c, 1, model);
        }
        dispatch(&mut app, Command::Undo, 1);
        dispatch(&mut app, Command::Redo, 1);
        text(&app)
    }
    let models = [InputModel::KeyTrie, InputModel::Modal, InputModel::Standard];
    let results: Vec<String> = models.iter().map(|&m| run(m)).collect();
    for (model, result) in models.iter().zip(results.iter()) {
        assert_eq!(
            result, &results[0],
            "model {model:?} diverged from the shared editing core"
        );
    }
    // And it must be a sane, non-empty result (not a vacuous assertion).
    assert!(!results[0].is_empty());
    assert!(results[0].contains("ab"));
}

/// The refactor is behavior-preserving: the plain public entry points
/// (which pass the runtime INPUT_MODEL = KeyTrie) must produce exactly the
/// same buffers as the `*_in_model` variants called with KeyTrie.
#[test]
fn in_model_keytrie_matches_runtime_dispatch() {
    // Typing.
    let mut a = app_with("abc");
    let mut b = app_with("abc");
    dispatch_insert(&mut a, 'X', 2);
    dispatch_insert_in_model(&mut b, 'X', 2, InputModel::KeyTrie);
    assert_eq!(text(&a), text(&b));
    assert_eq!(text(&a), "XXabc");

    // Backspace / Delete.
    let mut a = app_with("abc");
    let mut b = app_with("abc");
    set_cursor(&mut a, 2, 0);
    set_cursor(&mut b, 2, 0);
    dispatch(&mut a, Command::DeletePreviousCharacter, 1);
    delete_previous_character_in_model(&mut b, 1, InputModel::KeyTrie);
    assert_eq!(text(&a), text(&b));
    assert_eq!(text(&a), "ac");
    dispatch(&mut a, Command::DeleteNextCharacter, 1);
    delete_next_character_in_model(&mut b, 1, InputModel::KeyTrie);
    assert_eq!(text(&a), text(&b));
    assert_eq!(text(&a), "a");

    // Yank (kill-ring path in the KeyTrie model).
    let mut a = app_with("");
    let mut b = app_with("");
    a.kill_ring.push("K", false);
    b.kill_ring.push("K", false);
    dispatch(&mut a, Command::Yank, 1);
    yank_in_model(&mut b, 1, InputModel::KeyTrie);
    assert_eq!(text(&a), text(&b));
    assert_eq!(text(&a), "K");

    // Copy / cut with a mark (kill-ring path in the KeyTrie model).
    for copy in [true, false] {
        let mut a = app_with("hello");
        let mut b = app_with("hello");
        for app in [&mut a, &mut b] {
            set_mark(app, 0, 0);
            set_cursor(app, 5, 0);
        }
        if copy {
            dispatch(&mut a, Command::CopyRegion, 1);
            copy_region_in_model(&mut b, 1, InputModel::KeyTrie);
            assert_eq!(text(&a), "hello");
        } else {
            dispatch(&mut a, Command::KillRegion, 1);
            kill_region_in_model(&mut b, 1, InputModel::KeyTrie);
            assert_eq!(text(&a), "");
        }
        assert_eq!(text(&a), text(&b));
        assert_eq!(a.clipboard, None);
        assert_eq!(b.clipboard, None);
        assert_eq!(
            a.kill_ring.peek().map(String::as_str),
            b.kill_ring.peek().map(String::as_str)
        );
    }
}
