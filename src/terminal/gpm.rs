//! GPM (General Purpose Mouse) client for Linux console
//!
//! Implements the GPM client protocol by connecting to the GPM daemon
//! via Unix socket at /dev/gpmctl. Based on libgpm source code from
//! https://github.com/telmich/gpm
//!
//! The protocol:
//! 1. Connect to /dev/gpmctl (AF_UNIX, SOCK_STREAM)
//! 2. Send Gpm_Connect struct (16 bytes) with eventMask, defaultMask, minMod, maxMod, pid, vc
//! 3. Read Gpm_Event structs (28 bytes each) when mouse activity occurs

use crate::core::input::{MouseButton, MouseEvent, MouseEventKind};
use std::io::{self, Read, Write};
use std::os::unix::net::UnixStream;

/// GPM event type flags (from gpm.h enum Gpm_Etype)
const GPM_DRAG: i32 = 2;
const GPM_DOWN: i32 = 4;
const GPM_UP: i32 = 8;
const GPM_DOUBLE: i32 = 32;
const GPM_TRIPLE: i32 = 64;

/// GPM button masks (from gpm.h)
const GPM_B_LEFT: u8 = 4;
const GPM_B_MIDDLE: u8 = 2;
const GPM_B_RIGHT: u8 = 1;

/// Gpm_Connect struct: eventMask(2) + defaultMask(2) + minMod(2) + maxMod(2) + pid(4) + vc(4) = 16 bytes
const CONNECT_SIZE: usize = 16;

/// Gpm_Event struct: buttons(1) + modifiers(1) + vc(2) + dx(2) + dy(2) + x(2) + y(2) + 
/// type(4) + clicks(4) + margin(4) + wdx(2) + wdy(2) = 28 bytes
const EVENT_SIZE: usize = 28;

/// GPM client for Linux console mouse support
pub struct GpmClient {
    socket: Option<UnixStream>,
    last_x: u16,
    last_y: u16,
}

impl Default for GpmClient {
    fn default() -> Self {
        Self::new()
    }
}

impl GpmClient {
    pub fn new() -> Self {
        Self {
            socket: None,
            last_x: 0,
            last_y: 0,
        }
    }

    /// Check if we should use GPM (Linux console without X/Wayland)
    pub fn should_use_gpm() -> bool {
        // Not on graphical display
        if std::env::var("DISPLAY").is_ok() || std::env::var("WAYLAND_DISPLAY").is_ok() {
            return false;
        }
        // Must be linux or vt terminal type
        std::env::var("TERM")
            .map(|t| t == "linux" || t.starts_with("vt"))
            .unwrap_or(false)
    }

    /// Get current virtual console number from /sys
    fn get_vc() -> Option<i32> {
        // Try to get VC from tty name first
        if let Ok(tty) = std::fs::read_link("/proc/self/fd/0") {
            if let Some(name) = tty.to_str() {
                if let Some(num) = name.strip_prefix("/dev/tty") {
                    if let Ok(n) = num.parse::<i32>() {
                        if n > 0 && n < 64 {
                            return Some(n);
                        }
                    }
                }
            }
        }
        // Fallback to active tty
        std::fs::read_to_string("/sys/class/tty/tty0/active")
            .ok()
            .and_then(|s| s.trim().strip_prefix("tty")?.parse().ok())
    }

    /// Connect to GPM daemon
    pub fn connect(&mut self) -> io::Result<()> {
        if self.socket.is_some() {
            return Ok(());
        }

        // Connect to GPM control socket
        let socket = UnixStream::connect("/dev/gpmctl")?;
        
        // Get current VC - required for GPM to send us events
        let vc = Self::get_vc().unwrap_or(0);
        let pid = std::process::id() as i32;

        // Build Gpm_Connect struct (all fields little-endian on x86)
        let mut conn = [0u8; CONNECT_SIZE];
        
        // eventMask: request all events (0xFFFF)
        conn[0..2].copy_from_slice(&0xFFFFu16.to_ne_bytes());
        // defaultMask: don't pass events to selection (0)
        conn[2..4].copy_from_slice(&0u16.to_ne_bytes());
        // minMod: accept all modifiers (0)
        conn[4..6].copy_from_slice(&0u16.to_ne_bytes());
        // maxMod: accept all modifiers (0xFFFF)
        conn[6..8].copy_from_slice(&0xFFFFu16.to_ne_bytes());
        // pid: our process ID
        conn[8..12].copy_from_slice(&pid.to_ne_bytes());
        // vc: virtual console number
        conn[12..16].copy_from_slice(&vc.to_ne_bytes());

        let mut sock = socket;
        sock.write_all(&conn)?;
        
        // Set non-blocking AFTER the connect handshake
        sock.set_nonblocking(true)?;
        
        self.socket = Some(sock);
        Ok(())
    }

    /// Poll for mouse event (non-blocking, returns None if no event ready)
    pub fn poll(&mut self) -> Option<MouseEvent> {
        let socket = self.socket.as_mut()?;
        let mut buf = [0u8; EVENT_SIZE];

        match socket.read_exact(&mut buf) {
            Ok(()) => {
                let event = self.parse_event(&buf);
                self.last_x = event.column;
                self.last_y = event.row;
                Some(event)
            }
            Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => None,
            Err(_) => {
                // Connection lost - GPM daemon may have restarted
                self.socket = None;
                None
            }
        }
    }

    fn parse_event(&self, buf: &[u8; EVENT_SIZE]) -> MouseEvent {
        // Gpm_Event struct layout (from gpm.h):
        // unsigned char buttons;      // byte 0
        // unsigned char modifiers;    // byte 1  
        // unsigned short vc;          // bytes 2-3
        // short dx, dy;               // bytes 4-7 (relative movement)
        // short x, y;                 // bytes 8-11 (absolute position, 1-based)
        // enum Gpm_Etype type;        // bytes 12-15 (int)
        // int clicks;                 // bytes 16-19
        // enum Gpm_Margin margin;     // bytes 20-23
        // short wdx, wdy;             // bytes 24-27 (wheel movement)
        
        let buttons = buf[0];
        let modifiers = buf[1];
        let x = i16::from_ne_bytes([buf[8], buf[9]]);
        let y = i16::from_ne_bytes([buf[10], buf[11]]);
        let event_type = i32::from_ne_bytes([buf[12], buf[13], buf[14], buf[15]]);
        let wdy = i16::from_ne_bytes([buf[26], buf[27]]);

        // Determine which button is active
        let button = if buttons & GPM_B_LEFT != 0 {
            MouseButton::Left
        } else if buttons & GPM_B_MIDDLE != 0 {
            MouseButton::Middle
        } else if buttons & GPM_B_RIGHT != 0 {
            MouseButton::Right
        } else {
            MouseButton::Left // Default for move events
        };

        // Determine event kind - check wheel first, then button events
        let kind = if wdy > 0 {
            MouseEventKind::ScrollUp
        } else if wdy < 0 {
            MouseEventKind::ScrollDown
        } else if event_type & GPM_DOWN != 0 {
            MouseEventKind::Down(button)
        } else if event_type & GPM_UP != 0 {
            MouseEventKind::Up(button)
        } else if event_type & GPM_DRAG != 0 {
            MouseEventKind::Drag(button)
        } else {
            MouseEventKind::Moved
        };

        // Click count from event type flags
        let click_count = if event_type & GPM_TRIPLE != 0 {
            3
        } else if event_type & GPM_DOUBLE != 0 {
            2
        } else {
            1
        };

        // GPM coordinates are 1-based, convert to 0-based
        MouseEvent {
            column: (x.max(1) - 1) as u16,
            row: (y.max(1) - 1) as u16,
            kind,
            shift: modifiers & 1 != 0,
            ctrl: modifiers & 4 != 0,
            alt: modifiers & 8 != 0,
            click_count,
        }
    }

    pub fn is_connected(&self) -> bool {
        self.socket.is_some()
    }

    /// Get current cursor position (column, row)
    pub fn cursor_pos(&self) -> (u16, u16) {
        (self.last_x, self.last_y)
    }


}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper to create a Gpm_Event buffer for testing
    fn make_event(
        buttons: u8,
        modifiers: u8,
        x: i16,
        y: i16,
        event_type: i32,
        wdy: i16,
    ) -> [u8; EVENT_SIZE] {
        let mut buf = [0u8; EVENT_SIZE];
        buf[0] = buttons;
        buf[1] = modifiers;
        // bytes 2-3: vc (unused in parsing)
        // bytes 4-7: dx, dy (unused in parsing)
        buf[8..10].copy_from_slice(&x.to_ne_bytes());
        buf[10..12].copy_from_slice(&y.to_ne_bytes());
        buf[12..16].copy_from_slice(&event_type.to_ne_bytes());
        // bytes 16-19: clicks (unused - we use event_type flags)
        // bytes 20-23: margin (unused)
        // bytes 24-25: wdx (unused)
        buf[26..28].copy_from_slice(&wdy.to_ne_bytes());
        buf
    }

    #[test]
    fn test_parse_move_event() {
        let client = GpmClient::new();
        let buf = make_event(0, 0, 10, 5, 1, 0); // GPM_MOVE = 1
        let event = client.parse_event(&buf);
        
        assert_eq!(event.column, 9); // 1-based to 0-based
        assert_eq!(event.row, 4);
        assert!(matches!(event.kind, MouseEventKind::Moved));
        assert!(!event.shift);
        assert!(!event.ctrl);
        assert!(!event.alt);
    }

    #[test]
    fn test_parse_left_button_down() {
        let client = GpmClient::new();
        let buf = make_event(GPM_B_LEFT, 0, 20, 15, GPM_DOWN, 0);
        let event = client.parse_event(&buf);
        
        assert_eq!(event.column, 19);
        assert_eq!(event.row, 14);
        assert!(matches!(event.kind, MouseEventKind::Down(MouseButton::Left)));
    }

    #[test]
    fn test_parse_right_button_up() {
        let client = GpmClient::new();
        let buf = make_event(GPM_B_RIGHT, 0, 30, 25, GPM_UP, 0);
        let event = client.parse_event(&buf);
        
        assert_eq!(event.column, 29);
        assert_eq!(event.row, 24);
        assert!(matches!(event.kind, MouseEventKind::Up(MouseButton::Right)));
    }

    #[test]
    fn test_parse_middle_button_drag() {
        let client = GpmClient::new();
        let buf = make_event(GPM_B_MIDDLE, 0, 40, 35, GPM_DRAG, 0);
        let event = client.parse_event(&buf);
        
        assert!(matches!(event.kind, MouseEventKind::Drag(MouseButton::Middle)));
    }

    #[test]
    fn test_parse_scroll_up() {
        let client = GpmClient::new();
        let buf = make_event(0, 0, 10, 10, 1, 1); // wdy > 0 = scroll up
        let event = client.parse_event(&buf);
        
        assert!(matches!(event.kind, MouseEventKind::ScrollUp));
    }

    #[test]
    fn test_parse_scroll_down() {
        let client = GpmClient::new();
        let buf = make_event(0, 0, 10, 10, 1, -1); // wdy < 0 = scroll down
        let event = client.parse_event(&buf);
        
        assert!(matches!(event.kind, MouseEventKind::ScrollDown));
    }

    #[test]
    fn test_parse_modifiers() {
        let client = GpmClient::new();
        // Shift = 1, Ctrl = 4, Alt = 8
        let buf = make_event(0, 1 | 4 | 8, 10, 10, 1, 0);
        let event = client.parse_event(&buf);
        
        assert!(event.shift);
        assert!(event.ctrl);
        assert!(event.alt);
    }

    #[test]
    fn test_parse_shift_only() {
        let client = GpmClient::new();
        let buf = make_event(0, 1, 10, 10, 1, 0);
        let event = client.parse_event(&buf);
        
        assert!(event.shift);
        assert!(!event.ctrl);
        assert!(!event.alt);
    }

    #[test]
    fn test_parse_double_click() {
        let client = GpmClient::new();
        let buf = make_event(GPM_B_LEFT, 0, 10, 10, GPM_DOWN | GPM_DOUBLE, 0);
        let event = client.parse_event(&buf);
        
        assert_eq!(event.click_count, 2);
        assert!(matches!(event.kind, MouseEventKind::Down(MouseButton::Left)));
    }

    #[test]
    fn test_parse_triple_click() {
        let client = GpmClient::new();
        let buf = make_event(GPM_B_LEFT, 0, 10, 10, GPM_DOWN | GPM_TRIPLE, 0);
        let event = client.parse_event(&buf);
        
        assert_eq!(event.click_count, 3);
    }

    #[test]
    fn test_parse_coordinate_clamping() {
        let client = GpmClient::new();
        // Test with x=0, y=0 (edge case - GPM uses 1-based)
        let buf = make_event(0, 0, 0, 0, 1, 0);
        let event = client.parse_event(&buf);
        
        // Should clamp to 0, not underflow
        assert_eq!(event.column, 0);
        assert_eq!(event.row, 0);
    }

    #[test]
    fn test_parse_negative_coordinates() {
        let client = GpmClient::new();
        // Test with negative coordinates (shouldn't happen but be safe)
        let buf = make_event(0, 0, -5, -3, 1, 0);
        let event = client.parse_event(&buf);
        
        // Should clamp to 0
        assert_eq!(event.column, 0);
        assert_eq!(event.row, 0);
    }

    #[test]
    fn test_should_use_gpm_with_display() {
        // This test checks the logic, actual env vars may vary
        // When DISPLAY is set, should_use_gpm returns false
        // We can't easily test this without modifying env, so just verify it compiles
        let _ = GpmClient::should_use_gpm();
    }

    #[test]
    fn test_new_client_not_connected() {
        let client = GpmClient::new();
        assert!(!client.is_connected());
        assert_eq!(client.last_x, 0);
        assert_eq!(client.last_y, 0);
    }
}
