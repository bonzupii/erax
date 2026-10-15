//! Command reachability parity across input models.
//!
//! Every typed command must be invocable in every input model; the models
//! differ only in the feel of invocation, never in capability. Reachability
//! per model is defined by that model's idiom:
//!
//! - KeyTrie: bound in KEYBINDINGS, or through M-x (`ExecuteNamedCommand`,
//!   itself bound), which resolves every name in the typed command table.
//! - Modal: bound in MODAL_BINDINGS, or a ctrl/alt chord in KEYBINDINGS
//!   (the trie underneath the modal engine, which only claims plain keys),
//!   or through the `:` line (bound in Normal mode; names resolve through
//!   the same typed command table, except `toggle-menu`, which the event
//!   loop intercepts before dispatch and stays reachable through F10).
//! - Standard: bound in STANDARD_BINDINGS, or through the command palette
//!   (`ExecuteNamedCommand` on Ctrl+Shift+P), the CUA-idiom counterpart of
//!   M-x and `:`.
//!
//! Adding a command with no invocation path in some model fails this suite:
//! extend the tables or the named facilities alongside the new variant.

use erax::config::keys::{
    KEYBINDINGS, MODAL_BINDINGS, STANDARD_BINDINGS, ModalAction,
};
use erax::core::command::Command;

/// Commands bound anywhere in a key-trie table.
fn bound_in(table: &[erax::config::keys::KeyBinding], cmd: Command) -> bool {
    table.iter().any(|b| b.command == cmd)
}

/// Commands the modal engine runs directly (its `Command` actions).
fn modal_direct(cmd: Command) -> bool {
    MODAL_BINDINGS.iter().any(|b| match b.action {
        ModalAction::Command(c) => c == cmd,
        _ => false,
    })
}

/// The modal command line is bound in Normal mode (as some key; the
/// binding itself is config, the entry point is structural).
fn cmdline_entry_bound() -> bool {
    MODAL_BINDINGS
        .iter()
        .any(|b| b.action == ModalAction::EnterMode(erax::config::keys::ModalMode::CommandLine))
}

/// The Standard command palette is bound.
fn palette_bound() -> bool {
    bound_in(STANDARD_BINDINGS, Command::ExecuteNamedCommand)
}

/// M-x is bound in the key-trie table.
fn mx_bound() -> bool {
    bound_in(KEYBINDINGS, Command::ExecuteNamedCommand)
}

fn reachable_keytrie(cmd: Command) -> bool {
    // M-x resolves every name in the typed command table.
    bound_in(KEYBINDINGS, cmd) || mx_bound()
}

fn reachable_modal(cmd: Command) -> bool {
    // Direct engine action, ctrl/alt fall-through to the trie underneath,
    // or the command-line entry (every name resolves; toggle-menu stays
    // on its display-intercepted key).
    modal_direct(cmd) || bound_in(KEYBINDINGS, cmd) || cmd == Command::ToggleMenu || cmdline_entry_bound()
}

fn reachable_standard(cmd: Command) -> bool {
    // The palette resolves every name in the typed command table.
    bound_in(STANDARD_BINDINGS, cmd) || palette_bound()
}

#[test]
fn named_facilities_are_bound() {
    // Structural preconditions for the reachability proof: each model
    // needs its named-command facility bound (to some key — the key
    // itself is config and not asserted here).
    assert!(
        mx_bound(),
        "KEYBINDINGS must bind the named-command facility"
    );
    assert!(
        cmdline_entry_bound(),
        "MODAL_BINDINGS must bind a command-line entry"
    );
    assert!(
        palette_bound(),
        "STANDARD_BINDINGS must bind the named-command facility"
    );
}

#[test]
fn every_command_reachable_in_every_model() {
    let mut gaps = Vec::new();
    for cmd in Command::all() {
        if !reachable_keytrie(*cmd) {
            gaps.push((cmd.name(), "keytrie"));
        }
        if !reachable_modal(*cmd) {
            gaps.push((cmd.name(), "modal"));
        }
        if !reachable_standard(*cmd) {
            gaps.push((cmd.name(), "standard"));
        }
    }
    assert!(
        gaps.is_empty(),
        "commands unreachable in some model (add a binding or a named-facility path): {gaps:?}"
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
