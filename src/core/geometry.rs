//! Unified grid-geometry utilities.
//!
//! This module provides a single source of truth for grid↔pixel coordinate math,
//! used by both the GUI renderer and TUI input handling.

/// Metrics for grid-based rendering (cell dimensions in pixels).
///
/// `cell_width` and `cell_height` are whole pixels: [`GridMetrics::new`]
/// snaps them once, at derivation, and every consumer (glyph origins,
/// background cells, pixel-to-cell tests) derives from these integers,
/// so fractional drift cannot accumulate across rows or frames.
#[derive(Debug, Clone, Copy)]
// Used by gui/grid_renderer for unified grid↔pixel coordinate math
pub struct GridMetrics {
    /// Width of a single cell in pixels (whole)
    pub cell_width: f32,
    /// Height of a single cell in pixels (whole)
    pub cell_height: f32,
    /// Total width of the viewport in pixels
    pub width_px: f32,
    /// Total height of the viewport in pixels
    pub height_px: f32,
    /// Horizontal offset from left edge (for centering)
    pub offset_x: f32,
    /// Vertical offset from top edge (for centering)
    pub offset_y: f32,
}

/// Minimum padding around the grid (prevents text touching edges)
pub const MIN_PADDING: f32 = 4.0;

impl GridMetrics {
    /// Create new metrics from cell and viewport dimensions.
    ///
    /// The cell dimensions snap to whole pixels exactly once, here; the
    /// sub-pixel remainder is absorbed at the window edge by the
    /// centered offset.
    pub fn new(cell_width: f32, cell_height: f32, width_px: f32, height_px: f32) -> Self {
        let cell_width = cell_width.round().max(1.0);
        let cell_height = cell_height.round().max(1.0);
        let (cols, rows) =
            Self::compute_grid_dimensions_static(width_px, height_px, cell_width, cell_height);
        let (offset_x, offset_y) = Self::compute_centered_offset_static(
            width_px,
            height_px,
            cols,
            rows,
            cell_width,
            cell_height,
        );

        Self {
            cell_width,
            cell_height,
            width_px,
            height_px,
            offset_x,
            offset_y,
        }
    }

    /// Convert pixel coordinates to grid coordinates
    ///
    /// # Arguments
    /// * `x` - X pixel coordinate
    /// * `y` - Y pixel coordinate
    ///
    /// # Returns
    /// Grid coordinates (col, row), or None if outside the grid area
    pub fn px_to_grid(&self, x: f32, y: f32) -> Option<(usize, usize)> {
        // Check if within grid bounds (accounting for offset)
        if x < self.offset_x || y < self.offset_y {
            return None;
        }

        let col = ((x - self.offset_x) / self.cell_width) as usize;
        let row = ((y - self.offset_y) / self.cell_height) as usize;

        let (max_cols, max_rows) = self.grid_dimensions();
        if col < max_cols as usize && row < max_rows as usize {
            Some((col, row))
        } else {
            None
        }
    }

    /// Get grid dimensions in cells (columns, rows)
    pub fn grid_dimensions(&self) -> (u32, u32) {
        Self::compute_grid_dimensions_static(
            self.width_px,
            self.height_px,
            self.cell_width,
            self.cell_height,
        )
    }

    /// Pixel coordinates clamped onto the grid: the same derivation as
    /// [`GridMetrics::px_to_grid`], but out-of-grid points (scrollbar
    /// strips, status line, window chrome) map to the nearest grid cell
    /// instead of `None`. Scroll events use this so a wheel over the
    /// strips scrolls the adjacent pane instead of being misattributed
    /// to cell (0, 0).
    pub fn clamp_to_grid(&self, x: f32, y: f32) -> (usize, usize) {
        let (max_cols, max_rows) = self.grid_dimensions();
        let max_cols = max_cols.max(1) as usize;
        let max_rows = max_rows.max(1) as usize;
        let col = ((x - self.offset_x) / self.cell_width) as usize;
        let row = ((y - self.offset_y) / self.cell_height) as usize;
        (col.min(max_cols - 1), row.min(max_rows - 1))
    }

    /// Pixel y of the top edge of grid row `row`.
    ///
    /// Derived from the metrics, never from a pixel-sized cache table:
    /// a grid row count that outgrows the viewport (oversized resize,
    /// sub-pixel cell heights) cannot index past anything. The cell
    /// height is whole by construction, so no rounding is needed.
    pub fn row_y(&self, row: usize) -> f32 {
        self.offset_y + row as f32 * self.cell_height
    }

    /// Compute grid dimensions (static version for use before construction)
    fn compute_grid_dimensions_static(
        width: f32,
        height: f32,
        cell_w: f32,
        cell_h: f32,
    ) -> (u32, u32) {
        let usable_width = (width - MIN_PADDING * 2.0).max(0.0);
        let usable_height = (height - MIN_PADDING * 2.0).max(0.0);
        let cols = (usable_width / cell_w).floor() as u32;
        let rows = (usable_height / cell_h).floor() as u32;
        (cols.max(1), rows.max(1))
    }

    /// Compute centered offset (static version for use before construction)
    fn compute_centered_offset_static(
        width: f32,
        height: f32,
        cols: u32,
        rows: u32,
        cell_w: f32,
        cell_h: f32,
    ) -> (f32, f32) {
        let grid_width = cols as f32 * cell_w;
        let grid_height = rows as f32 * cell_h;
        let offset_x = ((width - grid_width) / 2.0).floor().max(MIN_PADDING);
        let offset_y = ((height - grid_height) / 2.0).floor().max(MIN_PADDING);
        (offset_x, offset_y)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_px_to_grid() {
        let metrics = GridMetrics::new(10.0, 20.0, 800.0, 600.0);
        // Test that valid pixel coordinates map to grid
        let result = metrics.px_to_grid(50.0, 60.0);
        assert!(result.is_some());
    }

    #[test]
    fn cell_metrics_snap_to_whole_pixels_once() {
        // Fractional shaping advances become whole pixels at the
        // canonical derivation point; the remainder is absorbed by
        // the centered offset, not by the cells.
        let metrics = GridMetrics::new(10.6, 20.4, 808.0, 488.0);
        assert_eq!(metrics.cell_width, 11.0);
        assert_eq!(metrics.cell_height, 20.0);
        assert_eq!(metrics.cell_width.fract(), 0.0);
        assert_eq!(metrics.cell_height.fract(), 0.0);
        // Whole cells make row edges exact: no rounding needed.
        assert_eq!(metrics.row_y(3), metrics.offset_y + 3.0 * 20.0);
    }

    #[test]
    fn test_px_to_grid_outside() {
        let metrics = GridMetrics::new(10.0, 20.0, 100.0, 100.0);
        // Outside grid bounds
        assert!(metrics.px_to_grid(0.0, 0.0).is_none());
    }

    #[test]
    fn clamp_to_grid_pins_chrome_to_the_nearest_cell() {
        // Regression: the native shell's scroll handler mapped an
        // out-of-grid wheel position to (0, 0) via unwrap_or_default,
        // scrolling the top-left pane instead of the pane under the
        // cursor. Points past the grid edge (scrollbar strips, status
        // line, window chrome) clamp to the nearest cell.
        let metrics = GridMetrics::new(10.0, 20.0, 805.0, 600.0);
        let (cols, rows) = metrics.grid_dimensions();
        let (cols, rows) = (cols as usize, rows as usize);
        assert!(cols > 1 && rows > 1);

        // Far past the viewport: last cell, where px_to_grid reports None.
        let far = (10_000.0, 10_000.0);
        assert!(metrics.px_to_grid(far.0, far.1).is_none());
        assert_eq!(metrics.clamp_to_grid(far.0, far.1), (cols - 1, rows - 1));

        // Negative coordinates saturate to the origin, not wrap.
        assert_eq!(metrics.clamp_to_grid(-50.0, -50.0), (0, 0));

        // In-grid points agree with px_to_grid.
        let inside = (100.0, 100.0);
        assert_eq!(
            Some(metrics.clamp_to_grid(inside.0, inside.1)),
            metrics.px_to_grid(inside.0, inside.1)
        );
    }

    #[test]
    fn row_y_is_total_for_any_row() {
        // The renderer used to index a pixel-sized y table by grid row:
        // any grid taller than the viewport (oversized resize, sub-pixel
        // cells) panicked. The derived computation has no table to
        // overrun: it stays finite and monotonic far past it.
        let metrics = GridMetrics::new(10.0, 20.0, 800.0, 600.0);
        let mut prev = metrics.row_y(0);
        assert!(prev.is_finite());
        for row in [1usize, 29, 30, 31, 1000, 100_000] {
            let y = metrics.row_y(row);
            assert!(y.is_finite(), "row {row}");
            assert!(y >= prev, "row {row} went backwards: {y} < {prev}");
            prev = y;
        }
        // Same formula the table used, so rendered output is unchanged.
        let m2 = GridMetrics::new(8.0, 16.0, 800.0, 600.0);
        assert_eq!(m2.row_y(3), (m2.offset_y + 3.0 * 16.0).round());
    }
}

// =====================================================================
// Frame layout: one cell-geometry source for TUI, GUI, and WebUI.
// =====================================================================

use std::collections::HashMap;

use crate::core::id::WindowId;
use crate::core::layout::{LayoutNode, Rect};
#[cfg(not(target_arch = "wasm32"))]
use crate::core::window::Window;

/// Options for [`compute_frame`]: the cell grid plus frame-wide display
/// choices. All three frontends build this from their own size source
/// (terminal size, pixel-derived grid); the model owns everything below.
#[derive(Debug, Clone, Copy)]
pub struct FrameOptions {
    /// Frame width in cells.
    pub cols: usize,
    /// Frame height in cells.
    pub rows: usize,
    /// Rows reserved for the menu bar at the top (0 or 1).
    pub menu_rows: usize,
    /// Line numbers shown in the gutter.
    pub show_line_numbers: bool,
}

/// Per-window model inputs for [`compute_frame`]. Plain data in;
/// the module computes everything. [`EditorApp::geom_inputs`] fills
/// these from window and buffer state.
#[derive(Debug, Clone, Copy)]
pub struct WindowGeomInput {
    pub window: WindowId,
    pub scroll_offset: usize,
    pub scroll_x: usize,
    pub cursor_y: usize,
    pub visual_cursor_x: usize,
    pub line_count: usize,
    pub content_width: usize,
}

/// One scrollbar's derived geometry, in absolute cell coordinates.
/// The track is the arrow-free span; the thumb is a sub-range of it.
/// Renderers paint from this; none of them recompute positions.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScrollbarGeom {
    /// Cross-axis coordinate: x for a vertical bar, y for a horizontal one.
    pub cross: usize,
    /// First cell of the arrow-free track.
    pub track_start: usize,
    /// Track length in cells (arrows excluded).
    pub track_len: usize,
    /// Thumb offset within the track.
    pub thumb_start: usize,
    /// Thumb length in cells.
    pub thumb_len: usize,
}

impl ScrollbarGeom {
    /// First cell of the painted strip, arrows included when the
    /// config shows them. The single derivation site for the
    /// arrow-inclusive span; painters and hit-testing use this.
    pub fn strip_start(&self) -> usize {
        if crate::config::scrollbar::SHOW_ARROWS {
            self.track_start.saturating_sub(1)
        } else {
            self.track_start
        }
    }

    /// Painted strip length in cells, arrows included when configured.
    pub fn strip_len(&self) -> usize {
        if crate::config::scrollbar::SHOW_ARROWS {
            self.track_len + 2
        } else {
            self.track_len
        }
    }
}

/// One cell region of a pane frame, in absolute cell coordinates.
///
/// The pane's regions tile the pane frame exactly: every cell of the
/// frame belongs to exactly one region, so a renderer painting each
/// region with its owner's background can never leave an unpainted
/// cell. There is no representable "nobody owns this cell" state; a
/// region with no owner is a model bug, fixed in the model, never
/// swept after at render time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegionOwner {
    /// Editable text area (the gutter is a separate region).
    Text,
    /// Line-number gutter, `gutter_width` columns at the text rect's
    /// left (after the vertical strip when the bar is on the left).
    Gutter,
    /// Vertical scrollbar strip (only when the bar is present).
    VScrollbar,
    /// Horizontal scrollbar strip (only when the bar is present).
    HScrollbar,
    /// The pane's status row.
    Status,
}

/// One pane's derived geometry, in absolute cell coordinates.
///
/// `frame` is the whole pane (status row included). `text` is the
/// editable area: the gutter is a separate owned region to its left,
/// the status row is excluded, and each scrollbar strip is excluded
/// only while its bar is present (the presence law). A bar that is
/// absent owns no strip: its cells are text cells, painted by the text
/// renderer. The text rect therefore grows and shrinks as bars appear
/// and disappear; it never wastes a row or column on a bar that is not
/// there, and no bar ever paints over the text rect.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PaneGeometry {
    pub window: WindowId,
    /// Whole pane, status row included.
    pub frame: Rect,
    /// Text area: the gutter is a separate owned region to its left;
    /// the status row is excluded; scrollbar strips are excluded only
    /// while their bar is present.
    pub text: Rect,
    pub gutter_width: usize,
    /// Absolute y of the pane's status row.
    pub status_row: usize,
    /// Vertical scrollbar geometry, when shown.
    pub v_scrollbar: Option<ScrollbarGeom>,
    /// Horizontal scrollbar geometry, when shown.
    pub h_scrollbar: Option<ScrollbarGeom>,
    /// Absolute cell of the text cursor, clamped to the text rect. The
    /// cursor is `None` only when the text rect is empty; otherwise it
    /// is always inside the visible text area, even when the model
    /// cursor is scrolled out of view.
    pub cursor: Option<(usize, usize)>,
}

impl PaneGeometry {
    /// The pane's owned cell regions in a fixed order: text, gutter,
    /// vertical strip, horizontal strip, status row. A bar that is
    /// absent contributes no region — its cells are text cells then,
    /// which is exactly why no post-render sweep is needed: presence
    /// is computed, and the text rect owns whatever the bars do not
    /// take. The present regions tile `frame` exactly (pairwise
    /// disjoint, union equals the frame);
    /// `pane_regions_tile_frame_exactly` proves it over a sweep of
    /// frames and contents. Each region's owner paints its background
    /// as part of constructing the frame: text and gutter by the
    /// window renderer, strips by the scrollbar painters, the status
    /// row by the status renderer.
    pub fn owned_regions(&self) -> [Option<(RegionOwner, Rect)>; 5] {
        use crate::config::scrollbar::{VERTICAL_SIDE, VerticalSide};
        // The gutter sits after the vertical strip when the bar is on
        // the left; the strip exists only while the bar is present
        // (dynamic presence), so the gutter origin follows it. The
        // region clamps to the frame like the painter does, and stays
        // clear of a right-side vertical strip: a gutter wider than a
        // degenerate pane owns only the cells strictly left of it.
        // Vertical chrome spans the horizontal strip's row (the same
        // corner rule that gives the vertical bar the strip-corner
        // cell), so the gutter covers the strip row when the bar is
        // present.
        let v_present = self.v_scrollbar.is_some();
        let gutter_x =
            self.frame.x + usize::from(v_present && VERTICAL_SIDE == VerticalSide::Left);
        let gutter_end = (gutter_x + self.gutter_width)
            .min(self.frame.x + self.frame.width)
            .min(self.frame.x + self.frame.width - usize::from(v_present && VERTICAL_SIDE == VerticalSide::Right));
        let gutter_height = self
            .text
            .height
            .saturating_add(usize::from(self.h_scrollbar.is_some()));
        let gutter_region = (gutter_end > gutter_x).then_some((
            RegionOwner::Gutter,
            Rect::new(gutter_x, self.frame.y, gutter_end - gutter_x, gutter_height),
        ));
        let v_region = self.v_scrollbar.map(|bar| {
            (
                RegionOwner::VScrollbar,
                Rect::new(bar.cross, bar.strip_start(), 1, bar.strip_len()),
            )
        });
        let h_region = self.h_scrollbar.map(|bar| {
            (
                RegionOwner::HScrollbar,
                Rect::new(bar.strip_start(), bar.cross, bar.strip_len(), 1),
            )
        });
        // A zero-height pane owns nothing: it has no cells, so the
        // status region is vacuous and the union stays exact.
        let status_region = (self.frame.height >= 1).then_some((
            RegionOwner::Status,
            Rect::new(self.frame.x, self.status_row, self.frame.width, 1),
        ));
        [
            Some((RegionOwner::Text, self.text)),
            gutter_region,
            v_region,
            h_region,
            status_region,
        ]
    }
}

/// One frame's complete derived geometry.
///
/// Invariants, all by construction:
/// - The pane frames tile the editor area (below the menu rows) exactly:
///   no gaps, no overlaps, at any size including degenerate ones.
/// - Split remainder rule: the first pane takes floor(ratio * span);
///   the sibling takes the remainder. Nested splits tile the parent
///   rect exactly, because the second span is always span minus first.
/// - Degenerate spans are deterministic: a 0-cell span splits into two
///   0-cell panes; a 1-cell span at ratio 0.5 splits into (0, 1).
/// - MinimizeWindow seam: a collapsed pane is a leaf whose span
///   computes to zero size. The tiling handles zero-size leaves with
///   no special case, so the minimize command only needs to mark
///   panes collapsed in the layout tree; the geometry follows.
/// - Scrollbar presence law: each scrollbar owns a dedicated strip
///   outside the text rect (one column on the configured side, one row
///   above the status row) only while it is present. The text rect
///   surrenders exactly one column for a present vertical bar and
///   exactly one row for a present horizontal bar, nothing otherwise;
///   an absent bar owns no cells, so its strip row/column is text.
///   Presence is a pure fixed point of (frame size, content metrics)
///   (`scrollbar_presence`, same module): no oscillation, no
///   per-frontend geometry, and no bar ever paints over the text rect.
/// - Cell ownership: the pane's [`PaneGeometry::owned_regions`] tile
///   the pane frame exactly, and the pane frames tile the editor area
///   exactly, so every frame cell has exactly one owner. Each owner
///   paints its background as part of constructing the frame; there
///   is no unowned cell for a renderer to sweep after.
/// - The cursor is `Some` exactly when the text rect is non-empty, and
///   the cell is always inside the text rect. Out-of-viewport model
///   cursors clamp to the nearest viewport edge, never hide.
#[derive(Debug, Clone)]
pub struct FrameLayout {
    pub menu_rows: usize,
    pub cols: usize,
    pub rows: usize,
    pub panes: Vec<PaneGeometry>,
}

impl FrameLayout {
    /// The pane geometry for `window`, if it is in this frame.
    pub fn pane_for(&self, window: WindowId) -> Option<&PaneGeometry> {
        self.panes.iter().find(|p| p.window == window)
    }

    /// The pane whose frame contains cell (x, y), if any.
    pub fn hit_test(&self, x: usize, y: usize) -> Option<&PaneGeometry> {
        self.panes.iter().find(|p| {
            x >= p.frame.x
                && x < p.frame.x + p.frame.width
                && y >= p.frame.y
                && y < p.frame.y + p.frame.height
        })
    }

    /// Refresh the derived width/height cache on every window from the
    /// model. This is the single writer of those fields: split growth
    /// can no longer leave them stale. The contract is unchanged from
    /// the old resize-only writer (full frame width, frame height minus
    /// the status row), so consumers like paragraph reflow keep their
    /// exact behavior. Unavailable on wasm32: the web client is a dumb
    /// renderer with no window map to refresh.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn apply_window_dims(&self, windows: &mut HashMap<WindowId, Window>) {
        for pane in &self.panes {
            if let Some(window) = windows.get_mut(&pane.window) {
                window.set_dimensions(pane.frame.width, pane.frame.height.saturating_sub(1));
            }
        }
    }
}

/// Split point for one axis: the first pane takes floor(ratio * span),
/// the sibling takes the remainder. Integer result, deterministic for
/// every span including 0 and 1.
fn split_point(span: usize, ratio: f32) -> usize {
    (span as f32 * ratio) as usize
}

/// Gutter width for one pane: digits of the largest visible line number
/// plus one border column, or 0 without line numbers. Two-pass callers
/// pass the estimated text height first, then the final one.
fn gutter_width(show_line_numbers: bool, line_count: usize, max_visible_line: usize) -> usize {
    if !show_line_numbers {
        return 0;
    }
    let max_line_num = line_count.min(max_visible_line).max(1);
    // Digits via the same decimal rendering the painter emits.
    format!("{max_line_num}").len() + 1
}

/// Scrollbar thumb position and size within a track.
///
/// Returns (thumb_start, thumb_size) with thumb_start + thumb_size <=
/// track_size for track_size > 0. (A zero track is degenerate: the
/// function returns (0, 1) as a no-paint sentinel; callers guard the
/// track size before deriving.) The thumb is proportional to the
/// visible/total ratio, never smaller than 2 cells, and fills the track
/// when all content is visible.
pub fn thumb_range(
    visible: usize,
    total: usize,
    scroll_offset: usize,
    track_size: usize,
) -> (usize, usize) {
    if total == 0 || track_size == 0 {
        return (0, track_size.max(1));
    }

    // Minimum thumb size from the typed scrollbar config.
    let min_thumb_size = crate::config::scrollbar::THUMB_MIN_CELLS.max(1);

    // Calculate thumb size proportional to visible/total ratio
    let thumb_size = if total <= visible {
        track_size // Full track if all content is visible
    } else {
        (visible * track_size / total)
            .max(min_thumb_size)
            .min(track_size)
    };

    // Calculate thumb position
    let scrollable_range = total.saturating_sub(visible);
    let thumb_start = if scrollable_range == 0 {
        0
    } else {
        let available_track = track_size.saturating_sub(thumb_size);
        (scroll_offset * available_track)
            .checked_div(scrollable_range)
            .unwrap_or(0)
            .min(available_track)
    };

    (thumb_start, thumb_size)
}

/// Scrollbar presence as a pure fixed point of (frame extent, content
/// metrics). Returns (vertical, horizontal).
///
/// The vertical bar shrinks the width, which can create horizontal
/// need; the horizontal bar shrinks the height, which can create
/// vertical need. With the gutter fixed (see `pane_geometry`), each
/// need predicate is monotone non-decreasing in the shrinking rect: a
/// bar appearing can only add bars, never remove one. The width floor
/// for the horizontal bar is evaluated against the pre-bar width
/// (`avail_w`), not the post-vertical-bar width, for the same reason:
/// a bar appearing must not un-appear another. The state space is four
/// (v, h) pairs, so iterating from (false, false) converges in at most
/// two transitions and cannot oscillate between frames for boundary
/// content. The loop cap is a backstop; reaching it would be a logic
/// bug, not a longer search.
fn scrollbar_presence(
    avail_w: usize,
    avail_h: usize,
    vbar_rows: usize,
    line_count: usize,
    content_width: usize,
    scroll_x: usize,
) -> (bool, bool) {
    use crate::config::scrollbar::{SHOW_ARROWS, SHOW_WHEN_FITS};
    // Minimum pre-bar width that can host a horizontal bar: the bar is
    // suppressed on narrower panes (same thresholds the painter
    // needs, plus the column a vertical bar may take).
    let min_host_w = if SHOW_ARROWS { 6 } else { 4 };
    let (mut v, mut h) = (false, false);
    for _ in 0..4 {
        let text_w = avail_w.saturating_sub(usize::from(v));
        let text_h = avail_h.saturating_sub(usize::from(h));
        let v_next = vbar_rows >= 4 && (SHOW_WHEN_FITS || line_count > text_h);
        // The horizontal bar needs its own row distinct from the status
        // row: it appears only when at least one text row survives it.
        let h_next = avail_h >= 2
            && avail_w >= min_host_w
            && (content_width > text_w || scroll_x > 0);
        if (v_next, h_next) == (v, h) {
            return (v, h);
        }
        v = v_next;
        h = h_next;
    }
    (v, h)
}

/// Derive one pane's full geometry from its frame rect and model inputs.
///
/// Scrollbar presence is computed, never reserved: the text rect keeps
/// the full pane extent minus the status row, surrendering exactly one
/// column when the vertical bar is present and exactly one row when the
/// horizontal bar is present. A bar that is absent owns no strip — its
/// cells are text cells, painted by the text renderer — so a bar
/// appearing or disappearing moves the text rect by exactly its strip,
/// and no bar ever covers the cursor: the cursor clamps to the text
/// rect by construction (see `cursor_cell`).
///
/// The gutter resolves in a single pass from the pre-bar candidate
/// height (an overestimate of the visible lines, so the digit width
/// never underestimates the range the text rect can show). Fixing the
/// gutter before the presence iteration is what keeps the iteration
/// monotone (see `scrollbar_presence`).
fn pane_geometry(
    frame: Rect,
    input: &WindowGeomInput,
    show_line_numbers: bool,
) -> PaneGeometry {
    use crate::config::scrollbar::{SHOW_ARROWS, VERTICAL_SIDE, VerticalSide};

    let status_row = frame.y + frame.height.saturating_sub(1);
    // Rows available to text plus the horizontal bar; the status row is
    // the only permanent reservation in the frame.
    let avail_h = frame.height.saturating_sub(1);
    let gutter = gutter_width(
        show_line_numbers,
        input.line_count,
        input.scroll_offset + avail_h,
    );
    let avail_w = frame.width.saturating_sub(gutter);
    let vbar_left = VERTICAL_SIDE == VerticalSide::Left;
    // The vertical strip spans the pane minus the status row.
    let vbar_rows = frame.height.saturating_sub(1);

    let (v_present, h_present) = scrollbar_presence(
        avail_w,
        avail_h,
        vbar_rows,
        input.line_count,
        input.content_width,
        input.scroll_x,
    );

    let text_w = avail_w.saturating_sub(usize::from(v_present));
    let text_h = avail_h.saturating_sub(usize::from(h_present));
    // Degenerate panes (gutter wider than the frame) keep the text
    // rect inside the frame: the origin clamps, the size saturates.
    let text_x =
        (frame.x + gutter + usize::from(v_present && vbar_left)).min(frame.x + frame.width);
    let text = Rect::new(text_x, frame.y, text_w, text_h);

    let v_scrollbar = if v_present {
        let (track_start, track_len) = if SHOW_ARROWS {
            (frame.y + 1, vbar_rows.saturating_sub(2))
        } else {
            (frame.y, vbar_rows)
        };
        let (thumb_start, thumb_len) = thumb_range(
            text_h,
            input.line_count.max(1),
            input.scroll_offset,
            track_len,
        );
        Some(ScrollbarGeom {
            cross: if vbar_left {
                frame.x
            } else {
                frame.x + frame.width.saturating_sub(1)
            },
            track_start,
            track_len,
            thumb_start,
            thumb_len,
        })
    } else {
        None
    };

    let h_scrollbar = if h_present {
        let hscroll_row = status_row.saturating_sub(1);
        // The bar spans the text width; the corner cell under the
        // vertical strip belongs to the vertical bar.
        let (track_start, track_len) = if SHOW_ARROWS {
            (text.x + 1, text.width.saturating_sub(2))
        } else {
            (text.x, text.width)
        };
        let content = input.content_width.max(input.scroll_x + text.width);
        let (thumb_start, thumb_len) =
            thumb_range(text.width, content, input.scroll_x, track_len);
        Some(ScrollbarGeom {
            cross: hscroll_row,
            track_start,
            track_len,
            thumb_start,
            thumb_len,
        })
    } else {
        None
    };

    PaneGeometry {
        window: input.window,
        frame,
        text,
        gutter_width: gutter,
        status_row,
        v_scrollbar,
        h_scrollbar,
        cursor: cursor_cell(&text, input),
    }
}

/// The model cursor cell: `Some` exactly when the text rect is
/// non-empty and the model names a live cursor. The cell always lands
/// inside the text rect: a model cursor above/left of the viewport
/// clamps to the top/left edge, one below/right clamps to the
/// bottom/right edge. Unknown windows (cursor_y = MAX) and empty text
/// rects carry no cursor.
fn cursor_cell(text: &Rect, input: &WindowGeomInput) -> Option<(usize, usize)> {
    if text.width == 0 || text.height == 0 {
        return None;
    }
    // Unknown windows (no live model cursor) carry cursor_y=MAX; they
    // have no cursor to clamp.
    if input.cursor_y == usize::MAX {
        return None;
    }
    // Clamp to the text region: the cursor never leaves the visible
    // viewport, even when the model position is scrolled out of view.
    // The saturating_sub handles cursor above/left of the viewport;
    // the min handles cursor below/right of the viewport.
    let rel_y = input
        .cursor_y
        .saturating_sub(input.scroll_offset)
        .min(text.height - 1);
    let rel_x = input
        .visual_cursor_x
        .saturating_sub(input.scroll_x)
        .min(text.width - 1);
    Some((text.x + rel_x, text.y + rel_y))
}

/// Input for a layout leaf with no live window: the pane tiles (the
/// frame stays total) but carries no cursor and no content.
fn unknown_window_input(window: WindowId) -> WindowGeomInput {
    WindowGeomInput {
        window,
        scroll_offset: 0,
        scroll_x: 0,
        cursor_y: usize::MAX,
        visual_cursor_x: 0,
        line_count: 0,
        content_width: 0,
    }
}

/// Tile one layout node into `out`, honoring the remainder rule at
/// every level so nested splits tile their parent exactly.
fn tile_node(
    node: &LayoutNode,
    area: Rect,
    inputs: &HashMap<WindowId, WindowGeomInput>,
    show_line_numbers: bool,
    out: &mut Vec<PaneGeometry>,
) {
    match node {
        LayoutNode::Leaf(id) => {
            let input = inputs.get(id).copied().unwrap_or_else(|| unknown_window_input(*id));
            out.push(pane_geometry(area, &input, show_line_numbers));
        }
        LayoutNode::HSplit {
            left,
            right,
            split_ratio,
        } => {
            // Clamped to the span: the sibling's rect can never go
            // negative, whatever ratio the layout tree carries.
            let first = split_point(area.width, *split_ratio).min(area.width);
            tile_node(
                left,
                Rect::new(area.x, area.y, first, area.height),
                inputs,
                show_line_numbers,
                out,
            );
            tile_node(
                right,
                Rect::new(area.x + first, area.y, area.width - first, area.height),
                inputs,
                show_line_numbers,
                out,
            );
        }
        LayoutNode::VSplit {
            top,
            bottom,
            split_ratio,
        } => {
            let first = split_point(area.height, *split_ratio).min(area.height);
            tile_node(
                top,
                Rect::new(area.x, area.y, area.width, first),
                inputs,
                show_line_numbers,
                out,
            );
            tile_node(
                bottom,
                Rect::new(area.x, area.y + first, area.width, area.height - first),
                inputs,
                show_line_numbers,
                out,
            );
        }
    }
}

/// Compute one frame's complete geometry: the single source every
/// frontend consumes. Inputs are plain model data; outputs are absolute
/// cell coordinates. No frontend recomputes any of this.
pub fn compute_frame(
    options: FrameOptions,
    layout: &LayoutNode,
    inputs: &[WindowGeomInput],
) -> FrameLayout {
    let by_window: HashMap<WindowId, WindowGeomInput> =
        inputs.iter().map(|i| (i.window, *i)).collect();
    let editor = Rect::new(
        0,
        options.menu_rows,
        options.cols,
        options.rows.saturating_sub(options.menu_rows),
    );
    let mut panes = Vec::new();
    tile_node(
        layout,
        editor,
        &by_window,
        options.show_line_numbers,
        &mut panes,
    );
    FrameLayout {
        menu_rows: options.menu_rows,
        cols: options.cols,
        rows: options.rows,
        panes,
    }
}

#[cfg(test)]
mod frame_tests {
    use super::*;
    use crate::core::id::WindowId;

    fn input_for(window: usize) -> WindowGeomInput {
        WindowGeomInput {
            window: WindowId(window),
            scroll_offset: 0,
            scroll_x: 0,
            cursor_y: 0,
            visual_cursor_x: 0,
            line_count: 20,
            content_width: 40,
        }
    }

    fn leaf(id: usize) -> LayoutNode {
        LayoutNode::Leaf(WindowId(id))
    }

    fn trees() -> Vec<LayoutNode> {
        let mut t = leaf(0);
        t.split_window(WindowId(0), WindowId(1), crate::core::layout::SplitDirection::Vertical);
        let mut nested = leaf(0);
        nested.split_window(WindowId(0), WindowId(1), crate::core::layout::SplitDirection::Vertical);
        nested.split_window(WindowId(0), WindowId(2), crate::core::layout::SplitDirection::Horizontal);
        let mut deep = leaf(0);
        deep.split_window(WindowId(0), WindowId(1), crate::core::layout::SplitDirection::Horizontal);
        deep.split_window(WindowId(1), WindowId(2), crate::core::layout::SplitDirection::Vertical);
        deep.split_window(WindowId(2), WindowId(3), crate::core::layout::SplitDirection::Horizontal);
        vec![leaf(0), t, nested, deep]
    }

    fn inputs_for(tree: &LayoutNode) -> Vec<WindowGeomInput> {
        tree.window_ids()
            .iter()
            .map(|id| input_for(id.0))
            .collect()
    }

    /// Every cell of the editor area belongs to exactly one pane frame,
    /// and every pane frame lies inside the editor area.
    fn assert_tiling(frame: &FrameLayout) {
        let editor_y = frame.menu_rows;
        let editor_h = frame.rows.saturating_sub(editor_y);
        for pane in &frame.panes {
            assert!(pane.frame.x + pane.frame.width <= frame.cols, "pane overflows cols");
            assert!(
                pane.frame.y >= editor_y
                    && pane.frame.y + pane.frame.height <= editor_y + editor_h,
                "pane outside editor area"
            );
            assert!(
                pane.text.x + pane.text.width <= pane.frame.x + pane.frame.width,
                "text rect overflows pane frame"
            );
            assert!(
                pane.text.y + pane.text.height <= pane.frame.y + pane.frame.height,
                "text rect overflows pane frame"
            );
            assert_eq!(
                pane.status_row,
                pane.frame.y + pane.frame.height.saturating_sub(1),
                "status row is the pane's last row"
            );
            if let Some((cx, cy)) = pane.cursor {
                assert!(
                    cx >= pane.text.x
                        && cx < pane.text.x + pane.text.width
                        && cy >= pane.text.y
                        && cy < pane.text.y + pane.text.height,
                    "cursor outside its text rect"
                );
            }
            for bar in pane.v_scrollbar.iter().chain(pane.h_scrollbar.iter()) {
                assert!(
                    bar.thumb_start + bar.thumb_len <= bar.track_len,
                    "thumb overflows its track"
                );
                assert!(bar.thumb_len >= 1, "thumb must exist");
            }
        }
        // Pairwise: no two pane frames overlap.
        for (i, a) in frame.panes.iter().enumerate() {
            for b in frame.panes.iter().skip(i + 1) {
                let overlap_x = a.frame.x < b.frame.x + b.frame.width
                    && b.frame.x < a.frame.x + a.frame.width;
                let overlap_y = a.frame.y < b.frame.y + b.frame.height
                    && b.frame.y < a.frame.y + a.frame.height;
                assert!(!(overlap_x && overlap_y), "pane frames overlap");
            }
        }
        // Area: the frames exactly cover the editor area.
        let covered: usize = frame.panes.iter().map(|p| p.frame.width * p.frame.height).sum();
        assert_eq!(
            covered,
            frame.cols * editor_h,
            "pane frames must tile the editor area exactly"
        );
    }

    #[test]
    fn remainder_rule_is_deterministic() {
        assert_eq!(split_point(7, 0.5), 3);
        assert_eq!(split_point(1, 0.5), 0);
        assert_eq!(split_point(0, 0.5), 0);
        assert_eq!(split_point(80, 0.5), 40);
        assert_eq!(split_point(80, 0.1), 8);
        // Sibling always takes the remainder: nested splits tile exactly.
        for span in 0..=40usize {
            for ratio in [0.1f32, 0.25, 0.5, 0.75, 0.9] {
                let first = split_point(span, ratio);
                assert_eq!(first + (span - first), span);
            }
        }
    }

    #[test]
    fn frame_tiles_at_every_size() {
        let cols = [0usize, 1, 2, 3, 5, 40, 79, 80, 81, 200];
        let rows = [0usize, 1, 2, 3, 10, 23, 24, 25, 100];
        for tree in trees() {
            let inputs = inputs_for(&tree);
            for &c in &cols {
                for &r in &rows {
                    for menu_rows in [0usize, 1] {
                        for show_line_numbers in [false, true] {
                            let frame = compute_frame(
                                FrameOptions {
                                    cols: c,
                                    rows: r,
                                    menu_rows,
                                    show_line_numbers,
                                },
                                &tree,
                                &inputs,
                            );
                            assert_tiling(&frame);
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn cursor_clamped_outside_viewport_to_text_rect() {
        let tree = leaf(0);
        let opts = FrameOptions {
            cols: 80,
            rows: 24,
            menu_rows: 0,
            show_line_numbers: true,
        };
        let mut base = input_for(0);
        base.line_count = 100;
        // Above the viewport: clamps to top edge.
        let mut above = base;
        above.cursor_y = 0;
        above.scroll_offset = 50;
        let f = compute_frame(opts, &tree, &[above]);
        let (cx, cy) = f.panes[0].cursor.expect("cursor must clamp, not hide");
        let text = f.panes[0].text;
        assert_eq!(cy, text.y, "cursor above viewport clamps to top");
        assert!(cx >= text.x && cx < text.x + text.width);
        // Below the viewport: clamps to bottom edge.
        let mut below = base;
        below.cursor_y = 99;
        below.scroll_offset = 0;
        let f = compute_frame(opts, &tree, &[below]);
        let (cx, cy) = f.panes[0].cursor.expect("cursor must clamp, not hide");
        let text = f.panes[0].text;
        assert_eq!(cy, text.y + text.height - 1, "cursor below viewport clamps to bottom");
        assert!(cx >= text.x && cx < text.x + text.width);
        // Past the right edge: clamps to right edge.
        let mut wide = base;
        wide.cursor_y = 0;
        wide.visual_cursor_x = 500;
        wide.scroll_x = 0;
        let f = compute_frame(opts, &tree, &[wide]);
        let (cx, _) = f.panes[0].cursor.expect("cursor must clamp, not hide");
        let text = f.panes[0].text;
        assert_eq!(cx, text.x + text.width - 1, "cursor past width clamps to right");
        // Visible: exact cell, no clamping.
        let mut shown = base;
        shown.cursor_y = 3;
        shown.scroll_offset = 0;
        shown.visual_cursor_x = 10;
        let f = compute_frame(opts, &tree, &[shown]);
        let pane = &f.panes[0];
        assert_eq!(
            pane.cursor,
            Some((pane.text.x + 10, pane.text.y + 3)),
            "visible cursor lands on its exact cell"
        );
    }

    #[test]
    fn thumb_never_leaves_its_track() {
        for visible in [1usize, 2, 5, 20, 100] {
            for total in [0usize, 1, 5, 19, 20, 21, 1000] {
                for offset in [0usize, 1, 7, 500] {
                    for track in [0usize, 1, 3, 10, 50] {
                        let (start, len) = thumb_range(visible, total, offset, track);
                        assert!(
                            start + len <= track.max(1),
                            "thumb overflow: v{visible} t{total} o{offset} track{track}"
                        );
                        assert!(len >= 1, "thumb must exist");
                        if total <= visible && track > 0 {
                            assert_eq!(len, track, "all visible: thumb fills track");
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn resize_storm_keeps_every_invariant() {
        let tree = trees()[3].clone();
        let inputs = inputs_for(&tree);
        // Deterministic pseudo-random size sequence (xorshift).
        let mut state: u64 = 0x243F_6A88_85A3_08D3;
        let mut next = move || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state
        };
        for _ in 0..500 {
            let cols = (next() % 220) as usize;
            let rows = (next() % 110) as usize;
            let frame = compute_frame(
                FrameOptions {
                    cols,
                    rows,
                    menu_rows: 1,
                    show_line_numbers: true,
                },
                &tree,
                &inputs,
            );
            assert_tiling(&frame);
        }
    }

    #[test]
    fn unknown_window_tiles_without_cursor() {
        // A layout leaf with no live window keeps the frame total; it
        // carries no cursor and the renderer skips it.
        let tree = leaf(7);
        let frame = compute_frame(
            FrameOptions {
                cols: 80,
                rows: 24,
                menu_rows: 0,
                show_line_numbers: true,
            },
            &tree,
            &[],
        );
        assert_eq!(frame.panes.len(), 1);
        assert_eq!(frame.panes[0].cursor, None);
        assert_tiling(&frame);
    }

    /// Placement-law reproduction: no scrollbar may paint inside the text
    /// rect. Each bar owns a dedicated strip; the text rect excludes it
    /// by construction. Fails while the vertical bar overlaps the text
    /// area's last column.
    #[test]
    fn scrollbars_never_paint_inside_the_text_rect() {
        let tree = leaf(0);
        let mut input = input_for(0);
        input.line_count = 200;
        input.content_width = 200;
        let frame = compute_frame(
            FrameOptions {
                cols: 80,
                rows: 24,
                menu_rows: 0,
                show_line_numbers: true,
            },
            &tree,
            &[input],
        );
        let pane = &frame.panes[0];
        assert!(
            pane.h_scrollbar.is_some(),
            "fixture must need the horizontal bar"
        );
        if let Some(v) = pane.v_scrollbar {
            assert!(
                v.cross < pane.text.x || v.cross >= pane.text.x + pane.text.width,
                "vertical bar column {} lies inside the text rect x={}..{}",
                v.cross,
                pane.text.x,
                pane.text.x + pane.text.width
            );
        }
        let row = pane
            .h_scrollbar
            .map(|bar| bar.cross)
            .expect("fixture must need the horizontal bar");
        assert!(
            row < pane.text.y || row >= pane.text.y + pane.text.height,
            "horizontal bar row {row} lies inside the text rect y={}..{}",
            pane.text.y,
            pane.text.y + pane.text.height
        );
    }

    /// Cursor-swallow reproduction: the cursor sits on the bottom text
    /// row. Widening the content summons the horizontal bar; the cursor
    /// must stay on its cell. Fails while the bar steals the cursor's
    /// row from the text rect instead of owning a dedicated strip.
    #[test]
    fn cursor_survives_horizontal_bar_appearance() {
        let tree = leaf(0);
        let opts = FrameOptions {
            cols: 80,
            rows: 24,
            menu_rows: 0,
            show_line_numbers: false,
        };
        let narrow = WindowGeomInput {
            window: WindowId(0),
            scroll_offset: 0,
            scroll_x: 0,
            cursor_y: 21,
            visual_cursor_x: 5,
            line_count: 100,
            content_width: 40,
        };
        let before = compute_frame(opts, &tree, &[narrow]).panes[0].cursor;
        assert!(before.is_some(), "fixture must show the cursor first");
        let mut wide = narrow;
        wide.content_width = 500;
        let after = compute_frame(opts, &tree, &[wide]).panes[0].cursor;
        assert_eq!(
            after, before,
            "cursor on the bottom text row was swallowed by the horizontal bar"
        );
    }

    /// Construction proof for the ownership invariant: the pane's
    /// owned regions tile the pane frame exactly — pairwise disjoint,
    /// union equals the frame. Every cell of the frame therefore has
    /// exactly one owner, and each owner paints its background as part
    /// of constructing the frame (text and gutter by the window
    /// renderer, strips by the scrollbar painters, the status row by
    /// the status renderer). There is no representable "nobody owns
    /// this cell" state for a renderer to patch; a hole is a model bug.
    /// The sweep covers frame sizes (including degenerate narrow and
    /// short panes), menu rows, scroll offsets, and content shapes at
    /// and past the presence thresholds.
    #[test]
    fn pane_regions_tile_frame_exactly() {
        fn area(r: &Rect) -> usize {
            r.width * r.height
        }
        fn within(inner: &Rect, outer: &Rect) -> bool {
            inner.x >= outer.x
                && inner.y >= outer.y
                && inner.x + inner.width <= outer.x + outer.width
                && inner.y + inner.height <= outer.y + outer.height
        }
        fn disjoint(a: &Rect, b: &Rect) -> bool {
            a.x + a.width <= b.x
                || b.x + b.width <= a.x
                || a.y + a.height <= b.y
                || b.y + b.height <= a.y
        }
        let tree = leaf(0);
        for (cols, rows) in [
            (80usize, 24usize),
            (3, 12),
            (5, 5),
            (10, 3),
            (40, 10),
            (200, 60),
        ] {
            for menu_rows in [0usize, 1] {
                let opts = FrameOptions {
                    cols,
                    rows,
                    menu_rows,
                    show_line_numbers: true,
                };
                // Content shapes: narrow/wide × short/tall, plus the exact
                // boundary values where presence flips.
                for line_count in [0usize, 1, 10, 21, 22, 23, 100, 2_000_000] {
                    for content_width in [0usize, 1, 40, 76, 77, 78, 200, 10_000] {
                        for scroll_x in [0usize, 1] {
                            for scroll_offset in [0usize, 1_000_000] {
                                let input = WindowGeomInput {
                                    window: WindowId(0),
                                    scroll_offset,
                                    scroll_x,
                                    cursor_y: 0,
                                    visual_cursor_x: 0,
                                    line_count,
                                    content_width,
                                };
                                let frame = compute_frame(opts, &tree, &[input]);
                                let pane = &frame.panes[0];
                                let regions: Vec<(RegionOwner, Rect)> = pane
                                    .owned_regions()
                                    .into_iter()
                                    .flatten()
                                    .collect();
                                for (owner, rect) in &regions {
                                    assert!(
                                        within(rect, &pane.frame),
                                        "{owner:?} region {rect:?} escapes pane frame {:?} (cols={cols} rows={rows} lc={line_count} cw={content_width})",
                                        pane.frame
                                    );
                                }
                                for (i, (_, a)) in regions.iter().enumerate() {
                                    for (_, b) in regions.iter().skip(i + 1) {
                                        assert!(
                                            disjoint(a, b) || area(a) == 0 || area(b) == 0,
                                            "owned regions overlap: {a:?} vs {b:?} (cols={cols} rows={rows} lc={line_count} cw={content_width})"
                                        );
                                    }
                                }
                                let union: usize =
                                    regions.iter().map(|(_, r)| area(r)).sum();
                                assert_eq!(
                                    union,
                                    area(&pane.frame),
                                    "owned regions do not tile the frame: {regions:?} vs {:?} (cols={cols} rows={rows} lc={line_count} cw={content_width})",
                                    pane.frame
                                );
                            }
                        }
                    }
                }
            }
        }
    }

    /// The derived presence is a fixed point of the need predicates
    /// evaluated at the final text rect: re-deriving need from the
    /// pane's public outputs must reproduce the derived presence.
    /// This is the no-oscillation proof — a non-converged iteration
    /// would flip a predicate here. Boundary contents (exactly one
    /// line/column past the threshold, and the cross-triggering
    /// pairs) are swept explicitly.
    #[test]
    fn scrollbar_presence_is_a_stable_fixed_point() {
        use crate::config::scrollbar::{SHOW_ARROWS, SHOW_WHEN_FITS};
        let tree = leaf(0);
        let opts = FrameOptions {
            cols: 80,
            rows: 24,
            menu_rows: 0,
            show_line_numbers: false,
        };
        // Same host-width floor the implementation uses: the bar the
        // painter needs, plus the column a vertical bar may take.
        let min_host_w = if SHOW_ARROWS { 6 } else { 4 };
        for line_count in [0usize, 1, 22, 23, 24, 25, 100] {
            for content_width in [0usize, 1, 78, 79, 80, 81, 200] {
                for scroll_x in [0usize, 3] {
                    let input = WindowGeomInput {
                        window: WindowId(0),
                        scroll_offset: 0,
                        scroll_x,
                        cursor_y: 0,
                        visual_cursor_x: 0,
                        line_count,
                        content_width,
                    };
                    let frame = compute_frame(opts, &tree, &[input]);
                    let pane = &frame.panes[0];
                    // Re-derive from public outputs only.
                    let avail_h = pane.frame.height.saturating_sub(1);
                    let avail_w = pane
                        .frame
                        .width
                        .saturating_sub(pane.gutter_width);
                    let vbar_rows = pane.frame.height.saturating_sub(1);
                    let v_need = vbar_rows >= 4
                        && (SHOW_WHEN_FITS || line_count > pane.text.height);
                    let h_need = avail_h >= 2
                        && avail_w >= min_host_w
                        && (content_width > pane.text.width || scroll_x > 0);
                    assert_eq!(
                        pane.v_scrollbar.is_some(),
                        v_need,
                        "vertical presence not a fixed point: lc={line_count} cw={content_width} sx={scroll_x}"
                    );
                    assert_eq!(
                        pane.h_scrollbar.is_some(),
                        h_need,
                        "horizontal presence not a fixed point: lc={line_count} cw={content_width} sx={scroll_x}"
                    );
                    // Purity: same inputs, identical rects.
                    let again = compute_frame(opts, &tree, &[input]);
                    assert_eq!(frame.panes[0], again.panes[0]);
                }
            }
        }
    }

    /// Presence rules: an absent bar surrenders nothing; a present bar
    /// surrenders exactly one row/column; no bar ever intersects the
    /// text rect.
    #[test]
    fn presence_surrenders_exactly_its_strip() {
        let tree = leaf(0);
        let opts = FrameOptions {
            cols: 80,
            rows: 24,
            menu_rows: 0,
            show_line_numbers: false,
        };
        // Tall and narrow: vertical bar only.
        let tall = WindowGeomInput {
            window: WindowId(0),
            scroll_offset: 0,
            scroll_x: 0,
            cursor_y: 0,
            visual_cursor_x: 0,
            line_count: 500,
            content_width: 10,
        };
        let pane = &compute_frame(opts, &tree, &[tall]).panes[0];
        assert!(pane.v_scrollbar.is_some());
        assert!(pane.h_scrollbar.is_none());
        assert_eq!((pane.text.width, pane.text.height), (79, 23));
        // Short and wide: horizontal bar only.
        let wide = WindowGeomInput {
            window: WindowId(0),
            scroll_offset: 0,
            scroll_x: 0,
            cursor_y: 0,
            visual_cursor_x: 0,
            line_count: 5,
            content_width: 500,
        };
        let pane = &compute_frame(opts, &tree, &[wide]).panes[0];
        assert!(pane.v_scrollbar.is_none());
        assert!(pane.h_scrollbar.is_some());
        assert_eq!((pane.text.width, pane.text.height), (80, 22));
        // Neither: full extent.
        let small = WindowGeomInput {
            window: WindowId(0),
            scroll_offset: 0,
            scroll_x: 0,
            cursor_y: 0,
            visual_cursor_x: 0,
            line_count: 5,
            content_width: 10,
        };
        let pane = &compute_frame(opts, &tree, &[small]).panes[0];
        assert!(pane.v_scrollbar.is_none() && pane.h_scrollbar.is_none());
        assert_eq!((pane.text.width, pane.text.height), (80, 23));
        // Strips never intersect the text rect. Orientation is
        // definitional: the horizontal bar's cross is the row above the
        // status row; the vertical bar's is the frame's edge column.
        for p in [&tall, &wide] {
            let pane = &compute_frame(opts, &tree, &[*p]).panes[0];
            for bar in pane.v_scrollbar.iter().chain(pane.h_scrollbar.iter()) {
                let (rx, ry, rw, rh) = if bar.cross == pane.status_row.saturating_sub(1) {
                    (bar.strip_start(), bar.cross, bar.strip_len(), 1)
                } else {
                    (bar.cross, bar.strip_start(), 1, bar.strip_len())
                };
                let t = pane.text;
                let overlap = rx < t.x + t.width
                    && t.x < rx + rw
                    && ry < t.y + t.height
                    && t.y < ry + rh;
                assert!(!overlap, "scrollbar strip intersects the text rect");
            }
        }
    }

    /// Appearance/disappearance transitions: widening the content
    /// summons the horizontal bar, surrendering exactly one row; the
    /// cursor stays on its cell and the top line (the scroll input,
    /// which the pure geometry never rewrites) does not move. Narrowing
    /// reverses it; the cursor is still inside the regrown text rect.
    #[test]
    fn bar_transitions_keep_cursor_and_top_line() {
        let tree = leaf(0);
        let opts = FrameOptions {
            cols: 80,
            rows: 24,
            menu_rows: 0,
            show_line_numbers: false,
        };
        let narrow = WindowGeomInput {
            window: WindowId(0),
            scroll_offset: 5,
            scroll_x: 0,
            cursor_y: 25,
            visual_cursor_x: 5,
            line_count: 100,
            content_width: 40,
        };
        let before = compute_frame(opts, &tree, &[narrow]).panes[0];
        assert!(before.h_scrollbar.is_none());
        let mut wide = narrow;
        wide.content_width = 500;
        let after = compute_frame(opts, &tree, &[wide]).panes[0];
        let bar = after.h_scrollbar.expect("wide content needs the bar");
        assert_eq!(after.text.height, before.text.height - 1);
        assert_eq!(after.text.y, before.text.y, "top line must not move");
        assert_eq!(after.text.width, before.text.width);
        assert_eq!(after.cursor, before.cursor, "cursor must not move");
        assert_eq!(bar.cross, after.status_row - 1);
        // And back: the bar disappears, the text rect regrows by exactly
        // its row, the cursor is still inside.
        let back = compute_frame(opts, &tree, &[narrow]).panes[0];
        assert!(back.h_scrollbar.is_none());
        assert_eq!(back.text.height, before.text.height);
        let (cx, cy) = back.cursor.expect("cursor visible");
        let t = back.text;
        assert!(cx >= t.x && cx < t.x + t.width && cy >= t.y && cy < t.y + t.height);
    }

    #[test]
    fn hit_test_finds_owning_pane() {
        let mut tree = leaf(0);
        tree.split_window(WindowId(0), WindowId(1), crate::core::layout::SplitDirection::Horizontal);
        let inputs = inputs_for(&tree);
        let frame = compute_frame(
            FrameOptions {
                cols: 80,
                rows: 24,
                menu_rows: 1,
                show_line_numbers: false,
            },
            &tree,
            &inputs,
        );
        let left = frame.pane_for(WindowId(0)).unwrap();
        let right = frame.pane_for(WindowId(1)).unwrap();
        assert_eq!(left.frame.width, 40);
        assert_eq!(right.frame.x, 40);
        assert_eq!(frame.hit_test(0, 5).unwrap().window, WindowId(0));
        assert_eq!(frame.hit_test(79, 5).unwrap().window, WindowId(1));
        assert_eq!(frame.hit_test(40, 5).unwrap().window, WindowId(1));
        assert!(frame.hit_test(200, 200).is_none());
    }
}
