//! Chrome contracts, verified without a GPU.
//!
//! The composite pass's chrome treatments are data-driven: the CPU
//! derives chrome uniforms from the theme and the chrome configuration,
//! and the WGSL consumes per-cell roles uploaded as `CellRole as u32`.
//! These tests pin the derivation (a pure function over explicit inputs
//! — no config values asserted), the style-preset mappings, the role
//! predicates, and the role-discriminant contract the WGSL depends on.

use erax::gui::core::chrome::{is_floating_panel_role, is_panel_role, ChromeStyle};
use erax::gui::core::quad_renderer::{resolve_chrome_uniforms, ChromeKnobs};
use erax::terminal::display::CellRole;

fn knobs() -> ChromeKnobs {
    ChromeKnobs {
        enabled: true,
        glass: true,
        blur: true,
        blur_radius_px: 12.0,
        glass_tint: 0.35,
        specular: true,
        specular_strength: 0.12,
        shadow: true,
        shadow_strength: 0.35,
        shadow_px: 10.0,
        solid_panels: false,
        panel_shift: 0.035,
        panel_edge_delta: 0.075,
    }
}

#[test]
fn disabled_chrome_resolves_to_identity() {
    // The master switch zeroes every slot even with all layers on: the
    // composite then reproduces the cell colors exactly (the regression
    // baseline).
    let mut k = knobs();
    k.enabled = false;
    assert_eq!(resolve_chrome_uniforms(k, true), [0.0; 8]);
    assert_eq!(resolve_chrome_uniforms(k, false), [0.0; 8]);
}

#[test]
fn luminance_steps_sign_with_theme_darkness() {
    // Dark themes lift, light themes press; the shader only adds, so the
    // sign must be resolved on the CPU.
    let mut k = knobs();
    k.glass = false;
    k.specular = false;
    k.shadow = false;
    k.solid_panels = true;
    let dark = resolve_chrome_uniforms(k, true);
    let light = resolve_chrome_uniforms(k, false);
    assert!(dark[6] > 0.0 && dark[7] > 0.0);
    assert!(light[6] < 0.0 && light[7] < 0.0);
    assert_eq!(dark[6], -light[6]);
    assert_eq!(dark[7], -light[7]);
}

#[test]
fn glass_and_specular_and_shadow_are_never_signed() {
    // Only the Solid panel steps carry a theme sign; the glass, specular,
    // and shadow slots are magnitudes and tints, identical on both.
    let dark = resolve_chrome_uniforms(knobs(), true);
    let light = resolve_chrome_uniforms(knobs(), false);
    assert_eq!(dark[1], light[1]);
    assert_eq!(dark[2], light[2]);
    assert_eq!(dark[3], light[3]);
    assert_eq!(dark[4], light[4]);
    assert_eq!(dark[5], light[5]);
}

#[test]
fn each_layer_gates_its_own_slot() {
    // Slot order: [glass, blur_radius_px, glass_tint, specular,
    // shadow_strength, shadow_px, panel_shift, panel_edge].
    let mut k = knobs();
    k.glass = false;
    let fx = resolve_chrome_uniforms(k, true);
    assert_eq!(fx[0], 0.0);
    assert_eq!(fx[1], 0.0);
    assert_eq!(fx[2], 0.0);
    assert!(fx[3] > 0.0);

    let mut k = knobs();
    k.blur = false;
    let fx = resolve_chrome_uniforms(k, true);
    assert_eq!(fx[0], 1.0); // glass stays: tint without the kernel
    assert_eq!(fx[1], 0.0);

    let mut k = knobs();
    k.specular = false;
    assert_eq!(resolve_chrome_uniforms(k, true)[3], 0.0);

    let mut k = knobs();
    k.shadow = false;
    let fx = resolve_chrome_uniforms(k, true);
    assert_eq!(fx[4], 0.0);
    assert_eq!(fx[5], 0.0);

    let mut k = knobs();
    k.solid_panels = false;
    let fx = resolve_chrome_uniforms(k, true);
    assert_eq!(fx[6], 0.0);
    assert_eq!(fx[7], 0.0);
}

#[test]
fn style_presets_map_to_documented_layers() {
    // Each style maps to its documented layer set: the config layer
    // consts default to these, and the renderer reads the layers.
    assert!(!ChromeStyle::Solid.glass());
    assert!(!ChromeStyle::Solid.blur());
    assert!(!ChromeStyle::Solid.specular());
    assert!(!ChromeStyle::Solid.shadow());
    assert!(ChromeStyle::Solid.solid_panels());

    assert!(ChromeStyle::Frosted.glass());
    assert!(ChromeStyle::Frosted.blur());
    assert!(ChromeStyle::Frosted.specular());
    assert!(ChromeStyle::Frosted.shadow());
    assert!(!ChromeStyle::Frosted.solid_panels());

    assert!(ChromeStyle::Acrylic.glass());
    assert!(!ChromeStyle::Acrylic.blur()); // tint only: the cheap frosted look
    assert!(ChromeStyle::Acrylic.specular());
    assert!(ChromeStyle::Acrylic.shadow());
    assert!(!ChromeStyle::Acrylic.solid_panels());
}

#[test]
fn panel_role_predicates() {
    // The chrome surfaces: menu bar, dropdown items, prompt, message
    // line, status bar. Only dropdown items float (and cast shadows).
    for role in [
        CellRole::MenuBar,
        CellRole::MenuItem,
        CellRole::Prompt,
        CellRole::MessageLine,
        CellRole::StatusBar,
    ] {
        assert!(is_panel_role(role), "{role:?} is chrome");
    }
    for role in [
        CellRole::EditorText,
        CellRole::Gutter,
        CellRole::Cursor,
        CellRole::Selection,
        CellRole::Scrollbar,
        CellRole::Diagnostic,
        CellRole::Separator,
    ] {
        assert!(!is_panel_role(role), "{role:?} is not chrome");
    }
    assert!(is_floating_panel_role(CellRole::MenuItem));
    assert!(!is_floating_panel_role(CellRole::MenuBar));
    assert!(!is_floating_panel_role(CellRole::StatusBar));
    assert!(!is_floating_panel_role(CellRole::EditorText));
}

#[test]
fn cell_role_discriminants_are_stable() {
    // The WGSL composite pass matches these discriminants by value;
    // reordering CellRole without updating the shader silently retargets
    // chrome at the wrong cells.
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
    let wgsl = include_str!("../src/gui/core/shaders/composite.wgsl");
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
        assert!(wgsl.contains(&line), "composite.wgsl is missing `{line}`");
    }
}

#[test]
fn wgsl_has_no_hardcoded_background() {
    // The old clear color must not reappear as a literal anywhere
    // in the composite shader; out-of-grid fragments use the bg uniform.
    let wgsl = include_str!("../src/gui/core/shaders/composite.wgsl");
    assert!(!wgsl.contains("0.1, 0.1, 0.12"));
    assert!(wgsl.contains("return uniforms.bg;"));
}

#[test]
fn blur_kernel_weights_sum_to_one() {
    // The glass blur kernel must preserve brightness: its tap weights
    // sum to 1.0. Parsed from the shader source so a typo in the kernel
    // cannot silently darken or brighten panel fills.
    let wgsl = include_str!("../src/gui/core/shaders/composite.wgsl");
    let start = wgsl.find("fn blur_bg").expect("blur_bg missing");
    let body = &wgsl[start..];
    let end = body.find("\n}\n").expect("blur_bg body end missing");
    let body = &body[..end];
    let mut sum = 0.0f32;
    let mut taps = 0u32;
    // Only the tap lines carry ".rgb * <weight>"; the offset math
    // (`* 0.5`, `* 0.7071`) has no `.rgb` on its left.
    for part in body.split(".rgb * ") {
        let num: String = part
            .chars()
            .take_while(|c| c.is_ascii_digit() || *c == '.')
            .collect();
        if let Ok(w) = num.parse::<f32>() {
            sum += w;
            taps += 1;
        }
    }
    assert_eq!(taps, 13, "blur kernel tap count changed");
    assert!(
        (sum - 1.0).abs() < 1e-6,
        "blur kernel weights sum to {sum}, not 1.0"
    );
}
