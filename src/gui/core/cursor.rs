//! Cursor presentation geometry for the GUI/WebUI (dumb passthrough).
//!
//! The backend owns cursor semantics: it stamps the cursor cell with
//! [`CellRole::Cursor`], styles it with the theme's cursor colors, and ships
//! [`CursorState`] (position, visibility, *shape*) per frame. This module
//! turns that state into draw geometry and DOM anchor positions. Nothing
//! here knows about input models or modes — shapes come from the backend
//! verbatim.
//!
//! Rendering contract (mirrors the TUI):
//! - Block/Default: no overlay — the cursor cell's backend-styled colors
//!   already form the block, exactly like the TUI's hardware cursor over
//!   its styled cell.
//! - Bar / Underline / HollowBlock: a thin overlay in the cursor cell's
//!   foreground color (the theme's `cursor_fg`), drawn over the cell.

use crate::config::cursor_style::CursorStyle;

/// A solid rectangle in physical pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PxRect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

/// Grid geometry for cursor overlay computation (all physical pixels).
#[derive(Debug, Clone, Copy)]
pub struct CursorGeometry {
    /// Physical x of the grid's left edge (`text_origin_x`).
    pub origin_x: f32,
    /// Physical y of the cursor row's top (from `GridMetrics::row_y`).
    pub row_y: f32,
    pub cell_w: f32,
    pub cell_h: f32,
    pub cols: u16,
    pub rows: u16,
    /// Device pixel ratio (overlay thickness scales with it).
    pub dpr: f32,
}

/// Overlay rectangles for the cursor, in physical pixels.
///
/// - `col`, `row`: the cursor cell.
/// - `geo`: grid geometry (physical pixels).
///
/// Returns an empty vec for [`CursorStyle::Default`] /
/// [`CursorStyle::SteadyBlock`] (the styled cell is the block) and for
/// out-of-grid positions or degenerate metrics.
pub fn cursor_overlay_rects(
    col: u16,
    row: u16,
    geo: CursorGeometry,
    style: CursorStyle,
) -> Vec<PxRect> {
    if col >= geo.cols
        || row >= geo.rows
        || geo.cell_w <= 0.0
        || geo.cell_h <= 0.0
    {
        return Vec::new();
    }
    let x = geo.origin_x + f32::from(col) * geo.cell_w;
    // Overlay thickness: ~2 device pixels, never sub-pixel, never chunky.
    let t = (2.0 * geo.dpr.max(1.0)).round().max(2.0);
    let (cell_w, cell_h, row_y) = (geo.cell_w, geo.cell_h, geo.row_y);
    match style {
        CursorStyle::Default | CursorStyle::SteadyBlock => Vec::new(),
        CursorStyle::SteadyBar => vec![PxRect {
            x,
            y: row_y,
            w: t.min(cell_w),
            h: cell_h,
        }],
        CursorStyle::SteadyUnderline => vec![PxRect {
            x,
            y: row_y + cell_h - t,
            w: cell_w,
            h: t,
        }],
        CursorStyle::HollowBlock => vec![
            PxRect { x, y: row_y, w: cell_w, h: t },                       // top
            PxRect { x, y: row_y + cell_h - t, w: cell_w, h: t },           // bottom
            PxRect { x, y: row_y + t, w: t, h: (cell_h - 2.0 * t).max(0.0) }, // left
            PxRect { x: x + cell_w - t, y: row_y + t, w: t, h: (cell_h - 2.0 * t).max(0.0) }, // right
        ],
    }
}

/// CSS-pixel anchor for the hidden IME textarea: the cursor cell's top-left
/// corner in page coordinates.
///
/// The textarea is positioned here (1px, transparent) and focused so the
/// OS IME opens its composition popup at the text cursor — this is what
/// makes CJK composition land in the right place. When the cursor is
/// hidden the caller should park the textarea offscreen instead.
pub fn ime_anchor_css(
    cursor_x: u16,
    cursor_y: u16,
    canvas_left_css: f64,
    canvas_top_css: f64,
    cell_w_css: f64,
    cell_h_css: f64,
) -> (f64, f64) {
    (
        canvas_left_css + f64::from(cursor_x) * cell_w_css,
        canvas_top_css + f64::from(cursor_y) * cell_h_css,
    )
}

/// Per-frame IME anchor decision from the backend's cursor state.
///
/// Visible: the anchor follows the cursor cell exactly (nothing inferred —
/// x/y come from the frame). Hidden: the textarea parks offscreen at
/// (-100, -100) so no IME popup can appear away from the text cursor.
/// This is the client's whole frame-application logic for the cursor
/// besides the overlay rects above; the wasm shell calls it verbatim.
pub fn ime_anchor_for_frame(
    visible: bool,
    cursor_x: u16,
    cursor_y: u16,
    canvas_left_css: f64,
    canvas_top_css: f64,
    cell_w_css: f64,
    cell_h_css: f64,
) -> (f64, f64) {
    if visible {
        ime_anchor_css(
            cursor_x,
            cursor_y,
            canvas_left_css,
            canvas_top_css,
            cell_w_css,
            cell_h_css,
        )
    } else {
        (-100.0, -100.0)
    }
}

/// Whether a Screen frame's `seq` indicates a dropped frame.
///
/// Frames are contiguous per connection starting at 1; `last_seq == 0`
/// is the pre-first-frame state (never a gap). A gap means the client
/// rendered a stale grid for a frame — warn, don't die.
pub fn seq_gap_detected(last_seq: u64, seq: u64) -> bool {
    last_seq != 0 && seq != last_seq + 1
}

#[cfg(test)]
mod tests {
    use super::*;

    const COLS: u16 = 80;
    const ROWS: u16 = 24;

    fn geo() -> CursorGeometry {
        CursorGeometry {
            origin_x: 0.0,
            row_y: 0.0,
            cell_w: 10.0,
            cell_h: 20.0,
            cols: COLS,
            rows: ROWS,
            dpr: 1.0,
        }
    }

    #[test]
    fn block_styles_need_no_overlay() {
        for style in [CursorStyle::Default, CursorStyle::SteadyBlock] {
            let rects = cursor_overlay_rects(5, 5, geo(), style);
            assert!(rects.is_empty(), "style {style:?} must not overlay");
        }
    }

    #[test]
    fn bar_is_left_edge_vertical() {
        let mut g = geo();
        g.origin_x = 8.0;
        g.row_y = 40.0;
        let rects = cursor_overlay_rects(3, 2, g, CursorStyle::SteadyBar);
        assert_eq!(rects.len(), 1);
        let r = rects[0];
        assert_eq!((r.x, r.y, r.h), (8.0 + 3.0 * 10.0, 40.0, 20.0));
        assert!(r.w >= 2.0 && r.w <= 10.0, "bar width sane: {}", r.w);
    }

    #[test]
    fn underline_is_bottom_edge_horizontal() {
        let rects = cursor_overlay_rects(0, 0, geo(), CursorStyle::SteadyUnderline);
        assert_eq!(rects.len(), 1);
        let r = rects[0];
        assert_eq!((r.x, r.w), (0.0, 10.0));
        assert!((r.y + r.h - 20.0).abs() < 0.01, "sits on the bottom edge");
        assert!(r.h >= 2.0);
    }

    #[test]
    fn hollow_is_four_sided_outline() {
        let mut g = geo();
        g.origin_x = 0.0;
        g.row_y = 20.0;
        let rects = cursor_overlay_rects(1, 1, g, CursorStyle::HollowBlock);
        assert_eq!(rects.len(), 4);
        // Top and bottom span the full width; left/right sit between them.
        assert_eq!((rects[0].x, rects[0].w), (10.0, 10.0));
        assert_eq!((rects[1].x, rects[1].w), (10.0, 10.0));
        assert!((rects[0].y - 20.0).abs() < 0.01);
        assert!((rects[1].y + rects[1].h - 40.0).abs() < 0.01);
    }

    #[test]
    fn out_of_grid_yields_nothing() {
        for (col, row) in [(80, 0), (0, 24), (200, 200)] {
            let rects = cursor_overlay_rects(col, row, geo(), CursorStyle::SteadyBar);
            assert!(rects.is_empty(), "({col},{row}) out of grid");
        }
    }

    #[test]
    fn degenerate_metrics_yield_nothing() {
        let mut g = geo();
        g.cell_w = 0.0;
        let rects = cursor_overlay_rects(0, 0, g, CursorStyle::SteadyBar);
        assert!(rects.is_empty());
    }

    #[test]
    fn thickness_scales_with_dpr() {
        let g1 = geo();
        let mut g2 = geo();
        g2.dpr = 2.0;
        let lo = cursor_overlay_rects(0, 0, g1, CursorStyle::SteadyBar);
        let hi = cursor_overlay_rects(0, 0, g2, CursorStyle::SteadyBar);
        assert!(hi[0].w > lo[0].w, "dpr 2 bar wider than dpr 1");
        assert!(hi[0].w <= 10.0, "still within the cell");
    }

    #[test]
    fn ime_anchor_is_cell_top_left_in_page_coords() {
        let (left, top) = ime_anchor_css(5, 3, 100.0, 50.0, 9.5, 19.0);
        assert!((left - (100.0 + 5.0 * 9.5)).abs() < 1e-9);
        assert!((top - (50.0 + 3.0 * 19.0)).abs() < 1e-9);
    }

    #[test]
    fn ime_anchor_origin_cell() {
        assert_eq!(ime_anchor_css(0, 0, 100.0, 50.0, 9.5, 19.0), (100.0, 50.0));
    }

    #[test]
    fn frame_anchor_follows_visible_cursor() {
        // The anchor tracks the backend cursor cell exactly.
        let (l1, t1) = ime_anchor_for_frame(true, 5, 3, 100.0, 50.0, 9.5, 19.0);
        let (l2, t2) = ime_anchor_for_frame(true, 6, 3, 100.0, 50.0, 9.5, 19.0);
        assert!((l2 - l1 - 9.5).abs() < 1e-9, "one cell right = one cell width");
        assert!((t2 - t1).abs() < 1e-9, "same row = same top");
        // Cursor state applied: a new frame's position replaces the old.
        let (l3, t3) = ime_anchor_for_frame(true, 0, 0, 100.0, 50.0, 9.5, 19.0);
        assert_eq!((l3, t3), (100.0, 50.0));
    }

    #[test]
    fn frame_anchor_parks_when_hidden() {
        // Hidden cursor: offscreen, never at a stale cursor position.
        assert_eq!(
            ime_anchor_for_frame(false, 5, 3, 100.0, 50.0, 9.5, 19.0),
            (-100.0, -100.0)
        );
        assert_eq!(
            ime_anchor_for_frame(false, 0, 0, 100.0, 50.0, 9.5, 19.0),
            (-100.0, -100.0)
        );
    }

    #[test]
    fn seq_gap_first_frame_never_gaps() {
        assert!(!seq_gap_detected(0, 1));
        assert!(!seq_gap_detected(0, 999));
    }

    #[test]
    fn seq_gap_contiguous_ok() {
        assert!(!seq_gap_detected(41, 42));
    }

    #[test]
    fn seq_gap_skips_and_repeats_flagged() {
        assert!(seq_gap_detected(41, 43));
        assert!(seq_gap_detected(41, 41));
        assert!(seq_gap_detected(41, 7));
    }
}
