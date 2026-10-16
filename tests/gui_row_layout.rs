//! Row-layout invariants against the demo corpus.
//!
//! The GUI's row layout partitions chrome (gutter, scrollbar) from text
//! before shaping, so no script's bidi or cluster behavior can move the
//! line numbers, displace the gutter, or drag the scrollbar glyph into
//! the text. This test shapes every demo-corpus sample row through the
//! real [`layout_row`] and asserts the invariant: gutter glyphs stay on
//! their grid columns, the scrollbar glyph stays on its column, and no
//! glyph's origin leaves the row's grid. A violating row fails CI.

use cosmic_text::{Buffer, FontSystem, Metrics};
use erax::core::geometry::GridMetrics;
use erax::core::utf8::layout_grapheme_width;
use erax::gui::core::renderer::layout_row;
use erax::terminal::display::{Cell, CellRole, Color as TermColor};
use serde_json::Value;
use std::path::PathBuf;
use unicode_segmentation::UnicodeSegmentation;

/// Build one screen row: a 2-cell gutter, the sample text, one
/// scrollbar cell. Mirrors the TUI's cell filling (wide chars get a
/// hidden continuation cell carrying the cluster background).
fn row_cells_for(text: &str) -> Vec<Cell> {
    let mut cells = Vec::new();
    for ch in ['1', '│'] {
        cells.push(Cell::with_role(
            ch,
            TermColor::Rgb {
                r: 125,
                g: 133,
                b: 144,
            },
            TermColor::Reset,
            CellRole::Gutter,
        ));
    }
    for grapheme in text.graphemes(true) {
        let w = layout_grapheme_width(grapheme);
        if w == 0 {
            continue;
        }
        cells.push(Cell::with_text(
            grapheme,
            TermColor::White,
            TermColor::Reset,
            CellRole::EditorText,
        ));
        for _ in 1..w {
            cells.push(Cell::hidden_with_bg(TermColor::Reset));
        }
    }
    cells.push(Cell::with_role(
        '┃',
        TermColor::Rgb {
            r: 125,
            g: 133,
            b: 144,
        },
        TermColor::Reset,
        CellRole::Scrollbar,
    ));
    cells
}

fn load_texts() -> Vec<(String, String)> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let raw = std::fs::read_to_string(root.join("tests/fixtures/demo-expectations.json"))
        .expect("demo-expectations.json is readable");
    let doc: Value = serde_json::from_str(&raw).expect("sidecar is valid JSON");
    doc["entries"]
        .as_array()
        .expect("sidecar has an entries array")
        .iter()
        .map(|e| {
            (
                e["section"].as_str().expect("section").to_owned(),
                e["text"].as_str().expect("text").to_owned(),
            )
        })
        .collect()
}

#[test]
fn corpus_rows_keep_chrome_on_its_columns() {
    let mut font_system = FontSystem::new();
    let mut shape_buffer = Buffer::new(&mut font_system, Metrics::new(16.0, 20.0));
    let cell_w = 10.0f32;
    let cell_h = 20.0f32;

    let mut failures = Vec::new();
    for (section, text) in load_texts() {
        let cells = row_cells_for(&text);
        let cols = cells.len();
        // The grid exactly fits the row plus centering slack.
        let metrics = GridMetrics::new(
            cell_w,
            cell_h,
            cols as f32 * cell_w + 8.0,
            cell_h + 8.0,
        );
        let origin_x = metrics.offset_x;
        let (glyphs, _carets) = layout_row(
            &cells,
            0.0,
            16.0,
            &metrics,
            &mut font_system,
            &mut shape_buffer,
            None,
        );

        let grid_left = origin_x;
        let grid_right = origin_x + cols as f32 * cell_w;

        // Every glyph's clip rectangle stays inside the row's grid: the
        // fragment shader discards outside it, so no painted pixel can
        // leave the grid even when the shaper's natural width exceeds
        // the model's cell allocation (unligated ZWJ sequences).
        for g in &glyphs {
            let [x0, y0, x1, y1] = g.clip;
            if x0 < grid_left - 1.0 || x1 > grid_right + 1.0 || x1 < x0 || y1 < y0 {
                failures.push(format!(
                    "[{section}] glyph clip [{x0}, {y0}, {x1}, {y1}] leaves the grid [{grid_left}, {grid_right}] (row text: {text:?})"
                ));
            }
            // The glyph origin itself stays near the grid: a glyph placed
            // at a wildly wrong column (combining-mark pileup, bidi
            // displacement) fails here even if its clip is contained.
            // Unligated sequences (ZWJ emoji) legitimately overflow the
            // model's cell allocation — the clip contains their paint —
            // so two cells of slack are allowed for shaper reality.
            let gx = g.x as f32;
            if gx < grid_left - 2.0 * cell_w || gx > grid_right + 2.0 * cell_w {
                failures.push(format!(
                    "[{section}] glyph origin x={gx} leaves the grid [{grid_left}, {grid_right}] (row text: {text:?})"
                ));
            }
        }

        // The gutter digits/border stay on the gutter columns: a glyph
        // whose origin falls within the column's cell (left edge plus
        // one cell width). The origin is the pen position, so it sits
        // near the column's left edge, not its center.
        for (c, ch) in ['1', '│'].iter().enumerate() {
            let col_left = origin_x + c as f32 * cell_w;
            let found = glyphs.iter().any(|g| {
                let gx = g.x as f32;
                gx >= col_left - 1.0 && gx < col_left + cell_w
            });
            if !found {
                failures.push(format!(
                    "[{section}] gutter char {ch:?} missing from column {c} (row text: {text:?})"
                ));
            }
        }

        // The scrollbar glyph stays on the last column.
        let bar_left = origin_x + (cols as f32 - 1.0) * cell_w;
        if !glyphs.iter().any(|g| {
            let gx = g.x as f32;
            gx >= bar_left - 1.0 && gx < bar_left + cell_w
        }) {
            failures.push(format!(
                "[{section}] scrollbar glyph missing from last column (row text: {text:?})"
            ));
        }
    }

    assert!(
        failures.is_empty(),
        "row-layout invariant violations:\n{}",
        failures.join("\n")
    );
}
