//! Box-drawing and block-element ink-box metrics from the atlas path.

use erax::gui::core::font_manager::FontManager;
use erax::gui::shell::native::resolve_config_fonts;
use cosmic_text::{Attrs, Buffer, Family, Metrics, Shaping};

fn ink_metrics(ch: char) -> Option<(i32, i32, u32, u32)> {
    let fonts = resolve_config_fonts();
    let mut manager = FontManager::from_font_stack(16.0, 19.2, &fonts);
    let metrics = Metrics::new(16.0, 19.2);
    let mut buffer = Buffer::new(&mut manager.font_system, metrics);
    buffer.set_text(
        &ch.to_string(),
        &Attrs::new().family(Family::Monospace),
        Shaping::Advanced,
        None,
    );
    buffer.shape_until_scroll(&mut manager.font_system, false);
    for run in buffer.layout_runs() {
        for glyph in run.glyphs {
            let physical = glyph.physical((0.0, 0.0), 1.0);
            if let Some(img) = manager
                .swash_cache
                .get_image(&mut manager.font_system, physical.cache_key)
                && img.placement.width > 0
            {
                return Some((
                    img.placement.left,
                    img.placement.top,
                    img.placement.width,
                    img.placement.height,
                ));
            }
        }
    }
    None
}

#[test]
fn box_drawing_ink_tiles_horizontally() {
    let (left, _top, width, _height) = ink_metrics('\u{2500}').expect("U+2500 rasterizes");
    let right_edge = left + width as i32;
    eprintln!("U+2500: left={left}, width={width}, right_edge={right_edge}");
    assert!(right_edge >= 9, "horizontal line ink must reach cell right edge");
}

#[test]
fn full_block_is_full_bleed() {
    let (left, top, width, height) = ink_metrics('\u{2588}').expect("U+2588 rasterizes");
    eprintln!("U+2588: left={left}, top={top}, width={width}, height={height}");
    assert!(left <= 0, "full block must start at or before cell left");
    assert!(width >= 9, "full block must span cell width");
}
