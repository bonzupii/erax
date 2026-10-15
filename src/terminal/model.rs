//! Pure screen-model types: `Cell`, `CellRole`, `ScreenBuffer`.
//!
//! These are the backend's screen model — the single representation of
//! editor state that the TUI renders, the GUI presentation core consumes,
//! and the WebUI protocol serializes. They are pure (no crossterm, no
//! EditorApp, no platform code) so the wasm web shell can use them
//! directly: no second representation.
//!
//! (Moved from `display.rs`; `display.rs` re-exports them.)

pub use crate::terminal::color::Color;

/// Maximum UTF-8 bytes stored inline for one cell's grapheme cluster: a
/// base character plus its combining marks. Tamil/Devanagari/Thai/Arabic
/// clusters and the canonical ZWJ emoji sequences fit; a longer cluster
/// keeps only its base character — always a complete grapheme, never a
/// split UTF-8 sequence or a dangling joiner.
pub const CLUSTER_TEXT_MAX: usize = 32;

/// The grapheme cluster occupying one screen cell, stored inline so `Cell`
/// stays `Copy` (the frame diff copies cells; nothing allocates on the
/// render path).
///
/// A cell holds one cluster, not one `char`: Indic vowel signs, viramas,
/// and other combining marks ride on their base character and the terminal
/// shapes the whole cluster into the cell's columns. Emitting a mark as its
/// own cell disagrees with the width model by one cell per mark and renders
/// wrong on every terminal.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ClusterText {
    bytes: [u8; CLUSTER_TEXT_MAX],
    len: u8,
}

impl ClusterText {
    /// Store one grapheme cluster. A cluster longer than
    /// [`CLUSTER_TEXT_MAX`] bytes keeps only its base character, so the
    /// cell always holds a complete grapheme.
    pub fn new(cluster: &str) -> Self {
        let mut bytes = [0u8; CLUSTER_TEXT_MAX];
        let first = cluster.chars().next();
        let mut buf = [0u8; 4];
        // Whole cluster fits, or just the base character.
        let text = match first {
            Some(c) if cluster.len() > CLUSTER_TEXT_MAX => c.encode_utf8(&mut buf),
            _ => cluster,
        };
        let len = text.len();
        bytes[..len].copy_from_slice(text.as_bytes());
        Self {
            bytes,
            len: len as u8,
        }
    }

    /// The stored cluster text.
    pub fn as_str(&self) -> &str {
        // The only writer is `new`, which encodes whole chars, so the
        // bytes are valid UTF-8 by construction; a failure here would be
        // a programming error, not a runtime condition.
        std::str::from_utf8(&self.bytes[..self.len as usize])
            .expect("ClusterText holds valid UTF-8 by construction")
    }

    /// Whether this cell carries no text.
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
}

impl serde::Serialize for ClusterText {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.as_str())
    }
}

impl<'de> serde::Deserialize<'de> for ClusterText {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let text = <&str>::deserialize(d)?;
        Ok(Self::new(text))
    }
}

/// Represents a single cell on the terminal screen
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Cell {
    pub text: ClusterText,
    pub fg: Color,
    pub bg: Color,
    /// If true, this cell is covered by the previous wide character and should be skipped
    pub hidden: bool,
    /// Semantic role of the UI element that produced this cell (N-273).
    pub role: CellRole,
}

/// Typed semantic metadata for a screen cell (N-273).
///
/// The GUI (and the WebUI child) must never infer what a cell *means* from its
/// glyph, color, or coordinates. Renderers stamp every cell with the semantic
/// role of the UI element that produced it; presentation layers consume the role
/// for effects (shaders, panels, hover) without changing behavior. The TUI
/// remains the behavioral source of truth.
///
/// `repr(u8)` so the role packs to a single byte for shader/wire use.
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, serde::Serialize, serde::Deserialize)]
pub enum CellRole {
    /// Editor text area (plain document text, continuation cells).
    #[default]
    EditorText,
    /// Line-number gutter, tilde filler, other window chrome on the left.
    Gutter,
    /// Window status line (filename, position, mode indicators).
    StatusBar,
    /// Menu bar (top-level titles and bar background).
    MenuBar,
    /// Open menu dropdown item.
    MenuItem,
    /// Minibuffer prompt input area.
    Prompt,
    /// Message / echo area line.
    MessageLine,
    /// Cell under the text cursor.
    Cursor,
    /// Cell inside the active selection.
    Selection,
    /// Scrollbar track or thumb.
    Scrollbar,
    /// Diagnostic marker (error/warning gutter icon, etc.).
    Diagnostic,
    /// Window separator / border cell.
    Separator,
}

impl Cell {
    pub fn new(ch: char, fg: Color, bg: Color) -> Self {
        let mut buf = [0u8; 4];
        let text = ClusterText::new(ch.encode_utf8(&mut buf));
        Self {
            text,
            fg,
            bg,
            hidden: false,
            role: CellRole::EditorText,
        }
    }

    /// Construct a cell with an explicit semantic role (N-273).
    pub fn with_role(ch: char, fg: Color, bg: Color, role: CellRole) -> Self {
        let mut buf = [0u8; 4];
        let text = ClusterText::new(ch.encode_utf8(&mut buf));
        Self {
            text,
            fg,
            bg,
            hidden: false,
            role,
        }
    }

    /// Construct a cell holding a full grapheme cluster (combining marks
    /// included); the layout source for complex-script text.
    pub fn with_text(cluster: &str, fg: Color, bg: Color, role: CellRole) -> Self {
        Self {
            text: ClusterText::new(cluster),
            fg,
            bg,
            hidden: false,
            role,
        }
    }

    /// The cluster text occupying this cell.
    pub fn text(&self) -> &str {
        self.text.as_str()
    }

    pub fn hidden() -> Self {
        Self {
            text: ClusterText::new(" "),
            fg: Color::Reset,
            bg: Color::Reset,
            hidden: true,
            role: CellRole::EditorText,
        }
    }

    pub fn empty() -> Self {
        Self {
            text: ClusterText::new(" "),
            fg: Color::Reset,
            bg: Color::Reset,
            hidden: false,
            role: CellRole::EditorText,
        }
    }
}

/// Represents the state of the entire screen
#[derive(Clone, Debug)]
pub struct ScreenBuffer {
    pub cells: Vec<Cell>,

    pub width: u16,

    pub height: u16,
}

impl ScreenBuffer {
    pub fn new(width: u16, height: u16) -> Self {
        let cells = vec![Cell::empty(); (width as usize) * (height as usize)];
        Self {
            cells,
            width,
            height,
        }
    }

    pub fn resize(&mut self, width: u16, height: u16) {
        self.width = width;
        self.height = height;
        self.cells = vec![Cell::empty(); (width as usize) * (height as usize)];
    }

    pub fn clear(&mut self) {
        for cell in &mut self.cells {
            *cell = Cell::empty();
        }
    }

    pub fn set(&mut self, x: u16, y: u16, cell: Cell) {
        if x < self.width && y < self.height {
            let idx = (y as usize) * (self.width as usize) + (x as usize);
            self.cells[idx] = cell;
        }
    }

    pub fn get(&self, x: u16, y: u16) -> Option<&Cell> {
        if x < self.width && y < self.height {
            let idx = (y as usize) * (self.width as usize) + (x as usize);
            Some(&self.cells[idx])
        } else {
            None
        }
    }
}

#[cfg(test)]
mod cluster_tests {
    use super::*;

    #[test]
    fn cluster_text_holds_a_tamil_cluster_whole() {
        // "மி": base + vowel sign, 6 bytes.
        let t = ClusterText::new("மி");
        assert_eq!(t.as_str(), "மி");
        assert!(!t.is_empty());
    }

    #[test]
    fn overlong_cluster_keeps_only_its_base_character() {
        // Family emoji: 25 bytes, fits in CLUSTER_TEXT_MAX; a longer
        // synthetic cluster must degrade to its base char, never a
        // dangling joiner or split UTF-8.
        let family = "👨‍👩‍👧‍👦";
        assert!(family.len() <= CLUSTER_TEXT_MAX);
        assert_eq!(ClusterText::new(family).as_str(), family);
        let long = format!("{family}{family}");
        assert!(long.len() > CLUSTER_TEXT_MAX);
        assert_eq!(ClusterText::new(&long).as_str(), "👨");
    }

    #[test]
    fn cluster_text_round_trips_through_json() {
        let cell = Cell::with_text("மி", Color::White, Color::Reset, CellRole::EditorText);
        let json = serde_json::to_string(&cell).expect("serializes");
        assert!(json.contains("மி"), "cluster stored as text, not bytes: {json}");
        let back: Cell = serde_json::from_str(&json).expect("deserializes");
        assert_eq!(back, cell);
    }
}
