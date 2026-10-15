//! Complex-script GUI shaping through the real font stack.
//! (N-120 run shaping, N-259 caret mapping, N-260 international text).
//!
//! These tests shape mixed-script lines through a real `FontSystem` using the
//! same call the renderer makes (`set_rich_text` with per-cell color spans,
//! `Shaping::Advanced`), then exercise the public pure mapping API
//! (`build_shaped_line`, `clusters_from_runs`, `caret_x_for_byte_offset`)
//! against the real shaped output. Assertions are structural (cluster
//! partition of the text, caret monotonicity / direction inversion, byte
//! ranges), never hard-coded pixel positions, so they hold for any font that
//! shapes the script.

use cosmic_text::{Align, Attrs, Buffer, Family, FontSystem, Metrics, Shaping};
use erax::gui::renderer::{
    build_shaped_line, caret_x_for_byte_offset, clusters_from_runs, ShapedCluster, TextSpan,
};
use erax::terminal::display::{Cell, CellRole, Color as TermColor};
use unicode_width::UnicodeWidthChar;

fn cell(ch: char, fg: TermColor) -> Cell {
    Cell {
        ch,
        fg,
        bg: TermColor::Reset,
        hidden: false,
        role: CellRole::EditorText,
    }
}

/// Build grid cells the way the TUI does: wide chars take two columns, the
/// second one hidden.
fn cells_for(text: &str) -> Vec<Cell> {
    let mut cells = Vec::new();
    for ch in text.chars() {
        cells.push(cell(ch, TermColor::White));
        if ch.width().unwrap_or(1) > 1 {
            cells.push(Cell {
                ch: ' ',
                fg: TermColor::Reset,
                bg: TermColor::Reset,
                hidden: true,
                role: CellRole::EditorText,
            });
        }
    }
    cells
}

/// Shape one built line exactly the way `Renderer::render` does, and return
/// the caret clusters, the shaped line width, and the raw glyph count.
fn shape_line(text: &str, spans: &[TextSpan]) -> (Vec<ShapedCluster>, f32, usize) {
    let mut font_system = FontSystem::new();
    let metrics = Metrics::new(16.0, 19.2);
    let mut buffer = Buffer::new(&mut font_system, metrics);
    let default_attrs = Attrs::new().family(Family::Monospace);
    buffer.set_rich_text(
        spans.iter().map(|span| {
            (
                &text[span.start..span.end],
                Attrs::new()
                    .family(Family::Monospace)
                    .color(cosmic_text::Color(span.color)),
            )
        }),
        &default_attrs,
        Shaping::Advanced,
        Some(Align::Left),
    );
    buffer.shape_until_scroll(&mut font_system, false);
    let runs: Vec<_> = buffer.layout_runs().collect();
    let line_w = runs.first().map_or(0.0, |run| run.line_w);
    let glyphs: usize = runs.iter().map(|run| run.glyphs.len()).sum();
    (clusters_from_runs(&runs), line_w, glyphs)
}

fn shaped(text: &str) -> (String, Vec<ShapedCluster>, f32, usize) {
    let cells = cells_for(text);
    let input = build_shaped_line(&cells);
    let (clusters, line_w, glyphs) = shape_line(&input.text, &input.spans);
    (input.text, clusters, line_w, glyphs)
}

/// Clusters must partition the shaped text: contiguous, non-overlapping,
/// covering [0, text.len()).
fn assert_partition(text: &str, clusters: &[ShapedCluster]) {
    let mut expected = 0;
    for cluster in clusters {
        assert_eq!(
            cluster.start, expected,
            "cluster gap/overlap in {text:?}: {clusters:?}"
        );
        assert!(cluster.end > cluster.start, "empty cluster in {text:?}");
        expected = cluster.end;
    }
    assert_eq!(
        expected,
        text.len(),
        "clusters do not cover {text:?}: {clusters:?}"
    );
}

#[test]
fn ltr_ascii_clusters_partition_and_caret_is_monotonic() {
    let (text, clusters, line_w, _) = shaped("Hello, world!");
    assert_eq!(text, "Hello, world!");
    assert_partition(&text, &clusters);
    assert_eq!(clusters.len(), 13, "one cluster per ASCII char");

    let mut prev = f32::NEG_INFINITY;
    for (i, _) in text.char_indices().chain(std::iter::once((text.len(), ' '))) {
        let x = caret_x_for_byte_offset(&clusters, &text, i);
        assert!(x >= prev, "caret not monotonic at byte {i} in {text:?}");
        prev = x;
    }
    assert_eq!(caret_x_for_byte_offset(&clusters, &text, 0), 0.0);
    let end = caret_x_for_byte_offset(&clusters, &text, text.len());
    assert!(
        (end - line_w).abs() < 0.01,
        "end caret {end} != line width {line_w}"
    );
}

#[test]
fn combining_marks_share_one_cluster() {
    // "cafe\u{301}": the precomposed-in-shaping acute merges with its base.
    let (text, clusters, _, _) = shaped("cafe\u{301}");
    assert_partition(&text, &clusters);
    assert_eq!(clusters.len(), 4, "expected clusters: {clusters:?}");
    let last = clusters[3];
    assert_eq!((last.start, last.end), (3, 6), "acute must join 'e'");
    assert!(!last.rtl);
    // No caret stop between the base char and its mark.
    let x3 = caret_x_for_byte_offset(&clusters, &text, 3);
    assert_eq!(x3, last.leading_edge());
}

#[test]
fn stacked_combining_marks_form_single_cluster() {
    // e + 3 combining marks: one cluster, one grapheme.
    let (text, clusters, line_w, _) = shaped("e\u{301}\u{302}\u{303}");
    assert_partition(&text, &clusters);
    assert_eq!(clusters.len(), 1, "expected one cluster: {clusters:?}");
    assert_eq!((clusters[0].start, clusters[0].end), (0, text.len()));
    assert_eq!(caret_x_for_byte_offset(&clusters, &text, 0), 0.0);
    let end = caret_x_for_byte_offset(&clusters, &text, text.len());
    assert!((end - line_w).abs() < 0.01);
}

#[test]
fn zwj_emoji_sequence_is_one_cluster() {
    let (text, clusters, line_w, glyphs) = shaped("👨‍👩‍👧‍👦");
    assert_partition(&text, &clusters);
    assert_eq!(glyphs, 1, "ZWJ sequence must shape to one glyph");
    assert_eq!(clusters.len(), 1);
    assert_eq!((clusters[0].start, clusters[0].end), (0, text.len()));
    assert_eq!(caret_x_for_byte_offset(&clusters, &text, 0), 0.0);
    let end = caret_x_for_byte_offset(&clusters, &text, text.len());
    assert!((end - line_w).abs() < 0.01);
}

#[test]
fn hebrew_rtl_caret_inverts() {
    let (text, clusters, line_w, _) = shaped("שלום");
    assert_partition(&text, &clusters);
    assert!(clusters.iter().all(|c| c.rtl), "all Hebrew clusters RTL");
    // Logical start is the visual right edge; logical end the left edge.
    let start_x = caret_x_for_byte_offset(&clusters, &text, 0);
    assert!(
        (start_x - line_w).abs() < 0.01,
        "RTL start {start_x} != line width {line_w}"
    );
    assert_eq!(caret_x_for_byte_offset(&clusters, &text, text.len()), 0.0);
    // Moving forward through the text moves the caret left on screen.
    let mut prev = f32::INFINITY;
    for i in [0, 2, 4, 6, 8] {
        let x = caret_x_for_byte_offset(&clusters, &text, i);
        assert!(x <= prev, "RTL caret not decreasing at byte {i}");
        prev = x;
    }
}

#[test]
fn arabic_contextual_shaping_keeps_caret_sane() {
    let (text, clusters, line_w, _) = shaped("مرحبا");
    assert_partition(&text, &clusters);
    assert!(clusters.iter().all(|c| c.rtl));
    let start_x = caret_x_for_byte_offset(&clusters, &text, 0);
    assert!(
        (start_x - line_w).abs() < 0.01,
        "RTL start {start_x} != line width {line_w}"
    );
    assert_eq!(caret_x_for_byte_offset(&clusters, &text, text.len()), 0.0);
}

#[test]
fn devanagari_conjunct_merges_glyphs_into_clusters() {
    // "नमस्ते": स + ् + त form one conjunct cluster; त + ो split into two
    // glyphs sharing one cluster.
    let (text, clusters, _, glyphs) = shaped("नमस्ते");
    assert_partition(&text, &clusters);
    assert!(
        glyphs > clusters.len(),
        "conjunct parts must merge: {glyphs} glyphs, {n} clusters",
        n = clusters.len()
    );
    assert!(
        clusters.iter().any(|c| (c.start, c.end) == (6, 12)),
        "expected conjunct cluster (6,12) in {clusters:?}"
    );
    assert!(
        clusters.iter().any(|c| (c.start, c.end) == (12, 18)),
        "expected vowel-sign cluster (12,18) in {clusters:?}"
    );
}

#[test]
fn khmer_cluster_merges_marks() {
    let (text, clusters, _, glyphs) = shaped("ខ្មែរ");
    assert_partition(&text, &clusters);
    assert!(glyphs > clusters.len(), "marks must merge into clusters");
    assert_eq!(clusters.len(), 2, "expected clusters: {clusters:?}");
    assert_eq!((clusters[0].start, clusters[0].end), (0, 12));
    assert_eq!((clusters[1].start, clusters[1].end), (12, 15));
}

#[test]
fn mixed_bidi_boundary_prefers_cluster_start() {
    let (text, clusters, _, _) = shaped("aב");
    assert_partition(&text, &clusters);
    assert_eq!(clusters.len(), 2);
    assert!(!clusters[0].rtl);
    assert!(clusters[1].rtl);
    // At the direction boundary the cluster starting at the offset wins:
    // the caret sits at the Hebrew cluster's leading (right) edge.
    let x = caret_x_for_byte_offset(&clusters, &text, 1);
    assert_eq!(x, clusters[1].leading_edge());
    assert_eq!(caret_x_for_byte_offset(&clusters, &text, 0), 0.0);
}

#[test]
fn cjk_wide_chars_map_columns_to_cluster_edges() {
    let cells = cells_for("日本");
    let input = build_shaped_line(&cells);
    assert_eq!(input.col_to_byte, vec![0, 3, 3, 6, 6]);
    let (clusters, line_w, _) = shape_line(&input.text, &input.spans);
    assert_partition(&input.text, &clusters);
    assert_eq!(clusters.len(), 2);
    // Caret at each grid column lands on a cluster boundary.
    let xs: Vec<f32> = input
        .col_to_byte
        .iter()
        .map(|&b| caret_x_for_byte_offset(&clusters, &input.text, b))
        .collect();
    assert_eq!(xs[0], 0.0);
    assert_eq!(xs[2], clusters[1].leading_edge());
    assert!((xs[4] - line_w).abs() < 0.01);
    // Hidden continuation columns share the wide char's trailing edge.
    assert_eq!(xs[1], xs[2]);
}

#[test]
fn per_cell_colors_reach_shaped_glyphs() {
    // The renderer's rich-text spans must carry each cell's fg color through
    // shaping (glyph.color_opt), so syntax colors survive run shaping.
    let red = cosmic_text::Color::rgba(255, 0, 0, 255).0;
    let blue = cosmic_text::Color::rgba(0, 0, 255, 255).0;
    let cells = vec![cell('a', TermColor::Red), cell('b', TermColor::Blue)];
    let input = build_shaped_line(&cells);
    assert_eq!(input.spans.len(), 2);
    assert_eq!(input.spans[0].color, red);
    assert_eq!(input.spans[1].color, blue);

    let mut font_system = FontSystem::new();
    let mut buffer = Buffer::new(&mut font_system, Metrics::new(16.0, 19.2));
    let default_attrs = Attrs::new().family(Family::Monospace);
    buffer.set_rich_text(
        input.spans.iter().map(|span| {
            (
                &input.text[span.start..span.end],
                Attrs::new()
                    .family(Family::Monospace)
                    .color(cosmic_text::Color(span.color)),
            )
        }),
        &default_attrs,
        Shaping::Advanced,
        Some(Align::Left),
    );
    buffer.shape_until_scroll(&mut font_system, false);
    let glyphs: Vec<_> = buffer
        .layout_runs()
        .flat_map(|run| run.glyphs.iter().map(|g| g.color_opt))
        .collect();
    assert_eq!(glyphs.len(), 2);
    assert_eq!(glyphs[0], Some(cosmic_text::Color(red)));
    assert_eq!(glyphs[1], Some(cosmic_text::Color(blue)));
}
