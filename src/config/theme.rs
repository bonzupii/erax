//! Theme and color settings: typed `pub const`s.
//!
//! The theme is a [`ThemeId`], the truecolor mode a [`TrueColorMode`]; an
//! invalid selection is a compile error, never a runtime fallback.

use crate::terminal::color::TrueColorMode;
use crate::terminal::theme::ThemeId;

/// Color theme.
pub const THEME: ThemeId = ThemeId::GithubDark;

/// Whether to emit 24-bit color.
pub const TRUECOLOR: TrueColorMode = TrueColorMode::Auto;
