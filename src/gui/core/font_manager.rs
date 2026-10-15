//! Font system for the presentation core: the configured stack, no OS scan.
//!
//! The presentation core never asks the OS *which* fonts to use (the
//! owner's design: font selection is configuration, not probing). The
//! caller provides the fonts as bytes, in fallback order:
//! - Native shell: resolves the typed config `FONT_STACK` to bytes via the
//!   platform's font lookup (outside the core), then passes the bytes in.
//! - Web shell: fetches the font stack from the server (which serves the
//!   configured stack) and passes the bytes in.
//!
//! The shaping database holds the whole stack in order, then the system
//! fonts as a last resort: the configured fallback wins ties, and glyphs
//! the stack cannot cover (emoji, CJK, historic scripts) still resolve
//! through the platform fallback instead of shaping as tofu. Loading the
//! system fonts last is not font *selection* — selection already happened
//! in the stack order above; it only keeps uncovered scripts renderable.
//! The primary family (the stack head) is the shaping family; every entry
//! is a real fallback candidate, which is what makes ligatures, Braille,
//! box drawing, and multi-script text work instead of depending on which
//! single font happened to resolve.

use cosmic_text::{Align, Attrs, Buffer, Family, FontSystem, Metrics, Shaping, SwashCache};
use std::sync::Arc;

/// One font file for the shaping database: bytes plus the family name the
/// shaper selects it by. The stack order is the fallback order.
#[derive(Clone, Debug)]
pub struct FontSource {
    /// The font file contents (TTF/OTF/WOFF2).
    pub bytes: Arc<Vec<u8>>,
    /// Family name for shaping selection. `None` selects by generic
    /// monospace instead of a named family.
    pub family: Option<String>,
}

pub struct FontManager {
    pub font_system: FontSystem,
    pub swash_cache: SwashCache,
    pub primary_family: Option<String>,
    pub cell_width: f32,
    pub cell_height: f32,
    pub ascent: f32,
}

impl FontManager {
    /// System locale for shaping, in BCP 47 form.
    ///
    /// cosmic-text keeps its locale query private, so the core asks the
    /// same source directly. Only the language subtag matters here (CJK
    /// han-unification in the platform fallback lists); anything
    /// unparseable falls back to `en-US`, matching cosmic-text's own
    /// fallback.
    fn locale() -> String {
        sys_locale::get_locale()
            .and_then(|tag| {
                let lang = tag.split(['_', '-', '.']).next()?.to_ascii_lowercase();
                if lang.len() == 2 && lang.bytes().all(|b| b.is_ascii_alphabetic()) {
                    Some(format!("{lang}-US"))
                } else {
                    None
                }
            })
            .unwrap_or_else(|| "en-US".to_string())
    }

    /// Create a font manager from the configured font stack.
    ///
    /// Every entry is loaded into the shaping database in order, then the
    /// system fonts as a last resort: stack entries sort before system
    /// fonts on ties, so the configured fallback wins over the arbitrary
    /// system pick. An empty stack means "no fonts" (empty database, no
    /// system scan either); callers should always provide the stack.
    pub fn from_font_stack(font_size: f32, line_height: f32, fonts: &[FontSource]) -> Self {
        let mut db = fontdb::Database::new();
        for font in fonts {
            db.load_font_source(fontdb::Source::Binary(font.bytes.clone()));
        }
        // Last resort only: the stack above sorts first, so configured
        // families win over whatever the platform happens to have.
        db.load_system_fonts();
        // Generic-family settings mirror cosmic-text's own defaults so a
        // `Family::Monospace` query behaves the same as through
        // `FontSystem::new_with_fonts`.
        db.set_monospace_family("Noto Sans Mono");
        db.set_sans_serif_family("Open Sans");
        db.set_serif_family("DejaVu Serif");

        let mut font_system = FontSystem::new_with_locale_and_db_and_fallback(
            Self::locale(),
            db,
            cosmic_text::fallback::PlatformFallback,
        );
        let swash_cache = SwashCache::new();
        let metrics = Metrics::new(font_size, line_height);

        let primary_family = fonts.first().and_then(|f| f.family.clone());
        let mut attrs = Attrs::new();
        if let Some(fam) = primary_family.as_deref() {
            attrs = attrs.family(Family::Name(fam));
        } else {
            attrs = attrs.family(Family::Monospace);
        }

        let mut buffer = Buffer::new(&mut font_system, metrics);
        buffer.set_text("M", &attrs, Shaping::Advanced, Some(Align::Left));
        buffer.shape_until_scroll(&mut font_system, false);

        let cell_width = buffer
            .layout_runs()
            .next()
            .and_then(|run| run.glyphs.first())
            .map(|g| g.w)
            .unwrap_or(font_size * 0.6);

        let ascent = buffer
            .layout_runs()
            .next()
            .map(|run| run.line_y)
            .unwrap_or(font_size * 0.8);

        Self {
            font_system,
            swash_cache,
            primary_family,
            cell_width,
            cell_height: line_height,
            ascent,
        }
    }

    /// The grid cell size the renderer must use: the shaped advance,
    /// exactly, not rounded.
    ///
    /// Background cells are located per fragment by `floor(pixel /
    /// cell_size)`, which is exact for fractional sizes, while shaped glyph
    /// x positions are multiples of the fractional advance. Rounding the
    /// cell width to an integer makes the background grid drift away from
    /// the glyphs one fraction of a pixel per column — the broken
    /// box-drawing alignment. Pure: the renderer and the tests share this
    /// one definition.
    pub fn grid_cell_size(&self) -> (f32, f32) {
        (self.cell_width, self.cell_height)
    }
}
