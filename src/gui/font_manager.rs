//! Font system using cosmic-text

use cosmic_text::{Align, Attrs, Buffer, Family, FontSystem, Metrics, Shaping, SwashCache};

pub struct FontManager {
    pub font_system: FontSystem,
    pub swash_cache: SwashCache,
    pub primary_family: Option<String>,
    pub cell_width: f32,
    pub cell_height: f32,
    pub ascent: f32,
}

impl FontManager {
    pub fn new(font_size: f32, line_height: f32, primary_font: Option<&str>) -> Self {
        let mut font_system = FontSystem::new();
        let swash_cache = SwashCache::new();
        let metrics = Metrics::new(font_size, line_height);

        let mut attrs = Attrs::new();
        if let Some(fam) = primary_font {
            attrs = attrs.family(Family::Name(fam));
        } else {
            attrs = attrs.family(Family::Monospace);
        }

        let mut buffer = Buffer::new(&mut font_system, metrics);
        buffer.set_text(
            &mut font_system,
            "M",
            &attrs,
            Shaping::Advanced,
            Some(Align::Left),
        );
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
            primary_family: primary_font.map(|s| s.to_string()),
            cell_width,
            cell_height: line_height,
            ascent,
        }
    }

    pub fn cell_dimensions(&mut self) -> (f32, f32) {
        (self.cell_width, self.cell_height)
    }
}
