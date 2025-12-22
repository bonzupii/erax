//! Unified grid geometry utilities
//!
//! This module provides a single source of truth for grid↔pixel coordinate math,
//! used by both the GUI renderer and TUI input handling.

/// Metrics for grid-based rendering (cell dimensions in pixels)
#[derive(Debug, Clone, Copy)]
// Used by gui/grid_renderer for unified grid↔pixel coordinate math
pub struct GridMetrics {
    /// Width of a single cell in pixels
    pub cell_width: f32,
    /// Height of a single cell in pixels  
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
    /// Create new metrics from cell and viewport dimensions
    pub fn new(cell_width: f32, cell_height: f32, width_px: f32, height_px: f32) -> Self {
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

/// Calculate scrollbar thumb position and size.
///
/// Returns (thumb_start, thumb_end) as absolute positions within the track.
/// Both TUI and GUI should use this for consistent scrollbar rendering.
///
/// # Arguments
/// * `visible_rows` - Number of rows currently visible in the viewport
/// * `total_rows` - Total number of rows in the buffer
/// * `scroll_offset` - Current scroll position (first visible row index)
/// * `track_height` - Height of the scrollbar track in rows/pixels
///
/// # Returns
/// (thumb_start, thumb_end) - Start and end positions of the scrollbar thumb
#[allow(dead_code)] // Will be used when TUI/GUI scrollbar rendering is unified
pub fn scrollbar_thumb_range(
    visible_rows: usize,
    total_rows: usize,
    scroll_offset: usize,
    track_height: usize,
) -> (usize, usize) {
    if total_rows == 0 || track_height == 0 {
        return (0, track_height.max(1));
    }

    // Minimum thumb size (at least 1 unit, or 10% of track)
    let min_thumb_size = (track_height / 10).max(1);

    // Calculate thumb size proportional to visible/total ratio
    let thumb_size = if total_rows <= visible_rows {
        track_height // Full track if all content is visible
    } else {
        let ratio = visible_rows as f64 / total_rows as f64;
        (ratio * track_height as f64).round() as usize
    }
    .max(min_thumb_size);

    // Calculate thumb position
    let scrollable_range = total_rows.saturating_sub(visible_rows);
    let thumb_start = if scrollable_range == 0 {
        0
    } else {
        let position_ratio = scroll_offset as f64 / scrollable_range as f64;
        let available_track = track_height.saturating_sub(thumb_size);
        (position_ratio * available_track as f64).round() as usize
    };

    let thumb_end = (thumb_start + thumb_size).min(track_height);

    (thumb_start, thumb_end)
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
    fn test_px_to_grid_outside() {
        let metrics = GridMetrics::new(10.0, 20.0, 100.0, 100.0);
        // Outside grid bounds
        assert!(metrics.px_to_grid(0.0, 0.0).is_none());
    }
}
