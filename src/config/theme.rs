//! Theme and color settings: typed `pub const`s (N-014).
//!
//! The theme is a [`ThemeId`], the truecolor mode a [`TrueColorMode`]; an
//! invalid selection is a compile error, never a runtime fallback.

use crate::terminal::theme::ThemeId;

/// Color theme.
pub const THEME: ThemeId = ThemeId::GithubDark;

/// Terminal truecolor (24-bit) mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TrueColorMode {
    /// Detect from the environment.
    Auto,
    /// Always emit 24-bit color.
    Always,
    /// Never emit 24-bit color; use the 16-color fallback.
    Never,
}

/// Whether to emit 24-bit color.
pub const TRUECOLOR: TrueColorMode = TrueColorMode::Auto;
