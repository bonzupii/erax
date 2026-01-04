//! Scrollbar rendering for TUI mode
//!
//! All scrollbar logic is consolidated here for vertical and horizontal scrollbars.

use crate::core::layout::Rect;
use crate::terminal::display::{Cell, Color, ScreenBuffer};

/// Scrollbar character theme
#[derive(Clone, Copy, Debug)]
pub struct ScrollbarTheme {
    pub track_v: char,
    pub track_h: char,
    pub thumb: char,
    pub arrow_up: char,
    pub arrow_down: char,
    pub arrow_left: char,
    pub arrow_right: char,
}

impl ScrollbarTheme {
    /// Heavy style: ┃ track, █ thumb, ▲▼◀▶ arrows
    pub const HEAVY: Self = Self {
        track_v: '┃',
        track_h: '━',
        thumb: '█',
        arrow_up: '▲',
        arrow_down: '▼',
        arrow_left: '◀',
        arrow_right: '▶',
    };
}

/// Calculate scrollbar thumb position and size.
///
/// Returns (thumb_start, thumb_size) positions within the track.
/// This is the unified calculation used by both vertical and horizontal scrollbars.
///
/// # Arguments
/// * `visible` - Number of rows/cols currently visible in the viewport
/// * `total` - Total number of rows/cols in the content
/// * `scroll_offset` - Current scroll position (first visible row/col index)
/// * `track_size` - Size of the scrollbar track in rows/cols
pub fn thumb_range(
    visible: usize,
    total: usize,
    scroll_offset: usize,
    track_size: usize,
) -> (usize, usize) {
    if total == 0 || track_size == 0 {
        return (0, track_size.max(1));
    }

    // Minimum thumb size (at least 2 units)
    let min_thumb_size = 2;

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
        (scroll_offset * available_track / scrollable_range).min(available_track)
    };

    (thumb_start, thumb_size)
}

/// Render a vertical scrollbar on the right edge of a window
pub fn render_vertical(
    buffer: &mut ScreenBuffer,
    rect: &Rect,
    line_count: usize,
    scroll_offset: usize,
    track_fg: Color,
    thumb_fg: Color,
    track_bg: Color,
) {
    let text_height = rect.height.saturating_sub(1); // Exclude status line
    if text_height < 4 {
        return;
    }

    let scrollbar_x = (rect.x + rect.width).saturating_sub(1) as u16;
    let arrow_rows = 1;
    let track_height = text_height.saturating_sub(arrow_rows * 2);
    let line_count = line_count.max(1);

    // Use unified thumb calculation
    // visible = text_height (how many lines fit in viewport)
    // total = line_count (total lines in buffer)
    // track_size = track_height (scrollbar track size, excluding arrows)
    let (thumb_start, thumb_size) =
        thumb_range(text_height, line_count, scroll_offset, track_height);

    let theme = ScrollbarTheme::HEAVY;

    for y in 0..text_height {
        let screen_y = (rect.y + y) as u16;
        let (ch, fg, bg) = if y == 0 {
            (theme.arrow_up, thumb_fg, track_bg)
        } else if y == text_height - 1 {
            (theme.arrow_down, thumb_fg, track_bg)
        } else {
            let track_y = y - arrow_rows;
            if track_y >= thumb_start && track_y < thumb_start + thumb_size {
                (theme.thumb, thumb_fg, track_bg)
            } else {
                (theme.track_v, track_fg, track_bg)
            }
        };
        buffer.set(scrollbar_x, screen_y, Cell::new(ch, fg, bg));
    }
}

/// Render a horizontal scrollbar at the bottom of a window with arrows
/// Returns (scrollbar_y, scrollbar_start_x, scrollbar_width) for click detection
pub fn render_horizontal(
    buffer: &mut ScreenBuffer,
    rect: &Rect,
    gutter_width: usize,
    content_width: usize,
    text_width: usize,
    scroll_x: usize,
    track_fg: Color,
    thumb_fg: Color,
    track_bg: Color,
) -> Option<(u16, u16, usize)> {
    // Only render if content is wider than viewport or we're scrolled
    if content_width <= text_width && scroll_x == 0 {
        return None;
    }

    let scrollbar_y = (rect.y + rect.height).saturating_sub(2) as u16;
    let scrollbar_start_x = (rect.x + gutter_width) as u16;
    let scrollbar_width = text_width.saturating_sub(1); // Full width minus vertical scrollbar column

    if scrollbar_width < 5 {
        // Need at least 5 chars: left arrow, track, thumb, track, right arrow
        return None;
    }

    let content_width = content_width.max(scroll_x + text_width);
    let arrow_cols = 1;
    let track_width = scrollbar_width.saturating_sub(arrow_cols * 2);

    // Use unified thumb calculation for the track area (excluding arrows)
    let (thumb_start, thumb_size) = thumb_range(text_width, content_width, scroll_x, track_width);

    let theme = ScrollbarTheme::HEAVY;

    // Render with arrows at each end
    for x in 0..scrollbar_width {
        let screen_x = scrollbar_start_x + x as u16;
        let (ch, fg, bg) = if x == 0 {
            (theme.arrow_left, thumb_fg, track_bg)
        } else if x == scrollbar_width - 1 {
            (theme.arrow_right, thumb_fg, track_bg)
        } else {
            let track_x = x - arrow_cols;
            if track_x >= thumb_start && track_x < thumb_start + thumb_size {
                (theme.thumb, thumb_fg, track_bg)
            } else {
                (theme.track_h, track_fg, track_bg)
            }
        };
        buffer.set(screen_x, scrollbar_y, Cell::new(ch, fg, bg));
    }

    Some((scrollbar_y, scrollbar_start_x, scrollbar_width))
}
