//! Probe: bare cosmic-text (no FontManager) with synthetic default-on liga font.

use cosmic_text::{Align, Attrs, Buffer, Family, Metrics, Shaping};
use std::sync::Arc;

#[test]
fn probe_bare_cosmic_synthetic() {
    let bytes: Vec<u8> = std::fs::read("/tmp/DejaVuLigaTest.ttf").expect("synthetic font");
    let fonts = [fontdb::Source::Binary(Arc::new(bytes))];
    let mut font_system = cosmic_text::FontSystem::new_with_fonts(fonts);
    let metrics = Metrics::new(16.0, 19.0);
    let mut buffer = Buffer::new(&mut font_system, metrics);
    let attrs = Attrs::new().family(Family::Name("DejaVu Liga Test"));
    buffer.set_text("fi", &attrs, Shaping::Advanced, Some(Align::Left));
    buffer.shape_until_scroll(&mut font_system, false);
    let n: usize = buffer.layout_runs().map(|r| r.glyphs.len()).sum();
    for run in buffer.layout_runs() {
        for g in run.glyphs {
            println!("glyph start={} end={} x={:.2} w={:.2}", g.start, g.end, g.x, g.w);
        }
    }
    println!("bare cosmic-text glyphs for 'fi': {n}");
    assert_eq!(n, 1, "bare cosmic-text must form the default-on fi ligature");
}
