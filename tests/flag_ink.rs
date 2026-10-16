//! Adjacent flag emoji ink-box metrics from the atlas path.
//!
//! Two regional-indicator flags side by side must each keep their ink
//! inside their own two-cell span: the paint path clips rows to the text
//! region, not clusters to their spans, so ink wider than the span
//! overlaps the neighbor's cells. This pins the span property through
//! the real shaping path; the visual verdict on any machine stays the
//! owner's.

use cosmic_text::{Align, Attrs, Buffer, Family, Metrics, Shaping};
use erax::gui::core::font_manager::FontManager;
use erax::gui::shell::native::resolve_config_fonts;

/// Shape `text` through the real configured stack and return the family
/// of the font that supplied the first glyph.
fn winning_family(text: &str) -> Option<String> {
    let fonts = resolve_config_fonts();
    let mut manager = FontManager::from_font_stack(16.0, 19.2, &fonts);
    let metrics = Metrics::new(16.0, 19.2);
    let mut buffer = Buffer::new(&mut manager.font_system, metrics);
    buffer.set_text(
        text,
        &Attrs::new().family(Family::Monospace),
        Shaping::Advanced,
        Some(Align::Left),
    );
    buffer.shape_until_scroll(&mut manager.font_system, false);
    for run in buffer.layout_runs() {
        if let Some(glyph) = run.glyphs.iter().next() {
            let face = manager.font_system.db().face(glyph.font_id)?;
            return face.families.first().map(|(name, _)| name.clone());
        }
    }
    None
}

fn stack_resolves(family: &str) -> bool {
    resolve_config_fonts()
        .iter()
        .any(|f| f.family.as_deref() == Some(family))
}

#[test]
fn adjacent_flags_keep_ink_inside_their_spans() {
    if !stack_resolves("Noto Color Emoji") {
        eprintln!("skip: Noto Color Emoji not installed on this machine");
        return;
    }
    // Two adjacent US flags (four regional indicators). Each flag must
    // resolve to the color font: a monochrome fallback (or no ligature at
    // all) changes the ink geometry this test guards. This fails loudly
    // on a machine whose primary font shadows the color emoji font —
    // that failure is the diagnostic.
    let two_flags = "\u{1F1FA}\u{1F1F8}\u{1F1FA}\u{1F1F8}";
    assert_eq!(
        winning_family(two_flags).as_deref(),
        Some("Noto Color Emoji"),
        "flags must resolve to the color font"
    );

    let fonts = resolve_config_fonts();
    let mut manager = FontManager::from_font_stack(16.0, 19.2, &fonts);
    let (cell_w, _) = manager.grid_cell_size();
    let metrics = Metrics::new(16.0, 19.2);
    let mut buffer = Buffer::new(&mut manager.font_system, metrics);
    buffer.set_text(
        two_flags,
        &Attrs::new().family(Family::Monospace),
        Shaping::Advanced,
        Some(Align::Left),
    );
    buffer.shape_until_scroll(&mut manager.font_system, false);

    // The TUI model gives each regional-indicator pair two cells; each
    // pair must shape to one ligature whose ink stays in that pair's own
    // span. The second flag's span starts at 2*cell_w (the paint path
    // re-snaps each cluster to the integer grid).
    let mut glyphs = 0;
    for run in buffer.layout_runs() {
        for glyph in run.glyphs {
            let flag_index = glyphs;
            let span_start = flag_index as f32 * 2.0 * cell_w;
            let span_end = span_start + 2.0 * cell_w;
            let physical = glyph.physical((0.0, 0.0), 1.0);
            let img = manager
                .swash_cache
                .get_image(&mut manager.font_system, physical.cache_key)
                .as_ref()
                .expect("flag glyph rasterizes");
            assert!(img.placement.width > 0, "flag glyph must have ink");
            // Ink is relative to the glyph's pen; the pen sits at the
            // flag's span start after grid snapping.
            let ink_left = span_start + img.placement.left as f32;
            let ink_right = ink_left + img.placement.width as f32;
            eprintln!("flag {flag_index} ink: left={ink_left} right={ink_right} span=[{span_start}, {span_end}]");
            assert!(
                ink_left >= span_start,
                "flag {flag_index} ink must not start before its span"
            );
            assert!(
                ink_right <= span_end,
                "flag {flag_index} ink must not cross into the adjacent flag's span"
            );
            glyphs += 1;
        }
    }
    assert_eq!(
        glyphs, 2,
        "two regional-indicator pairs must shape to two ligature glyphs"
    );
}
