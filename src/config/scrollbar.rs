//! Scrollbar presentation and behavior configuration.
//!
//! Typed compile-time constants choosing how scrollbars paint and
//! behave. Scrollbars own a dedicated strip outside the text rect
//! (a bar never paints inside the viewport); these constants choose
//! the strip's contents only, never its position. The frame model in
//! [`crate::core::geometry`] consumes them.
//!
//! Character candidate survey for the glyph constants below.
//! Copy-pasteable families, all single-cell wide in monospace fonts:
//! full blocks (█ ▉ ▊ ▋ ▌ ▍ ▎ ▏ ▐), shade ramps (░ ▒ ▓), halves
//! (▀ ▄ ▌ ▐), quadrants (▘ ▙ ▚ ▛ ▜ ▝ ▞ ▟), box drawing
//! (─ ━ │ ┃ ┄ ┅ ┆ ┇ ┈ ┉ ┊ ┋), braille dots (⠁ ⠃ ⠉ ⠒ ⠤ ⠦ ⠧),
//! arrows (▲ ▼ ◀ ▶ △ ▽ ◁ ▷ ↑ ↓ ← →). A track glyph reads as
//! background; a thumb glyph must contrast it. The survey informed the
//! glyph presets, which are [`ScrollbarGlyphs`] associated consts in
//! [`crate::core::geometry`]; [`GLYPHS`] selects the active set.

use crate::core::geometry::{ScrollbarGlyphs, VerticalSide};

/// Frame edge carrying the vertical scrollbar strip.
pub const VERTICAL_SIDE: VerticalSide = VerticalSide::Right;

/// Paint arrow heads at the ends of each scrollbar track.
/// When false the track spans the whole strip.
pub const SHOW_ARROWS: bool = true;

/// Minimum thumb length in cells. The frame model clamps the
/// proportional thumb to at least this many cells.
pub const THUMB_MIN_CELLS: usize = 2;

/// Paint a scrollbar even when all content fits without scrolling.
/// When false the strip exists only while the bar is present: the text
/// rect grows and shrinks as the bar appears and disappears.
pub const SHOW_WHEN_FITS: bool = false;

/// Active scrollbar glyph set.
pub const GLYPHS: ScrollbarGlyphs = ScrollbarGlyphs::HEAVY;
