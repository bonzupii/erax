//! Probe: full erax GUI shaping path with a synthetic default-on ligature font.
//!
//! /tmp/DejaVuLigaTest.ttf is DejaVu Sans Mono with its `dlig` fi/fl lookup
//! re-tagged as `liga` (default-on for Latin). If the presentation core's
//! shaping path forms the fi ligature here, ligation is NOT disabled in the
//! core — the observed absence is a font-coverage matter.

use cosmic_text::{Align, Attrs, Buffer, Family, Metrics, Shaping};
use erax::gui::core::font_manager::FontManager;
use erax::gui::core::renderer::{build_shaped_line, clusters_from_runs};
use erax::terminal::display::{Cell, CellRole, Color as TermColor};
use std::sync::Arc;

fn cell(ch: char) -> Cell {
    Cell::with_role(ch, TermColor::White, TermColor::Reset, CellRole::EditorText)
}

#[test]
fn probe_erax_path_forms_fi_ligature() {
    let bytes: Arc<Vec<u8>> =
        Arc::new(std::fs::read("/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf").expect("synthetic font"));
    let mut fm = FontManager::from_font_stack(16.0, 19.0, &[erax::gui::core::font_manager::FontSource { bytes, family: Some("DejaVu Sans Mono".to_string()) }]);

    // Build the row exactly like the renderer does.
    let cells: Vec<Cell> = "fi".chars().map(cell).collect();
    let input = build_shaped_line(&cells);
    assert_eq!(input.text, "fi");

    let mut shape_buffer = Buffer::new(&mut fm.font_system, Metrics::new(16.0, 19.0));
    let family = fm.primary_family.as_deref();
    let default_attrs = Attrs::new();
    let default_attrs = match family {
        Some(name) => default_attrs.family(Family::Name(name)),
        None => default_attrs.family(Family::Monospace),
    };
    // Explicitly enable liga + calt, to distinguish "defaults not applied"
    // from "feature broken entirely".
    let mut ff = cosmic_text::FontFeatures::new();
    ff.enable(cosmic_text::FeatureTag::new(b"dlig"));
    ff.enable(cosmic_text::FeatureTag::CONTEXTUAL_ALTERNATES);
    let default_attrs = default_attrs.font_features(ff.clone());
    shape_buffer.set_rich_text(
        input.spans.iter().map(|span| {
            (
                &input.text[span.start..span.end],
                Attrs::new()
                    .family(default_attrs.family)
                    .font_features(ff.clone())
                    .color(cosmic_text::Color(span.color)),
            )
        }),
        &default_attrs,
        Shaping::Advanced,
        Some(Align::Left),
    );
    shape_buffer.shape_until_scroll(&mut fm.font_system, false);

    let runs: Vec<_> = shape_buffer.layout_runs().collect();
    let total_glyphs: usize = runs.iter().map(|r| r.glyphs.len()).sum();
    for run in &runs {
        for g in run.glyphs {
            println!("glyph start={} end={} x={:.2} w={:.2}", g.start, g.end, g.x, g.w);
        }
    }
    println!("total layout glyphs for 'fi': {total_glyphs}");
    let clusters = clusters_from_runs(&runs);
    println!("clusters: {clusters:?}");
    assert_eq!(
        total_glyphs, 1,
        "the fi ligature must form through the erax shaping path"
    );
}
