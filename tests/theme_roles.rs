//! Structural invariants over the 34 shipped themes.
//!
//! These tests assert shape, never taste: every theme fills every role
//! (the struct is a flat literal, so the compiler enforces this, but the
//! relationships between roles are runtime properties), emphasis roles
//! stay pairwise distinct, and every text pair in the UI chrome clears a
//! stated WCAG contrast floor. No test names a hex value; the type system
//! owns the values.
//!
//! Contrast floors (reasoning in the theme audit, `THEME-AUDIT.md`):
//! - `MIN_TEXT_CONTRAST` 4.5: WCAG 2.2 AA for normal text. Applies to
//!   selection text and both status bars.
//! - `MIN_BODY_CONTRAST` 4.0: body text floor; canonical low-contrast
//!   palettes (solarized) keep their upstream fg/bg, dim but readable.
//! - `MIN_INCIDENTAL_CONTRAST` 3.0: WCAG's floor for incidental and
//!   logotype-adjacent text. Applies to comments in non-canonical themes
//!   (canonical themes keep their upstream comment values verbatim, even
//!   the dim ones) and to the block cursor against the background
//!   (WCAG 2.1 SC 1.4.11, non-text UI component).
//! - `MIN_SIMULATED_SEPARATION` 24: under a Brettel deficiency simulation
//!   the three diagnostic colors must differ by at least 24 steps in one
//!   channel, or a color-deficient user cannot tell error from warning.

use erax::terminal::color::Color;
use erax::terminal::theme::{Theme, ThemeId};

/// WCAG 2.2 AA normal-text floor.
const MIN_TEXT_CONTRAST: f64 = 4.5;
/// Body-text floor. Canonical low-contrast palettes (solarized) keep their
/// upstream fg/bg values; 4.0 is the floor below which body text is
/// unreadable rather than merely dim.
const MIN_BODY_CONTRAST: f64 = 4.0;
/// WCAG incidental-text / non-text-UI-component floor.
const MIN_INCIDENTAL_CONTRAST: f64 = 3.0;
/// Minimum single-channel separation under deficiency simulation (0-255).
const MIN_SIMULATED_SEPARATION: i32 = 24;

fn rgb(c: Color) -> (u8, u8, u8) {
    match c {
        Color::Rgb { r, g, b } => (r, g, b),
        _ => panic!("theme roles must be truecolor RGB, got {:?}", c),
    }
}

fn luminance(c: Color) -> f64 {
    let (r, g, b) = rgb(c);
    let f = |v: u8| {
        let v = f64::from(v) / 255.0;
        if v <= 0.04045 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * f(r) + 0.7152 * f(g) + 0.0722 * f(b)
}

fn contrast(a: Color, b: Color) -> f64 {
    let (la, lb) = (luminance(a), luminance(b));
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}

/// Brettel-style deficiency simulation matrices, sRGB in and out.
fn simulate(c: Color, matrix: [[f64; 3]; 3]) -> (i32, i32, i32) {
    let (r, g, b) = rgb(c);
    let v = [f64::from(r) / 255.0, f64::from(g) / 255.0, f64::from(b) / 255.0];
    let mix =
        |row: [f64; 3]| ((row[0] * v[0] + row[1] * v[1] + row[2] * v[2]) * 255.0).round() as i32;
    (mix(matrix[0]), mix(matrix[1]), mix(matrix[2]))
}

const PROTANOPIA: [[f64; 3]; 3] = [
    [0.567, 0.433, 0.0],
    [0.558, 0.442, 0.0],
    [0.0, 0.242, 0.758],
];
const DEUTERANOPIA: [[f64; 3]; 3] = [
    [0.625, 0.375, 0.0],
    [0.7, 0.3, 0.0],
    [0.0, 0.3, 0.7],
];
const TRITANOPIA: [[f64; 3]; 3] = [
    [0.95, 0.05, 0.0],
    [0.0, 0.433, 0.567],
    [0.0, 0.475, 0.525],
];

fn channel_separation(a: (i32, i32, i32), b: (i32, i32, i32)) -> i32 {
    (a.0 - b.0).abs().max((a.1 - b.1).abs()).max((a.2 - b.2).abs())
}

/// Themes with a canonical upstream palette. Their comment colors are the
/// upstream's choice, exempt from the incidental floor.
fn is_canonical(t: &Theme) -> bool {
    matches!(
        t.name,
        "dracula"
            | "monokai"
            | "solarized_dark"
            | "solarized_light"
            | "gruvbox"
            | "nord"
            | "tokyo_night"
            | "catppuccin_mocha"
            | "one_dark"
            | "rose_pine"
            | "kanagawa"
            | "material_ocean"
            | "night_owl"
            | "synthwave_84"
            | "everforest"
            | "github_dark"
            | "github_light"
            | "ayu_mirage"
            | "horizon"
            | "one_light"
    )
}

#[test]
fn every_theme_fills_every_role_with_truecolor() {
    for id in ThemeId::all() {
        let t = id.theme();
        assert!(!t.name.is_empty(), "theme name must not be empty");
        // Touch every color role; rgb() panics on non-RGB.
        let roles = [
            t.bg, t.fg, t.selection_bg, t.selection_fg, t.current_line_bg,
            t.cursor_bg, t.cursor_fg, t.gutter_bg, t.gutter_fg, t.status_bg,
            t.status_fg, t.status_inactive_bg, t.status_inactive_fg,
            t.scrollbar_track, t.scrollbar_thumb, t.spell_tint, t.keyword,
            t.type_name, t.string, t.char_lit, t.number, t.comment,
            t.preprocessor, t.function, t.operator, t.punctuation, t.error,
            t.warning, t.info, t.search_match_bg, t.diff_added_bg,
            t.diff_removed_bg,
        ];
        for role in roles {
            rgb(role);
        }
        assert_eq!(roles.len(), 32, "theme {} must fill 32 roles", t.name);
    }
}

#[test]
fn emphasis_roles_are_pairwise_distinct() {
    for id in ThemeId::all() {
        let t = id.theme();
        // Selection must read as selection, not as the current line (D1).
        assert_ne!(
            t.selection_bg, t.current_line_bg,
            "{}: selection_bg == current_line_bg",
            t.name
        );
        // Search matches must read as neither.
        assert_ne!(
            t.search_match_bg, t.selection_bg,
            "{}: search_match_bg == selection_bg",
            t.name
        );
        assert_ne!(
            t.search_match_bg, t.current_line_bg,
            "{}: search_match_bg == current_line_bg",
            t.name
        );
        assert_ne!(
            t.search_match_bg, t.bg,
            "{}: search_match_bg == bg (invisible)",
            t.name
        );
        // Diagnostics must be three distinct hues.
        assert_ne!(t.error, t.warning, "{}: error == warning", t.name);
        assert_ne!(t.error, t.info, "{}: error == info", t.name);
        assert_ne!(t.warning, t.info, "{}: warning == info", t.name);
    }
}

#[test]
fn text_pairs_clear_the_wcag_floors() {
    for id in ThemeId::all() {
        let t = id.theme();
        let c = contrast(t.fg, t.bg);
        assert!(
            c >= MIN_BODY_CONTRAST,
            "{}: fg/bg contrast {:.2} < {:.1}",
            t.name, c, MIN_BODY_CONTRAST
        );
        let c = contrast(t.selection_fg, t.selection_bg);
        assert!(
            c >= MIN_TEXT_CONTRAST,
            "{}: selection contrast {:.2} < {:.1}",
            t.name, c, MIN_TEXT_CONTRAST
        );
        let c = contrast(t.status_fg, t.status_bg);
        assert!(
            c >= MIN_TEXT_CONTRAST,
            "{}: status contrast {:.2} < {:.1}",
            t.name, c, MIN_TEXT_CONTRAST
        );
        let c = contrast(t.status_inactive_fg, t.status_inactive_bg);
        assert!(
            c >= MIN_TEXT_CONTRAST,
            "{}: inactive status contrast {:.2} < {:.1}",
            t.name, c, MIN_TEXT_CONTRAST
        );
        let c = contrast(t.cursor_bg, t.bg);
        assert!(
            c >= MIN_INCIDENTAL_CONTRAST,
            "{}: cursor/bg contrast {:.2} < {:.1}",
            t.name, c, MIN_INCIDENTAL_CONTRAST
        );
        if !is_canonical(t) {
            let c = contrast(t.comment, t.bg);
            assert!(
                c >= MIN_INCIDENTAL_CONTRAST,
                "{}: comment/bg contrast {:.2} < {:.1}",
                t.name, c, MIN_INCIDENTAL_CONTRAST
            );
        }
    }
}

/// The deficiency-engineered themes keep error/warning/info separable
/// under the simulation for their deficiency. Victorian and ANSI256
/// themes are high-legibility originals, not deficiency-engineered,
/// and carry no such guarantee.
#[test]
fn engineered_cvd_themes_survive_their_deficiency_simulation() {
    let plan: &[(ThemeId, [[f64; 3]; 3])] = &[
        (ThemeId::ProtanopiaLumos, PROTANOPIA),
        (ThemeId::DeuteranopiaAzure, DEUTERANOPIA),
        (ThemeId::TritanopiaBlossom, TRITANOPIA),
        (ThemeId::AchromaNoir, PROTANOPIA),
        (ThemeId::AchromaNoir, DEUTERANOPIA),
        (ThemeId::AchromaNoir, TRITANOPIA),
    ];
    for (id, matrix) in plan {
        let t = id.theme();
        let diags = [t.error, t.warning, t.info];
        let simmed: Vec<(i32, i32, i32)> =
            diags.iter().map(|c| simulate(*c, *matrix)).collect();
        for (i, a) in simmed.iter().enumerate() {
            for b in &simmed[i + 1..] {
                let sep = channel_separation(*a, *b);
                assert!(
                    sep >= MIN_SIMULATED_SEPARATION,
                    "{}: diagnostics collapse under simulation (separation {})",
                    t.name, sep
                );
            }
        }
    }
}

#[test]
fn default_theme_is_a_real_theme() {
    let default = ThemeId::default();
    let t = default.theme();
    assert!(!t.name.is_empty());
    assert!(contrast(t.fg, t.bg) >= MIN_BODY_CONTRAST);
}
