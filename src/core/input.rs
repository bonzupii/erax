//! Raw input event types and key-press representation.
use crate::core::command::Command;
use std::collections::HashMap;
use std::fmt;

/// A base key without modifiers.
///
/// Modifiers are NOT variants of this enum (N-056): there is one representation
/// of modifiers — the modifier fields on [`KeyPress`] — used by events,
/// bindings, and configuration alike. `Key::Ctrl`/`Key::Alt` are gone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum Key {
    Char(char),
    F(u8),
    Esc,
    Enter,
    Backspace,
    Tab,
    Delete,
    Home,
    End,
    PageUp,
    PageDown,
    Up,
    Down,
    Left,
    Right,
    Insert,
    Null,
}

impl fmt::Display for Key {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Key::Char(c) => write!(f, "{c}"),
            Key::F(n) => write!(f, "F{n}"),
            Key::Esc => write!(f, "ESC"),
            Key::Enter => write!(f, "RET"),
            Key::Backspace => write!(f, "BS"),
            Key::Tab => write!(f, "TAB"),
            Key::Delete => write!(f, "DEL"),
            Key::Home => write!(f, "Home"),
            Key::End => write!(f, "End"),
            Key::PageUp => write!(f, "PgUp"),
            Key::PageDown => write!(f, "PgDn"),
            Key::Up => write!(f, "Up"),
            Key::Down => write!(f, "Down"),
            Key::Left => write!(f, "Left"),
            Key::Right => write!(f, "Right"),
            Key::Insert => write!(f, "Ins"),
            Key::Null => write!(f, "NUL"),
        }
    }
}

/// The canonical keypress: one typed key with explicit modifier fields (N-056).
///
/// This is the single representation used by terminal/GUI event sources, the
/// keybinding trie, and the typed configuration. Case is significant (N-057):
/// `Key::Char('x')` and `Key::Char('X')` are different keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct KeyPress {
    pub key: Key,
    pub shift: bool,
    pub alt: bool,
    pub ctrl: bool,
}

impl KeyPress {
    /// A plain key with no modifiers.
    pub const fn plain(key: Key) -> Self {
        Self {
            key,
            shift: false,
            alt: false,
            ctrl: false,
        }
    }

    /// A key with Ctrl held.
    pub const fn ctrl(key: Key) -> Self {
        Self {
            key,
            shift: false,
            alt: false,
            ctrl: true,
        }
    }

    /// A key with Alt/Meta held.
    pub const fn alt(key: Key) -> Self {
        Self {
            key,
            shift: false,
            alt: true,
            ctrl: false,
        }
    }

    /// A key with Shift held.
    pub const fn shift(key: Key) -> Self {
        Self {
            key,
            shift: true,
            alt: false,
            ctrl: false,
        }
    }

    /// A key with an explicit modifier combination.
    pub const fn mods(key: Key, shift: bool, alt: bool, ctrl: bool) -> Self {
        Self {
            key,
            shift,
            alt,
            ctrl,
        }
    }

    /// Normalize a terminal-produced keypress for binding lookup.
    ///
    /// Terminals report Ctrl+letter as a single byte regardless of Shift, so a
    /// ctrl-modified character key is lowercased for matching (this reflects
    /// terminal reality, not case-folding of bindings: `Char('X')` without
    /// modifiers still differs from `Char('x')`). All other keys pass through
    /// unchanged.
    pub fn normalize_for_lookup(self) -> Self {
        if self.ctrl
            && let Key::Char(c) = self.key {
                return Self {
                    key: Key::Char(c.to_ascii_lowercase()),
                    ..self
                };
            }
        self
    }
}

impl fmt::Display for KeyPress {
    /// Display is presentation: one chord in the canonical notation.
    /// [`crate::core::keyfmt::format_chord`] is the single definition;
    /// this impl delegates so no second formatter can drift from it.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", crate::core::keyfmt::format_chord(self))
    }
}

/// Mouse button types
#[derive(Debug, Clone, PartialEq, Eq, Copy, Hash, serde::Serialize, serde::Deserialize)]
pub enum MouseButton {
    Left,
    Right,
    Middle,
}

/// Type of mouse event
#[derive(Debug, Clone, PartialEq, Eq, Copy, serde::Serialize, serde::Deserialize)]
pub enum MouseEventKind {
    Down(MouseButton),
    Up(MouseButton),
    Drag(MouseButton),
    Moved,
    ScrollDown,
    ScrollUp,
    ScrollLeft,
    ScrollRight,
}

/// Native mouse event representation for erax
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct MouseEvent {
    pub column: u16,
    pub row: u16,
    pub kind: MouseEventKind,
    pub shift: bool,
    pub alt: bool,
    pub ctrl: bool,
    pub click_count: u8, // 1 = single, 2 = double, 3 = triple
}

/// Result of a key lookup in the trie.
///
/// The caller matches this exhaustively (N-058): a typed command, a prefix
/// needing more keys, a dead end, or a character to insert.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LookupResult {
    /// A complete binding matched; execute this typed command.
    Command(Command),
    /// A valid prefix; more keys are needed.
    Prefix,
    /// A leaf with `AwaitLonger` policy; waiting for a possible longer
    /// sequence up to the timeout. The caller should wait; if the timeout
    /// expires, `take_awaited()` yields the leaf command.
    AwaitingLonger { leaf: Command },
    /// No binding matches. `leaf` carries an `await_longer` leaf command that
    /// was pending when the dead end occurred; the caller executes it, then
    /// reprocesses the current key as a fresh press (N-059).
    DeadEnd { leaf: Option<Command> },
    /// No binding; insert this character literally.
    InsertChar(char),
}

/// Policy for a binding that is both a complete command and a prefix of a
/// longer binding (N-013). Declared explicitly in the typed configuration;
/// the old code executed the leaf immediately without saying so.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PrefixPolicy {
    /// The leaf command executes as soon as it is complete; longer sequences
    /// through this key are unreachable.
    ExecuteLeaf,
    /// Wait for a possible longer sequence up to the key-sequence timeout;
    /// if none arrives, the leaf executes.
    AwaitLonger,
}

/// A node in the key binding trie.
struct TrieNode {
    /// The command bound to this sequence, if any.
    command: Option<Command>,
    /// Policy when this node is both a leaf and a prefix (N-013).
    await_longer: bool,
    /// Child nodes for multi-key sequences.
    children: HashMap<KeyPress, Box<TrieNode>>,
}

impl TrieNode {
    fn new() -> Self {
        Self {
            command: None,
            await_longer: false,
            children: HashMap::new(),
        }
    }
}

/// Trie of typed keybindings: sequences of [`KeyPress`] to [`Command`].
///
/// ESC-prefix normalization happens once, here (N-010, N-060): a bare ESC
/// press arms the meta prefix and the following key is looked up with
/// `alt: true`. There are no duplicate ESC spellings in the configuration.
pub struct KeyTrie {
    root: TrieNode,
    /// Keys of the in-progress sequence.
    current: Vec<KeyPress>,
    /// A bare ESC was just pressed; the next key gets `alt: true`.
    pending_alt: bool,
}

impl KeyTrie {
    pub fn new() -> Self {
        Self {
            root: TrieNode::new(),
            current: Vec::new(),
            pending_alt: false,
        }
    }

    /// Insert a typed binding: a sequence of keypresses to a command.
    pub fn insert(&mut self, keys: &[KeyPress], command: Command, policy: PrefixPolicy) {
        let mut node = &mut self.root;
        for key in keys {
            node = node
                .children
                .entry(*key)
                .or_insert_with(|| Box::new(TrieNode::new()));
        }
        node.command = Some(command);
        node.await_longer = policy == PrefixPolicy::AwaitLonger;
    }

    /// Look up the node for a key sequence, if the whole sequence matches.
    fn lookup(&self, seq: &[KeyPress]) -> Option<&TrieNode> {
        let mut node = &self.root;
        for key in seq {
            node = node.children.get(key)?;
        }
        Some(node)
    }

    /// Process one keypress and return the typed result (N-058).
    ///
    /// Dead ends reset the sequence and report any pending `await_longer`
    /// leaf; the caller reprocesses the current key as a fresh press so typed
    /// text after a partial prefix is never swallowed (N-059).
    pub fn process_key(&mut self, press: KeyPress) -> LookupResult {
        // Bare ESC arms the meta prefix (single normalization point, N-060).
        if press == KeyPress::plain(Key::Esc) {
            if self.pending_alt {
                // ESC ESC: treat as Alt+ESC.
                self.pending_alt = false;
                return self.process_key(KeyPress::alt(Key::Esc));
            }
            self.pending_alt = true;
            return LookupResult::Prefix;
        }

        let mut press = press.normalize_for_lookup();
        if self.pending_alt {
            self.pending_alt = false;
            // A real Alt modifier from the terminal and an ESC prefix mean
            // the same thing: one representation (N-056).
            press.alt = true;
        }

        self.current.push(press);
        match self.lookup(&self.current) {
            Some(node) => {
                if let Some(cmd) = node.command {
                    if node.children.is_empty() {
                        self.current.clear();
                        return LookupResult::Command(cmd);
                    }
                    // Leaf that is also a prefix: the policy is explicit (N-013).
                    if node.await_longer {
                        return LookupResult::AwaitingLonger { leaf: cmd };
                    }
                    self.current.clear();
                    return LookupResult::Command(cmd);
                }
                if !node.children.is_empty() {
                    return LookupResult::Prefix;
                }
                // Unreachable: nodes only exist along inserted sequences.
                self.current.clear();
                LookupResult::DeadEnd { leaf: None }
            }
            None => {
                // Dead end: was the sequence-so-far an await_longer leaf?
                let was_in_sequence = self.current.len() > 1;
                let leaf = self.lookup(&self.current[..self.current.len() - 1]).and_then(
                    |node| {
                        if node.await_longer {
                            node.command
                        } else {
                            None
                        }
                    },
                );
                self.current.clear();
                // A plain character with no modifiers self-inserts — but only
                // for a single keypress, not in the middle of a sequence
                // (N-059: ^X z is undefined, it does not insert 'z').
                if leaf.is_none() && !was_in_sequence
                    && let Key::Char(c) = press.key
                        && !press.ctrl && !press.alt {
                            return LookupResult::InsertChar(c);
                        }
                LookupResult::DeadEnd { leaf }
            }
        }
    }

    /// The key-sequence timeout expired while positioned on an `await_longer`
    /// leaf: take the leaf command to execute now.
    pub fn take_awaited(&mut self) -> Option<Command> {
        let leaf = self
            .lookup(&self.current)
            .and_then(|node| if node.await_longer { node.command } else { None });
        if leaf.is_some() {
            self.current.clear();
        }
        leaf
    }

    /// Whether the trie is positioned on an `await_longer` leaf.
    pub fn is_awaiting(&self) -> bool {
        self.lookup(&self.current)
            .map(|node| node.await_longer)
            .unwrap_or(false)
    }

    /// The in-progress sequence as display text (for the status line).
    ///
    /// Rendered in the canonical notation ([`crate::core::keyfmt`]); the
    /// trailing `-` marks the step the trie is still waiting for.
    pub fn current_sequence(&self) -> String {
        if self.current.is_empty() && !self.pending_alt {
            return String::new();
        }
        let mut seq = crate::core::keyfmt::format_sequence(&self.current);
        if self.pending_alt {
            if !seq.is_empty() {
                seq.push('-');
            }
            seq.push_str("Esc");
        }
        seq.push('-');
        seq
    }
}

impl Default for KeyTrie {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::command::Command;

    fn ctrl(c: char) -> KeyPress {
        KeyPress::ctrl(Key::Char(c))
    }

    #[test]
    fn single_key_binding_dispatches_typed_command() {
        let mut trie = KeyTrie::new();
        trie.insert(
            &[ctrl('f')],
            Command::ForwardCharacter,
            PrefixPolicy::ExecuteLeaf,
        );
        let result = trie.process_key(ctrl('f'));
        assert_eq!(result, LookupResult::Command(Command::ForwardCharacter));
    }

    #[test]
    fn multi_key_sequence_needs_prefix_then_dispatches() {
        let mut trie = KeyTrie::new();
        let cx = ctrl('x');
        trie.insert(&[cx, ctrl('f')], Command::FindFile, PrefixPolicy::ExecuteLeaf);
        trie.insert(&[cx, ctrl('c')], Command::ExitErax, PrefixPolicy::ExecuteLeaf);

        assert_eq!(trie.process_key(cx), LookupResult::Prefix);
        assert_eq!(
            trie.process_key(ctrl('f')),
            LookupResult::Command(Command::FindFile)
        );
    }

    #[test]
    fn esc_prefix_normalizes_to_alt_once() {
        // One canonical representation: ESC x and Alt-x are the same keypress.
        let mut trie = KeyTrie::new();
        trie.insert(
            &[KeyPress::alt(Key::Char('x'))],
            Command::ExecuteNamedCommand,
            PrefixPolicy::ExecuteLeaf,
        );
        // Via ESC prefix:
        assert_eq!(
            trie.process_key(KeyPress::plain(Key::Esc)),
            LookupResult::Prefix
        );
        assert_eq!(
            trie.process_key(KeyPress::plain(Key::Char('x'))),
            LookupResult::Command(Command::ExecuteNamedCommand)
        );
        // Via direct Alt modifier:
        assert_eq!(
            trie.process_key(KeyPress::alt(Key::Char('x'))),
            LookupResult::Command(Command::ExecuteNamedCommand)
        );
    }

    #[test]
    fn dead_end_reports_no_leaf_and_resets() {
        let mut trie = KeyTrie::new();
        let cx = ctrl('x');
        trie.insert(&[cx, ctrl('f')], Command::FindFile, PrefixPolicy::ExecuteLeaf);

        assert_eq!(trie.process_key(cx), LookupResult::Prefix);
        // 'z' does not continue the sequence: dead end, no pending leaf.
        assert_eq!(
            trie.process_key(KeyPress::plain(Key::Char('z'))),
            LookupResult::DeadEnd { leaf: None }
        );
        // The trie reset: a fresh ^X ^F still works.
        assert_eq!(trie.process_key(cx), LookupResult::Prefix);
        assert_eq!(
            trie.process_key(ctrl('f')),
            LookupResult::Command(Command::FindFile)
        );
    }

    #[test]
    fn plain_character_self_inserts() {
        let mut trie = KeyTrie::new();
        assert_eq!(
            trie.process_key(KeyPress::plain(Key::Char('a'))),
            LookupResult::InsertChar('a')
        );
        // Case is significant and preserved for insertion.
        assert_eq!(
            trie.process_key(KeyPress::plain(Key::Char('A'))),
            LookupResult::InsertChar('A')
        );
    }

    #[test]
    fn await_longer_leaf_reports_pending_on_dead_end() {
        // ^C is both insert-space (leaf) and the prefix of ^C Tab.
        let mut trie = KeyTrie::new();
        let cc = ctrl('c');
        trie.insert(&[cc], Command::InsertSpace, PrefixPolicy::AwaitLonger);
        trie.insert(
            &[cc, KeyPress::plain(Key::Tab)],
            Command::ExpandSnippet,
            PrefixPolicy::ExecuteLeaf,
        );

        // ^C alone: waits for a longer sequence.
        assert_eq!(trie.process_key(cc), LookupResult::AwaitingLonger { leaf: Command::InsertSpace });
        // Tab extends it.
        assert_eq!(
            trie.process_key(KeyPress::plain(Key::Tab)),
            LookupResult::Command(Command::ExpandSnippet)
        );

        // ^C then an unrelated key: dead end with the pending leaf reported,
        // so the caller can execute insert-space and reprocess the key.
        assert_eq!(trie.process_key(cc), LookupResult::AwaitingLonger { leaf: Command::InsertSpace });
        assert_eq!(
            trie.process_key(KeyPress::plain(Key::Char('z'))),
            LookupResult::DeadEnd {
                leaf: Some(Command::InsertSpace)
            }
        );
    }

    #[test]
    fn await_longer_leaf_fires_on_timeout() {
        let mut trie = KeyTrie::new();
        let cc = ctrl('c');
        trie.insert(&[cc], Command::InsertSpace, PrefixPolicy::AwaitLonger);
        trie.insert(
            &[cc, KeyPress::plain(Key::Tab)],
            Command::ExpandSnippet,
            PrefixPolicy::ExecuteLeaf,
        );

        assert_eq!(trie.process_key(cc), LookupResult::AwaitingLonger { leaf: Command::InsertSpace });
        assert_eq!(trie.take_awaited(), Some(Command::InsertSpace));
        // After the timeout the trie reset.
        assert_eq!(trie.take_awaited(), None);
    }

    #[test]
    fn execute_leaf_policy_fires_immediately() {
        let mut trie = KeyTrie::new();
        let cc = ctrl('c');
        trie.insert(&[cc], Command::InsertSpace, PrefixPolicy::ExecuteLeaf);
        trie.insert(
            &[cc, KeyPress::plain(Key::Tab)],
            Command::ExpandSnippet,
            PrefixPolicy::ExecuteLeaf,
        );
        // Leaf executes at once; the longer sequence is unreachable by design.
        assert_eq!(
            trie.process_key(cc),
            LookupResult::Command(Command::InsertSpace)
        );
    }

    #[test]
    fn ctrl_letter_normalizes_case_for_lookup() {
        // Terminals cannot distinguish Ctrl-X from Ctrl-x.
        let press = KeyPress {
            key: Key::Char('X'),
            shift: true,
            alt: false,
            ctrl: true,
        };
        assert_eq!(press.normalize_for_lookup().key, Key::Char('x'));
        // Without ctrl, case is preserved.
        let press = KeyPress::plain(Key::Char('X'));
        assert_eq!(press.normalize_for_lookup().key, Key::Char('X'));
    }

    #[test]
    fn keypress_display_shows_modifiers() {
        assert_eq!(ctrl('x').to_string(), "Ctrl+x");
        assert_eq!(KeyPress::alt(Key::Char('x')).to_string(), "Alt+x");
        assert_eq!(KeyPress::plain(Key::Char('a')).to_string(), "a");
    }

    #[test]
    fn current_sequence_renders_the_canonical_notation() {
        let mut trie = KeyTrie::new();
        let cx = ctrl('x');
        trie.insert(&[cx, ctrl('f')], Command::FindFile, PrefixPolicy::ExecuteLeaf);

        assert_eq!(trie.current_sequence(), "");
        assert_eq!(trie.process_key(cx), LookupResult::Prefix);
        assert_eq!(trie.current_sequence(), "Ctrl+x-");
        // Completing the sequence clears it.
        assert_eq!(
            trie.process_key(ctrl('f')),
            LookupResult::Command(Command::FindFile)
        );
        assert_eq!(trie.current_sequence(), "");
    }

    #[test]
    fn current_sequence_shows_a_pending_esc_prefix() {
        let mut trie = KeyTrie::new();
        trie.insert(
            &[KeyPress::alt(Key::Char('x'))],
            Command::ExecuteNamedCommand,
            PrefixPolicy::ExecuteLeaf,
        );
        assert_eq!(
            trie.process_key(KeyPress::plain(Key::Esc)),
            LookupResult::Prefix
        );
        assert_eq!(trie.current_sequence(), "Esc-");
    }
}
