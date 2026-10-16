//! Typed keybinding manager.
//!
//! Binds typed key sequences to typed commands (N-055, N-058): the hand-rolled
//! string sequence parser is deleted. Bindings come from the typed
//! configuration as `&[KeyPress]`; there is exactly one canonical key
//! representation (N-056, N-060).

use crate::core::bindings::KeyBinding;
use crate::core::command::Command;
use crate::core::input::{KeyPress, KeyTrie, LookupResult};

/// The typed result of processing one keypress (N-058).
///
/// The caller matches this exhaustively: a command to dispatch, a prefix
/// awaiting more keys (or a timeout), a dead end, or a character to insert.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyOutcome {
    /// Dispatch this typed command now.
    Command(Command),
    /// A valid prefix; wait for more keys.
    Prefix,
    /// A leaf with `AwaitLonger` policy; the caller waits up to the timeout
    /// for a longer sequence, then takes the leaf via `take_awaited()`.
    AwaitingLonger { leaf: Command },
    /// Dead end: optionally execute the pending `await_longer` leaf first,
    /// then reprocess this keypress as a fresh press (N-059).
    DeadEnd { leaf: Option<Command> },
    /// Insert this character literally.
    InsertChar(char),
}

/// KeyBindingManager: typed key sequences to typed commands.
///
/// Loads the typed [`KeyBinding`] list from configuration once; processes
/// [`KeyPress`] events through the trie. No strings, no parsing (N-055).
pub struct KeyBindingManager {
    trie: KeyTrie,
}

impl KeyBindingManager {
    pub fn new() -> Self {
        Self {
            trie: KeyTrie::new(),
        }
    }

    /// Load every binding from the typed configuration.
    pub fn load(&mut self, bindings: &[KeyBinding]) {
        for binding in bindings {
            self.trie
                .insert(binding.keys, binding.command, binding.policy);
        }
    }

    /// Process one keypress; the caller matches the outcome exhaustively.
    pub fn process_key(&mut self, press: KeyPress) -> KeyOutcome {
        match self.trie.process_key(press) {
            LookupResult::Command(cmd) => KeyOutcome::Command(cmd),
            LookupResult::Prefix => KeyOutcome::Prefix,
            LookupResult::AwaitingLonger { leaf } => KeyOutcome::AwaitingLonger { leaf },
            LookupResult::DeadEnd { leaf } => KeyOutcome::DeadEnd { leaf },
            LookupResult::InsertChar(c) => KeyOutcome::InsertChar(c),
        }
    }

    /// The key-sequence timeout expired: take a pending `await_longer` leaf.
    pub fn take_awaited(&mut self) -> Option<Command> {
        self.trie.take_awaited()
    }

    /// Whether the trie is positioned on an `await_longer` leaf, waiting for
    /// a possible longer sequence or the timeout.
    pub fn is_awaiting(&self) -> bool {
        self.trie.is_awaiting()
    }

    /// The in-progress sequence as display text.
    pub fn current_sequence(&self) -> String {
        self.trie.current_sequence()
    }
}

impl Default for KeyBindingManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::input::{Key, PrefixPolicy};

    fn ctrl(c: char) -> KeyPress {
        KeyPress::ctrl(Key::Char(c))
    }

    fn bindings() -> Vec<KeyBinding> {
        // Static bindings for the test (KeyBinding requires 'static slices).
        const CTRL_F: &[KeyPress] = &[KeyPress::ctrl(Key::Char('f'))];
        const CTRL_X_CTRL_F: &[KeyPress] = &[
            KeyPress::ctrl(Key::Char('x')),
            KeyPress::ctrl(Key::Char('f')),
        ];
        vec![
            KeyBinding {
                keys: CTRL_F,
                command: Command::ForwardCharacter,
                policy: PrefixPolicy::ExecuteLeaf,
            },
            KeyBinding {
                keys: CTRL_X_CTRL_F,
                command: Command::FindFile,
                policy: PrefixPolicy::ExecuteLeaf,
            },
        ]
    }

    #[test]
    fn typed_bindings_dispatch_typed_commands() {
        let mut manager = KeyBindingManager::new();
        manager.load(&bindings());
        assert_eq!(
            manager.process_key(ctrl('f')),
            KeyOutcome::Command(Command::ForwardCharacter)
        );
    }

    #[test]
    fn multi_key_sequence_flows_through_prefix() {
        let mut manager = KeyBindingManager::new();
        manager.load(&bindings());
        assert_eq!(manager.process_key(ctrl('x')), KeyOutcome::Prefix);
        assert_eq!(
            manager.process_key(ctrl('f')),
            KeyOutcome::Command(Command::FindFile)
        );
    }

    #[test]
    fn dead_end_reports_no_leaf() {
        let mut manager = KeyBindingManager::new();
        manager.load(&bindings());
        assert_eq!(manager.process_key(ctrl('x')), KeyOutcome::Prefix);
        assert_eq!(
            manager.process_key(KeyPress::plain(Key::Char('z'))),
            KeyOutcome::DeadEnd { leaf: None }
        );
    }

    #[test]
    fn unbound_plain_key_inserts() {
        let mut manager = KeyBindingManager::new();
        manager.load(&bindings());
        assert_eq!(
            manager.process_key(KeyPress::plain(Key::Char('q'))),
            KeyOutcome::InsertChar('q')
        );
    }
}
