use crate::core::input::{InputEvent, KeyTrie, LookupResult};

/// KeyBindingManager handles conversion of key events to commands
/// Uses a KeyTrie for efficient multi-key sequence matching
pub struct KeyBindingManager {
    trie: KeyTrie,
}

impl KeyBindingManager {
    pub fn new() -> Self {
        Self {
            trie: KeyTrie::new(),
        }
    }

    /// Add a key binding from a sequence string like "^X^F"
    pub fn bind(&mut self, sequence: &str, command: String) {
        // Parse sequence into individual keys
        let keys: Vec<String> = self.parse_sequence(sequence);
        let key_refs: Vec<&str> = keys.iter().map(|s| s.as_str()).collect();
        self.trie.insert(&key_refs, command);
    }

    /// Parse a sequence string like "^X^F" into vec of owned strings
    fn parse_sequence(&self, sequence: &str) -> Vec<String> {
        let mut keys = Vec::new();
        let mut i = 0;
        let chars: Vec<char> = sequence.chars().collect();

        while i < chars.len() {
            if chars[i].is_whitespace() {
                i += 1;
                continue;
            }

            if chars[i] == '^' {
                if i + 1 < chars.len() {
                    // Control key: ^ + char
                    let mut s = String::new();
                    s.push('^');
                    s.push(chars[i + 1]);
                    keys.push(s);
                    i += 2;
                } else {
                    // Dangling ^ at end, ignore
                    i += 1;
                }
            } else if i + 3 < chars.len()
                && chars[i] == 'E'
                && chars[i + 1] == 'S'
                && chars[i + 2] == 'C'
                && chars[i + 3] == '-'
            {
                // Meta/Alt key: ESC- + char
                // Expand to [Esc, char] sequence because terminals send ESC followed by the key
                // as two separate events (not a single Alt event)
                if i + 4 < chars.len() {
                    keys.push("Esc".to_string());
                    let c = chars[i + 4].to_ascii_lowercase();
                    keys.push(c.to_string());
                    i += 5;
                } else {
                    // Dangling ESC- at end, ignore
                    i += 4;
                }
            } else if i + 2 < chars.len()
                && chars[i] == 'S'
                && chars[i + 1] == '-'
            {
                // Shift key: S- + char/key
                if i + 2 < chars.len() {
                    let mut s = String::new();
                    s.push_str("S-");
                    
                    // Consume the rest of the key (could be single char or named key)
                    // We need to parse the *next* key part from rest. 
                    // This is tricky because we need to reuse the named key logic.
                    // For now, let's assume it's followed by a named key or char.
                    // Actually, let's just push "S-" and let the next iteration handle the key?
                    // No, keys list expects full strings.
                    
                    // Recursive call or lookahead?
                    // Let's grab the next char/named key manually.
                    let mut key_len = 0;
                    let mut matched_key = String::new();
                    
                    let remaining: String = chars[i+2..].iter().collect();
                    
                    let known_keys = [
                        "PageUp", "PageDown", "Backspace", "Delete", "Enter", "Tab",
                        "Home", "End", "Up", "Down", "Left", "Right", "Esc", "Insert"
                    ];
                    
                    let mut found = false;
                    for key_name in &known_keys {
                        if remaining.to_lowercase().starts_with(&key_name.to_lowercase()) {
                            matched_key = key_name.to_string();
                            key_len = key_name.len();
                            found = true;
                            break;
                        }
                    }
                    
                    if !found && !remaining.is_empty() {
                        matched_key = chars[i+2].to_string();
                        key_len = 1;
                    }
                    
                    if !matched_key.is_empty() {
                        s.push_str(&matched_key);
                        keys.push(s);
                        i += 2 + key_len;
                    } else {
                        i += 2; // Dangling S-
                    }
                } else {
                    i += 2;
                }
            } else {
                // Check for multi-character key names (Up, Down, Left, Right, etc.)
                let remaining: String = chars[i..].iter().collect();
                let mut matched = false;

                // List of known multi-character key names
                // Longest matches first to avoid prefix issues
                let known_keys = [
                    "PageUp",
                    "PageDown",
                    "Backspace",
                    "Delete",
                    "Enter",
                    "Tab",
                    "Home",
                    "End",
                    "Up",
                    "Down",
                    "Left",
                    "Right",
                    "Esc",
                ];

                for key_name in &known_keys {
                    // Case insensitive check for robustness
                    if remaining
                        .to_lowercase()
                        .starts_with(&key_name.to_lowercase())
                    {
                        keys.push(key_name.to_string());
                        i += key_name.len();
                        matched = true;
                        break;
                    }
                }

                if !matched {
                    // Single character
                    keys.push(chars[i].to_string());
                    i += 1;
                }
            }
        }

        keys
    }

    /// Process a key event and return the result
    /// Returns (command_name, char_to_insert, is_complete)
    /// If is_complete is false, more keys are needed for the sequence
    pub fn process_key(
        &mut self,
        input_event: &InputEvent,
    ) -> (Option<String>, Option<char>, bool) {
        match self.trie.process_key(input_event) {
            LookupResult::Command(cmd) => (Some(cmd), None, true),
            LookupResult::Prefix => (None, None, false),
            LookupResult::DeadEnd => (None, None, true),
            LookupResult::InsertChar(c) => (None, Some(c), true),
        }
    }

    /// Get the current partial key sequence for display
    pub fn current_sequence(&self) -> String {
        self.trie.current_sequence()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::input::Key;

    #[test]
    fn test_multi_key_sequence() {
        let mut manager = KeyBindingManager::new();
        manager.bind("^X^F", "find-file".to_string());

        // Press ^X
        let input_event1 = InputEvent {
            key: Key::Ctrl('x'),
            shift: false,
            alt: false,
            ctrl: true,
        };
        let (cmd, char_to_insert, complete) = manager.process_key(&input_event1);
        assert_eq!(cmd, None);
        assert_eq!(char_to_insert, None);
        assert!(!complete);

        // Press ^F
        let input_event2 = InputEvent {
            key: Key::Ctrl('f'),
            shift: false,
            alt: false,
            ctrl: true,
        };
        let (cmd, char_to_insert, complete) = manager.process_key(&input_event2);
        assert_eq!(cmd, Some("find-file".to_string()));
        assert_eq!(char_to_insert, None);
        assert!(complete);
    }

    #[test]
    fn test_single_key_binding() {
        let mut manager = KeyBindingManager::new();
        manager.bind("^F", "forward-char".to_string());

        let input_event = InputEvent {
            key: Key::Ctrl('f'),
            shift: false,
            alt: false,
            ctrl: true,
        };
        let (cmd, char_to_insert, complete) = manager.process_key(&input_event);
        assert_eq!(cmd, Some("forward-char".to_string()));
        assert_eq!(char_to_insert, None);
        assert!(complete);
    }

    #[test]
    fn test_dead_end() {
        let mut manager = KeyBindingManager::new();
        manager.bind("^X^F", "find-file".to_string());

        // Press ^X
        let input_event1 = InputEvent {
            key: Key::Ctrl('x'),
            shift: false,
            alt: false,
            ctrl: true,
        };
        let (cmd, char_to_insert, complete) = manager.process_key(&input_event1);
        assert_eq!(cmd, None);
        assert_eq!(char_to_insert, None);
        assert!(!complete);

        // Press wrong key
        let input_event2 = InputEvent {
            key: Key::Ctrl('z'),
            shift: false,
            alt: false,
            ctrl: true,
        };
        let (cmd, char_to_insert, complete) = manager.process_key(&input_event2);
        assert_eq!(cmd, None);
        assert_eq!(char_to_insert, None);
        assert!(complete); // Dead end, sequence reset
    }

    #[test]
    fn test_parse_sequence_edge_cases() {
        let manager = KeyBindingManager::new();

        // Empty
        assert_eq!(manager.parse_sequence(""), Vec::<String>::new());

        // Malformed - just ^ with no following char
        let result = manager.parse_sequence("^");
        assert_eq!(result, Vec::<String>::new());

        // Malformed - ESC- with no following char
        let result2 = manager.parse_sequence("ESC-");
        assert_eq!(result2, Vec::<String>::new());
    }

    #[test]
    fn test_parse_sequence_valid() {
        let manager = KeyBindingManager::new();

        // Single control key
        let result = manager.parse_sequence("^X");
        assert_eq!(result, vec!["^X"]);

        // Multi-key sequence
        let result2 = manager.parse_sequence("^X^F");
        assert_eq!(result2, vec!["^X", "^F"]);

        // Meta key (ESC- prefix becomes Esc + char sequence)
        let result3 = manager.parse_sequence("ESC-x");
        assert_eq!(result3, vec!["Esc", "x"]);
    }

    #[test]
    fn test_bind_and_lookup() {
        let mut manager = KeyBindingManager::new();
        manager.bind("^G", "abort".to_string());

        let input_event = InputEvent {
            key: Key::Ctrl('g'),
            shift: false,
            alt: false,
            ctrl: true,
        };
        let (cmd, char_to_insert, complete) = manager.process_key(&input_event);
        assert_eq!(cmd, Some("abort".to_string()));
        assert_eq!(char_to_insert, None);
        assert!(complete);
    }

    #[test]
    fn test_insert_char_passthrough() {
        let mut manager = KeyBindingManager::new();
        manager.bind("^A", "select-all".to_string());

        // Unbound character, should be InsertChar
        let input_event = InputEvent {
            key: Key::Char('b'),
            shift: false,
            alt: false,
            ctrl: false,
        };
        let (cmd, char_to_insert, complete) = manager.process_key(&input_event);
        assert_eq!(cmd, None);
        assert_eq!(char_to_insert, Some('b'));
        assert!(complete);

        // Bound command, should be Command
        let input_event_ctrl_a = InputEvent {
            key: Key::Ctrl('a'),
            shift: false,
            alt: false,
            ctrl: true,
        };
        let (cmd, char_to_insert, complete) = manager.process_key(&input_event_ctrl_a);
        assert_eq!(cmd, Some("select-all".to_string()));
        assert_eq!(char_to_insert, None);
        assert!(complete);
    }

    #[test]
    fn test_shift_modifier() {
        let mut manager = KeyBindingManager::new();
        manager.bind("S-Right", "forward-char-extend".to_string());

        // Press Shift+Right
        let input_event = InputEvent {
            key: Key::Right,
            shift: true,
            alt: false,
            ctrl: false,
        };
        let (cmd, char_to_insert, complete) = manager.process_key(&input_event);
        assert_eq!(cmd, Some("forward-char-extend".to_string()));
        assert_eq!(char_to_insert, None);
        assert!(complete);

        // Press Right (no shift)
        let input_event_plain = InputEvent {
            key: Key::Right,
            shift: false,
            alt: false,
            ctrl: false,
        };
        let (cmd_plain, _, _) = manager.process_key(&input_event_plain);
        assert_eq!(cmd_plain, None); // Should not match
    }

    #[test]
    fn test_esc_sequence_behavior() {
        let mut manager = KeyBindingManager::new();
        // Bind ESC-f (Alt-f)
        manager.bind("ESC-f", "forward-word".to_string());

        // Sequence: ESC then f
        let event_esc = InputEvent {
            key: Key::Esc,
            shift: false,
            alt: false,
            ctrl: false,
        };
        let (cmd1, _, complete1) = manager.process_key(&event_esc);
        assert_eq!(cmd1, None);
        assert!(!complete1); // Prefix

        let event_f = InputEvent {
            key: Key::Char('f'),
            shift: false,
            alt: false,
            ctrl: false,
        };
        let (cmd2, _, complete2) = manager.process_key(&event_f);
        assert_eq!(cmd2, Some("forward-word".to_string()));
        assert!(complete2);
    }
}
