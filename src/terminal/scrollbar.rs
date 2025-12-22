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
}

impl ScrollbarTheme {
    /// Heavy style: ┃ track, █ thumb, ▲▼ arrows
    pub const HEAVY: Self = Self {
        track_v: '┃',
        track_h: '━',
        thumb: '█',
        arrow_up: '▲',
        arrow_down: '▼',
    };
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

    // Calculate thumb position and size
    let visible_lines = track_height;
    let thumb_size = (visible_lines * visible_lines / line_count)
        .max(2)
        .min(track_height);
    let max_scroll = line_count.saturating_sub(visible_lines);
    let thumb_start = if max_scroll > 0 {
        (scroll_offset * (track_height - thumb_size) / max_scroll).min(track_height - thumb_size)
    } else {
        0
    };

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

/// Render a horizontal scrollbar at the bottom of a window (streamlined, no arrows)
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
    let scrollbar_width = text_width.saturating_sub(1); // Full width minus scrollbar column

    if scrollbar_width < 3 {
        return None;
    }

    let content_width = content_width.max(scroll_x + text_width);

    // Calculate thumb size and position (same logic as vertical)
    let thumb_size = (scrollbar_width * text_width / content_width)
        .max(2)
        .min(scrollbar_width);
    let max_scroll = content_width.saturating_sub(text_width);
    let thumb_start = if max_scroll > 0 {
        (scroll_x * (scrollbar_width - thumb_size) / max_scroll).min(scrollbar_width - thumb_size)
    } else {
        0
    };

    let theme = ScrollbarTheme::HEAVY;

    // Track and thumb only (no arrows for streamlined look)
    for x in 0..scrollbar_width {
        let screen_x = scrollbar_start_x + x as u16;
        let (ch, fg) = if x >= thumb_start && x < thumb_start + thumb_size {
            (theme.thumb, thumb_fg)
        } else {
            (theme.track_h, track_fg)
        };
        buffer.set(screen_x, scrollbar_y, Cell::new(ch, fg, track_bg));
    }

    Some((scrollbar_y, scrollbar_start_x, scrollbar_width))
}
