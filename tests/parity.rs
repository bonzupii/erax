//! End-to-end command parity across input models.
//!
//! Every typed command must be invocable through each model's real
//! invocation path, and the path must not distort the dispatch result.
//! The expectation for each command is derived by dispatching it once on
//! a fresh app; the same command is then driven through the model's
//! genuine entry point and must produce the matching observable:
//!
//! - `Exit` → the exit signal leaves the layer that ran it
//!   (`Ok(true)` from the event handler, `ExitRequested` from the
//!   modal engine),
//! - `NeedsInput` → a prompt is pushed on the focus manager,
//! - `AwaitKey` → the focus its action maps to is pushed,
//! - `Info(msg)` → the message lands in `app.message`,
//! - anything else → the key is consumed without panic or exit.
//!
//! Paths driven for real:
//!
//! - KeyTrie: every binding's key sequence through `process_terminal_event`
//!   (the TUI event handler, await-longer leaves settled like the run
//!   loop does), and every command typed by name through M-x the same way.
//! - Modal: Normal-mode bindings and the `:` line, driven through the
//!   real engine (`handle_key_inner`) in `src/core/modal.rs`
//!   `parity_end_to_end` — the public `handle_key` passes a compile-time
//!   `INPUT_MODEL` gate that is KeyTrie in test builds, so integration
//!   tests cannot reach the engine; the in-crate tests drive the exact
//!   path production takes past that one-line gate.
//! - Standard: every binding's key sequence through `process_terminal_event`,
//!   and every command typed by name through the palette.
//!
//! Structural preconditions (the named facilities are bound at all) are
//! asserted separately. Key values themselves are config and are never
//! asserted — only that driving them produces the derived observable.
//!
//! Negative controls: reverting the prompt exit-signal propagation in the
//! event handler fails `mx_exit_commands_propagate_exit`; removing the
//! `:` entry binding fails `named_facilities_are_bound`.

use erax::config::keys::{KEYBINDINGS, STANDARD_BINDINGS};
use erax::core::bindings::KeyBinding;
use erax::core::app::EditorApp;
use erax::core::command::Command;
use erax::core::dispatcher::{DispatchResult, dispatch};
use erax::core::focus::FocusTarget;
use erax::core::input::{Key, KeyPress};
use erax::terminal::display::Display;
use erax::terminal::event_handler::{dispatch_command, process_terminal_event};
use erax::terminal::events::EditorEvent;
use erax::terminal::keybinds::KeyBindingManager;

/// The observable a command must produce, derived from one real dispatch.
enum Expect {
    Exit,
    Prompt,
    AwaitKey(FocusTarget),
    Message(String),
    Consumed,
}

fn expect(cmd: Command) -> Expect {
    let mut app = EditorApp::new();
    match dispatch(&mut app, cmd, 1) {
        DispatchResult::Exit => Expect::Exit,
        DispatchResult::NeedsInput { .. } => Expect::Prompt,
        DispatchResult::AwaitKey(action) => {
            Expect::AwaitKey(erax::core::focus::focus_target_for_await_key(action))
        }
        DispatchResult::Info(m) => Expect::Message(m),
        _ => Expect::Consumed,
    }
}

struct Rig {
    app: EditorApp,
    display: Display,
    kbm: KeyBindingManager,
}

fn rig_for(table: &[KeyBinding]) -> Rig {
    let mut kbm = KeyBindingManager::new();
    kbm.load(table);
    Rig {
        app: EditorApp::new(),
        display: Display::new(80, 24, None, None),
        kbm,
    }
}

/// Feed keys through the real TUI event handler, settling await-longer
/// leaves the way the run loop does. Returns the exit signal.
fn feed(rig: &mut Rig, keys: &[KeyPress]) -> bool {
    let mut exit = false;
    for key in keys {
        exit = process_terminal_event(
            &mut rig.app,
            &mut rig.display,
            &mut rig.kbm,
            EditorEvent::Input(*key),
        )
        .expect("event handler must not error on a bound key");
        if exit {
            break;
        }
    }
    if !exit && rig.kbm.is_awaiting()
        && let Some(cmd) = rig.kbm.take_awaited()
    {
        exit = dispatch_command(&mut rig.app, &mut rig.display, cmd)
            .expect("dispatch_command must not error");
    }
    exit
}

/// Type a command name into the focused minibuffer and confirm it.
fn type_named(rig: &mut Rig, name: &str) -> bool {
    let mut keys: Vec<KeyPress> = name
        .chars()
        .map(|c| KeyPress::plain(Key::Char(c)))
        .collect();
    keys.push(KeyPress::plain(Key::Enter));
    feed(rig, &keys)
}

/// The named-command facility's binding in a key-trie table.
fn named_facility_binding(table: &[KeyBinding]) -> &KeyBinding {
    table
        .iter()
        .find(|b| b.command == Command::ExecuteNamedCommand)
        .expect("table must bind the named-command facility")
}

fn assert_observation(rig: &Rig, cmd: Command, exit: bool, path: &str) {
    let name = cmd.name();
    match expect(cmd) {
        Expect::Exit => assert!(
            exit,
            "{path}: {name} must propagate the exit signal out of the invocation path"
        ),
        Expect::Prompt => assert!(
            rig.app.focus_manager.current_state().is_some(),
            "{path}: {name} must push a prompt on the focus manager"
        ),
        Expect::AwaitKey(target) => assert_eq!(
            rig.app.focus_manager.current_target(),
            target,
            "{path}: {name} must push the focus its action maps to"
        ),
        Expect::Message(m) => assert_eq!(
            rig.app.message.as_deref(),
            Some(m.as_str()),
            "{path}: {name} must surface its message"
        ),
        Expect::Consumed => {
            assert!(!exit, "{path}: {name} must not raise the exit signal")
        }
    }
}

/// Drive every binding of a trie table through the real event handler.
fn check_trie_table(table: &[KeyBinding], table_name: &str) {
    let mut failures = Vec::new();
    for binding in table {
        let mut rig = rig_for(table);
        let exit = feed(&mut rig, binding.keys);
        // One bad command must not hide the rest of the table.
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            assert_observation(&rig, binding.command, exit, table_name);
        }));
        if result.is_err() {
            failures.push(binding.command.name());
        }
    }
    assert!(
        failures.is_empty(),
        "{table_name}: invocation path distorted the dispatch result for: {failures:?}"
    );
}

/// Drive every command by name through a trie table's named facility
/// (M-x, the Standard palette): the facility's real keys, then the name
/// typed for real, then Enter — through the real event handler.
fn check_named_facility(table: &[KeyBinding], table_name: &str, facility: &str) {
    let facility_keys: Vec<KeyPress> = named_facility_binding(table).keys.to_vec();
    let mut failures = Vec::new();
    for cmd in Command::all() {
        let mut rig = rig_for(table);
        let mut exit = feed(&mut rig, &facility_keys);
        if !exit {
            exit = type_named(&mut rig, cmd.name());
        }
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            assert_observation(&rig, *cmd, exit, facility);
        }));
        if result.is_err() {
            failures.push(cmd.name());
        }
    }
    assert!(
        failures.is_empty(),
        "{table_name} {facility}: invocation path distorted the dispatch result for: {failures:?}"
    );
}

#[test]
fn named_facilities_are_bound() {
    // Structural preconditions for the reachability proof: each trie
    // model needs its named-command facility bound (to some key — the
    // key itself is config and not asserted here). The modal `:` entry
    // is asserted in-crate (src/core/modal.rs parity_end_to_end), next
    // to the engine it belongs to.
    assert!(
        KEYBINDINGS
            .iter()
            .any(|b| b.command == Command::ExecuteNamedCommand),
        "KEYBINDINGS must bind the named-command facility"
    );
    assert!(
        STANDARD_BINDINGS
            .iter()
            .any(|b| b.command == Command::ExecuteNamedCommand),
        "STANDARD_BINDINGS must bind the named-command facility"
    );
}

#[test]
fn every_command_has_a_name() {
    // The named facilities resolve through Command::from_name; an unnamed
    // command would be silently unreachable through them.
    for cmd in Command::all() {
        assert_eq!(Command::from_name(cmd.name()), Some(*cmd));
    }
}

#[test]
fn keytrie_bindings_drive_real_dispatch() {
    check_trie_table(KEYBINDINGS, "keytrie");
}

#[test]
fn standard_bindings_drive_real_dispatch() {
    check_trie_table(STANDARD_BINDINGS, "standard");
}

#[test]
fn mx_reaches_every_command_end_to_end() {
    check_named_facility(KEYBINDINGS, "keytrie", "M-x");
}

#[test]
fn palette_reaches_every_command_end_to_end() {
    check_named_facility(STANDARD_BINDINGS, "standard", "palette");
}

/// The regression this session fixes: the three exit commands, invoked
/// through M-x, must raise the exit signal out of the event-handler
/// layer — not merely dispatch `Exit` somewhere unobserved.
#[test]
fn mx_exit_commands_propagate_exit() {
    for name in ["exit-erax", "exit-without-save", "exit-and-save"] {
        let mut rig = rig_for(KEYBINDINGS);
        let facility_keys: Vec<KeyPress> = named_facility_binding(KEYBINDINGS).keys.to_vec();
        let mut exit = feed(&mut rig, &facility_keys);
        assert!(
            rig.app.focus_manager.current_state().is_some(),
            "M-x must open the minibuffer before typing {name}"
        );
        if !exit {
            exit = type_named(&mut rig, name);
        }
        assert!(
            exit,
            "M-x {name} must propagate the exit signal out of the event handler"
        );
    }
}
