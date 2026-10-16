//! Shader effect contracts, verified without a GPU.
//!
//! The background pass's presentation treatments are data-driven: the CPU
//! derives effect uniforms from the theme and the effects configuration,
//! and the WGSL consumes per-cell roles uploaded as `CellRole as u32`.
//! These tests pin the derivation (a pure function over explicit inputs —
//! no config values asserted) and the role-discriminant contract the WGSL
//! depends on.

use erax::gui::core::quad_renderer::{resolve_effect_uniforms, EffectKnobs};
use erax::terminal::display::CellRole;

fn knobs() -> EffectKnobs {
    EffectKnobs {
        enabled: true,
        panel: true,
        panel_shift: 0.035,
        panel_edge: true,
        panel_edge_delta: 0.075,
        selection_edge: true,
        selection_edge_delta: 0.06,
        cursor_halo: true,
        cursor_halo_alpha: 0.18,
    }
}

#[test]
fn disabled_effects_resolve_to_identity() {
    // The master switch zeroes every slot even with all treatments on:
    // the shader then reproduces the theme's cell colors exactly.
    let mut k = knobs();
    k.enabled = false;
    assert_eq!(resolve_effect_uniforms(k, true), [0.0; 4]);
    assert_eq!(resolve_effect_uniforms(k, false), [0.0; 4]);
}

#[test]
fn luminance_steps_sign_with_theme_darkness() {
    // Dark themes lift, light themes press; the shader only adds, so the
    // sign must be resolved on the CPU.
    let dark = resolve_effect_uniforms(knobs(), true);
    let light = resolve_effect_uniforms(knobs(), false);
    assert!(dark[0] > 0.0 && dark[1] > 0.0 && dark[2] > 0.0);
    assert!(light[0] < 0.0 && light[1] < 0.0 && light[2] < 0.0);
    assert_eq!(dark[0], -light[0]);
    // The halo is a blend factor, never signed.
    assert_eq!(dark[3], 0.18);
    assert_eq!(light[3], 0.18);
}

#[test]
fn each_effect_gates_its_own_slot() {
    let mut k = knobs();
    k.panel = false;
    let fx = resolve_effect_uniforms(k, true);
    assert_eq!(fx[0], 0.0);
    assert!(fx[1] > 0.0);

    let mut k = knobs();
    k.panel_edge = false;
    let fx = resolve_effect_uniforms(k, true);
    assert!(fx[0] > 0.0);
    assert_eq!(fx[1], 0.0);

    let mut k = knobs();
    k.selection_edge = false;
    assert_eq!(resolve_effect_uniforms(k, true)[2], 0.0);

    let mut k = knobs();
    k.cursor_halo = false;
    assert_eq!(resolve_effect_uniforms(k, true)[3], 0.0);
}

#[test]
fn cell_role_discriminants_are_stable() {
    // The WGSL background pass matches these discriminants by value;
    // reordering CellRole without updating the shader silently retargets
    // effects at the wrong cells.
    let roles = [
        CellRole::EditorText,
        CellRole::Gutter,
        CellRole::StatusBar,
        CellRole::MenuBar,
        CellRole::MenuItem,
        CellRole::Prompt,
        CellRole::MessageLine,
        CellRole::Cursor,
        CellRole::Selection,
        CellRole::Scrollbar,
        CellRole::Diagnostic,
        CellRole::Separator,
    ];
    for (i, role) in roles.iter().enumerate() {
        assert_eq!(*role as u32, i as u32, "CellRole::{role:?}");
    }
}

#[test]
fn wgsl_role_constants_match_cell_role() {
    // The shader's ROLE_* constants must name the same discriminants the
    // renderer uploads.
    let wgsl = include_str!("../src/gui/core/shaders/background.wgsl");
    let roles = [
        ("ROLE_EDITOR_TEXT", CellRole::EditorText),
        ("ROLE_GUTTER", CellRole::Gutter),
        ("ROLE_STATUS_BAR", CellRole::StatusBar),
        ("ROLE_MENU_BAR", CellRole::MenuBar),
        ("ROLE_MENU_ITEM", CellRole::MenuItem),
        ("ROLE_PROMPT", CellRole::Prompt),
        ("ROLE_MESSAGE_LINE", CellRole::MessageLine),
        ("ROLE_CURSOR", CellRole::Cursor),
        ("ROLE_SELECTION", CellRole::Selection),
        ("ROLE_SCROLLBAR", CellRole::Scrollbar),
        ("ROLE_DIAGNOSTIC", CellRole::Diagnostic),
        ("ROLE_SEPARATOR", CellRole::Separator),
    ];
    for (name, role) in roles {
        let line = format!("const {name}: u32 = {}u;", role as u32);
        assert!(
            wgsl.contains(&line),
            "background.wgsl is missing `{line}`"
        );
    }
}

#[test]
fn wgsl_has_no_hardcoded_background() {
    // The old clear color must not reappear as a literal anywhere
    // in the background shader; out-of-grid fragments use the bg uniform.
    let wgsl = include_str!("../src/gui/core/shaders/background.wgsl");
    assert!(!wgsl.contains("0.1, 0.1, 0.12"));
    assert!(wgsl.contains("return uniforms.bg;"));
}
