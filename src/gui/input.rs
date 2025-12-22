//! Input translation layer for GUI mode
//!
//! Translates winit keyboard/mouse events to the TUI's input event format.

use crate::core::geometry::GridMetrics;
use crate::core::input::{InputEvent, Key};
use winit::event::ElementState;
use winit::keyboard::{KeyCode, ModifiersState, PhysicalKey};

/// Convert a winit physical key to our Key enum
pub fn winit_key_to_key(physical_key: PhysicalKey, mods: ModifiersState) -> Option<Key> {
    match physical_key {
        PhysicalKey::Code(code) => {
            // Handle letter keys
            if let Some(ch) = keycode_to_char(code, mods.shift_key()) {
                if mods.control_key() {
                    // Ctrl+letter -> Key::Ctrl(letter)
                    Some(Key::Ctrl(ch.to_ascii_lowercase()))
                } else if mods.alt_key() {
                    Some(Key::Alt(ch))
                } else {
                    Some(Key::Char(ch))
                }
            } else {
                // Handle special keys
                match code {
                    KeyCode::Enter => Some(Key::Enter),
                    KeyCode::Tab => Some(Key::Tab),
                    KeyCode::Backspace => Some(Key::Backspace),
                    KeyCode::Delete => Some(Key::Delete),
                    KeyCode::Escape => Some(Key::Esc),
                    KeyCode::ArrowUp => Some(Key::Up),
                    KeyCode::ArrowDown => Some(Key::Down),
                    KeyCode::ArrowLeft => Some(Key::Left),
                    KeyCode::ArrowRight => Some(Key::Right),
                    KeyCode::Home => Some(Key::Home),
                    KeyCode::End => Some(Key::End),
                    KeyCode::PageUp => Some(Key::PageUp),
                    KeyCode::PageDown => Some(Key::PageDown),
                    KeyCode::Space => {
                        if mods.control_key() {
                            Some(Key::Ctrl(' '))
                        } else {
                            Some(Key::Char(' '))
                        }
                    }
                    _ => None,
                }
            }
        }
        PhysicalKey::Unidentified(_) => None,
    }
}

/// Convert a keycode to a character, considering shift state
fn keycode_to_char(code: KeyCode, shift: bool) -> Option<char> {
    let ch = match code {
        KeyCode::KeyA => 'a',
        KeyCode::KeyB => 'b',
        KeyCode::KeyC => 'c',
        KeyCode::KeyD => 'd',
        KeyCode::KeyE => 'e',
        KeyCode::KeyF => 'f',
        KeyCode::KeyG => 'g',
        KeyCode::KeyH => 'h',
        KeyCode::KeyI => 'i',
        KeyCode::KeyJ => 'j',
        KeyCode::KeyK => 'k',
        KeyCode::KeyL => 'l',
        KeyCode::KeyM => 'm',
        KeyCode::KeyN => 'n',
        KeyCode::KeyO => 'o',
        KeyCode::KeyP => 'p',
        KeyCode::KeyQ => 'q',
        KeyCode::KeyR => 'r',
        KeyCode::KeyS => 's',
        KeyCode::KeyT => 't',
        KeyCode::KeyU => 'u',
        KeyCode::KeyV => 'v',
        KeyCode::KeyW => 'w',
        KeyCode::KeyX => 'x',
        KeyCode::KeyY => 'y',
        KeyCode::KeyZ => 'z',
        KeyCode::Digit0 => {
            if shift {
                ')'
            } else {
                '0'
            }
        }
        KeyCode::Digit1 => {
            if shift {
                '!'
            } else {
                '1'
            }
        }
        KeyCode::Digit2 => {
            if shift {
                '@'
            } else {
                '2'
            }
        }
        KeyCode::Digit3 => {
            if shift {
                '#'
            } else {
                '3'
            }
        }
        KeyCode::Digit4 => {
            if shift {
                '$'
            } else {
                '4'
            }
        }
        KeyCode::Digit5 => {
            if shift {
                '%'
            } else {
                '5'
            }
        }
        KeyCode::Digit6 => {
            if shift {
                '^'
            } else {
                '6'
            }
        }
        KeyCode::Digit7 => {
            if shift {
                '&'
            } else {
                '7'
            }
        }
        KeyCode::Digit8 => {
            if shift {
                '*'
            } else {
                '8'
            }
        }
        KeyCode::Digit9 => {
            if shift {
                '('
            } else {
                '9'
            }
        }
        KeyCode::Minus => {
            if shift {
                '_'
            } else {
                '-'
            }
        }
        KeyCode::Equal => {
            if shift {
                '+'
            } else {
                '='
            }
        }
        KeyCode::BracketLeft => {
            if shift {
                '{'
            } else {
                '['
            }
        }
        KeyCode::BracketRight => {
            if shift {
                '}'
            } else {
                ']'
            }
        }
        KeyCode::Backslash => {
            if shift {
                '|'
            } else {
                '\\'
            }
        }
        KeyCode::Semicolon => {
            if shift {
                ':'
            } else {
                ';'
            }
        }
        KeyCode::Quote => {
            if shift {
                '"'
            } else {
                '\''
            }
        }
        KeyCode::Comma => {
            if shift {
                '<'
            } else {
                ','
            }
        }
        KeyCode::Period => {
            if shift {
                '>'
            } else {
                '.'
            }
        }
        KeyCode::Slash => {
            if shift {
                '?'
            } else {
                '/'
            }
        }
        KeyCode::Backquote => {
            if shift {
                '~'
            } else {
                '`'
            }
        }
        _ => return None,
    };

    Some(if shift && ch.is_ascii_lowercase() {
        ch.to_ascii_uppercase()
    } else {
        ch
    })
}

/// Create an InputEvent from key information
pub fn create_input_event(key: Key, mods: ModifiersState) -> InputEvent {
    InputEvent {
        key,
        shift: mods.shift_key(),
        alt: mods.alt_key(),
        ctrl: mods.control_key(),
    }
}

/// Check if a key event should be processed (key down only)
pub fn should_process(state: ElementState) -> bool {
    state == ElementState::Pressed
}

/// Convert mouse click position to grid coordinates
///
/// Uses GridMetrics to properly handle cell dimensions and viewport offsets.
/// Returns (col, row) if click is within the grid, None otherwise.
pub fn mouse_pos_to_grid(
    x: f32,
    y: f32,
    cell_width: f32,
    cell_height: f32,
    viewport_width: f32,
    viewport_height: f32,
) -> Option<(usize, usize)> {
    let metrics = GridMetrics::new(cell_width, cell_height, viewport_width, viewport_height);
    metrics.px_to_grid(x, y)
}
