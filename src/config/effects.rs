//! GUI shader effect configuration.
//!
//! Typed compile-time constants choosing which presentation treatments the
//! background pass applies. Effects read the screen model's typed cell roles
//! as shader inputs; they never alter input routing, layout, commands, or
//! text content. Every treatment is a small luminance step of a cell's own
//! theme background, so effects stay inside the theme palette on dark and
//! light themes alike: the CPU signs the shifts from the theme's darkness
//! and the shader only adds.
//!
//! With [`EFFECTS_ENABLED`] false every effect uniform resolves to 0.0 and
//! the background pass reproduces the theme's cell colors exactly.

/// Master switch for all shader effects. When false the background pass
/// paints each cell's theme background unmodified.
pub const EFFECTS_ENABLED: bool = true;

/// Lift panel roles (menu bar, dropdown items, prompt, message line,
/// status bar) off the editor surface with a uniform luminance step.
pub const PANEL_DEPTH: bool = true;

/// Magnitude of the panel luminance step. Signed on the CPU: added on dark
/// themes, subtracted on light ones.
pub const PANEL_SHIFT: f32 = 0.035;

/// Draw a stronger luminance edge on the first and last rows of each panel
/// run, separating the panel from neighboring content.
pub const PANEL_EDGE: bool = true;

/// Magnitude of the panel edge step, signed like [`PANEL_SHIFT`].
pub const PANEL_EDGE_DELTA: f32 = 0.075;

/// Draw a top edge on the first row of each selection run, giving the
/// selection a defined upper boundary beyond its background tint.
pub const SELECTION_EDGE: bool = true;

/// Magnitude of the selection edge step, signed like [`PANEL_SHIFT`].
pub const SELECTION_EDGE_DELTA: f32 = 0.06;

/// Blend the cells neighboring the cursor toward the cursor cell's
/// background: a one-cell halo around the cursor block.
pub const CURSOR_HALO: bool = true;

/// Halo blend factor: 0.0 is no halo, 1.0 paints neighbors fully in the
/// cursor background.
pub const CURSOR_HALO_ALPHA: f32 = 0.18;

// Compile-time sanity bounds: a treatment stronger than a subtle lift is a
// config typo, not a look. Invalid values fail the build, never the run.
const _: () = {
    assert!(PANEL_SHIFT > 0.0 && PANEL_SHIFT <= 0.2);
    assert!(PANEL_EDGE_DELTA > 0.0 && PANEL_EDGE_DELTA <= 0.2);
    assert!(SELECTION_EDGE_DELTA > 0.0 && SELECTION_EDGE_DELTA <= 0.2);
    assert!(CURSOR_HALO_ALPHA >= 0.0 && CURSOR_HALO_ALPHA <= 1.0);
};
