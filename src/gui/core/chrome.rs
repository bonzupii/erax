//! GUI chrome: style presets, layer predicates, and knob resolution.
//!
//! The presentation core's chrome system (glassmorphism panels over the
//! screen model). `src/config/chrome.rs` holds the tunable consts (data
//! only); this module holds the machinery: the `ChromeStyle` preset enum
//! with its layer mappings, the panel-role predicates shared by the Rust
//! side and (by pinned constant) the WGSL composite pass, and the pure
//! knob-resolution function the renderer feeds to the shader.

use crate::terminal::display::CellRole;

/// A chrome style preset: a named combination of shader layers.
///
/// The config's layer consts default to the active style's mapping
/// (each is `CHROME_STYLE.<layer>()`), and the renderer reads only the
/// layer consts — the style is the preset, the layers are the selection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChromeStyle {
    /// Opaque panels with the legacy luminance lift and run edge.
    /// Reproduces the pre-chrome panel look through the new pipeline;
    /// the selection edge treatment is not carried over (it was a
    /// per-cell text decoration, not chrome).
    Solid,
    /// Frosted glass: blurred panel fills, specular edges, shadows.
    Frosted,
    /// Frosted glass without the blur pass (tint only): the cheap
    /// approximation for weak GPUs.
    Acrylic,
}

impl ChromeStyle {
    /// Frosted panel backgrounds (blur kernel + theme tint).
    pub const fn glass(self) -> bool {
        match self {
            ChromeStyle::Solid => false,
            ChromeStyle::Frosted | ChromeStyle::Acrylic => true,
        }
    }

    /// Backdrop blur inside the glass treatment. Acrylic keeps the tint
    /// but skips the kernel.
    pub const fn blur(self) -> bool {
        match self {
            ChromeStyle::Frosted => true,
            ChromeStyle::Solid | ChromeStyle::Acrylic => false,
        }
    }

    /// Specular edge highlights on panel boundaries. The glass styles
    /// get the always-add highlight; Solid keeps the legacy signed run
    /// edge instead (via `solid_panels`), never both.
    pub const fn specular(self) -> bool {
        match self {
            ChromeStyle::Solid => false,
            ChromeStyle::Frosted | ChromeStyle::Acrylic => true,
        }
    }

    /// Drop shadows under floating panels.
    pub const fn shadow(self) -> bool {
        match self {
            ChromeStyle::Solid => false,
            ChromeStyle::Frosted | ChromeStyle::Acrylic => true,
        }
    }

    /// Legacy opaque panel lift and run edge (the pre-chrome look).
    pub const fn solid_panels(self) -> bool {
        match self {
            ChromeStyle::Solid => true,
            ChromeStyle::Frosted | ChromeStyle::Acrylic => false,
        }
    }
}

/// Whether a cell role is GUI chrome: menu bar, dropdown items, prompt,
/// message line, status bar. The composite pass treats these roles; the
/// WGSL copy of this predicate is pinned to it by
/// `tests/gui_chrome.rs`.
pub const fn is_panel_role(role: CellRole) -> bool {
    matches!(
        role,
        CellRole::MenuBar
            | CellRole::MenuItem
            | CellRole::Prompt
            | CellRole::MessageLine
            | CellRole::StatusBar
    )
}

/// Whether a panel role floats over content (as opposed to being docked
/// to a frame edge). Only floating panels cast drop shadows. Dropdown
/// items are the only floating chrome; the bar, prompt, message line,
/// and status line are docked.
pub const fn is_floating_panel_role(role: CellRole) -> bool {
    matches!(role, CellRole::MenuItem)
}
