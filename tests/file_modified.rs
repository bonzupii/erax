//! The reload question asked when a file changes on disk.
//!
//! `save-buffer` used to surface "Reload? (y/n)" as a message no keypress
//! could answer. The question is now a real confirm focus state: `y`
//! reloads through the normal open path, anything else declines and
//! records the divergence so the same on-disk state is never asked about
//! twice.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use erax::core::app::EditorApp;
use erax::core::command::Command;
use erax::core::dispatcher::InputAction;
use erax::core::focus::FocusTarget;
use erax::core::id::BufferId;
use erax::core::input::{Key, KeyPress};
use erax::terminal::display::Display;
use erax::terminal::event_handler::{dispatch_command, process_terminal_event};
use erax::terminal::events::EditorEvent;
use erax::terminal::keybinds::KeyBindingManager;

static NEXT_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

fn temp_path() -> PathBuf {
    let id = NEXT_ID.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    std::env::temp_dir().join(format!("erax-reload-test-{}-{id}", std::process::id()))
}

/// Rewrite the file from outside the editor. The mtime is pushed into the
/// future so the change is visible even on coarse filesystem timestamps.
fn external_write(path: &Path, content: &str) {
    std::fs::write(path, content).expect("write fixture file");
    let f = std::fs::File::options()
        .write(true)
        .open(path)
        .expect("open fixture file");
    f.set_modified(SystemTime::now() + Duration::from_secs(30))
        .expect("bump mtime");
}

struct Fixture {
    app: EditorApp,
    display: Display,
    kbm: KeyBindingManager,
    path: PathBuf,
    bid: BufferId,
}

fn fixture(content: &str) -> Fixture {
    let path = temp_path();
    std::fs::write(&path, content).expect("write fixture file");
    let mut app = EditorApp::new();
    let bid = app.load_file(&path).expect("load fixture file");
    // Show the loaded buffer the way the open path does.
    let win = app.active_window;
    app.windows.get_mut(&win).expect("test app has a window").buffer_id = bid;
    Fixture {
        app,
        display: Display::new(80, 24, None, None),
        kbm: KeyBindingManager::new(),
        path,
        bid,
    }
}

fn buffer_text(app: &EditorApp, bid: BufferId) -> String {
    let b = app.buffers.get(&bid).expect("buffer still open");
    (0..b.line_count())
        .filter_map(|i| b.line(i))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Feed one key through the real TUI event handler.
fn feed(fx: &mut Fixture, key: KeyPress) -> bool {
    process_terminal_event(
        &mut fx.app,
        &mut fx.display,
        &mut fx.kbm,
        EditorEvent::Input(key),
    )
    .expect("event handler does not error on a confirm key")
}

/// Save through the direct-key consumer, the way a bound key dispatches.
fn save(fx: &mut Fixture) {
    let exit = dispatch_command(&mut fx.app, &mut fx.display, Command::SaveBuffer)
        .expect("save dispatch does not error");
    assert!(!exit, "save must not request exit");
}

fn confirm_pending(fx: &Fixture) -> bool {
    fx.app.focus_manager.current_target() == FocusTarget::ConfirmReload
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

#[test]
fn yes_reloads_buffer_from_disk() {
    let mut fx = fixture("original\n");
    external_write(&fx.path, "changed\n");
    save(&mut fx);
    assert!(confirm_pending(&fx), "save must raise the reload question");

    feed(&mut fx, KeyPress::plain(Key::Char('y')));

    assert_eq!(buffer_text(&fx.app, fx.bid), "changed\n");
    assert!(
        fx.app.focus_manager.current_state().is_none(),
        "question answered"
    );
    assert!(
        fx.app
            .message
            .as_deref()
            .unwrap_or("")
            .contains("Reloaded"),
        "user sees the reload confirmation, got {:?}",
        fx.app.message
    );
    // The mtime bookkeeping resynced: no question on the next save.
    save(&mut fx);
    assert!(
        !confirm_pending(&fx),
        "reloaded state must not re-ask"
    );
}

#[test]
fn no_keeps_buffer_and_stops_asking_for_that_state() {
    let mut fx = fixture("original\n");
    external_write(&fx.path, "changed\n");
    save(&mut fx);
    assert!(confirm_pending(&fx));

    feed(&mut fx, KeyPress::plain(Key::Char('n')));

    assert_eq!(buffer_text(&fx.app, fx.bid), "original\n");
    assert!(
        fx.app.message.as_deref().unwrap_or("").contains("Not saved"),
        "user sees why the save did not happen, got {:?}",
        fx.app.message
    );
    // A second save is the deliberate overwrite: no second question.
    save(&mut fx);
    assert!(!confirm_pending(&fx), "declined state must not re-ask");
    assert_eq!(
        std::fs::read_to_string(&fx.path).expect("read back"),
        "original\n",
        "the second save overwrites the external change"
    );
}

#[test]
fn enter_declines_without_reloading() {
    let mut fx = fixture("original\n");
    external_write(&fx.path, "changed\n");
    save(&mut fx);
    assert!(confirm_pending(&fx));

    feed(&mut fx, KeyPress::plain(Key::Enter));

    assert_eq!(buffer_text(&fx.app, fx.bid), "original\n");
    save(&mut fx);
    assert!(!confirm_pending(&fx), "Enter declines like n");
}

#[test]
fn stray_keys_do_not_answer() {
    let mut fx = fixture("original\n");
    external_write(&fx.path, "changed\n");
    save(&mut fx);
    assert!(confirm_pending(&fx));

    feed(&mut fx, KeyPress::plain(Key::Char('x')));
    feed(&mut fx, KeyPress::plain(Key::Char(' ')));

    assert!(confirm_pending(&fx), "unrelated keys leave the question up");
    assert_eq!(buffer_text(&fx.app, fx.bid), "original\n");
}

#[test]
fn unsaved_edits_prompt_names_the_consequence() {
    let mut fx = fixture("original\n");
    fx.app
        .buffers
        .get_mut(&fx.bid)
        .expect("buffer")
        .insert(0, "edited ");
    assert!(fx.app.buffers.get(&fx.bid).expect("buffer").modified);
    external_write(&fx.path, "changed\n");
    save(&mut fx);
    assert!(confirm_pending(&fx));
    let prompt = fx
        .app
        .focus_manager
        .current_state()
        .map(|s| s.prompt.clone())
        .unwrap_or_default();
    assert!(
        prompt.contains("unsaved edits") && prompt.contains("lose the edits"),
        "the question must name what y destroys, got {prompt:?}"
    );

    feed(&mut fx, KeyPress::plain(Key::Char('y')));
    assert_eq!(buffer_text(&fx.app, fx.bid), "changed\n");
    assert!(
        !fx.app.buffers.get(&fx.bid).expect("buffer").modified,
        "reloaded buffer is clean"
    );
}

#[test]
fn unsaved_edits_declined_keeps_the_edits() {
    let mut fx = fixture("original\n");
    fx.app
        .buffers
        .get_mut(&fx.bid)
        .expect("buffer")
        .insert(0, "edited ");
    external_write(&fx.path, "changed\n");
    save(&mut fx);
    assert!(confirm_pending(&fx));

    feed(&mut fx, KeyPress::plain(Key::Char('n')));

    assert_eq!(buffer_text(&fx.app, fx.bid), "edited original\n");
    assert!(fx.app.buffers.get(&fx.bid).expect("buffer").modified);
}

#[test]
fn duplicate_question_is_not_pushed() {
    let mut fx = fixture("original\n");
    // Two pushes for the same buffer while the question is pending:
    // still one question, cleared by one answer.
    erax::core::focus::push_reload_question(&mut fx.app, fx.bid);
    erax::core::focus::push_reload_question(&mut fx.app, fx.bid);
    assert!(fx.app.focus_manager.has_reload_question(fx.bid));
    feed(&mut fx, KeyPress::plain(Key::Char('n')));
    assert!(
        fx.app.focus_manager.current_state().is_none(),
        "one answer clears the single question"
    );
}

#[test]
fn newer_external_change_rearms_the_question() {
    let mut fx = fixture("original\n");
    external_write(&fx.path, "changed once\n");
    save(&mut fx);
    feed(&mut fx, KeyPress::plain(Key::Char('n')));
    assert!(!confirm_pending(&fx));

    external_write(&fx.path, "changed twice\n");
    save(&mut fx);
    assert!(
        confirm_pending(&fx),
        "a newer on-disk state must ask again"
    );
    feed(&mut fx, KeyPress::plain(Key::Char('y')));
    assert_eq!(buffer_text(&fx.app, fx.bid), "changed twice\n");
}

#[test]
fn palette_save_buffer_raises_the_real_question() {
    // The Standard palette / M-x path goes through the prompt handler.
    let mut fx = fixture("original\n");
    external_write(&fx.path, "changed\n");
    let exit = erax::core::prompt::handle_prompt_action(
        &mut fx.app,
        InputAction::ExecuteNamedCommand,
        "save-buffer".to_string(),
    )
    .expect("prompt handler does not error");
    assert!(!exit);
    assert!(
        confirm_pending(&fx),
        "M-x save-buffer must raise the real question"
    );
    feed(&mut fx, KeyPress::plain(Key::Char('y')));
    assert_eq!(buffer_text(&fx.app, fx.bid), "changed\n");
}
