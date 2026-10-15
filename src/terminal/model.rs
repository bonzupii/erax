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

/// Represents a single cell on the terminal screen
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Cell {
    pub ch: char,
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
        Self {
            ch,
            fg,
            bg,
            hidden: false,
            role: CellRole::EditorText,
        }
    }

    /// Construct a cell with an explicit semantic role (N-273).
    pub fn with_role(ch: char, fg: Color, bg: Color, role: CellRole) -> Self {
        Self {
            ch,
            fg,
            bg,
            hidden: false,
            role,
        }
    }

    pub fn hidden() -> Self {
        Self {
            ch: ' ',
            fg: Color::Reset,
            bg: Color::Reset,
            hidden: true,
            role: CellRole::EditorText,
        }
    }

    pub fn empty() -> Self {
        Self {
            ch: ' ',
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
