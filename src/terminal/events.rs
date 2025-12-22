use crate::core::input::{InputEvent, Key, MouseButton, MouseEvent, MouseEventKind};
// use crate::terminal::parser::AnsiParser; // Removed

use crossterm::event::{self, Event, KeyCode, KeyModifiers};
use std::time::{Duration, Instant};

/// Editor events
#[derive(Debug, Clone, PartialEq)]
pub enum EditorEvent {
    Input(InputEvent),
    Resize(u16, u16),
    Mouse(MouseEvent),
    None,
}

pub struct EventHandler {
    // Click tracking for double/triple clicks
    last_click_time: Option<Instant>,
    last_click_pos: (u16, u16),
    last_click_button: Option<MouseButton>,
    current_click_count: u8,
}

impl EventHandler {
    /// Create a new EventHandler
    pub fn new() -> Self {
        Self {
            last_click_time: None,
            last_click_pos: (0, 0),
            last_click_button: None,
            current_click_count: 0,
        }
    }

    /// Check for available events with a timeout
    pub fn poll(&self, timeout: Duration) -> Result<bool, Box<dyn std::error::Error>> {
        Ok(crossterm::event::poll(timeout)?)
    }

    /// Read the next event
    pub fn read(&mut self) -> Result<EditorEvent, Box<dyn std::error::Error>> {
        // We use a small poll first to be non-blocking compliant with the loop structure if needed,
        // but read() blocks. The main loop calls poll() first anyway.
        if event::poll(Duration::from_millis(0))? {
            match event::read()? {
                Event::Key(key_event) => {
                    // Normalize key event
                    let input_event = self.crossterm_key_to_input(key_event);
                    return Ok(EditorEvent::Input(input_event));
                }
                Event::Resize(cols, rows) => {
                    return Ok(EditorEvent::Resize(cols, rows));
                }
                Event::Mouse(mouse_event) => {
                    let event = self.process_mouse_event(mouse_event);
                    return Ok(EditorEvent::Mouse(event));
                }
                _ => return Ok(EditorEvent::None),
            }
        }
        Ok(EditorEvent::None)
    }

    fn process_mouse_event(&mut self, event: crossterm::event::MouseEvent) -> MouseEvent {
        let modifiers = event.modifiers;
        let shift = modifiers.contains(KeyModifiers::SHIFT);
        let alt = modifiers.contains(KeyModifiers::ALT);
        let ctrl = modifiers.contains(KeyModifiers::CONTROL);

        let kind = match event.kind {
            crossterm::event::MouseEventKind::Down(btn) => {
                MouseEventKind::Down(self.convert_button(btn))
            }
            crossterm::event::MouseEventKind::Up(btn) => {
                MouseEventKind::Up(self.convert_button(btn))
            }
            crossterm::event::MouseEventKind::Drag(btn) => {
                MouseEventKind::Drag(self.convert_button(btn))
            }
            crossterm::event::MouseEventKind::Moved => MouseEventKind::Moved,
            crossterm::event::MouseEventKind::ScrollDown => MouseEventKind::ScrollDown,
            crossterm::event::MouseEventKind::ScrollUp => MouseEventKind::ScrollUp,
            crossterm::event::MouseEventKind::ScrollLeft => MouseEventKind::ScrollLeft,
            crossterm::event::MouseEventKind::ScrollRight => MouseEventKind::ScrollRight,
        };

        // Handle click counting
        if let MouseEventKind::Down(btn) = kind {
            let now = Instant::now();
            let mut is_multi_click = false;

            if let Some(last_time) = self.last_click_time {
                if now.duration_since(last_time) < Duration::from_millis(500)
                    && self.last_click_pos == (event.column, event.row)
                    && self.last_click_button == Some(btn)
                {
                    is_multi_click = true;
                }
            }

            if is_multi_click {
                self.current_click_count = self.current_click_count.saturating_add(1);
            } else {
                self.current_click_count = 1;
            }

            self.last_click_time = Some(now);
            self.last_click_pos = (event.column, event.row);
            self.last_click_button = Some(btn);
        } else if matches!(kind, MouseEventKind::Moved | MouseEventKind::Drag(_)) {
            // Do not reset click count on move/drag
        } else {
            // For Up/Scroll, preserve context if needed, but usually reset tracking if too long?
            // Actually, Up matches Down, so we don't reset count.
        }

        MouseEvent {
            column: event.column,
            row: event.row,
            kind,
            shift,
            alt,
            ctrl,
            click_count: self.current_click_count,
        }
    }

    fn convert_button(&self, btn: crossterm::event::MouseButton) -> MouseButton {
        match btn {
            crossterm::event::MouseButton::Left => MouseButton::Left,
            crossterm::event::MouseButton::Right => MouseButton::Right,
            crossterm::event::MouseButton::Middle => MouseButton::Middle,
        }
    }

    fn crossterm_key_to_input(&self, key_event: event::KeyEvent) -> InputEvent {
        let code = key_event.code;
        let modifiers = key_event.modifiers;

        let shift = modifiers.contains(KeyModifiers::SHIFT);
        let alt = modifiers.contains(KeyModifiers::ALT);
        let ctrl = modifiers.contains(KeyModifiers::CONTROL);

        let key = match code {
            KeyCode::Char(c) => {
                if ctrl && !alt {
                    Key::Ctrl(c)
                } else if alt && !ctrl {
                    Key::Alt(c)
                } else {
                    Key::Char(c)
                }
            }
            KeyCode::Backspace => Key::Backspace,
            KeyCode::Enter => Key::Enter,
            KeyCode::Left => Key::Left,
            KeyCode::Right => Key::Right,
            KeyCode::Up => Key::Up,
            KeyCode::Down => Key::Down,
            KeyCode::Home => Key::Home,
            KeyCode::End => Key::End,
            KeyCode::PageUp => Key::PageUp,
            KeyCode::PageDown => Key::PageDown,
            KeyCode::Tab => Key::Tab,
            KeyCode::BackTab => Key::Tab, // handle as shift-tab ideally
            KeyCode::Delete => Key::Delete,
            KeyCode::Insert => Key::Insert,
            KeyCode::F(n) => Key::F(n),
            KeyCode::Esc => Key::Esc,
            KeyCode::Null => Key::Null,
            _ => Key::Null,
        };

        InputEvent {
            key,
            shift,
            alt,
            ctrl,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_event_handler_structure() {
        // Just verify the struct and functions exist
        let _ = EventHandler::new();
    }

    #[test]
    fn test_double_click_detection() {
        let mut handler = EventHandler::new();

        // First click
        let evt1 = crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
            column: 10,
            row: 10,
            modifiers: crossterm::event::KeyModifiers::empty(),
        };
        let res1 = handler.process_mouse_event(evt1);
        assert_eq!(res1.click_count, 1);
        assert_eq!(res1.kind, MouseEventKind::Down(MouseButton::Left));

        // Release
        let evt2 = crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::Up(crossterm::event::MouseButton::Left),
            column: 10,
            row: 10,
            modifiers: crossterm::event::KeyModifiers::empty(),
        };
        let _res2 = handler.process_mouse_event(evt2);

        // Second click (immediate)
        let evt3 = crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
            column: 10,
            row: 10,
            modifiers: crossterm::event::KeyModifiers::empty(),
        };
        let res3 = handler.process_mouse_event(evt3);
        assert_eq!(res3.click_count, 2);

        // Third click (immediate)
        let evt4 = crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
            column: 10,
            row: 10,
            modifiers: crossterm::event::KeyModifiers::empty(),
        };
        let res4 = handler.process_mouse_event(evt4);
        assert_eq!(res4.click_count, 3);
    }
}
