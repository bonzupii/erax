//! GUI font settings: typed `pub const`s.
//!
//! The font is a family-name stack, not a path (N-011): the old `font_path`
//! setting was documented as "path to a TrueType font file" but held the
//! family name `"JetBrains Mono"`. The setting now means what its type says —
//! a list of font families in fallback order (N-119).

/// Font families for GUI text, in fallback order.
///
/// The first family is the primary; later entries cover glyphs the primary
/// lacks (explicit stack per N-119, instead of relying on implicit fontdb
/// fallback the editor cannot configure). The tail entries are widely
/// available system monospaces so the GUI and the WebUI have a font even
/// where the primary is not installed.
pub const FONT_STACK: &[&str] = &[
    "JetBrains Mono",
    "Noto Sans Mono",
    "DejaVu Sans Mono",
    "Liberation Mono",
];

/// Font size in pixels. Used exactly as given (N-025: no magic `+ 1.0`).
pub const FONT_SIZE_PX: f32 = 16.0;
