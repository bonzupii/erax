//! `quote-character` inserts the next keypress literally, bypassing all
//! keybindings. The old implementation only printed a message claiming
//! the event loop would handle the capture; no such handling existed.
//! These tests drive the real TUI event handler end to end.
use erax::config::keys::{KEYBINDINGS, KeyBinding};
use erax::core::app::EditorApp;
use erax::core::focus::FocusTarget;
use erax::core::input::{Key, KeyPress};
use erax::terminal::display::Display;
use erax::terminal::event_handler::process_terminal_event;
use erax::terminal::events::EditorEvent;
use erax::terminal::keybinds::KeyBindingManager;

struct Rig {
    app: EditorApp,
    display: Display,
    kbm: KeyBindingManager,
}

fn rig() -> Rig {
    let mut kbm = KeyBindingManager::new();
    kbm.load(KEYBINDINGS as &[KeyBinding]);
    Rig {
        app: EditorApp::new(),
        display: Display::new(80, 24, None, None),
        kbm,
    }
}

fn feed(rig: &mut Rig, key: KeyPress) {
    process_terminal_event(
        &mut rig.app,
        &mut rig.display,
        &mut rig.kbm,
        EditorEvent::Input(key),
    )
    .expect("event handler must not error");
}

fn buffer_text(rig: &Rig) -> String {
    rig.app
        .active_buffer()
        .map(|b| b.content_string())
        .unwrap_or_default()
}

fn ctrl(c: char) -> KeyPress {
    KeyPress::ctrl(Key::Char(c))
}

fn plain(c: char) -> KeyPress {
    KeyPress::plain(Key::Char(c))
}

#[test]
fn quote_character_inserts_control_code_literally() {
    let mut rig = rig();
    feed(&mut rig, ctrl('q'));
    assert_eq!(
        rig.app.focus_manager.current_target(),
        FocusTarget::QuoteCharacter,
        "C-q must arm the one-keypress quote focus"
    );
    // C-g would normally be a keybinding prefix candidate; quoted, it
    // inserts the BEL control code instead of doing anything else.
    feed(&mut rig, ctrl('g'));
    assert_eq!(buffer_text(&rig), "\u{7}");
    assert_eq!(
        rig.app.focus_manager.current_target(),
        FocusTarget::Editor,
        "the quote focus consumes exactly one keypress"
    );
}

#[test]
fn quote_character_inserts_plain_char_and_enter() {
    let mut rig = rig();
    feed(&mut rig, ctrl('q'));
    feed(&mut rig, plain('z'));
    assert_eq!(buffer_text(&rig), "z");
    feed(&mut rig, ctrl('q'));
    feed(&mut rig, KeyPress::plain(Key::Enter));
    assert_eq!(buffer_text(&rig), "z\n");
}

#[test]
fn quote_character_escape_cancels_without_inserting() {
    let mut rig = rig();
    feed(&mut rig, ctrl('q'));
    feed(&mut rig, KeyPress::plain(Key::Esc));
    assert_eq!(buffer_text(&rig), "");
    assert_eq!(
        rig.app.focus_manager.current_target(),
        FocusTarget::Editor
    );
}
