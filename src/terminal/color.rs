//! Terminal color definitions.
//!
//! One `Color` type for themes and terminal alike (N-098): the theme module
//! stores this type and render sites read it directly, with no conversion
//! layer between two same-named types.
//!
//! The render path never heap-allocates for colors: it maps `Color` to
//! `crossterm::style::Color` (a small `Copy` value compared by value, N-106)
//! and emits everything through crossterm commands (N-107).

/// Terminal color: `Reset`, the 16 ANSI names, or 24-bit RGB.
///
/// This is the single color type used by themes, the TUI renderer, and the
/// GUI atlas upload path.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
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
    /// Map to the crossterm color used by the TUI render path.
    ///
    /// The mapping is exact: named colors become crossterm's named variants,
    /// RGB becomes crossterm RGB. No allocation, no string formatting.
    ///
    /// Not compiled on wasm32 (the web shell has no crossterm).
    #[cfg(not(target_arch = "wasm32"))]
    pub fn to_crossterm(self) -> crossterm::style::Color {
        use crossterm::style::Color as C;
        match self {
            Color::Reset => C::Reset,
            Color::Black => C::Black,
            Color::Red => C::DarkRed,
            Color::Green => C::DarkGreen,
            Color::Yellow => C::DarkYellow,
            Color::Blue => C::DarkBlue,
            Color::Magenta => C::DarkMagenta,
            Color::Cyan => C::DarkCyan,
            Color::White => C::Grey,
            Color::BrightBlack => C::DarkGrey,
            Color::BrightRed => C::Red,
            Color::BrightGreen => C::Green,
            Color::BrightYellow => C::Yellow,
            Color::BrightBlue => C::Blue,
            Color::BrightMagenta => C::Magenta,
            Color::BrightCyan => C::Cyan,
            Color::BrightWhite => C::White,
            Color::Rgb { r, g, b } => C::Rgb { r, g, b },
        }
    }

    /// The 8-bit channel triple for this color.
    ///
    /// Values match the previous float-table conversion exactly
    /// (e.g. `BrightRed` is `(255, 127, 127)`), so packed GPU values are
    /// unchanged by the N-105 integer-packing fix.
    pub fn rgb8(self) -> (u8, u8, u8) {
        match self {
            Color::Reset | Color::Black => (0, 0, 0),
            Color::Red => (255, 0, 0),
            Color::Green => (0, 255, 0),
            Color::Yellow => (255, 255, 0),
            Color::Blue => (0, 0, 255),
            Color::Magenta => (255, 0, 255),
            Color::Cyan => (0, 255, 255),
            Color::White => (255, 255, 255),
            Color::BrightBlack => (127, 127, 127),
            Color::BrightRed => (255, 127, 127),
            Color::BrightGreen => (127, 255, 127),
            Color::BrightYellow => (255, 255, 127),
            Color::BrightBlue => (127, 127, 255),
            Color::BrightMagenta => (255, 127, 255),
            Color::BrightCyan => (127, 255, 255),
            Color::BrightWhite => (255, 255, 255),
            Color::Rgb { r, g, b } => (r, g, b),
        }
    }

    /// Convert color to GPU-compatible RGBA floats (0.0-1.0).
    ///
    /// Used by the GUI shader path, which consumes floats.
    pub fn to_rgba_f32(self) -> [f32; 4] {
        let (r, g, b) = self.rgb8();
        [
            r as f32 / 255.0,
            g as f32 / 255.0,
            b as f32 / 255.0,
            1.0,
        ]
    }

    /// Convert color to packed u32 RGBA (for GPU storage buffers).
    ///
    /// Packs the integer channels directly (N-105): no float round trip.
    /// Layout is `r | g << 8 | b << 16 | a << 24` with opaque alpha.
    pub fn to_packed_rgba(self) -> u32 {
        let (r, g, b) = self.rgb8();
        (r as u32) | ((g as u32) << 8) | ((b as u32) << 16) | (255u32 << 24)
    }

    /// Convert RGB color to nearest 16-color ANSI for terminals without TrueColor.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crossterm_mapping_is_exact_for_named_colors() {
        use crossterm::style::Color as C;
        assert_eq!(Color::Red.to_crossterm(), C::DarkRed);
        assert_eq!(Color::BrightRed.to_crossterm(), C::Red);
        assert_eq!(Color::Reset.to_crossterm(), C::Reset);
        assert_eq!(
            Color::Rgb { r: 1, g: 2, b: 3 }.to_crossterm(),
            C::Rgb { r: 1, g: 2, b: 3 }
        );
    }

    #[test]
    fn packed_rgba_packs_integer_channels_directly() {
        // r | g << 8 | b << 16 | a << 24, opaque alpha
        assert_eq!(Color::Rgb { r: 1, g: 2, b: 3 }.to_packed_rgba(), 0xFF_03_02_01);
        assert_eq!(Color::Red.to_packed_rgba(), 0xFF_00_00_FF);
        assert_eq!(Color::BrightRed.to_packed_rgba(), 0xFF_7F_7F_FF);
        assert_eq!(Color::Reset.to_packed_rgba(), 0xFF_00_00_00);
    }

    #[test]
    fn ansi_fallback_maps_rgb_to_nearest_named() {
        // Pure red maps to Red (exact match in the integer table).
        assert_eq!(
            Color::Rgb { r: 255, g: 0, b: 0 }.to_ansi_fallback(),
            Color::Red
        );
        assert_eq!(
            Color::Rgb { r: 10, g: 10, b: 10 }.to_ansi_fallback(),
            Color::Black
        );
        // Named colors pass through unchanged
        assert_eq!(Color::Green.to_ansi_fallback(), Color::Green);
    }
}
