use std::collections::HashMap;
use std::fmt;
use std::str::FromStr;

/// Native key representation for erax
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Key {
    Char(char),
    Ctrl(char),
    Alt(char),
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
            Key::Char(c) => write!(f, "{}", c),
            Key::Ctrl(c) => write!(f, "C-{}", c),
            Key::Alt(c) => write!(f, "M-{}", c),
            Key::F(n) => write!(f, "F{}", n),
            Key::Esc => write!(f, "ESC"),
            Key::Enter => write!(f, "RET"),
            Key::Backspace => write!(f, "BS"),
            Key::Tab => write!(f, "TAB"),
            Key::Delete => write!(f, "DEL"),
            Key::Home => write!(f, "Home"),
            Key::End => write!(f, "End"),
            Key::PageUp => write!(f, "PgUp"),
            Key::PageDown => write!(f, "PgDn"),
            Key::Up => write!(f, "↑"),
            Key::Down => write!(f, "↓"),
            Key::Left => write!(f, "←"),
            Key::Right => write!(f, "→"),
            Key::Insert => write!(f, "Ins"),
            Key::Null => write!(f, "NUL"),
        }
    }
}

impl FromStr for Key {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        // Handle Ctrl notation: ^X or C-x
        if s.starts_with('^') && s.len() == 2 {
            let c = s.chars().nth(1).unwrap();
            return Ok(Key::Ctrl(c.to_ascii_lowercase()));
        }

        if s.starts_with("C-") && s.len() > 2 {
            let c = s.chars().nth(2).unwrap();
            return Ok(Key::Ctrl(c.to_ascii_lowercase()));
        }

        // Handle Alt/Meta notation: M-x, ESC-x, or ESC x
        if s.starts_with("M-") || s.starts_with("ESC-") || s.starts_with("ESC ") {
            let prefix_len = if s.starts_with("M-") { 2 } else { 4 };
            let rest = &s[prefix_len..];
            if rest.len() == 1 {
                let c = rest.chars().next().unwrap().to_ascii_lowercase();
                return Ok(Key::Alt(c));
            }
        }

        // Handle special keys
        match s.to_ascii_uppercase().as_str() {
            "ENTER" | "RET" => Ok(Key::Enter),
            "TAB" => Ok(Key::Tab),
            "BACKSPACE" | "BS" => Ok(Key::Backspace),
            "ESC" => Ok(Key::Esc),
            "DELETE" | "DEL" => Ok(Key::Delete),
            "HOME" => Ok(Key::Home),
            "END" => Ok(Key::End),
            "PAGEUP" | "PGUP" => Ok(Key::PageUp),
            "PAGEDOWN" | "PGDN" => Ok(Key::PageDown),
            "UP" => Ok(Key::Up),
            "DOWN" => Ok(Key::Down),
            "LEFT" => Ok(Key::Left),
            "RIGHT" => Ok(Key::Right),
            "INSERT" | "INS" => Ok(Key::Insert),
            "NUL" | "NULL" => Ok(Key::Null),
            _ => {
                // Handle Function keys F1-F24
                if s.len() >= 2 && s.starts_with('F') {
                    if let Ok(f_num) = s[1..].parse::<u8>() {
                        if f_num >= 1 && f_num <= 24 {
                            return Ok(Key::F(f_num));
                        }
                    }
                }

                // Handle single raw character
                if s.len() == 1 {
                    let c = s.chars().next().unwrap();
                    // Preserve case for raw chars
                    return Ok(Key::Char(c));
                }

                Err(format!("Unknown key: {}", s))
            }
        }
    }
}

/// Native input event representation for erax
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InputEvent {
    pub key: Key,
    pub shift: bool,
    pub alt: bool,
    pub ctrl: bool,
}

/// Mouse button types
#[derive(Debug, Clone, PartialEq, Eq, Copy, Hash)]
pub enum MouseButton {
    Left,
    Right,
    Middle,
}

/// Type of mouse event
#[derive(Debug, Clone, PartialEq, Eq, Copy)]
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
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MouseEvent {
    pub column: u16,
    pub row: u16,
    pub kind: MouseEventKind,
    pub shift: bool,
    pub alt: bool,
    pub ctrl: bool,
    pub click_count: u8, // 1 = single, 2 = double, 3 = triple
}

/// Result of a key lookup in the trie
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LookupResult {
    /// Found a complete command binding
    Command(String),
    /// Found a prefix, more keys needed
    Prefix,
    /// No matching binding found (dead end)
    DeadEnd,
    /// Key should be inserted as a character
    InsertChar(char),
}

/// A node in the key binding trie
#[derive(Debug, Clone)]

struct TrieNode {
    /// The command bound to this sequence (if any)
    command: Option<String>,
    /// Child nodes for multi-key sequences
    children: HashMap<KeyInput, Box<TrieNode>>,
}

impl TrieNode {
    fn new() -> Self {
        Self {
            command: None,
            children: HashMap::new(),
        }
    }
}

/// Normalized key input for trie lookups
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct KeyInput {
    pub key: Key,
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
}

impl KeyInput {
    pub fn from_event(event: &InputEvent) -> Self {
        // Normalize all character keys to lowercase for case-insensitive binding matching
        // This ensures caps lock state doesn't affect keybindings
        let normalized_key = match &event.key {
            Key::Char(c) => Key::Char(c.to_ascii_lowercase()),
            Key::Alt(c) => Key::Alt(c.to_ascii_lowercase()),
            Key::Ctrl(c) => Key::Ctrl(c.to_ascii_lowercase()),
            other => other.clone(),
        };
        
        // For non-character keys (like Arrows), we want to preserve Shift state
        // For Char keys, Shift is implicit in the character itself (e.g. 'A' vs 'a')
        // so we don't usually need the shift flag for Char bindings unless we want explicit "Shift-a"
        // But to unify, we copy all modifiers.
        Self {
            key: normalized_key,
            shift: event.shift,
            ctrl: event.ctrl,
            alt: event.alt,
        }
    }

    /// Parse a binding string like "^X" or "ESC-F" into a KeyInput
    pub fn from_str(s: &str) -> Option<Self> {
        let mut key_str = s;
        let mut shift = false;
        
        if s.starts_with("S-") {
            shift = true;
            key_str = &s[2..];
        }

        Key::from_str(key_str).ok().map(|k| {
            let mut ctrl = false;
            let mut alt = false;

            match k {
                Key::Ctrl(_) => ctrl = true,
                Key::Alt(_) => alt = true,
                _ => {}
            }
            
            Self { key: k, shift, ctrl, alt }
        })
    }
}

impl fmt::Display for KeyInput {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.ctrl { write!(f, "C-")?; }
        if self.alt { write!(f, "M-")?; }
        if self.shift { write!(f, "S-")?; }
        write!(f, "{}", self.key)
    }
}

/// Trie structure for efficient multi-key binding lookups
pub struct KeyTrie {
    root: TrieNode,
    /// Current position in the trie (for stateful navigation)
    current: Vec<KeyInput>,
}

impl KeyTrie {
    pub fn new() -> Self {
        Self {
            root: TrieNode::new(),
            current: Vec::new(),
        }
    }

    /// Insert a binding sequence into the trie
    /// sequence: e.g., "^X^F" -> ["^X", "^F"]
    pub fn insert(&mut self, sequence: &[&str], command: String) {
        let mut node = &mut self.root;

        for key_str in sequence {
            let key = match KeyInput::from_str(key_str) {
                Some(k) => k,
                None => continue, // Skip invalid keys
            };

            node = node
                .children
                .entry(key)
                .or_insert_with(|| Box::new(TrieNode::new()));
        }

        node.command = Some(command);
    }

    /// Process a key event and return the lookup result
    pub fn process_key(&mut self, input_event: &InputEvent) -> LookupResult {
        let key = KeyInput::from_event(input_event);

        // Try to match the key as part of a sequence
        self.current.push(key.clone());

        let mut node = &self.root;
        let mut current_matches_prefix = true;
        for k in &self.current {
            match node.children.get(k) {
                Some(child) => node = child,
                None => {
                    current_matches_prefix = false;
                    break;
                }
            }
        }

        if current_matches_prefix {
            // Check if we found a command
            if let Some(cmd) = &node.command {
                let result = LookupResult::Command(cmd.clone());
                self.current.clear();
                return result;
            }

            // Check if there are children (prefix)
            if !node.children.is_empty() {
                return LookupResult::Prefix;
            }
        }

        // No command and no children (or current sequence didn't match any prefix)
        // If it's a simple character with no modifiers, it's a self-insert
        // Use the ORIGINAL character from the event (not normalized) for proper case-sensitive typing
        if let Key::Char(c) = &input_event.key {
            // Check if it's a plain character (not Ctrl, Alt, etc.)
            if !input_event.ctrl && !input_event.alt {
                self.current.clear();
                return LookupResult::InsertChar(*c);
            }
        }

        self.current.clear();
        LookupResult::DeadEnd
    }

    /// Get the current partial key sequence as a display string
    pub fn current_sequence(&self) -> String {
        if self.current.is_empty() {
            return String::new();
        }
        self.current
            .iter()
            .map(|k| k.to_string())
            .collect::<Vec<_>>()
            .join(" ")
            + " -"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_key_input_from_str() {
        let ctrl_x = KeyInput::from_str("^X").unwrap();
        assert_eq!(ctrl_x.key, Key::Ctrl('x'));
        assert!(ctrl_x.ctrl);

        let meta_f = KeyInput::from_str("ESC-f").unwrap();
        assert_eq!(meta_f.key, Key::Alt('f'));
        assert!(meta_f.alt);

        let plain = KeyInput::from_str("a").unwrap();
        assert_eq!(plain.key, Key::Char('a'));
        assert!(!plain.ctrl);
        assert!(!plain.alt);
    }

    #[test]
    fn test_single_key_binding() {
        let mut trie = KeyTrie::new();
        trie.insert(&["^F"], "forward-char".to_string());

        let event = InputEvent {
            key: Key::Ctrl('f'),
            shift: false,
            alt: false,
            ctrl: true,
        };
        let result = trie.process_key(&event);
        assert_eq!(result, LookupResult::Command("forward-char".to_string()));
    }

    #[test]
    fn test_multi_key_sequence() {
        let mut trie = KeyTrie::new();
        trie.insert(&["^X", "^F"], "find-file".to_string());
        trie.insert(&["^X", "^C"], "quit".to_string());

        // Press ^X
        let event1 = InputEvent {
            key: Key::Ctrl('x'),
            shift: false,
            alt: false,
            ctrl: true,
        };
        let result1 = trie.process_key(&event1);
        assert_eq!(result1, LookupResult::Prefix);

        // Press ^F
        let event2 = InputEvent {
            key: Key::Ctrl('f'),
            shift: false,
            alt: false,
            ctrl: true,
        };
        let result2 = trie.process_key(&event2);
        assert_eq!(result2, LookupResult::Command("find-file".to_string()));
    }

    #[test]
    fn test_dead_end() {
        let mut trie = KeyTrie::new();
        trie.insert(&["^X", "^F"], "find-file".to_string());

        // Press ^X
        let event1 = InputEvent {
            key: Key::Ctrl('x'),
            shift: false,
            alt: false,
            ctrl: true,
        };
        assert_eq!(trie.process_key(&event1), LookupResult::Prefix);

        // Press random key
        let event2 = InputEvent {
            key: Key::Ctrl('z'),
            shift: false,
            alt: false,
            ctrl: true,
        };
        assert_eq!(trie.process_key(&event2), LookupResult::DeadEnd);

        // State should be reset
        assert!(trie.current.is_empty());
    }

    #[test]
    fn test_insert_char() {
        let mut trie = KeyTrie::new();
        trie.insert(&["^F"], "forward-char".to_string()); // Some binding exists

        let event_a = InputEvent {
            key: Key::Char('a'),
            shift: false,
            alt: false,
            ctrl: false,
        };
        let result_a = trie.process_key(&event_a);
        assert_eq!(result_a, LookupResult::InsertChar('a'));
        assert!(trie.current.is_empty());

        let event_ctrl_a = InputEvent {
            key: Key::Ctrl('a'),
            shift: false,
            alt: false,
            ctrl: true,
        };
        let result_ctrl_a = trie.process_key(&event_ctrl_a);
        assert_eq!(result_ctrl_a, LookupResult::DeadEnd); // Not a binding, not a plain char

        let event_esc = InputEvent {
            key: Key::Esc,
            shift: false,
            alt: false,
            ctrl: false,
        };
        let result_esc = trie.process_key(&event_esc);
        assert_eq!(result_esc, LookupResult::DeadEnd); // Not a binding, not a plain char
    }
}
