//! GUI font settings: typed `pub const`s.
//!
//! The font is a family-name stack in fallback order, not a path: the
//! type is `&[&str]` and each entry names a font family.

/// Font families for GUI text, in fallback order.
///
/// The first family is the primary; later entries cover glyphs the primary
/// lacks (instead of relying on implicit fontdb fallback the editor cannot
/// configure). The tail entries are widely available system monospaces so
/// the GUI and the WebUI have a font even where the primary is not installed.
pub const FONT_STACK: &[&str] = &[
    "JetBrainsMono Nerd Font Mono",
    "Noto Sans Mono",
    // Emoji: after Noto Sans Mono (its ASCII digits resolve there, so the
    // color font's digit glyphs never hijack text) and before DejaVu Sans
    // Mono (its monochrome U+2764 would shadow the color heart).
    "Noto Color Emoji",
    "DejaVu Sans Mono",
    "Liberation Mono",
    // Braille: DejaVu Sans supplies U+2800..U+28FF (full 256-pattern
    // coverage, monospace advances). None of the mono fonts above cover
    // braille, so this changes no existing resolution; it pins the
    // braille provider instead of leaving it to system fallback.
    "DejaVu Sans",
    // CJK: last. Nothing earlier covers CJK, so placing it here changes
    // no existing resolution (box drawing and blocks still resolve to the
    // fonts above). The monospaced CJK variant preserves the cell model;
    // SC is the conventional default regional variant. No Nerd Fonts
    // patched CJK family exists upstream, so a Nerd Font name here would
    // be unverifiable and silently fall back.
    "Noto Sans Mono CJK SC",
];

/// Font size in pixels. Used exactly as given.
pub const FONT_SIZE_PX: f32 = 16.0;
