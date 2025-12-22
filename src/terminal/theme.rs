use std::collections::HashMap;

/// A color representation for the terminal - using RGB for precise color control
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Color {
    Rgb { r: u8, g: u8, b: u8 },
}

/// Helper macro to create RGB colors from hex values
#[macro_export]
macro_rules! rgb {
    ($r:expr, $g:expr, $b:expr) => {
        Color::Rgb {
            r: $r,
            g: $g,
            b: $b,
        }
    };
}

/// A semantic color palette for themes
///
/// This eliminates RGB duplication by defining colors once per theme,
/// then mapping them to UI/syntax roles via accessor methods.
#[derive(Clone, Debug)]
#[allow(dead_code)] // secondary field reserved for future UI accents
pub struct Palette {
    pub bg: Color,
    pub fg: Color,
    pub primary: Color,
    pub secondary: Color,
    pub error: Color,
    pub warning: Color,
    pub info: Color,
    pub red: Color,
    pub orange: Color,
    pub yellow: Color,
    pub green: Color,
    pub cyan: Color,
    pub blue: Color,
    pub purple: Color,
    pub pink: Color,
    pub gray_dark: Color,
    pub gray: Color,
    pub gray_light: Color,
    pub spell_tint: Color,
}

impl Palette {
    /// Create a palette from raw RGB tuples
    pub fn new(
        bg: (u8, u8, u8),
        fg: (u8, u8, u8),
        primary: (u8, u8, u8),
        secondary: (u8, u8, u8),
        error: (u8, u8, u8),
        warning: (u8, u8, u8),
        info: (u8, u8, u8),
        red: (u8, u8, u8),
        orange: (u8, u8, u8),
        yellow: (u8, u8, u8),
        green: (u8, u8, u8),
        cyan: (u8, u8, u8),
        blue: (u8, u8, u8),
        purple: (u8, u8, u8),
        pink: (u8, u8, u8),
        gray_dark: (u8, u8, u8),
        gray: (u8, u8, u8),
        gray_light: (u8, u8, u8),
        spell_tint: (u8, u8, u8),
    ) -> Self {
        Self {
            bg: rgb!(bg.0, bg.1, bg.2),
            fg: rgb!(fg.0, fg.1, fg.2),
            primary: rgb!(primary.0, primary.1, primary.2),
            secondary: rgb!(secondary.0, secondary.1, secondary.2),
            error: rgb!(error.0, error.1, error.2),
            warning: rgb!(warning.0, warning.1, warning.2),
            info: rgb!(info.0, info.1, info.2),
            red: rgb!(red.0, red.1, red.2),
            orange: rgb!(orange.0, orange.1, orange.2),
            yellow: rgb!(yellow.0, yellow.1, yellow.2),
            green: rgb!(green.0, green.1, green.2),
            cyan: rgb!(cyan.0, cyan.1, cyan.2),
            blue: rgb!(blue.0, blue.1, blue.2),
            purple: rgb!(purple.0, purple.1, purple.2),
            pink: rgb!(pink.0, pink.1, pink.2),
            gray_dark: rgb!(gray_dark.0, gray_dark.1, gray_dark.2),
            gray: rgb!(gray.0, gray.1, gray.2),
            gray_light: rgb!(gray_light.0, gray_light.1, gray_light.2),
            spell_tint: rgb!(spell_tint.0, spell_tint.1, spell_tint.2),
        }
    }
}

/// Represents a color theme for the editor
///
/// Uses a palette-first design where colors are defined once in the Palette,
/// then mapped to UI and syntax roles via accessor methods.
#[derive(Clone, Debug)]
pub struct Theme {
    pub name: String,
    pub palette: Palette,
}

#[allow(dead_code)]
impl Theme {
    /// Create a new theme with the given name and palette
    pub fn new(name: impl Into<String>, palette: Palette) -> Self {
        Self {
            name: name.into(),
            palette,
        }
    }

    // =========================================================================
    // UI COLOR ACCESSORS
    // =========================================================================

    /// Background color
    pub fn bg(&self) -> &Color {
        &self.palette.bg
    }

    /// Foreground color
    pub fn fg(&self) -> &Color {
        &self.palette.fg
    }

    /// Selection background
    pub fn selection_bg(&self) -> &Color {
        &self.palette.gray_dark
    }

    /// Current line background
    pub fn current_line_bg(&self) -> &Color {
        &self.palette.gray_dark
    }

    /// Selection foreground
    pub fn selection_fg(&self) -> &Color {
        &self.palette.fg
    }

    /// Cursor background (inverted: fg on dark theme)
    pub fn cursor_bg(&self) -> &Color {
        &self.palette.fg
    }

    /// Cursor foreground (inverted: bg on dark theme)
    pub fn cursor_fg(&self) -> &Color {
        &self.palette.bg
    }

    /// Gutter background (same as main bg)
    pub fn gutter_bg(&self) -> &Color {
        &self.palette.bg
    }

    /// Gutter foreground (dimmed text)
    pub fn gutter_fg(&self) -> &Color {
        &self.palette.gray
    }

    /// Status line background (primary accent)
    pub fn status_bg(&self) -> &Color {
        &self.palette.primary
    }

    /// Alias for status_bg to support existing code
    pub fn status_line_bg(&self) -> &Color {
        self.status_bg()
    }

    /// Status line foreground (contrast with primary)
    pub fn status_fg(&self) -> &Color {
        &self.palette.bg
    }

    /// Alias for status_fg to support existing code
    pub fn status_line_fg(&self) -> &Color {
        self.status_fg()
    }

    /// Inactive status line background
    pub fn status_line_inactive_bg(&self) -> &Color {
        &self.palette.gray_dark
    }

    /// Inactive status line foreground
    pub fn status_line_inactive_fg(&self) -> &Color {
        &self.palette.fg
    }

    /// Error color
    pub fn error(&self) -> &Color {
        &self.palette.error
    }

    /// Warning color
    pub fn warning(&self) -> &Color {
        &self.palette.warning
    }

    /// Info color
    pub fn info(&self) -> &Color {
        &self.palette.info
    }

    /// Secondary color
    pub fn secondary(&self) -> &Color {
        &self.palette.secondary
    }

    /// Red accent color
    pub fn red(&self) -> &Color {
        &self.palette.red
    }

    /// Orange accent color
    pub fn orange(&self) -> &Color {
        &self.palette.orange
    }

    /// Blue accent color
    pub fn blue(&self) -> &Color {
        &self.palette.blue
    }

    /// Light gray color
    pub fn gray_light(&self) -> &Color {
        &self.palette.gray_light
    }

    /// Dark gray color
    pub fn gray_dark(&self) -> &Color {
        &self.palette.gray_dark
    }

    /// Scrollbar track foreground (uses gray for subtle appearance)
    pub fn scrollbar_track(&self) -> &Color {
        &self.palette.gray_dark
    }

    /// Scrollbar thumb (uses primary accent for high visibility)
    pub fn scrollbar_thumb(&self) -> &Color {
        &self.palette.primary
    }

    /// Spell tint color
    pub fn spell_tint(&self) -> &Color {
        &self.palette.spell_tint
    }

    // =========================================================================
    // SYNTAX COLOR ACCESSORS
    // =========================================================================

    /// Keywords (if, else, for, while, etc.)
    pub fn keyword(&self) -> &Color {
        &self.palette.pink
    }

    /// Type names (int, String, etc.)
    pub fn type_name(&self) -> &Color {
        &self.palette.cyan
    }

    /// String literals
    pub fn string(&self) -> &Color {
        &self.palette.yellow
    }

    /// Character literals
    pub fn char(&self) -> &Color {
        &self.palette.yellow
    }

    /// Numeric literals
    pub fn number(&self) -> &Color {
        &self.palette.purple
    }

    /// Comments
    pub fn comment(&self) -> &Color {
        &self.palette.gray
    }

    /// Preprocessor directives (#include, #define)
    pub fn preprocessor(&self) -> &Color {
        &self.palette.pink
    }

    /// Function names
    pub fn function(&self) -> &Color {
        &self.palette.green
    }

    /// Operators
    pub fn operator(&self) -> &Color {
        &self.palette.fg
    }

    /// Punctuation
    pub fn punctuation(&self) -> &Color {
        &self.palette.fg
    }

    /// Normal text
    pub fn normal(&self) -> &Color {
        &self.palette.fg
    }

    // =========================================================================
    // THEME DEFINITIONS
    // =========================================================================

    pub fn dracula() -> Self {
        let bg = (40, 42, 54);
        let fg = (248, 248, 242);
        let purple = (189, 147, 249);
        let cyan = (139, 233, 253);
        let pink = (255, 121, 198);
        let selection = (68, 71, 90);
        let comment = (98, 114, 164);
        let red = (255, 85, 85);
        let orange = (255, 184, 108);
        let yellow = (241, 250, 140);
        let green = (80, 250, 123);

        Self::new(
            "dracula",
            Palette::new(
                bg,
                fg,
                purple,
                cyan,
                red,
                orange,
                cyan,
                red,
                orange,
                yellow,
                green,
                cyan,
                comment,
                purple,
                pink,
                selection,
                comment,
                fg,
                (0, 150, 255),
            ),
        )
    }

    pub fn monokai() -> Self {
        let bg = (39, 40, 34);
        let fg = (248, 248, 242);
        let green = (166, 226, 46);
        let cyan = (102, 217, 239);
        let red = (249, 38, 114);
        let selection = (73, 72, 62);
        let comment = (117, 113, 94);
        let orange = (253, 151, 31);
        let yellow = (230, 219, 116);
        let purple = (174, 129, 255);

        Self::new(
            "monokai",
            Palette::new(
                bg,
                fg,
                green,
                cyan,
                red,
                orange,
                cyan,
                red,
                orange,
                yellow,
                green,
                cyan,
                cyan,
                purple,
                red,
                selection,
                comment,
                fg,
                (0, 150, 255),
            ),
        )
    }

    pub fn solarized_dark() -> Self {
        let bg = (0, 43, 54);
        let fg = (131, 148, 150);
        let blue = (38, 139, 210);
        let cyan = (42, 161, 152);
        let green = (133, 153, 0);
        let selection = (7, 54, 66);
        let comment = (101, 123, 131);
        let yellow = (181, 137, 0);
        let red = (220, 50, 47);
        let orange = (203, 75, 22);
        let purple = (211, 54, 130);

        Self::new(
            "solarized_dark",
            Palette::new(
                bg,
                fg,
                blue,
                cyan,
                red,
                orange,
                cyan,
                red,
                orange,
                yellow,
                green,
                cyan,
                blue,
                purple,
                green,
                selection,
                comment,
                fg,
                (0, 150, 255),
            ),
        )
    }

    pub fn gruvbox() -> Self {
        let bg = (40, 40, 40);
        let fg = (235, 219, 178);
        let yellow = (250, 189, 47);
        let cyan = (131, 165, 152);
        let red = (251, 73, 52);
        let selection = (80, 73, 69);
        let comment = (146, 131, 116);
        let orange = (254, 128, 25);
        let green = (184, 187, 38);
        let blue = (69, 133, 136);
        let purple = (211, 134, 155);

        Self::new(
            "gruvbox",
            Palette::new(
                bg,
                fg,
                yellow,
                cyan,
                red,
                orange,
                cyan,
                red,
                orange,
                yellow,
                green,
                cyan,
                blue,
                purple,
                red,
                selection,
                comment,
                fg,
                (0, 150, 255),
            ),
        )
    }

    pub fn nord() -> Self {
        let bg = (46, 52, 64);
        let fg = (216, 222, 233);
        let cyan = (136, 192, 208);
        let blue = (129, 161, 193);
        let selection = (67, 76, 94);
        let comment = (76, 86, 106);
        let red = (191, 97, 106);
        let orange = (208, 135, 112);
        let yellow = (235, 203, 139);
        let green = (163, 190, 140);
        let purple = (180, 142, 173);

        Self::new(
            "nord",
            Palette::new(
                bg,
                fg,
                cyan,
                blue,
                red,
                orange,
                cyan,
                red,
                orange,
                yellow,
                green,
                cyan,
                blue,
                purple,
                blue,
                selection,
                comment,
                fg,
                (0, 150, 255),
            ),
        )
    }

    pub fn tokyo_night() -> Self {
        let bg = (26, 27, 38);
        let fg = (169, 177, 214);
        let blue = (122, 162, 247);
        let cyan = (45, 169, 175);
        let selection = (52, 59, 88);
        let comment = (59, 66, 97);
        let red = (247, 118, 142);
        let orange = (255, 158, 100);
        let yellow = (224, 175, 104);
        let green = (158, 206, 106);
        let purple = (187, 154, 247);
        let bright_blue = (125, 207, 255);

        Self::new(
            "tokyo_night",
            Palette::new(
                bg,
                fg,
                blue,
                cyan,
                red,
                orange,
                cyan,
                red,
                orange,
                yellow,
                green,
                cyan,
                bright_blue,
                purple,
                red,
                selection,
                comment,
                fg,
                (0, 150, 255),
            ),
        )
    }

    pub fn catppuccin_mocha() -> Self {
        let bg = (30, 30, 46);
        let fg = (205, 214, 244);
        let blue = (137, 180, 250);
        let cyan = (148, 226, 213);
        let selection = (88, 91, 112);
        let comment = (108, 112, 134);
        let red = (243, 139, 168);
        let orange = (250, 179, 135);
        let yellow = (249, 226, 175);
        let green = (166, 227, 161);
        let purple = (203, 166, 247);
        let pink = (245, 194, 231);

        Self::new(
            "catppuccin_mocha",
            Palette::new(
                bg,
                fg,
                blue,
                cyan,
                red,
                orange,
                cyan,
                red,
                orange,
                yellow,
                green,
                cyan,
                blue,
                purple,
                pink,
                selection,
                comment,
                fg,
                (0, 150, 255),
            ),
        )
    }

    pub fn one_dark() -> Self {
        let bg = (40, 44, 52);
        let fg = (171, 178, 191);
        let blue = (97, 175, 239);
        let cyan = (86, 182, 194);
        let selection = (62, 68, 81);
        let comment = (92, 99, 112);
        let red = (224, 108, 117);
        let orange = (209, 154, 102);
        let yellow = (229, 192, 123);
        let green = (152, 195, 121);
        let purple = (198, 120, 221);

        Self::new(
            "one_dark",
            Palette::new(
                bg,
                fg,
                blue,
                cyan,
                red,
                orange,
                cyan,
                red,
                orange,
                yellow,
                green,
                cyan,
                blue,
                purple,
                purple,
                selection,
                comment,
                fg,
                (0, 150, 255),
            ),
        )
    }

    pub fn rose_pine() -> Self {
        let bg = (25, 23, 36);
        let fg = (224, 222, 244);
        let foam = (156, 207, 216);
        let pine = (49, 116, 143);
        let selection = (64, 61, 82);
        let comment = (110, 106, 134);
        let love = (235, 111, 146);
        let gold = (246, 193, 119);
        let iris = (196, 167, 231);

        Self::new(
            "rose_pine",
            Palette::new(
                bg,
                fg,
                foam,
                pine,
                love,
                gold,
                foam,
                love,
                gold,
                gold,
                foam,
                foam,
                pine,
                iris,
                love,
                selection,
                comment,
                fg,
                (0, 150, 255),
            ),
        )
    }

    pub fn kanagawa() -> Self {
        let bg = (31, 31, 40);
        let fg = (220, 215, 186);
        let blue = (126, 156, 216);
        let green_cyan = (106, 149, 137);
        let selection = (84, 84, 109);
        let comment = (114, 113, 105);
        let red = (255, 92, 87);
        let orange = (255, 160, 102);
        let yellow = (228, 174, 104);
        let green = (152, 187, 108);
        let purple = (149, 127, 184);
        let pink = (210, 126, 153);

        Self::new(
            "kanagawa",
            Palette::new(
                bg,
                fg,
                blue,
                green_cyan,
                red,
                orange,
                blue,
                red,
                orange,
                yellow,
                green,
                blue,
                blue,
                purple,
                pink,
                selection,
                comment,
                fg,
                (0, 150, 255),
            ),
        )
    }

    pub fn material_ocean() -> Self {
        let bg = (15, 17, 26);
        let fg = (143, 161, 179);
        let blue = (130, 170, 255);
        let cyan = (137, 221, 255);
        let selection = (52, 59, 71);
        let comment = (68, 82, 100);
        let red = (255, 83, 112);
        let orange = (247, 140, 108);
        let yellow = (255, 203, 107);
        let green = (195, 232, 141);
        let purple = (199, 146, 234);

        Self::new(
            "material_ocean",
            Palette::new(
                bg,
                fg,
                blue,
                cyan,
                red,
                orange,
                cyan,
                red,
                orange,
                yellow,
                green,
                cyan,
                blue,
                purple,
                purple,
                selection,
                comment,
                fg,
                (0, 150, 255),
            ),
        )
    }

    pub fn night_owl() -> Self {
        let bg = (1, 22, 39);
        let fg = (214, 222, 235);
        let purple = (126, 87, 194);
        let cyan = (127, 219, 202);
        let selection = (29, 59, 83);
        let comment = (99, 119, 119);
        let red = (239, 83, 80);
        let orange = (247, 140, 108);
        let yellow = (255, 203, 139);
        let green = (173, 219, 103);
        let blue = (130, 170, 255);
        let light_purple = (199, 146, 234);

        Self::new(
            "night_owl",
            Palette::new(
                bg,
                fg,
                purple,
                cyan,
                red,
                orange,
                cyan,
                red,
                orange,
                yellow,
                green,
                cyan,
                blue,
                light_purple,
                light_purple,
                selection,
                comment,
                fg,
                (0, 150, 255),
            ),
        )
    }

    pub fn synthwave_84() -> Self {
        let bg = (38, 35, 53);
        let fg = (255, 255, 255);
        let pink = (255, 56, 100);
        let cyan = (54, 243, 243);
        let selection = (72, 49, 94);
        let comment = (104, 134, 197);
        let red = (254, 67, 101);
        let orange = (255, 141, 0);
        let green = (114, 244, 116);
        let purple = (230, 137, 255);

        Self::new(
            "synthwave_84",
            Palette::new(
                bg,
                fg,
                pink,
                cyan,
                red,
                orange,
                cyan,
                red,
                orange,
                orange,
                green,
                cyan,
                cyan,
                purple,
                red,
                selection,
                comment,
                fg,
                (0, 150, 255),
            ),
        )
    }

    pub fn everforest() -> Self {
        let bg = (47, 52, 55);
        let fg = (211, 198, 170);
        let green = (167, 192, 128);
        let cyan = (123, 187, 197);
        let selection = (80, 89, 82);
        let comment = (135, 144, 130);
        let red = (230, 126, 128);
        let orange = (230, 152, 117);
        let yellow = (219, 188, 127);
        let light_cyan = (131, 192, 146);
        let purple = (214, 153, 182);

        Self::new(
            "everforest",
            Palette::new(
                bg,
                fg,
                green,
                cyan,
                red,
                orange,
                cyan,
                red,
                orange,
                yellow,
                green,
                light_cyan,
                cyan,
                purple,
                red,
                selection,
                comment,
                fg,
                (0, 150, 255),
            ),
        )
    }

    pub fn github_dark() -> Self {
        let bg = (13, 17, 23);
        let fg = (201, 209, 217);
        let blue = (47, 129, 247);
        let light_blue = (165, 214, 255);
        let selection = (48, 54, 61);
        let comment = (110, 118, 129);
        let red = (255, 123, 114);
        let orange = (219, 171, 121);
        let green = (126, 231, 135);
        let cyan = (121, 192, 255);
        let purple = (210, 168, 255);

        Self::new(
            "github_dark",
            Palette::new(
                bg,
                fg,
                blue,
                light_blue,
                red,
                orange,
                cyan,
                red,
                orange,
                orange,
                green,
                cyan,
                blue,
                purple,
                red,
                selection,
                comment,
                fg,
                (0, 150, 255),
            ),
        )
    }

    pub fn ayu_mirage() -> Self {
        let bg = (31, 36, 48);
        let fg = (203, 204, 198);
        let orange = (255, 173, 102);
        let cyan = (115, 217, 237);
        let selection = (64, 75, 95);
        let comment = (91, 102, 125);
        let red = (255, 114, 94);
        let yellow = (255, 212, 140);
        let green = (186, 230, 126);
        let blue = (89, 194, 255);
        let purple = (215, 95, 255);

        Self::new(
            "ayu_mirage",
            Palette::new(
                bg,
                fg,
                orange,
                cyan,
                red,
                orange,
                cyan,
                red,
                orange,
                yellow,
                green,
                cyan,
                blue,
                purple,
                orange,
                selection,
                comment,
                fg,
                (0, 150, 255),
            ),
        )
    }

    pub fn horizon() -> Self {
        let bg = (28, 30, 38);
        let fg = (224, 224, 224);
        let primary = (233, 86, 120);
        let cyan = (38, 187, 194);
        let selection = (55, 57, 69);
        let comment = (107, 109, 122);
        let orange = (250, 180, 130);
        let green = (148, 226, 204);
        let purple = (240, 135, 195);

        Self::new(
            "horizon",
            Palette::new(
                bg,
                fg,
                primary,
                cyan,
                primary,
                orange,
                cyan,
                primary,
                orange,
                orange,
                green,
                cyan,
                cyan,
                purple,
                primary,
                selection,
                comment,
                fg,
                (0, 150, 255),
            ),
        )
    }

    // =========================================================================
    // VICTORIAN THEMES
    // =========================================================================

    pub fn victorian_gothic() -> Self {
        let bg = (20, 15, 15);
        let fg = (200, 190, 175);
        let primary = (120, 40, 50);
        let secondary = (130, 110, 140);
        let selection = (60, 45, 45);
        let comment = (100, 85, 80);
        let red = (180, 80, 90);
        let orange = (160, 100, 80);
        let yellow = (160, 140, 100);
        let green = (100, 120, 90);
        let purple = (140, 100, 120);

        Self::new(
            "victorian_gothic",
            Palette::new(
                bg,
                fg,
                primary,
                secondary,
                red,
                orange,
                secondary,
                red,
                orange,
                yellow,
                green,
                secondary,
                (100, 80, 100),
                purple,
                red,
                selection,
                comment,
                fg,
                (0, 150, 255),
            ),
        )
    }

    pub fn victorian_warm() -> Self {
        let bg = (45, 35, 30);
        let fg = (235, 220, 200);
        let primary = (180, 120, 80);
        let secondary = (160, 140, 110);
        let selection = (85, 65, 55);
        let comment = (130, 115, 100);
        let red = (200, 120, 90);
        let orange = (200, 150, 100);
        let yellow = (190, 170, 120);
        let green = (140, 150, 100);
        let purple = (180, 140, 100);

        Self::new(
            "victorian_warm",
            Palette::new(
                bg,
                fg,
                primary,
                secondary,
                red,
                orange,
                secondary,
                red,
                orange,
                yellow,
                green,
                secondary,
                (140, 120, 100),
                purple,
                red,
                selection,
                comment,
                fg,
                (0, 150, 255),
            ),
        )
    }

    pub fn victorian_neutral() -> Self {
        let bg = (40, 40, 38);
        let fg = (220, 215, 205);
        let primary = (155, 145, 130);
        let secondary = (145, 155, 150);
        let selection = (75, 75, 70);
        let comment = (120, 118, 110);
        let red = (175, 145, 135);
        let orange = (170, 150, 130);
        let yellow = (180, 170, 140);
        let green = (140, 155, 130);
        let purple = (160, 145, 155);

        Self::new(
            "victorian_neutral",
            Palette::new(
                bg,
                fg,
                primary,
                secondary,
                red,
                orange,
                secondary,
                red,
                orange,
                yellow,
                green,
                secondary,
                (130, 140, 145),
                purple,
                red,
                selection,
                comment,
                fg,
                (0, 150, 255),
            ),
        )
    }

    pub fn victorian_royal() -> Self {
        let bg = (25, 20, 35);
        let fg = (215, 210, 225);
        let primary = (140, 90, 160);
        let secondary = (130, 145, 180);
        let selection = (55, 45, 75);
        let comment = (105, 95, 125);
        let red = (180, 120, 200);
        let orange = (170, 130, 150);
        let yellow = (200, 180, 140);
        let green = (120, 150, 130);
        let purple = (170, 130, 175);

        Self::new(
            "victorian_royal",
            Palette::new(
                bg,
                fg,
                primary,
                secondary,
                red,
                orange,
                secondary,
                red,
                orange,
                yellow,
                green,
                secondary,
                (110, 130, 170),
                purple,
                red,
                selection,
                comment,
                fg,
                (0, 150, 255),
            ),
        )
    }

    pub fn victorian_gaslight() -> Self {
        let bg = (30, 28, 25);
        let fg = (225, 215, 190);
        let primary = (190, 150, 70);
        let secondary = (170, 155, 125);
        let selection = (60, 55, 45);
        let comment = (110, 100, 85);
        let red = (210, 160, 90);
        let orange = (200, 160, 100);
        let yellow = (195, 180, 130);
        let green = (150, 160, 110);
        let purple = (200, 170, 110);

        Self::new(
            "victorian_gaslight",
            Palette::new(
                bg,
                fg,
                primary,
                secondary,
                red,
                orange,
                secondary,
                red,
                orange,
                yellow,
                green,
                secondary,
                (150, 140, 120),
                purple,
                red,
                selection,
                comment,
                fg,
                (0, 150, 255),
            ),
        )
    }

    // =========================================================================
    // HIGH CONTRAST / ACCESSIBILITY THEMES
    // =========================================================================

    pub fn high_contrast_dark() -> Self {
        let bg = (0, 0, 0);
        let fg = (255, 255, 255);
        let primary = (255, 255, 0);
        let secondary = (0, 255, 255);
        let selection = (50, 50, 50);
        let comment = (180, 180, 180);

        Self::new(
            "high_contrast_dark",
            Palette::new(
                bg,
                fg,
                primary,
                secondary,
                (255, 0, 0),
                (255, 165, 0),
                secondary,
                (255, 0, 0),
                (255, 165, 0),
                (0, 255, 0),
                (0, 255, 0),
                secondary,
                (0, 128, 255),
                (255, 0, 255),
                primary,
                selection,
                comment,
                fg,
                (0, 150, 255),
            ),
        )
    }

    pub fn high_contrast_light() -> Self {
        let bg = (255, 255, 255);
        let fg = (0, 0, 0);
        let primary = (0, 0, 128);
        let secondary = (0, 100, 100);
        let selection = (200, 200, 200);
        let comment = (80, 80, 80);

        Self::new(
            "high_contrast_light",
            Palette::new(
                bg,
                fg,
                primary,
                secondary,
                (139, 0, 0),
                (204, 85, 0),
                secondary,
                (139, 0, 0),
                (204, 85, 0),
                (0, 100, 0),
                (0, 100, 0),
                (0, 0, 139),
                (0, 0, 200),
                (128, 0, 128),
                (139, 0, 0),
                selection,
                comment,
                fg,
                (0, 150, 255),
            ),
        )
    }

    /// Solarized Light - The iconic light theme for reduced eye strain
    pub fn solarized_light() -> Self {
        let bg = (253, 246, 227); // base3
        let fg = (101, 123, 131); // base00
        let blue = (38, 139, 210);
        let cyan = (42, 161, 152);
        let green = (133, 153, 0);
        let selection = (238, 232, 213); // base2
        let comment = (147, 161, 161); // base1
        let yellow = (181, 137, 0);
        let red = (220, 50, 47);
        let orange = (203, 75, 22);
        let purple = (108, 113, 196); // violet

        Self::new(
            "solarized_light",
            Palette::new(
                bg,
                fg,
                blue,
                cyan,
                red,
                orange,
                cyan,
                red,
                orange,
                yellow,
                green,
                cyan,
                blue,
                purple,
                purple,
                selection,
                comment,
                (88, 110, 117), // darker text for gray_light
                (255, 200, 200),
            ),
        )
    }

    /// GitHub Light - Clean, familiar GitHub-style light theme
    pub fn github_light() -> Self {
        let bg = (255, 255, 255);
        let fg = (36, 41, 46);
        let blue = (0, 92, 197);
        let cyan = (3, 47, 98);
        let green = (34, 134, 58);
        let selection = (225, 228, 232);
        let comment = (106, 115, 125);
        let orange = (227, 98, 9);
        let red = (215, 58, 73);
        let purple = (111, 66, 193);

        Self::new(
            "github_light",
            Palette::new(
                bg,
                fg,
                blue,
                cyan,
                red,
                orange,
                blue,
                red,
                orange,
                (227, 98, 9), // orange for yellow
                green,
                cyan,
                blue,
                purple,
                (215, 58, 73), // pink same as red
                selection,
                comment,
                fg,
                (255, 200, 200),
            ),
        )
    }

    /// One Light - Atom's light theme counterpart
    pub fn one_light() -> Self {
        let bg = (250, 250, 250);
        let fg = (56, 58, 66);
        let red = (228, 86, 73);
        let green = (80, 161, 79);
        let yellow = (193, 132, 1);
        let blue = (64, 120, 242);
        let purple = (166, 38, 164);
        let cyan = (1, 132, 188);
        let orange = (152, 104, 1);
        let selection = (230, 230, 230);
        let comment = (160, 161, 167);

        Self::new(
            "one_light",
            Palette::new(
                bg,
                fg,
                blue,
                cyan,
                red,
                orange,
                blue,
                red,
                orange,
                yellow,
                green,
                cyan,
                blue,
                purple,
                (166, 38, 164),
                selection,
                comment,
                fg,
                (255, 200, 200),
            ),
        )
    }

    pub fn high_contrast_solarized() -> Self {
        let bg = (0, 43, 54);
        let fg = (253, 246, 227);
        let primary = (38, 139, 210);
        let secondary = (42, 161, 152);
        let selection = (7, 54, 66);
        let comment = (147, 161, 161);
        let yellow = (181, 137, 0);

        Self::new(
            "high_contrast_solarized",
            Palette::new(
                bg,
                fg,
                primary,
                secondary,
                (220, 50, 47),
                (203, 75, 22),
                secondary,
                (220, 50, 47),
                (203, 75, 22),
                yellow,
                (133, 153, 0),
                secondary,
                primary,
                (211, 54, 130),
                yellow,
                selection,
                comment,
                fg,
                (0, 150, 255),
            ),
        )
    }

    pub fn monochrome_high_contrast() -> Self {
        let bg = (0, 0, 0);
        let fg = (255, 255, 255);
        let selection = (80, 80, 80);
        let comment = (140, 140, 140);

        Self::new(
            "monochrome_high_contrast",
            Palette::new(
                bg,
                fg,
                fg,
                (220, 220, 220),
                fg,
                (230, 230, 230),
                (220, 220, 220),
                fg,
                (230, 230, 230),
                (200, 200, 200),
                (230, 230, 230),
                (220, 220, 220),
                (220, 220, 220),
                (240, 240, 240),
                fg,
                selection,
                comment,
                fg,
                (0, 150, 255),
            ),
        )
    }

    /// Classic ANSI 256-color terminal theme
    ///
    /// The "kernel developer nerd" aesthetic - green text on black background,
    /// using the traditional ANSI 16-color palette. This is the default theme
    /// per project requirements.
    pub fn ansi256_classic() -> Self {
        // High-contrast Phosphor Green (P1/P39)
        let bg = (0, 0, 0); // True black background for best contrast
        let fg = (0, 255, 64); // Classic P1 green
        let primary = (0, 255, 64);
        let secondary = (0, 192, 192); // Cyan-green
        let selection = (0, 64, 24); // Dark forest green highlight
        let comment = (0, 100, 32); // Lower contrast green
        let red = (255, 48, 48); // Warning red (kept for accessibility)
        let orange = (255, 160, 0); // Amber hint
        let yellow = (192, 255, 0); // Lemon-green
        let green = (0, 255, 64);
        let cyan = (64, 255, 220); // Bright teal
        let blue = (96, 128, 255); // Pale blue
        let purple = (220, 128, 255); // Pale purple
        let white = (200, 255, 220); // Mint white

        Self::new(
            "ansi256_classic",
            Palette::new(
                bg,
                fg,
                primary,
                secondary,
                red,
                orange,
                cyan,
                red,
                orange,
                yellow,
                green,
                cyan,
                blue,
                purple,
                purple,
                selection,
                comment,
                white,
                (0, 80, 40), // Subtle green spell tint
            ),
        )
    }

    // =========================================================================
    // COLOR VISION DEFICIENCY (CVD) ACCESSIBLE THEMES
    // =========================================================================

    pub fn deuteranopia_azure() -> Self {
        let bg = (30, 34, 42);
        let fg = (230, 230, 230);
        let primary = (86, 180, 233);
        let secondary = (204, 121, 167);
        let selection = (62, 68, 81);
        let comment = (133, 144, 166);
        let orange = (230, 159, 0);
        let yellow = (245, 199, 16);
        let vermilion = (213, 94, 0);

        Self::new(
            "deuteranopia_azure",
            Palette::new(
                bg,
                fg,
                primary,
                secondary,
                vermilion,
                orange,
                secondary,
                vermilion,
                orange,
                yellow,
                (0, 158, 115),
                secondary,
                (0, 114, 178),
                primary,
                vermilion,
                selection,
                comment,
                fg,
                (0, 150, 255),
            ),
        )
    }

    pub fn protanopia_lumos() -> Self {
        let bg = (18, 18, 18);
        let fg = (255, 255, 255);
        let primary = (100, 143, 255);
        let secondary = (220, 38, 127);
        let selection = (60, 60, 80);
        let comment = (176, 176, 176);
        let orange = (254, 97, 0);

        Self::new(
            "protanopia_lumos",
            Palette::new(
                bg,
                fg,
                primary,
                secondary,
                orange,
                orange,
                secondary,
                orange,
                orange,
                (255, 176, 0),
                (0, 229, 255),
                secondary,
                (120, 94, 240),
                primary,
                (120, 94, 240),
                selection,
                comment,
                fg,
                (0, 150, 255),
            ),
        )
    }

    pub fn tritanopia_blossom() -> Self {
        let bg = (25, 23, 36);
        let fg = (224, 222, 244);
        let primary = (235, 111, 146);
        let secondary = (156, 207, 216);
        let selection = (64, 61, 82);
        let comment = (110, 106, 134);
        let gold = (246, 193, 119);
        let pine = (49, 116, 143);

        Self::new(
            "tritanopia_blossom",
            Palette::new(
                bg,
                fg,
                primary,
                secondary,
                primary,
                gold,
                secondary,
                primary,
                gold,
                (235, 188, 186),
                secondary,
                secondary,
                pine,
                pine,
                primary,
                selection,
                comment,
                fg,
                (0, 150, 255),
            ),
        )
    }

    pub fn achroma_noir() -> Self {
        let bg = (0, 0, 0);
        let fg = (208, 208, 208);
        let primary = (255, 255, 255);
        let secondary = (200, 200, 200);
        let selection = (80, 80, 80);
        let comment = (80, 80, 80);

        Self::new(
            "achroma_noir",
            Palette::new(
                bg,
                fg,
                primary,
                secondary,
                primary,
                (224, 224, 224),
                secondary,
                primary,
                (224, 224, 224),
                (144, 144, 144),
                (224, 224, 224),
                secondary,
                secondary,
                (224, 224, 224),
                (240, 240, 240),
                selection,
                comment,
                fg,
                (0, 150, 255),
            ),
        )
    }

    // =========================================================================
    // DEFAULT THEME
    // =========================================================================

    pub fn default() -> Self {
        Self::dracula()
    }
}

// =========================================================================
// THEME MANAGER
// =========================================================================

pub struct ThemeManager {
    themes: HashMap<String, Theme>,
}

impl ThemeManager {
    pub fn new() -> Self {
        let mut manager = Self {
            themes: HashMap::new(),
        };

        // Core themes
        manager.register(Theme::dracula());
        manager.register(Theme::monokai());
        manager.register(Theme::solarized_dark());
        manager.register(Theme::gruvbox());
        manager.register(Theme::nord());
        manager.register(Theme::tokyo_night());
        manager.register(Theme::catppuccin_mocha());
        manager.register(Theme::one_dark());

        // Modern & aesthetic themes
        manager.register(Theme::rose_pine());
        manager.register(Theme::kanagawa());
        manager.register(Theme::material_ocean());
        manager.register(Theme::night_owl());
        manager.register(Theme::synthwave_84());
        manager.register(Theme::everforest());
        manager.register(Theme::github_dark());
        manager.register(Theme::ayu_mirage());
        manager.register(Theme::horizon());

        // Victorian themes
        manager.register(Theme::victorian_gothic());
        manager.register(Theme::victorian_warm());
        manager.register(Theme::victorian_neutral());
        manager.register(Theme::victorian_royal());
        manager.register(Theme::victorian_gaslight());

        // High contrast / accessibility themes
        manager.register(Theme::high_contrast_dark());
        manager.register(Theme::high_contrast_light());

        // Light themes
        manager.register(Theme::solarized_light());
        manager.register(Theme::github_light());
        manager.register(Theme::one_light());

        manager.register(Theme::high_contrast_solarized());
        manager.register(Theme::monochrome_high_contrast());
        manager.register(Theme::ansi256_classic());

        // Color Vision Deficiency (CVD) accessible themes
        manager.register(Theme::deuteranopia_azure());
        manager.register(Theme::protanopia_lumos());
        manager.register(Theme::tritanopia_blossom());
        manager.register(Theme::achroma_noir());

        manager
    }

    pub fn register(&mut self, theme: Theme) {
        self.themes.insert(theme.name.clone(), theme);
    }

    pub fn get(&self, name: &str) -> Option<Theme> {
        self.themes.get(name).cloned()
    }
}
