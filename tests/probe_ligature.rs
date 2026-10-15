//! Probe: does the presentation core's shaping path form ligatures?

use cosmic_text::{Align, Attrs, Buffer, Family, Metrics, Shaping};
use std::sync::Arc;

#[test]
fn probe_fi_ligature_through_core_path() {
    let bytes: Vec<u8> = std::fs::read("/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf")
        .expect("DejaVu Sans Mono present");
    let fonts = [fontdb::Source::Binary(Arc::new(bytes))];
    let mut font_system = cosmic_text::FontSystem::new_with_fonts(fonts);
    let metrics = Metrics::new(16.0, 19.0);
    let mut buffer = Buffer::new(&mut font_system, metrics);
    // DejaVu carries fi/fl under `dlig` (discretionary), not default-on
    // `liga`: enable it explicitly to test whether the shaping path
    // applies substitution, rather than the font's default coverage.
    let mut features = cosmic_text::FontFeatures::new();
    features.enable(cosmic_text::FeatureTag::new(b"dlig"));
    let attrs = Attrs::new()
        .family(Family::Name("DejaVu Sans Mono"))
        .font_features(features);
    buffer.set_text("fi fl", &attrs, Shaping::Advanced, Some(Align::Left));
    buffer.shape_until_scroll(&mut font_system, false);
    for run in buffer.layout_runs() {
        for g in run.glyphs {
            println!(
                "glyph start={} end={} x={} w={}",
                g.start, g.end, g.x, g.w
            );
        }
    }
    let n: usize = buffer
        .layout_runs()
        .map(|r| r.glyphs.len())
        .sum();
    println!("total layout glyphs for 'fi fl' (5 chars): {n}");
    assert!(n < 5, "expected ligatures to merge glyphs");
}
