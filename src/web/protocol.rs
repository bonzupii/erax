//! Typed, versioned wire protocol for the WebUI server.
//!
//! Design (owner's, non-negotiable):
//! - Frames mirror the internal screen-model types — `Cell`, `CellRole`,
//!   `KeyPress`, `MouseEvent` are serialized directly (serde). There is no
//!   second representation of editor state.
//! - Dirty regions + semantic metadata go out (server → client); raw input
//!   comes in (client → server).
//! - The protocol is versioned: the server sends `Hello { version }` first;
//!   a client that doesn't speak the version must disconnect.
//! - Transport is WebSocket; each frame is one JSON text message. (JSON for
//!   debuggability; the frame types are the contract, not the encoding.)
//!
//! The GUI stays a dumb passthrough: input events are the backend's raw
//! types (`KeyPress`, `MouseEvent`), and the screen model is the backend's
//! `ScreenBuffer`. All input models and mode-aware UI live in the backend.

use crate::config::cursor_style::CursorStyle;
use crate::core::input::{KeyPress, MouseEvent};
use crate::terminal::display::{Cell, CellRole};
use serde::{Deserialize, Serialize};

/// Current protocol version. Bumped on any breaking frame change.
///
/// v2 (2026-10-15): `CursorState` gained `style` (the backend's cursor
/// shape for the current input model/mode — block/bar/underline/hollow).
/// The client must draw the cursor with this shape; it must not infer
/// shape from anything else.
pub const PROTOCOL_VERSION: u32 = 2;

/// A rectangle of cells, in cell coordinates (inclusive origin, exclusive end).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rect {
    pub x: u16,
    pub y: u16,
    pub w: u16,
    pub h: u16,
}

/// One cell update: position + the cell itself (the screen-model type).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CellUpdate {
    pub x: u16,
    pub y: u16,
    pub cell: Cell,
}

/// Cursor state for the client to render (from the backend screen model).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CursorState {
    pub x: u16,
    pub y: u16,
    /// Whether the cursor is visible (e.g. hidden in some modes).
    pub visible: bool,
    /// The semantic role under the cursor (N-273 metadata for shaders).
    pub role: CellRole,
    /// The cursor shape for the current input model/mode (block, bar,
    /// underline, hollow), decided by the backend. The client presents
    /// this shape verbatim — it never infers shape from mode or role.
    pub style: CursorStyle,
}

/// Frames sent by the server.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum ServerFrame {
    /// First frame on connect: protocol version + current screen size.
    /// The client MUST check `version == PROTOCOL_VERSION`.
    Hello { version: u32, width: u16, height: u16 },
    /// Authentication succeeded; screen streaming begins.
    AuthOk,
    /// Authentication failed; the server closes the connection after this.
    AuthError { reason: String },
    /// Screen update: sequence number, dirty regions, and the cells within
    /// them (full `Cell` values — text, colors, N-273 role). The client
    /// applies them to its local screen model and re-renders.
    Screen {
        seq: u64,
        dirty: Vec<Rect>,
        cells: Vec<CellUpdate>,
        cursor: CursorState,
    },
    /// The server is shutting down.
    Bye { reason: String },
}

/// Raw input events sent by the client (the backend's input types).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum InputEvent {
    Key(KeyPress),
    Mouse(MouseEvent),
    /// IME composition text (from browser `compositionupdate` / `input`
    /// events); the backend inserts it as text.
    ImeCommit { text: String },
}

/// Frames sent by the client.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum ClientFrame {
    /// Must be the first frame. The token is the per-session random token
    /// printed by the server at startup. No token / wrong token → the
    /// server replies `AuthError` and closes the connection. There is no
    /// unauthenticated mode.
    Auth { token: String },
    /// A raw input event for the backend.
    Input { event: InputEvent },
    /// The client's viewport changed (browser resize / zoom).
    Resize { width: u16, height: u16 },
}

/// Encode a frame to its wire representation (one JSON text message).
pub fn encode<T: Serialize>(frame: &T) -> String {
    serde_json::to_string(frame).expect("protocol frame serialization cannot fail")
}

/// Decode a wire message to a frame.
pub fn decode<T: for<'de> Deserialize<'de>>(msg: &str) -> Result<T, serde_json::Error> {
    serde_json::from_str(msg)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::input::{Key, MouseButton, MouseEventKind};
    use crate::terminal::color::Color;

    fn sample_cell() -> Cell {
        Cell::with_role('λ', Color::Rgb { r: 1, g: 2, b: 3 }, Color::Reset, CellRole::EditorText)
    }

    #[test]
    fn protocol_version_is_current() {
        assert_eq!(PROTOCOL_VERSION, 2);
    }

    #[test]
    fn server_frame_round_trip() {
        let frame = ServerFrame::Screen {
            seq: 42,
            dirty: vec![Rect { x: 0, y: 0, w: 80, h: 24 }],
            cells: vec![CellUpdate {
                x: 1,
                y: 2,
                cell: sample_cell(),
            }],
            cursor: CursorState {
                x: 5,
                y: 5,
                visible: true,
                role: CellRole::Cursor,
                style: CursorStyle::SteadyBar,
            },
        };
        let wire = encode(&frame);
        let back: ServerFrame = decode(&wire).expect("decode failed");
        assert_eq!(frame, back);
    }

    #[test]
    fn client_frame_round_trip() {
        let frames = vec![
            ClientFrame::Auth {
                token: "secret-token".to_string(),
            },
            ClientFrame::Input {
                event: InputEvent::Key(KeyPress {
                    key: Key::Char('x'),
                    shift: false,
                    alt: false,
                    ctrl: true,
                }),
            },
            ClientFrame::Input {
                event: InputEvent::Mouse(MouseEvent {
                    column: 10,
                    row: 20,
                    kind: MouseEventKind::Down(MouseButton::Left),
                    shift: false,
                    alt: false,
                    ctrl: false,
                    click_count: 1,
                    scroll_amount: 0,
                }),
            },
            ClientFrame::Input {
                event: InputEvent::ImeCommit {
                    text: "日本語".to_string(),
                },
            },
            ClientFrame::Resize {
                width: 120,
                height: 40,
            },
        ];
        for frame in frames {
            let wire = encode(&frame);
            let back: ClientFrame = decode(&wire).expect("decode failed");
            assert_eq!(frame, back);
        }
    }

    #[test]
    fn cell_role_wire_stability() {
        // CellRole is repr(u8) for shader/wire use; the JSON encoding must
        // carry the variant name (not the discriminant) so the protocol is
        // self-describing.
        let wire = encode(&CellRole::StatusBar);
        assert!(wire.contains("StatusBar"), "wire: {wire}");
        let back: CellRole = decode(&wire).expect("decode failed");
        assert_eq!(back, CellRole::StatusBar);
    }

    // ------------------------------------------------------------------
    // Hostile-frame fuzz (session 8 hardening): the decode path must turn
    // hostile input into `Err`, never panic, never hang. Deterministic
    // xorshift PRNG over a seed corpus + byte mutations. Not run under
    // Miri (exec count); the round-trip tests above do run under Miri.
    // ------------------------------------------------------------------

    /// Deterministic xorshift64* PRNG (fixed seed → reproducible corpus).
    #[cfg(not(miri))]
    struct XorShift64 {
        state: u64,
    }

    #[cfg(not(miri))]
    impl XorShift64 {
        fn next(&mut self) -> u64 {
            let mut x = self.state;
            x ^= x >> 12;
            x ^= x << 25;
            x ^= x >> 27;
            self.state = x;
            x.wrapping_mul(0x2545F4914F6CDD1D)
        }

        fn below(&mut self, n: usize) -> usize {
            (self.next() % n.max(1) as u64) as usize
        }
    }

    /// Mutate `buf` in place: byte flips, truncation, splice, repeat.
    #[cfg(not(miri))]
    fn mutate(rng: &mut XorShift64, buf: &mut Vec<u8>, seeds: &[Vec<u8>]) {
        match rng.below(5) {
            // Random byte flips.
            0 => {
                let flips = 1 + rng.below(8);
                for _ in 0..flips {
                    if buf.is_empty() {
                        break;
                    }
                    let i = rng.below(buf.len());
                    buf[i] = rng.next() as u8;
                }
            }
            // Truncate at a random point (including to empty).
            1 => {
                let at = rng.below(buf.len() + 1);
                buf.truncate(at);
            }
            // Splice in a chunk of another seed.
            2 => {
                let other = &seeds[rng.below(seeds.len())];
                if !other.is_empty() {
                    let at = rng.below(buf.len() + 1);
                    let len = 1 + rng.below(other.len().min(64));
                    let start = rng.below(other.len());
                    let end = (start + len).min(other.len());
                    buf.splice(at..at, other[start..end].iter().copied());
                }
            }
            // Repeat a random chunk (nesting/depth stress).
            3 => {
                if !buf.is_empty() {
                    let at = rng.below(buf.len());
                    let len = 1 + rng.below((buf.len() - at).min(32));
                    let chunk: Vec<u8> = buf[at..at + len].to_vec();
                    for _ in 0..(1 + rng.below(4)) {
                        let pos = rng.below(buf.len() + 1);
                        buf.splice(pos..pos, chunk.iter().copied());
                    }
                }
            }
            // Insert random bytes (control chars, quotes, backslashes).
            _ => {
                let at = rng.below(buf.len() + 1);
                let n = 1 + rng.below(16);
                let junk: Vec<u8> = (0..n).map(|_| rng.next() as u8).collect();
                buf.splice(at..at, junk.iter().copied());
            }
        }
    }

    #[test]
    #[cfg(not(miri))]
    fn hostile_frame_fuzz_no_panic_no_hang() {
        // Number of decode executions per direction (client + server
        // frames). Recorded in PROGRESS.md with wall time.
        const EXECS_PER_DIRECTION: usize = 100_000;

        let mut seeds: Vec<Vec<u8>> = vec![
            b"".to_vec(),
            b"{".to_vec(),
            b"}".to_vec(),
            b"null".to_vec(),
            b"[]".to_vec(),
            b"\"\"".to_vec(),
            b"{\"t\":\"auth\"}".to_vec(),
            b"{\"t\":\"auth\",\"token\":null}".to_vec(),
            b"{\"t\":\"auth\",\"token\":12345}".to_vec(),
            b"{\"t\":\"auth\",\"token\":\"\"}".to_vec(),
            // 10k-deep nesting: serde_json's recursion limit (128) must
            // turn this into Err, not a stack overflow.
            {
                let mut deep = vec![b'['; 10_000];
                deep.extend(std::iter::repeat_n(b']', 10_000));
                deep
            },
            b"{\"t\":\"input\",\"event\":{\"t\":\"key\"}}".to_vec(),
            b"{\"t\":\"resize\",\"width\":-1,\"height\":999999999999999999999}".to_vec(),
            b"{\"t\":\"screen\",\"seq\":18446744073709551616}".to_vec(),
            "λ{\"t\":\"auth\",\"token\":\"tokén💥\"}".as_bytes().to_vec(),
        ];
        // Valid frames as seeds too (mutations of valid frames are the
        // most likely to tickle real decoder paths).
        for frame in [
            ClientFrame::Auth {
                token: String::new(),
            },
            ClientFrame::Resize {
                width: 120,
                height: 36,
            },
        ] {
            seeds.push(encode(&frame).into_bytes());
        }

        let mut rng = XorShift64 { state: 0x1234_5678_9ABC_DEF1 };
        let mut ok_count = 0usize;
        let mut err_count = 0usize;
        for i in 0..EXECS_PER_DIRECTION {
            let mut buf = seeds[i % seeds.len()].clone();
            // A few pure seeds go through unmutated (valid-frame sanity).
            if i >= seeds.len() {
                mutate(&mut rng, &mut buf, &seeds);
            }
            // The transport guarantees UTF-8 for text frames; lossy
            // conversion models the worst case the decoder can see.
            let input = String::from_utf8_lossy(&buf);
            // Must not panic. Decoding the same input twice must agree.
            let r1: Result<ClientFrame, _> = decode(&input);
            let r2: Result<ClientFrame, _> = decode(&input);
            assert_eq!(r1.is_ok(), r2.is_ok(), "nondeterministic decode");
            match r1 {
                Ok(_) => ok_count += 1,
                Err(_) => err_count += 1,
            }
            let s1: Result<ServerFrame, _> = decode(&input);
            assert_eq!(s1.is_ok(), decode::<ServerFrame>(&input).is_ok());
            match s1 {
                Ok(_) => ok_count += 1,
                Err(_) => err_count += 1,
            }
        }
        // Sanity: the campaign saw both outcomes (not a vacuous corpus).
        assert!(ok_count > 0, "no input decoded Ok — corpus is vacuous");
        assert!(err_count > 0, "no input decoded Err — corpus is vacuous");
        eprintln!(
            "hostile_frame_fuzz: {} execs ({} ok, {} err)",
            2 * EXECS_PER_DIRECTION,
            ok_count,
            err_count
        );
    }
}
