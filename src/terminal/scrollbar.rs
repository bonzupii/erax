//! Scrollbar rendering for TUI mode.
//!
//! All scrollbar logic is consolidated here for vertical and horizontal scrollbars.

use crate::terminal::display::{Cell, CellRole, Color, ScreenBuffer};

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

/// Render a vertical scrollbar from the unified model geometry.
///
/// `track` is the arrow-free track; the arrows paint on the first and
/// last text rows. All positions come from [`ScrollbarGeom`]; nothing
/// here recomputes them.
pub fn render_vertical(
    buffer: &mut ScreenBuffer,
    geom: &crate::core::geometry::ScrollbarGeom,
    text_y: usize,
    text_height: usize,
    track_fg: Color,
    thumb_fg: Color,
    track_bg: Color,
) {
    if text_height < 4 {
        return;
    }
    let theme = ScrollbarTheme::HEAVY;
    for y in 0..text_height {
        let screen_y = (text_y + y) as u16;
        let (ch, fg, bg) = if y == 0 {
            (theme.arrow_up, thumb_fg, track_bg)
        } else if y == text_height - 1 {
            (theme.arrow_down, thumb_fg, track_bg)
        } else {
            let track_y = y - 1;
            if track_y >= geom.thumb_start && track_y < geom.thumb_start + geom.thumb_len {
                (theme.thumb, thumb_fg, track_bg)
            } else {
                (theme.track_v, track_fg, track_bg)
            }
        };
        buffer.set(geom.cross as u16, screen_y, Cell::with_role(ch, fg, bg, CellRole::Scrollbar));
    }
}

/// Render a horizontal scrollbar from the unified model geometry.
///
/// Paints the arrow-free track from [`ScrollbarGeom`] with arrows on
/// both ends. All positions come from the model; nothing here
/// recomputes them. Returns the painted span for click detection.
pub fn render_horizontal(
    buffer: &mut ScreenBuffer,
    geom: &crate::core::geometry::ScrollbarGeom,
    bar_start_x: usize,
    bar_width: usize,
    track_fg: Color,
    thumb_fg: Color,
    track_bg: Color,
) -> Option<(u16, u16, usize)> {
    if bar_width < 5 {
        return None;
    }
    let theme = ScrollbarTheme::HEAVY;
    for x in 0..bar_width {
        let screen_x = (bar_start_x + x) as u16;
        let (ch, fg, bg) = if x == 0 {
            (theme.arrow_left, thumb_fg, track_bg)
        } else if x == bar_width - 1 {
            (theme.arrow_right, thumb_fg, track_bg)
        } else {
            let track_x = x - 1;
            if track_x >= geom.thumb_start && track_x < geom.thumb_start + geom.thumb_len {
                (theme.thumb, thumb_fg, track_bg)
            } else {
                (theme.track_h, track_fg, track_bg)
            }
        };
        buffer.set(screen_x, geom.cross as u16, Cell::with_role(ch, fg, bg, CellRole::Scrollbar));
    }
    Some((geom.cross as u16, bar_start_x as u16, bar_width))
}
