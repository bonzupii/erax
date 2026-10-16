//! Scrollbar presentation and behavior configuration.
//!
//! Typed compile-time constants choosing how scrollbars paint and
//! behave. Scrollbars own a dedicated strip outside the text rect
//! (the placement law: a bar never paints inside the viewport); these
//! constants choose the strip's contents only, never its position.
//! The frame model in [`crate::core::geometry`] consumes them.
//!
//! Character candidate survey for the glyph constants below.
//! Copy-pasteable families, all single-cell wide in monospace fonts:
//! full blocks (█ ▉ ▊ ▋ ▌ ▍ ▎ ▏ ▐), shade ramps (░ ▒ ▓), halves
//! (▀ ▄ ▌ ▐), quadrants (▘ ▙ ▚ ▛ ▜ ▝ ▞ ▟), box drawing
//! (─ ━ │ ┃ ┄ ┅ ┆ ┇ ┈ ┉ ┊ ┋), braille dots (⠁ ⠃ ⠉ ⠒ ⠤ ⠦ ⠧),
//! arrows (▲ ▼ ◀ ▶ △ ▽ ◁ ▷ ↑ ↓ ← →). A track glyph reads as
//! background; a thumb glyph must contrast it. The presets below
//! combine surveyed families into coherent sets.

/// Which frame edge carries the vertical scrollbar strip.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerticalSide {
    /// Strip is the pane's first column; the text rect starts after it.
    Left,
    /// Strip is the pane's last column; the text rect ends before it.
    Right,
}

/// Frame edge carrying the vertical scrollbar strip.
pub const VERTICAL_SIDE: VerticalSide = VerticalSide::Right;

/// Paint arrow heads at the ends of each scrollbar track.
/// When false the track spans the whole strip.
pub const SHOW_ARROWS: bool = true;

/// Minimum thumb length in cells. The frame model clamps the
/// proportional thumb to at least this many cells.
pub const THUMB_MIN_CELLS: usize = 2;

/// Paint a scrollbar even when all content fits without scrolling.
/// When false the strip stays reserved but unpainted, so showing or
/// hiding a bar never moves the text rect.
pub const SHOW_WHEN_FITS: bool = false;

/// Glyphs painted on the scrollbar strips.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScrollbarGlyphs {
    /// Vertical track cell.
    pub track_v: char,
    /// Horizontal track cell.
    pub track_h: char,
    /// Thumb cell (both orientations).
    pub thumb: char,
    /// Arrow heads; painted only when [`SHOW_ARROWS`] is true.
    pub arrow_up: char,
    pub arrow_down: char,
    pub arrow_left: char,
    pub arrow_right: char,
}

impl ScrollbarGlyphs {
    /// Heavy set: the historical look. Box-drawing track, full-block
    /// thumb, triangular arrows.
    pub const HEAVY: Self = Self {
        track_v: '┃',
        track_h: '━',
        thumb: '█',
        arrow_up: '▲',
        arrow_down: '▼',
        arrow_left: '◀',
        arrow_right: '▶',
    };

    /// Light set: thin box-drawing track, shaded thumb, hollow arrows.
    pub const LIGHT: Self = Self {
        track_v: '│',
        track_h: '─',
        thumb: '▓',
        arrow_up: '△',
        arrow_down: '▽',
        arrow_left: '◁',
        arrow_right: '▷',
    };

    /// Shade set: the track is a shade ramp cell, the thumb a full
    /// block; reads well on low-contrast themes.
    pub const SHADE: Self = Self {
        track_v: '░',
        track_h: '░',
        thumb: '█',
        arrow_up: '▲',
        arrow_down: '▼',
        arrow_left: '◀',
        arrow_right: '▶',
    };

    /// Minimal set: no visible track, block thumb, plain arrow glyphs.
    pub const MINIMAL: Self = Self {
        track_v: ' ',
        track_h: ' ',
        thumb: '█',
        arrow_up: '^',
        arrow_down: 'v',
        arrow_left: '<',
        arrow_right: '>',
    };

    /// Braille set: the thumb is a braille dot pattern, the track a
    /// single dot; compact on narrow strips.
    pub const BRAILLE: Self = Self {
        track_v: '⠁',
        track_h: '⠁',
        thumb: '⣿',
        arrow_up: '⠉',
        arrow_down: '⠒',
        arrow_left: '⠤',
        arrow_right: '⠦',
    };
}

/// Active scrollbar glyph set.
pub const GLYPHS: ScrollbarGlyphs = ScrollbarGlyphs::HEAVY;
