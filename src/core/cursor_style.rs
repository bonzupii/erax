//! Terminal/GUI cursor shape, as a typed config value.
//!
//! A leaf module (no imports) so the wire protocol and the presentation
//! core (including the wasm web shell, which includes this file via
//! `#[path]`) can name the type without pulling in the display config.

/// Terminal/GUI cursor shape, as a typed config value.
///
/// `repr(u8)` with discriminants matching the DECSCUSR `Ps` parameter, so the
/// terminal emit path is `ESC [ Ps SP q` with `style as u8` — except
/// [`CursorStyle::HollowBlock`], which has no DECSCUSR equivalent and degrades
/// to a steady block on terminals (GUI shaders may render it hollow).
///
/// The style is backend-derived screen-model state: the display model
/// carries it to the terminal emit path and to the GUI/WebUI as
/// presentation metadata (presentation only; no input model reads it
/// back).
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum CursorStyle {
    /// `ESC [ 0 SP q`: restore the terminal's default cursor.
    #[default]
    Default = 0,
    /// `ESC [ 2 SP q`: steady block.
    SteadyBlock = 2,
    /// `ESC [ 4 SP q`: steady underline.
    SteadyUnderline = 4,
    /// `ESC [ 6 SP q`: steady vertical bar.
    SteadyBar = 6,
    /// No DECSCUSR equivalent; the terminal mapping degrades to
    /// [`CursorStyle::SteadyBlock`]. Kept distinct so GUI shaders can render
    /// the hollow variant the config asks for.
    HollowBlock = 7,
}

impl CursorStyle {
    /// The DECSCUSR `Ps` parameter to emit for this style on a terminal.
    pub const fn decscusr_param(self) -> u8 {
        match self {
            // No DECSCUSR hollow: degrade to the steady block.
            CursorStyle::HollowBlock => CursorStyle::SteadyBlock as u8,
            other => other as u8,
        }
    }

    /// The full `ESC [ Ps SP q` sequence as bytes (all params are one digit).
    pub const fn decscusr_bytes(self) -> [u8; 5] {
        [
            0x1b,
            b'[',
            b'0' + self.decscusr_param(),
            b' ',
            b'q',
        ]
    }
}
