//! Font loading and fallback management for GUI mode
//!
//! Handles primary font loading and fallback chain.
//! Uses font-kit for cross-platform font discovery (fontconfig on Linux,
//! Core Text on macOS, DirectWrite on Windows).
//! Uses fontdue for fast glyph rasterization.

use font_kit::family_name::FamilyName;
use font_kit::properties::{Properties, Weight};
use font_kit::source::SystemSource;
use fontdue::Font;
use std::collections::HashSet;

/// Font loader with cached system source and negative glyph cache
pub struct FontLoader {
    /// System font source (initialized once, reused everywhere)
    #[allow(dead_code)]
    source: SystemSource,

    /// Primary font for rendering
    pub primary: Font,

    /// Raw font data for primary (needed for rustybuzz shaping)
    #[allow(dead_code)]
    pub primary_data: Vec<u8>,

    /// Loaded fallback fonts (in priority order)
    pub fallbacks: Vec<Font>,

    /// Raw font data for fallbacks (needed for rustybuzz shaping)
    pub fallback_data: Vec<Vec<u8>>,

    /// Fallback font names still to try
    fallback_names: Vec<&'static str>,

    /// Index of next unloaded fallback
    next_fallback_index: usize,

    /// Characters known to have no font available (negative cache)
    missing_chars: HashSet<char>,
}

impl FontLoader {
    /// Create a new font loader with the specified primary font
    pub fn new(font_path: Option<&str>) -> Result<Self, Box<dyn std::error::Error>> {
        let source = SystemSource::new();
        let font_data = Self::load_primary_font(&source, font_path)?;
        let settings = fontdue::FontSettings::default();
        let primary = Font::from_bytes(font_data.as_slice(), settings)
            .map_err(|e| format!("Failed to parse font: {}", e))?;

        Ok(Self {
            source,
            primary,
            primary_data: font_data,
            fallbacks: Vec::new(),
            fallback_data: Vec::new(),
            fallback_names: Self::get_fallback_names(),
            next_fallback_index: 0,
            missing_chars: HashSet::new(),
        })
    }

    /// Check if character is a Unicode noncharacter (will NEVER have a glyph in any font)
    ///
    /// Unicode defines 66 noncharacters that are permanently reserved:
    /// - U+FDD0..U+FDEF (32 chars in Arabic Presentation Forms-A)
    /// - U+FFFE, U+FFFF (BOM-related)
    /// - U+nFFFE, U+nFFFF for each plane n=1..16 (34 chars)
    #[inline]
    fn is_noncharacter(ch: char) -> bool {
        let code = ch as u32;

        // U+FDD0..U+FDEF (noncharacter block)
        if (0xFDD0..=0xFDEF).contains(&code) {
            return true;
        }

        // U+FFFE/FFFF and U+nFFFE/nFFFF for each plane
        let low_word = code & 0xFFFF;
        if low_word == 0xFFFE || low_word == 0xFFFF {
            return true;
        }

        false
    }

    /// Check if character is a C0 or C1 control character
    /// These are terminal control codes, not printable - never have standard font glyphs
    #[inline]
    fn is_control_char(ch: char) -> bool {
        let code = ch as u32;
        // C0: U+0000..U+001F (except we allow space U+0020)
        // C1: U+0080..U+009F
        code <= 0x001F || (0x0080..=0x009F).contains(&code)
    }

    /// Check if character is in a Private Use Area (PUA)
    /// PUA chars are user-defined (Nerd Fonts, custom icons) - standard fonts never have them
    #[inline]
    #[allow(dead_code)]
    fn is_private_use_area(ch: char) -> bool {
        let code = ch as u32;
        // BMP PUA: U+E000..U+F8FF
        // Plane 15 PUA: U+F0000..U+FFFFD
        // Plane 16 PUA: U+100000..U+10FFFD
        (0xE000..=0xF8FF).contains(&code)
            || (0xF0000..=0xFFFFD).contains(&code)
            || (0x100000..=0x10FFFD).contains(&code)
    }

    /// Check if character should be skipped entirely (no font will ever have it)
    /// This includes noncharacters and control codes
    pub fn is_known_missing(&self, ch: char) -> bool {
        Self::is_noncharacter(ch) || Self::is_control_char(ch) || self.missing_chars.contains(&ch)
    }

    /// Mark a character as having no available font
    pub fn mark_missing(&mut self, ch: char) {
        self.missing_chars.insert(ch);
    }

    /// Get a font that can render this character, if any
    /// Checks primary font first, then loaded fallbacks
    /// Returns None if no loaded font has this glyph (caller should try loading more fallbacks)
    #[allow(dead_code)]
    pub fn get_font_for_char(&self, ch: char) -> Option<&Font> {
        // Check primary font
        if self.primary.lookup_glyph_index(ch) != 0 {
            return Some(&self.primary);
        }

        // Check loaded fallbacks
        for font in &self.fallbacks {
            if font.lookup_glyph_index(ch) != 0 {
                return Some(font);
            }
        }

        None
    }

    /// Try to load next fallback font
    /// Returns true if a font was loaded, false if no more fallbacks available
    pub fn load_next_fallback(&mut self) -> bool {
        while self.next_fallback_index < self.fallback_names.len() {
            let name = self.fallback_names[self.next_fallback_index];
            self.next_fallback_index += 1;

            if let Some((font, data)) = self.load_fallback_by_name(name) {
                #[cfg(debug_assertions)]
                eprintln!("GUI: Lazy-loaded fallback font: {}", name);
                self.fallbacks.push(font);
                self.fallback_data.push(data);
                return true;
            }
        }
        false
    }

    /// Check if there are more fallbacks to try
    pub fn has_more_fallbacks(&self) -> bool {
        self.next_fallback_index < self.fallback_names.len()
    }

    /// Load primary font from path or system font database
    fn load_primary_font(
        source: &SystemSource,
        configured_font: Option<&str>,
    ) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        let mut props_builder = Properties::new();
        let props = props_builder.weight(Weight::NORMAL);

        // If user specified a font path, try loading it directly first
        if let Some(path) = configured_font {
            if let Ok(data) = std::fs::read(path) {
                #[cfg(debug_assertions)]
                eprintln!("GUI: Using configured font path: {}", path);
                return Ok(data);
            }
        }

        // If user specified a font name, try it
        if let Some(name) = configured_font {
            if let Ok(handle) =
                source.select_best_match(&[FamilyName::Title(name.to_string())], props)
            {
                if let Ok(font) = handle.load() {
                    if let Some(data) = font.copy_font_data() {
                        #[cfg(debug_assertions)]
                        eprintln!("GUI: Using configured font: {}", name);
                        return Ok(data.to_vec());
                    }
                }
            }
        }

        // Try system monospace first (fastest - single fontconfig query)
        if let Ok(handle) = source.select_best_match(&[FamilyName::Monospace], props) {
            if let Ok(font) = handle.load() {
                if let Some(data) = font.copy_font_data() {
                    #[cfg(debug_assertions)]
                    eprintln!("GUI: Using system default monospace");
                    return Ok(data.to_vec());
                }
            }
        }

        // Fallback: try common monospace fonts
        let fallbacks = [
            "DejaVu Sans Mono",
            "Noto Sans Mono",
            "Liberation Mono",
            "Consolas", // Windows
            "Menlo",    // macOS
        ];

        for name in &fallbacks {
            if let Ok(handle) =
                source.select_best_match(&[FamilyName::Title(name.to_string())], props)
            {
                if let Ok(font) = handle.load() {
                    if let Some(data) = font.copy_font_data() {
                        #[cfg(debug_assertions)]
                        eprintln!("GUI: Using fallback font: {}", name);
                        return Ok(data.to_vec());
                    }
                }
            }
        }

        Err("No monospace font found. Install a monospace font (e.g., noto-fonts-mono).".into())
    }

    /// Load a single fallback font by name
    fn load_fallback_by_name(&self, name: &str) -> Option<(Font, Vec<u8>)> {
        let mut props_builder = Properties::new();
        let props = props_builder.weight(Weight::NORMAL);

        let handle = self
            .source
            .select_best_match(&[FamilyName::Title(name.to_string())], props)
            .ok()?;
        let font_kit_font = handle.load().ok()?;
        let data = font_kit_font.copy_font_data()?.to_vec();

        let settings = fontdue::FontSettings::default();
        let font = Font::from_bytes(data.as_slice(), settings).ok()?;

        Some((font, data))
    }

    /// Get list of fallback font names (OS-specific + universal)
    fn get_fallback_names() -> Vec<&'static str> {
        let mut names = Vec::new();

        // OS-specific fonts first (higher priority)
        #[cfg(target_os = "macos")]
        {
            names.extend_from_slice(&[
                "Menlo",
                "SF Mono",
                "Monaco",
                "Apple Symbols",
                "Apple Color Emoji",
            ]);
        }

        #[cfg(target_os = "windows")]
        {
            names.extend_from_slice(&[
                "Consolas",
                "Cascadia Mono",
                "Lucida Console",
                "MS Gothic",
                "MS Mincho",
                "Segoe UI Emoji",
                "Segoe UI Historic",
            ]);
        }

        #[cfg(target_os = "linux")]
        {
            names.extend_from_slice(&[
                "DejaVu Sans Mono",
                "Noto Sans Mono",
                "Noto Color Emoji",
                "WenQuanYi Micro Hei Mono",
            ]);
        }

        // Universal fallbacks (cross-platform)
        names.extend_from_slice(&[
            // Wide Unicode coverage
            "Noto Sans Mono",
            "DejaVu Sans Mono",
            "DejaVu Sans",
            "Unifont",
            "GNU Unifont",
            "FreeMono",
            "FreeSans",
            // CJK
            "Noto Sans Mono CJK JP",
            "Noto Sans Mono CJK SC",
            "Noto Sans CJK JP",
            "Source Han Sans",
            // Caucasian scripts
            "Noto Sans Georgian",
            "DejaVu Sans",
            "Noto Sans Armenian",
            // Middle East & Africa
            "Noto Sans Arabic",
            "Noto Sans Hebrew",
            "Noto Sans Ethiopic",
            "Noto Sans Thai",
            // Indic
            "Noto Sans Devanagari",
            "Noto Sans Tamil",
            "Noto Sans Bengali",
            // Symbols & Math
            "Noto Sans Symbols",
            "Noto Sans Symbols 2",
            "Noto Sans Math",
            "Symbola",
            // Historic/Runic
            "Noto Sans Runic",
            "Junicode",
            "Segoe UI Historic",
            // Emoji
            "Noto Emoji",
            "Noto Color Emoji",
        ]);

        names
    }
}
