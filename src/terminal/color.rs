//! Terminal color definitions and ANSI conversion
//!
//! Provides the Color enum used by TUI rendering and ANSI escape sequence generation.

/// Terminal color definition - supporting Reset, RGB, and 16-color ANSI fallback
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Color {
    Reset,
    Black,
    Red,
    Green,
    Yellow,
    Blue,
    Magenta,
    Cyan,
    White,
    BrightBlack,
    BrightRed,
    BrightGreen,
    BrightYellow,
    BrightBlue,
    BrightMagenta,
    BrightCyan,
    BrightWhite,
    Rgb { r: u8, g: u8, b: u8 },
}

impl Color {
    /// Convert Color enum to ANSI foreground color code
    pub fn to_ansi_fg_code(self) -> String {
        match self {
            Color::Reset => "39".to_string(),
            Color::Black => "30".to_string(),
            Color::Red => "31".to_string(),
            Color::Green => "32".to_string(),
            Color::Yellow => "33".to_string(),
            Color::Blue => "34".to_string(),
            Color::Magenta => "35".to_string(),
            Color::Cyan => "36".to_string(),
            Color::White => "37".to_string(),
            Color::BrightBlack => "90".to_string(),
            Color::BrightRed => "91".to_string(),
            Color::BrightGreen => "92".to_string(),
            Color::BrightYellow => "93".to_string(),
            Color::BrightBlue => "94".to_string(),
            Color::BrightMagenta => "95".to_string(),
            Color::BrightCyan => "96".to_string(),
            Color::BrightWhite => "97".to_string(),
            Color::Rgb { r, g, b } => format!("38;2;{};{};{}", r, g, b),
        }
    }

    /// Convert Color enum to ANSI background color code
    pub fn to_ansi_bg_code(self) -> String {
        match self {
            Color::Reset => "49".to_string(),
            Color::Black => "40".to_string(),
            Color::Red => "41".to_string(),
            Color::Green => "42".to_string(),
            Color::Yellow => "43".to_string(),
            Color::Blue => "44".to_string(),
            Color::Magenta => "45".to_string(),
            Color::Cyan => "46".to_string(),
            Color::White => "47".to_string(),
            Color::BrightBlack => "100".to_string(),
            Color::BrightRed => "101".to_string(),
            Color::BrightGreen => "102".to_string(),
            Color::BrightYellow => "103".to_string(),
            Color::BrightBlue => "104".to_string(),
            Color::BrightMagenta => "105".to_string(),
            Color::BrightCyan => "106".to_string(),
            Color::BrightWhite => "107".to_string(),
            Color::Rgb { r, g, b } => format!("48;2;{};{};{}", r, g, b),
        }
    }

    /// Convert color to GPU-compatible RGBA floats (0.0-1.0)
    pub fn to_rgba_f32(&self) -> [f32; 4] {
        match self {
            Color::Reset => [0.0, 0.0, 0.0, 1.0],
            Color::Black => [0.0, 0.0, 0.0, 1.0],
            Color::Red => [1.0, 0.0, 0.0, 1.0],
            Color::Green => [0.0, 1.0, 0.0, 1.0],
            Color::Yellow => [1.0, 1.0, 0.0, 1.0],
            Color::Blue => [0.0, 0.0, 1.0, 1.0],
            Color::Magenta => [1.0, 0.0, 1.0, 1.0],
            Color::Cyan => [0.0, 1.0, 1.0, 1.0],
            Color::White => [1.0, 1.0, 1.0, 1.0],
            Color::BrightBlack => [0.5, 0.5, 0.5, 1.0],
            Color::BrightRed => [1.0, 0.5, 0.5, 1.0],
            Color::BrightGreen => [0.5, 1.0, 0.5, 1.0],
            Color::BrightYellow => [1.0, 1.0, 0.5, 1.0],
            Color::BrightBlue => [0.5, 0.5, 1.0, 1.0],
            Color::BrightMagenta => [1.0, 0.5, 1.0, 1.0],
            Color::BrightCyan => [0.5, 1.0, 1.0, 1.0],
            Color::BrightWhite => [1.0, 1.0, 1.0, 1.0],
            Color::Rgb { r, g, b } => {
                [*r as f32 / 255.0, *g as f32 / 255.0, *b as f32 / 255.0, 1.0]
            }
        }
    }

    /// Convert color to packed u32 RGBA (for GPU storage buffers)
    pub fn to_packed_rgba(&self) -> u32 {
        let [r, g, b, a] = self.to_rgba_f32();
        let r = (r * 255.0) as u32;
        let g = (g * 255.0) as u32;
        let b = (b * 255.0) as u32;
        let a = (a * 255.0) as u32;
        r | (g << 8) | (b << 16) | (a << 24)
    }

    /// Convert RGB color to nearest 16-color ANSI for terminals without TrueColor
    pub fn to_ansi_fallback(self) -> Self {
        match self {
            Color::Rgb { r, g, b } => {
                let luminance = (r as u32 + g as u32 + b as u32) / 3;
                let bright = luminance > 127;
                let threshold = 85;

                let has_r = r > threshold;
                let has_g = g > threshold;
                let has_b = b > threshold;

                match (has_r, has_g, has_b, bright) {
                    (false, false, false, false) => Color::Black,
                    (false, false, false, true) => Color::BrightBlack,
                    (true, false, false, false) => Color::Red,
                    (true, false, false, true) => Color::BrightRed,
                    (false, true, false, false) => Color::Green,
                    (false, true, false, true) => Color::BrightGreen,
                    (true, true, false, false) => Color::Yellow,
                    (true, true, false, true) => Color::BrightYellow,
                    (false, false, true, false) => Color::Blue,
                    (false, false, true, true) => Color::BrightBlue,
                    (true, false, true, false) => Color::Magenta,
                    (true, false, true, true) => Color::BrightMagenta,
                    (false, true, true, false) => Color::Cyan,
                    (false, true, true, true) => Color::BrightCyan,
                    (true, true, true, false) => Color::White,
                    (true, true, true, true) => Color::BrightWhite,
                }
            }
            other => other,
        }
    }
}

impl From<crate::terminal::theme::Color> for Color {
    fn from(theme_color: crate::terminal::theme::Color) -> Self {
        match theme_color {
            crate::terminal::theme::Color::Rgb { r, g, b } => Color::Rgb { r, g, b },
        }
    }
}
