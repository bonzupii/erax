//! An oversized kill is reported to the user, never silently dropped:
//! the kill-ring entry cap is the config constant, and the kill
//! commands surface the rejection instead of claiming "Region killed".
use erax::core::app::EditorApp;
use erax::core::command::Command;
use erax::core::dispatcher::{DispatchResult, dispatch};
use erax::core::kill_ring::KillRing;

fn app_with(text: &str) -> EditorApp {
    let mut app = EditorApp::new();
    if let Some(buffer) = app.active_buffer_mut() {
        buffer.insert(0, text);
    }
    app
}

#[test]
fn kill_region_reports_oversize_instead_of_claiming_killed() {
    // One byte over the config cap.
    let big = "x".repeat(erax::config::editor::KILL_RING_MAX_BYTES + 1);
    let mut app = app_with(&big);
    // Mark at (0,0), cursor at end: the region is the whole text.
    if let Some(window) = app.windows.get_mut(&app.active_window) {
        window.selection_manager.set_mark(0);
        window.cursor_x = big.chars().count();
    }
    let result = dispatch(&mut app, Command::KillRegion, 1);
    match result {
        DispatchResult::Info(msg) => {
            assert!(
                msg.contains("not saved for yank"),
                "oversize must be reported, got: {msg}"
            );
            assert!(!msg.contains("Region killed"), "got: {msg}");
        }
        other => panic!("expected oversize Info, got {other:?}"),
    }
    // The text was still deleted from the buffer (Emacs semantics);
    // only the ring entry was refused.
    assert!(app
        .active_buffer()
        .is_some_and(|b| b.content_string().is_empty()));
    assert!(app.kill_ring.yank().is_none());
}

#[test]
fn kill_ring_cap_is_the_config_constant() {
    // The engine must read the documented constant, not a private
    // duplicate: pushing exactly the cap stores, one byte over rejects.
    let mut ring = KillRing::new();
    let at_cap = "x".repeat(erax::config::editor::KILL_RING_MAX_BYTES);
    assert!(ring.push(&at_cap, false));
    let over = "x".repeat(erax::config::editor::KILL_RING_MAX_BYTES + 1);
    assert!(!ring.push(&over, false));
}
