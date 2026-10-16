//! GUI chrome configuration.
//!
//! Typed compile-time constants choosing the presentation core's chrome:
//! glassmorphism panels (translucent fills, backdrop blur, specular
//! edges, shadows) over the screen model. Chrome reads the screen
//! model's typed cell roles as shader inputs; it never alters input
//! routing, layout, commands, cursor positioning, or text content.
//!
//! Data only: the style preset and the layer/parameter consts a user
//! tunes. The machinery (`ChromeStyle`, role predicates, knob
//! resolution) lives in `crate::gui::core::chrome`.
//!
//! With [`CHROME_ENABLED`] false every chrome uniform resolves to 0.0
//! and the composite pass reproduces the cell colors exactly: the
//! regression baseline, byte-for-byte the no-effects rendering.

use crate::gui::core::chrome::ChromeStyle;

/// Master switch for the chrome system. When false the composite pass
/// is the identity over the cell colors.
pub const CHROME_ENABLED: bool = true;

/// The chrome style preset. Each layer const below defaults to this
/// style's mapping; edit a layer const to diverge from the preset.
pub const CHROME_STYLE: ChromeStyle = ChromeStyle::Frosted;

/// Frosted panel backgrounds: the cell-background grid blurred under
/// panel roles, tinted toward the theme background.
pub const CHROME_GLASS: bool = CHROME_STYLE.glass();

/// Backdrop blur inside the glass treatment. Off, the glass keeps its
/// tint but skips the blur kernel (the Acrylic look).
pub const CHROME_BLUR: bool = CHROME_STYLE.blur();

/// Specular edge highlights on panel boundaries. The glass styles use
/// the always-add highlight; Solid keeps the legacy signed run edge
/// instead (via [`CHROME_SOLID_PANELS`]), never both.
pub const CHROME_SPECULAR: bool = CHROME_STYLE.specular();

/// Drop shadows under floating panels (dropdowns).
pub const CHROME_SHADOW: bool = CHROME_STYLE.shadow();

/// Legacy opaque panel treatment: a uniform luminance lift on panel
/// roles plus a stronger edge on panel run boundaries. The Solid style;
/// the lift keeps the pre-chrome numbers, signed on the CPU from the
/// theme's darkness.
pub const CHROME_SOLID_PANELS: bool = CHROME_STYLE.solid_panels();

/// Blur kernel radius in pixels for the glass treatment.
pub const CHROME_BLUR_RADIUS_PX: f32 = 12.0;

/// Glass tint: how far the blurred panel fill mixes toward the theme
/// background. 0.0 is pure blurred scene, 1.0 is the theme background.
pub const CHROME_GLASS_TINT: f32 = 0.35;

/// Specular edge highlight strength, added (never signed) on panel
/// boundary pixels.
pub const CHROME_SPECULAR_STRENGTH: f32 = 0.12;

/// Drop shadow strength: 0.0 is no shadow, 1.0 is full black.
pub const CHROME_SHADOW_STRENGTH: f32 = 0.35;

/// Drop shadow reach in pixels below/right of a floating panel's edge.
pub const CHROME_SHADOW_PX: f32 = 10.0;

/// Magnitude of the Solid panel luminance step. Signed on the CPU:
/// added on dark themes, subtracted on light ones.
pub const CHROME_PANEL_SHIFT: f32 = 0.035;

/// Magnitude of the Solid panel run-edge step, signed like
/// [`CHROME_PANEL_SHIFT`].
pub const CHROME_PANEL_EDGE_DELTA: f32 = 0.075;

// Compile-time sanity bounds: a treatment stronger than a subtle finish
// is a config typo, not a look. Invalid values fail the build, never
// the run.
const _: () = {
    assert!(CHROME_BLUR_RADIUS_PX >= 0.0 && CHROME_BLUR_RADIUS_PX <= 32.0);
    assert!(CHROME_GLASS_TINT >= 0.0 && CHROME_GLASS_TINT <= 1.0);
    assert!(CHROME_SPECULAR_STRENGTH >= 0.0 && CHROME_SPECULAR_STRENGTH <= 0.5);
    assert!(CHROME_SHADOW_STRENGTH >= 0.0 && CHROME_SHADOW_STRENGTH <= 1.0);
    assert!(CHROME_SHADOW_PX >= 0.0 && CHROME_SHADOW_PX <= 32.0);
    assert!(CHROME_PANEL_SHIFT > 0.0 && CHROME_PANEL_SHIFT <= 0.2);
    assert!(CHROME_PANEL_EDGE_DELTA > 0.0 && CHROME_PANEL_EDGE_DELTA <= 0.2);
};
