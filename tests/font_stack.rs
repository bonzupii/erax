//! Font-stack fallback probes for emoji and CJK through the real resolution path.
//!
//! Family-name typos fail silently (unresolvable names are skipped by
//! `resolve_config_fonts`), so these tests shape probe characters through
//! the configured stack plus the core `FontManager` and assert which
//! family wins each script. Each test skips when the expected family is
//! not installed on the test machine; the assertions are about observed
//! fallback behavior, not config values.

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
        for glyph in run.glyphs {
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
fn emoji_resolves_to_the_color_font() {
    if !stack_resolves("Noto Color Emoji") {
        eprintln!("skip: Noto Color Emoji not installed on this machine");
        return;
    }
    assert_eq!(
        winning_family("\u{1F600}").as_deref(),
        Some("Noto Color Emoji"),
        "emoji must resolve to the color font, not a monochrome fallback"
    );
}

#[test]
fn cjk_resolves_to_the_cjk_font() {
    if !stack_resolves("Noto Sans Mono CJK SC") {
        eprintln!("skip: Noto Sans Mono CJK SC not installed on this machine");
        return;
    }
    assert_eq!(
        winning_family("\u{4E2D}").as_deref(),
        Some("Noto Sans Mono CJK SC"),
        "CJK must resolve to the CJK font"
    );
}

#[test]
fn braille_resolves_to_distinct_glyphs() {
    use std::collections::HashSet;
    if !stack_resolves("DejaVu Sans") {
        eprintln!("skip: DejaVu Sans not installed on this machine");
        return;
    }
    // Distinct braille patterns must produce distinct glyphs through the
    // real shaping path; collapsing them to one glyph renders uniform
    // dense blocks instead of dot patterns.
    let fonts = resolve_config_fonts();
    let mut manager = FontManager::from_font_stack(16.0, 19.2, &fonts);
    let metrics = Metrics::new(16.0, 19.2);
    let patterns = ['\u{2800}', '\u{2801}', '\u{281F}', '\u{28FF}'];
    let mut glyph_ids = HashSet::new();
    for &ch in &patterns {
        let mut buffer = Buffer::new(&mut manager.font_system, metrics);
        buffer.set_text(
            &ch.to_string(),
            &Attrs::new().family(Family::Monospace),
            Shaping::Advanced,
            Some(Align::Left),
        );
        buffer.shape_until_scroll(&mut manager.font_system, false);
        for run in buffer.layout_runs() {
            for glyph in run.glyphs {
                glyph_ids.insert((glyph.font_id, glyph.glyph_id));
            }
        }
    }
    assert_eq!(
        glyph_ids.len(),
        patterns.len(),
        "braille patterns collapsed to fewer glyphs than patterns"
    );
}

#[test]
fn zwj_sequences_resolve_to_the_color_font() {
    if !stack_resolves("Noto Color Emoji") {
        eprintln!("skip: Noto Color Emoji not installed on this machine");
        return;
    }
    // ZWJ sequences the color font covers must compose as color glyphs,
    // not fall back to monochrome. Each sequence should shape to glyphs
    // from Noto Color Emoji.
    let sequences = [
        "\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467}\u{200D}\u{1F466}", // family
        "\u{1F441}\u{FE0F}\u{200D}\u{1F5E8}\u{FE0F}", // eye in speech bubble
        "\u{1F469}\u{200D}\u{2695}\u{FE0F}", // woman health worker
    ];
    for seq in sequences {
        let family = winning_family(seq);
        assert_eq!(
            family.as_deref(),
            Some("Noto Color Emoji"),
            "ZWJ sequence {seq:?} must resolve to the color font, got {family:?}"
        );
    }
}
