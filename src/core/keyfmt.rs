//! Canonical key-notation formatter: the single renderer of bindings as text.
//!
//! Every surface that shows a binding (menu hints, describe-key, the mode
//! line's pending sequence, help) renders through [`format_chord`] and
//! [`format_sequence`]. A binding string produced anywhere else is a defect:
//! two formatters would drift, and the drift class is what this module
//! exists to kill.
//!
//! The grammar is display-only; parsing and the keytrie are untouched:
//! `+` joins keys within a chord (`Ctrl+x`, `Ctrl+Shift+z`), `-` joins
//! sequence steps (`Ctrl+x-Ctrl+c` is press Ctrl+x, release, press Ctrl+c;
//! `Ctrl+x-u` is a chord, release, a plain u), and the modifier is always
//! `Alt` (never `M`).
//!
//! Key vocabulary (the chord's key half, single source here): printable
//! characters render as themselves; `F(n)` for function keys; `Esc`,
//! `Enter`, `Backspace`, `Tab`, `Delete`, `Home`, `End`, `PageUp`,
//! `PageDown`, `Up`, `Down`, `Left`, `Right`, `Insert`, `Nul` for the
//! special keys. Contemporary names, not terminal abbreviations: this
//! module is the one place that mapping exists.

use crate::core::input::{KeyPress, Key};

/// Render one chord: the key with its held modifiers.
///
/// Modifier order is Ctrl, Alt, Shift; a modifier absent from the press is
/// absent from the string.
pub fn format_chord(press: &KeyPress) -> String {
    let mut out = String::new();
    if press.ctrl {
        out.push_str("Ctrl+");
    }
    if press.alt {
        out.push_str("Alt+");
    }
    if press.shift {
        out.push_str("Shift+");
    }
    out.push_str(&format_key(press.key));
    out
}

/// Render a key sequence: chords joined with `-` between steps.
pub fn format_sequence(keys: &[KeyPress]) -> String {
    keys.iter()
        .map(format_chord)
        .collect::<Vec<_>>()
        .join("-")
}

/// The key half of a chord, in the notation's key vocabulary.
fn format_key(key: Key) -> String {
    match key {
        Key::Char(c) => c.to_string(),
        Key::F(n) => format!("F{n}"),
        Key::Esc => "Esc".to_string(),
        Key::Enter => "Enter".to_string(),
        Key::Backspace => "Backspace".to_string(),
        Key::Tab => "Tab".to_string(),
        Key::Delete => "Delete".to_string(),
        Key::Home => "Home".to_string(),
        Key::End => "End".to_string(),
        Key::PageUp => "PageUp".to_string(),
        Key::PageDown => "PageDown".to_string(),
        Key::Up => "Up".to_string(),
        Key::Down => "Down".to_string(),
        Key::Left => "Left".to_string(),
        Key::Right => "Right".to_string(),
        Key::Insert => "Insert".to_string(),
        Key::Null => "Nul".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::input::KeyPress;

    fn ctrl(c: char) -> KeyPress {
        KeyPress::ctrl(Key::Char(c))
    }

    #[test]
    fn chord_grammar() {
        assert_eq!(format_chord(&ctrl('x')), "Ctrl+x");
        assert_eq!(
            format_chord(&KeyPress::mods(Key::Char('z'), true, false, true)),
            "Ctrl+Shift+z"
        );
        assert_eq!(
            format_chord(&KeyPress::alt(Key::Char('x'))),
            "Alt+x"
        );
        assert_eq!(
            format_chord(&KeyPress::plain(Key::Char('a'))),
            "a"
        );
    }

    #[test]
    fn modifier_order_is_ctrl_alt_shift() {
        assert_eq!(
            format_chord(&KeyPress::mods(Key::Char('x'), true, true, true)),
            "Ctrl+Alt+Shift+x"
        );
    }

    #[test]
    fn alt_never_renders_as_meta() {
        let s = format_chord(&KeyPress::alt(Key::Char(' ')));
        assert!(s.starts_with("Alt+"), "got {s}");
        assert!(!s.contains('M'));
    }

    #[test]
    fn special_keys_use_the_notation_vocabulary() {
        assert_eq!(format_chord(&KeyPress::plain(Key::Esc)), "Esc");
        assert_eq!(format_chord(&KeyPress::plain(Key::Enter)), "Enter");
        assert_eq!(format_chord(&KeyPress::ctrl(Key::Tab)), "Ctrl+Tab");
        assert_eq!(format_chord(&KeyPress::plain(Key::F(10))), "F10");
    }

    #[test]
    fn sequence_grammar() {
        assert_eq!(
            format_sequence(&[ctrl('x'), ctrl('c')]),
            "Ctrl+x-Ctrl+c"
        );
        assert_eq!(
            format_sequence(&[ctrl('x'), KeyPress::plain(Key::Char('u'))]),
            "Ctrl+x-u"
        );
        assert_eq!(format_sequence(&[ctrl('s')]), "Ctrl+s");
        assert_eq!(format_sequence(&[]), "");
    }
}
