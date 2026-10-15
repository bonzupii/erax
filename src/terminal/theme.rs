//! Color themes as plain data.
//!
//! A theme is a plain data value with named fields (N-099): a new theme is a
//! copied struct literal with the colors changed. There is one `Color` type
//! for themes and terminal alike (N-098) — `crate::terminal::color::Color` —
//! stored directly in the theme and read by field at render sites, with no
//! conversion layer and no per-theme constructor functions (N-103's `rgb!`
//! macro is gone; construct the value directly).
//!
//! Theme selection is a typed match over `ThemeId` producing a `&'static Theme`
//! (N-101): no stringly registry, no HashMap, no per-lookup clone of the whole
//! palette. Light/dark is a field of the theme value, decided once where the
//! theme is defined (N-073). There is exactly one default theme, stated once
//! (N-102): `ThemeId::default()`.

use crate::terminal::color::Color;

pub mod themes;

/// A color theme: plain data, one named field per UI/syntax role (N-100).
///
/// Roles that previously collided on one palette slot (e.g. `selection_bg`,
/// `current_line_bg`, and `scrollbar_track`) are independent fields here; each
/// theme sets them explicitly. The compat aliases (`status_line_bg` etc.) are
/// gone — every role has exactly one name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Theme {
    /// Theme name, e.g. `"dracula"`.
    pub name: &'static str,
    /// True for dark-background themes, false for light ones (N-073).
    pub is_dark: bool,

    // -- editor surface --
    /// Editor background.
    pub bg: Color,
    /// Default text.
    pub fg: Color,
    /// Region selection background.
    pub selection_bg: Color,
    /// Region selection foreground.
    pub selection_fg: Color,
    /// Background of the line the cursor is on.
    pub current_line_bg: Color,
    /// Cursor block background.
    pub cursor_bg: Color,
    /// Cursor block foreground.
    pub cursor_fg: Color,
    /// Line-number gutter background.
    pub gutter_bg: Color,
    /// Line-number gutter foreground.
    pub gutter_fg: Color,

    // -- status line --
    /// Active status line background.
    pub status_bg: Color,
    /// Active status line foreground.
    pub status_fg: Color,
    /// Inactive status line background.
    pub status_inactive_bg: Color,
    /// Inactive status line foreground.
    pub status_inactive_fg: Color,

    // -- chrome --
    /// Scrollbar track.
    pub scrollbar_track: Color,
    /// Scrollbar thumb.
    pub scrollbar_thumb: Color,
    /// Underline tint for misspelled words.
    pub spell_tint: Color,

    // -- syntax roles (generic highlighter, N-269) --
    /// Keywords.
    pub keyword: Color,
    /// Type names.
    pub type_name: Color,
    /// String literals.
    pub string: Color,
    /// Character literals.
    pub char_lit: Color,
    /// Numeric literals.
    pub number: Color,
    /// Comments.
    pub comment: Color,
    /// Preprocessor directives.
    pub preprocessor: Color,
    /// Function names.
    pub function: Color,
    /// Operators.
    pub operator: Color,
    /// Punctuation.
    pub punctuation: Color,

    // -- diagnostics --
    /// Errors.
    pub error: Color,
    /// Warnings.
    pub warning: Color,
    /// Informational notes.
    pub info: Color,

    // -- overlays (32-slot standard) --
    /// Background of search-match highlights. Text keeps its own color.
    pub search_match_bg: Color,
    /// Background of added lines in diff views.
    pub diff_added_bg: Color,
    /// Background of removed lines in diff views.
    pub diff_removed_bg: Color,
}

/// Typed theme identifier. Selecting a theme is a match over this enum (N-101);
/// an invalid selection is a compile error or a typed CLI parse error, never a
/// silent runtime fallback (N-014, N-088).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ThemeId {
    Dracula,
    Monokai,
    SolarizedDark,
    Gruvbox,
    Nord,
    TokyoNight,
    CatppuccinMocha,
    OneDark,
    RosePine,
    Kanagawa,
    MaterialOcean,
    NightOwl,
    Synthwave84,
    Everforest,
    GithubDark,
    AyuMirage,
    Horizon,
    VictorianGothic,
    VictorianWarm,
    VictorianNeutral,
    VictorianRoyal,
    VictorianGaslight,
    HighContrastDark,
    HighContrastLight,
    SolarizedLight,
    GithubLight,
    OneLight,
    HighContrastSolarized,
    MonochromeHighContrast,
    Ansi256Classic,
    DeuteranopiaAzure,
    ProtanopiaLumos,
    TritanopiaBlossom,
    AchromaNoir,
}

impl ThemeId {
    /// All themes in a stable order (for listings/help).
    pub fn all() -> &'static [ThemeId] {
        use ThemeId::*;
        &[
            Dracula,
            Monokai,
            SolarizedDark,
            Gruvbox,
            Nord,
            TokyoNight,
            CatppuccinMocha,
            OneDark,
            RosePine,
            Kanagawa,
            MaterialOcean,
            NightOwl,
            Synthwave84,
            Everforest,
            GithubDark,
            AyuMirage,
            Horizon,
            VictorianGothic,
            VictorianWarm,
            VictorianNeutral,
            VictorianRoyal,
            VictorianGaslight,
            HighContrastDark,
            HighContrastLight,
            SolarizedLight,
            GithubLight,
            OneLight,
            HighContrastSolarized,
            MonochromeHighContrast,
            Ansi256Classic,
            DeuteranopiaAzure,
            ProtanopiaLumos,
            TritanopiaBlossom,
            AchromaNoir,
        ]
    }

    /// Kebab-case name used by `--theme` and listings.
    pub fn name(self) -> &'static str {
        self.theme().name
    }

    /// The theme value for this id. One typed match, no registry (N-101).
    pub fn theme(self) -> &'static Theme {
        use ThemeId::*;
        match self {
            Dracula => &themes::DRACULA,
            Monokai => &themes::MONOKAI,
            SolarizedDark => &themes::SOLARIZED_DARK,
            Gruvbox => &themes::GRUVBOX,
            Nord => &themes::NORD,
            TokyoNight => &themes::TOKYO_NIGHT,
            CatppuccinMocha => &themes::CATPPUCCIN_MOCHA,
            OneDark => &themes::ONE_DARK,
            RosePine => &themes::ROSE_PINE,
            Kanagawa => &themes::KANAGAWA,
            MaterialOcean => &themes::MATERIAL_OCEAN,
            NightOwl => &themes::NIGHT_OWL,
            Synthwave84 => &themes::SYNTHWAVE_84,
            Everforest => &themes::EVERFOREST,
            GithubDark => &themes::GITHUB_DARK,
            AyuMirage => &themes::AYU_MIRAGE,
            Horizon => &themes::HORIZON,
            VictorianGothic => &themes::VICTORIAN_GOTHIC,
            VictorianWarm => &themes::VICTORIAN_WARM,
            VictorianNeutral => &themes::VICTORIAN_NEUTRAL,
            VictorianRoyal => &themes::VICTORIAN_ROYAL,
            VictorianGaslight => &themes::VICTORIAN_GASLIGHT,
            HighContrastDark => &themes::HIGH_CONTRAST_DARK,
            HighContrastLight => &themes::HIGH_CONTRAST_LIGHT,
            SolarizedLight => &themes::SOLARIZED_LIGHT,
            GithubLight => &themes::GITHUB_LIGHT,
            OneLight => &themes::ONE_LIGHT,
            HighContrastSolarized => &themes::HIGH_CONTRAST_SOLARIZED,
            MonochromeHighContrast => &themes::MONOCHROME_HIGH_CONTRAST,
            Ansi256Classic => &themes::ANSI256_CLASSIC,
            DeuteranopiaAzure => &themes::DEUTERANOPIA_AZURE,
            ProtanopiaLumos => &themes::PROTANOPIA_LUMOS,
            TritanopiaBlossom => &themes::TRITANOPIA_BLOSSOM,
            AchromaNoir => &themes::ACHROMA_NOIR,
        }
    }
}

/// The one default theme, stated once (N-102).
///
/// GitHub Dark is the stock default: the most neutral dark palette in the
/// tree by background chroma, with a seamless status bar and full truecolor
/// role coverage. This matches the user's configured choice (`github-dark`
/// in the typed config); the previous three-way disagreement between
/// `Theme::default()`, the config map, and the display fallback is resolved
/// by having a single source.
impl Default for ThemeId {
    fn default() -> Self {
        ThemeId::GithubDark
    }
}

/// Parse error for `--theme` values.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UnknownTheme;

impl core::fmt::Display for UnknownTheme {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "unknown theme")
    }
}

impl core::str::FromStr for ThemeId {
    type Err = UnknownTheme;

    /// Parse a kebab-case theme name (e.g. `"gruvbox"`, `"solarized-dark"`).
    /// An invalid name is a typed parse error, never a silent fallback (N-014).
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        for id in Self::all() {
            if id.theme().name == s {
                return Ok(*id);
            }
        }
        Err(UnknownTheme)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_theme_id_resolves_and_round_trips_through_name() {
        for id in ThemeId::all() {
            let theme = id.theme();
            assert_eq!(theme.name, id.name());
            // kebab-case parse round-trips
            let parsed: ThemeId = theme.name.parse().expect("theme name must parse");
            assert_eq!(parsed, *id);
        }
    }

    #[test]
    fn unknown_theme_name_is_a_typed_error() {
        let result: Result<ThemeId, _> = "not-a-theme".parse();
        assert_eq!(result, Err(UnknownTheme));
    }

    #[test]
    fn default_theme_is_coherent() {
        // Structural: whatever the configured default is, its name
        // round-trips through the typed parser to itself. The type
        // system, not this test, is the proof of the config value.
        let default = ThemeId::default();
        let parsed: ThemeId = default.name().parse().expect("default name parses");
        assert_eq!(parsed, default);
    }

    #[test]
    fn light_themes_are_marked_not_dark() {
        for id in ThemeId::all() {
            let t = id.theme();
            // is_dark must agree with the polarity unambiguous from the name.
            match *id {
                ThemeId::HighContrastLight
                | ThemeId::SolarizedLight
                | ThemeId::GithubLight
                | ThemeId::OneLight => assert!(!t.is_dark, "{id:?} is a light theme"),
                ThemeId::HighContrastDark | ThemeId::SolarizedDark | ThemeId::GithubDark => {
                    assert!(t.is_dark, "{id:?} is a dark theme")
                }
                _ => {}
            }
        }
    }
}
