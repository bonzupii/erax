//! Config constants (wasm): the SAME source files as the native build.
//!
//! The font constants are `src/config/font.rs` included via `#[path]` —
//! there is exactly one definition of `FONT_STACK` / `FONT_SIZE_PX` in the
//! tree, so the web shell cannot drift from the native config.

#[path = "../../../src/config/font.rs"]
pub mod font;

/// Leaf cursor-shape enum (same source as native): the wire `CursorState`
/// and the presentation core name it without the whole display config.
#[path = "../../../src/config/cursor_style.rs"]
pub mod cursor_style;
