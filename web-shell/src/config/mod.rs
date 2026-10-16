//! Config constants (wasm): the SAME source files as the native build.
//!
//! The font constants are `src/config/font.rs` included via `#[path]` —
//! there is exactly one definition of `FONT_STACK` / `FONT_SIZE_PX` in the
//! tree, so the web shell cannot drift from the native config.

#[path = "../../../src/config/font.rs"]
pub mod font;

/// Scrollbar constants (same source as native).
#[path = "../../../src/config/scrollbar.rs"]
pub mod scrollbar;

/// Theme selection (same source as native): the presentation core reads the
/// theme background for the clear color and out-of-grid fragments.
#[path = "../../../src/config/theme.rs"]
pub mod theme;

/// Shader effect constants (same source as native).
#[path = "../../../src/config/chrome.rs"]
pub mod chrome;
